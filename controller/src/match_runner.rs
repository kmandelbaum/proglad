use anyhow::anyhow;
use anyhow::Context;
use futures_util::future::select_all;
use futures_util::{sink::SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::io::*;
use proglad_api::textapi;

pub type TextLogSink = Box<dyn AsyncWrite + Unpin + Send>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub send_timeout: std::time::Duration,
    pub sender_open_timeout: std::time::Duration,
    pub player_ready_timeout: std::time::Duration,
    pub kick_for_errors: bool,
    pub max_player_errors: usize,
    pub line_length_limit: usize,
}

pub struct MatchConfig {
    pub config: Config,
    // Game server is the first (index 0) in these vectors.
    pub ios: Vec<AgentIO>,
    pub params: Vec<String>,
    // Instead of using 'tee' which is a separate process, log here.
    pub game_log_sink: TextLogSink,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchResult {
    pub scores: Vec<f64>,
    // Reason for the game to finish.
    pub reason: String,
    pub errors: Vec<(usize, String)>,
}

// Runs the given match to completion. In case of game server failing to
// comply with the protocol, an error is returned and
// no MatchResult produced.
pub async fn run(mut config: MatchConfig) -> anyhow::Result<MatchResult> {
    if config.ios.is_empty() {
        return Err(anyhow!(
            "incorrect number of ios: 0; expected 1 + number of players"
        ));
    }
    let sender_open_timeout = config.config.sender_open_timeout;
    let line_length_limit = config.config.line_length_limit;
    let (game_stdout, game_stdin) = open(
        config.ios[0].clone(),
        line_length_limit,
        sender_open_timeout,
    )
    .await
    .context("Failed to open game server pipes")?;
    let ios = std::mem::take(&mut config.ios);
    let mut g = MatchOnServer::new(config, game_stdout, game_stdin);
    for (idx, a) in ios.into_iter().skip(1).enumerate() {
        match open(a, line_length_limit, sender_open_timeout).await {
            Err(e) => {
                info!("Failed to open player {} IO : {e}", idx + 1);
                g.add_disconnected_player();
            }
            Ok((r, w)) => {
                log::trace!("Adding player {}", idx + 1);
                g.add_player(r, w);
            }
        }
    }
    g.run().await
}

struct PlayerInMatch {
    ingame_id: usize,
    sink: LineSink,
    stream: LineStream,
    reported_ready: bool,
}

#[derive(Debug)]
enum State {
    Running,
    Over(MatchResult),
}

struct MatchOnServer {
    start_instant: std::time::Instant,
    // Indexed by agent ID. 0 for game, 1..=N for players.
    params: Vec<String>,
    // Indexed by player in match - 1.
    players: Vec<Option<PlayerInMatch>>,
    send_timeout: std::time::Duration,
    game_server_stream: LineStream,
    game_server_sink: LineSink,
    game_log_sink: TextLogSink,
    game_timers: BTreeSet<(std::time::Instant, u32)>,
    kick_for_errors: bool,
    state: State,
    player_ready_timeout: std::time::Duration,
    ready_deadline: Option<std::time::Instant>,
    player_errors: Vec<Vec<String>>,
    max_player_errors: usize,
}

type PinnedFuture<'a, T> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Option<Result<T, anyhow::Error>>> + 'a + Send>,
>;

impl MatchOnServer {
    fn new(
        config: MatchConfig,
        game_server_stream: LineStream,
        game_server_sink: LineSink,
    ) -> Self {
        MatchOnServer {
            players: vec![],
            params: config.params,
            send_timeout: config.config.send_timeout,
            game_server_stream,
            game_server_sink,
            game_timers: Default::default(),
            state: State::Running,
            game_log_sink: config.game_log_sink,
            kick_for_errors: config.config.kick_for_errors,
            player_ready_timeout: config.config.player_ready_timeout,
            ready_deadline: None,
            start_instant: std::time::Instant::now(),
            player_errors: vec![],
            max_player_errors: config.config.max_player_errors,
        }
    }

    fn add_player(&mut self, stream: LineStream, sink: LineSink) {
        self.players.push(Some(PlayerInMatch {
            ingame_id: self.players.len() + 1,
            sink,
            stream,
            reported_ready: false,
        }));
        self.player_errors.push(vec![]);
    }

    fn add_disconnected_player(&mut self) {
        self.players.push(None);
    }

    async fn kick_player(&mut self, ingame_id: usize) -> anyhow::Result<()> {
        self.game_send(format!("dropped {ingame_id}")).await?;
        if let Some(mp) = self.players.get_mut(ingame_id - 1) {
            *mp = None;
        }
        Ok(())
    }

    async fn game_send(&mut self, msg: String) -> anyhow::Result<()> {
        self.log_to_sink(LogDirection::In, &msg).await;
        tokio::time::timeout(self.send_timeout, self.game_server_sink.send(msg))
            .await?
            .context("Failed to send to the game server")?;
        Ok(())
    }

    async fn run(&mut self) -> anyhow::Result<MatchResult> {
        let mr = self.run_impl().await;
        if let Err(e) = self.game_log_sink.shutdown().await {
            log::error!("Failed to flush game_in_log_sink: {e}");
        }
        mr
    }

    async fn run_impl(&mut self) -> anyhow::Result<MatchResult> {
        self.ready_deadline = Some(std::time::Instant::now() + self.player_ready_timeout);
        self.game_send("vis inline".to_owned()).await?;
        if let Some(param_str) = self.params.first() {
            self.game_send(format!("param {param_str}")).await?;
        }
        loop {
            match &mut self.state {
                State::Running => {}
                State::Over(res) => return Ok(res.clone()),
            }
            let timeout = self
                .get_io_deadline()
                .saturating_duration_since(std::time::Instant::now());
            let gs = &mut self.game_server_stream;
            // TODO: hide or rewrite this abomination
            // Actually merge the streams instead. Or spawn a tokio task per client.
            // Anything will be better than select_all.
            let f: PinnedFuture<'_, String> = Box::pin(async move { gs.next().await });
            let mut streams = Vec::with_capacity(1 + self.players.len());
            let mut map_id = Vec::with_capacity(1 + self.players.len());
            streams.push(f);
            map_id.push(0);
            for p in self.players.iter_mut() {
                let Some(p) = p else {
                    continue;
                };
                map_id.push(p.ingame_id);
                let f: PinnedFuture<'_, String> = Box::pin(async move { p.stream.next().await });
                streams.push(f);
            }
            let action = match tokio::time::timeout(timeout, select_all(streams)).await {
                Ok((Some(Ok(line)), idx, _)) => Some(Ok((map_id[idx], line))),
                Ok((None, idx, _)) => {
                    let id = map_id[idx];
                    Some(Err((id, anyhow!("stream ended"))))
                }
                Ok((Some(Err(e)), idx, _)) => {
                    let id = map_id[idx];
                    Some(Err((id, e)))
                }
                Err(_ /*timeout elapsed*/) => None,
            };
            match action {
                Some(Ok((id, line))) => {
                    if id == 0 {
                        self.handle_game_msg(line).await?;
                    } else {
                        self.handle_player_msg(id, line).await?;
                    }
                }
                Some(Err((id, e))) => {
                    if id == 0 {
                        self.handle_game_dropoff(e).await?;
                    } else {
                        self.handle_player_dropoff(id, e).await?;
                    }
                }
                None => self.handle_timeout().await?,
            }
            if self.ready_deadline.is_some() && self.all_players_ready() {
                self.ready_deadline = None;
                self.game_send("start".to_owned()).await?;
            }
        }
    }

    fn all_players_ready(&self) -> bool {
        self.players
            .iter()
            .all(|op| op.as_ref().map_or(true, |p| p.reported_ready))
    }

    fn time_from_start_micros(&self) -> u128 {
        std::time::Instant::now()
            .duration_since(self.start_instant)
            .as_micros()
    }

    async fn log_to_sink(&mut self, d: LogDirection, msg: &str) {
        let micros = self.time_from_start_micros();
        let _ = self
            .game_log_sink
            .write_all(
                format!(
                    "{:03}.{:06} {} {msg}\n",
                    micros / 1000000,
                    micros % 1000000,
                    d.render()
                )
                .as_bytes(),
            )
            .await
            .map_err(|e| error!("Failed to write log to sink {d:?} : {e}"));
    }

    fn get_io_deadline(&self) -> std::time::Instant {
        if let Some(ready_deadline) = self.ready_deadline {
            return ready_deadline;
        };
        self.game_timers
            .first()
            .map_or_else(|| std::time::Instant::now() + GLOBAL_DEADLINE, |x| x.0)
    }

    async fn handle_game_msg(&mut self, line: String) -> anyhow::Result<()> {
        self.log_to_sink(LogDirection::Out, &line).await;
        let [cmd, rest] = textapi::split(&line);
        match cmd {
            "" => return Err(anyhow!("Empty game command")),
            "timer" => self.set_timer(rest).await?,
            "over" => self.over(rest).await?,
            "sendall" => self.sendall(rest).await?,
            "playererror" => self.playererror(rest).await?,
            "send" => self.game_to_player_send(rest).await?,
            "vis" => { /* intentionally ignore for now */ }
            _ => return Err(anyhow!("Unrecognized game command '{cmd}'")),
        }
        Ok(())
    }

    async fn handle_player_msg(&mut self, ingame_id: usize, line: String) -> anyhow::Result<()> {
        match self.players.get_mut(ingame_id - 1).unwrap_or(&mut None) {
            None => error!("Received data from disconnected player {ingame_id}"),
            Some(player) => {
                if player.reported_ready {
                    self.game_send(format!("recv {ingame_id} {line}")).await?
                } else if line == "ready" {
                    player.reported_ready = true;
                } else {
                    self.handle_player_dropoff(
                        ingame_id,
                        anyhow!("Received garbage before player is ready"),
                    )
                    .await?;
                }
            }
        };
        Ok(())
    }

    async fn handle_timeout(&mut self) -> anyhow::Result<()> {
        let now = std::time::Instant::now();
        if let Some(ready_deadline) = self.ready_deadline {
            if now >= ready_deadline {
                let kick_non_ready = self
                    .players
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        if p.as_ref().map(|pp| !pp.reported_ready) == Some(true) {
                            Some(i + 1)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                for p in kick_non_ready {
                    self.add_player_error(p, "timeout waiting for ready");
                    self.handle_player_dropoff(p, anyhow!("Timeout waiting for ready"))
                        .await?;
                }
                assert!(self.all_players_ready());
                self.ready_deadline = None;
                self.game_send("start".to_owned()).await?;
            }
        }
        while let Some(first) = self.game_timers.first() {
            if first.0 <= now {
                self.game_send(format!("timeout {}", first.1)).await?;
                self.game_timers.pop_first();
            } else {
                break;
            }
        }
        Ok(())
    }

    async fn handle_game_dropoff(&mut self, e: anyhow::Error) -> anyhow::Result<()> {
        Err(e.context("Game server dropped off"))
    }

    async fn handle_player_dropoff(
        &mut self,
        ingame_id: usize,
        e: anyhow::Error,
    ) -> anyhow::Result<()> {
        log::trace!("Player {ingame_id} dropped off; reason: {e}");
        self.kick_player(ingame_id).await
    }

    async fn game_to_player_send(&mut self, line: &str) -> anyhow::Result<()> {
        let [player_str, rest] = textapi::split(line);
        let id = player_str.parse::<usize>()?;
        if id == 0 {
            return Err(anyhow!(
                "Non-existent player id: {id} in 'send' from game server"
            ));
        }
        self.send_to_player(id, rest).await
    }

    async fn send_to_player(&mut self, ingame_id: usize, msg: &str) -> anyhow::Result<()> {
        let noplayer_err = || anyhow!("Non-existent player id: {ingame_id} send_to_player");
        let player = self
            .players
            .get_mut(ingame_id - 1)
            .ok_or_else(noplayer_err)?;
        let Some(player) = player.as_mut() else {
            // Do not penalize the game engine for not knowing that a player is dropped.
            return Ok(());
        };
        match tokio::time::timeout(self.send_timeout, player.sink.send(msg.to_owned())).await {
            Err(_ /*timeout elapsed*/) => {
                self.handle_player_dropoff(ingame_id, anyhow!("Send timed out"))
                    .await
            }
            Ok(Err(e)) => {
                self.handle_player_dropoff(ingame_id, e.context("Send failed"))
                    .await
            }
            Ok(Ok(())) => Ok(()),
        }
    }

    async fn sendall(&mut self, line: &str) -> anyhow::Result<()> {
        for id in 1..=self.players.len() {
            self.send_to_player(id, line).await?;
        }
        Ok(())
    }

    async fn over(&mut self, line: &str) -> anyhow::Result<()> {
        let mut it = line.splitn(self.players.len() + 1, ' ');
        let mut scores = Vec::with_capacity(self.players.len());
        for _ in 0..self.players.len() {
            let Some(score) = it.next() else {
                return Err(anyhow!("Game server returned not enough scores"));
            };
            let score = score
                .parse()
                .context("Failed to parse score from the 'over' command")?;
            scores.push(score);
        }
        let mut errors = vec![];
        for (i, pe) in self.player_errors.iter_mut().enumerate() {
            errors.extend(std::mem::take(pe).into_iter().map(|e| (i + 1, e)));
        }
        let reason = it.next().unwrap_or_default().to_owned();
        self.state = State::Over(MatchResult {
            scores,
            reason,
            errors,
        });
        Ok(())
    }

    async fn set_timer(&mut self, line: &str) -> anyhow::Result<()> {
        let [id_str, duration_str] = textapi::split(line);
        let id = id_str.parse::<u32>()?;
        if id == 0 {
            return Err(anyhow!("timer id should be > 0"));
        }
        let duration_str = duration_str
            .strip_suffix("ms")
            .ok_or(anyhow!("timer duration does not end with 'ms'"))?;
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(
                duration_str
                    .parse()
                    .context("parsing timer duration failed")?,
            );
        self.game_timers.insert((deadline, id));
        Ok(())
    }

    async fn playererror(&mut self, line: &str) -> anyhow::Result<()> {
        let [player_id_str, rest] = textapi::split(line);
        let ingame_id = player_id_str
            .parse::<usize>()
            .context("Failed to parse player number in 'playererror'")?;
        log::trace!("Player {ingame_id} error reported: {rest}");
        self.add_player_error(ingame_id, rest);
        if self.kick_for_errors {
            self.kick_player(ingame_id).await?
        }
        Ok(())
    }

    fn add_player_error(&mut self, ingame_id: usize, err: &str) {
        if let Some(pe) = self.player_errors.get_mut(ingame_id - 1) {
            if pe.len() < self.max_player_errors {
                pe.push(err.to_owned());
            }
        }
    }
}

const GLOBAL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

#[derive(Debug, Copy, Clone)]
enum LogDirection {
    In,
    Out,
}

impl LogDirection {
    fn render(self) -> &'static str {
        match self {
            LogDirection::In => ">",
            LogDirection::Out => "<",
        }
    }
}
