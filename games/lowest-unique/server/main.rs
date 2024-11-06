use std::collections::HashMap;
use std::io::{BufRead, Write};

struct Handler<W> {
    out: W,
    num_players: usize,
    num_options: usize,
    want_visualize: bool,
    visualizer: Option<visualizer::Visualizer>,
    players_alive: Vec<usize>,
    scores: Vec<f64>,
    has_moved: Vec<Option<usize>>,
    max_time_per_move_ms: i64,
    timer_id: usize,
    turn: usize,
    max_turn: usize,
}

impl<W: Write> Handler<W> {
    fn new(out: W) -> Self {
        Self {
            out,
            num_players: 0,
            num_options: 0,
            visualizer: None,
            players_alive: vec![],
            scores: vec![],
            has_moved: vec![],
            max_time_per_move_ms: 1000,
            timer_id: 0,
            turn: 0,
            max_turn: 0,
            want_visualize: false,
        }
    }
    fn handle_line(&mut self, line: &str) -> bool {
        let mut it = line.split_ascii_whitespace();
        match it.next().unwrap() {
            "vis" => {
                match it.next() {
                    Some("inline") => self.want_visualize = true,
                    _ => {}
                }
            }
            "param" => {
                self.num_players = it.next().unwrap().parse().unwrap();
                self.num_options = it.next().map_or(5, |x| x.parse().unwrap());
                self.max_turn = it.next().map_or(500, |x| x.parse().unwrap());
                self.players_alive = (1..=self.num_players).collect();
                self.has_moved = vec![None; 1 + self.num_players];
                self.scores = vec![0.; 1 + self.num_players];
                if self.want_visualize {
                    self.visualizer = Some(visualizer::Visualizer::new(
                        self.num_players,
                        self.num_options,
                    ));
                }
            }
            "start" => {
                self.visualizer
                    .as_mut()
                    .map(|v| v.handle_start(&mut self.out));
                for p in self.players_alive.iter() {
                    wln!(
                        self.out,
                        "send {p} start {} {p} {} {}",
                        self.num_players,
                        self.num_options,
                        self.max_turn
                    );
                }
                self.start_move();
            }
            "recv" => {
                let from = it.next().unwrap().parse::<usize>().unwrap();
                self.handle_recv(from, it);
            }
            "timeout" => {
                let id = it.next().unwrap().parse::<usize>().unwrap();
                self.handle_timeout(id);
            }
            "dropped" => {
                let player = it.next().unwrap().parse::<usize>().unwrap();
                self.handle_dropped(player);
            }
            "over" => {
                return false;
            }
            cmd => panic!("Unrecognized command {cmd}"),
        }
        true
    }
    fn start_move(&mut self) {
        wln!(self.out, "sendall yourmove");
        self.timer_id += 1;
        wln!(
            self.out,
            "timer {} {}ms",
            self.timer_id,
            self.max_time_per_move_ms
        );
    }
    fn handle_recv<'a>(&mut self, from: usize, mut rest: impl Iterator<Item = &'a str>) {
        if !self.players_alive.contains(&from) {
            return;
        }
        if let Some(Some(_)) = self.has_moved.get(from) {
            self.remove_player(from, "already moved this turn");
            return;
        }
        let (Some(mv), None) = (rest.next(), rest.next()) else {
            self.remove_player(from, "trailing data received");
            return;
        };
        match mv.parse::<usize>() {
            Err(_) => {
                self.remove_player(from, "failed to parse move");
            }
            Ok(mv) => {
                if !(1..=self.num_options).contains(&mv) {
                    self.remove_player(from, "move value out of range");
                } else {
                    self.has_moved[from] = Some(mv);
                }
            }
        }
        self.check_turn();
    }
    fn handle_dropped(&mut self, player: usize) {
        if !self.players_alive.contains(&player) {
            return;
        }
        self.players_alive.retain(|x| *x != player);
        self.check_turn();
    }
    fn handle_timeout(&mut self, id: usize) {
        if self.timer_id != id {
            return;
        }
        let mut remove = vec![];
        for &p in self.players_alive.iter() {
            if !self.has_moved[p].is_some() {
                remove.push(p);
            }
        }
        for p in remove {
            self.remove_player(p, "move timeout");
        }
        self.check_turn();
    }
    fn remove_player(&mut self, player: usize, reason: &str) {
        if !self.players_alive.contains(&player) {
            return;
        }
        self.players_alive.retain(|x| *x != player);
        wln!(self.out, "playererror {player} {reason}");
    }
    fn check_turn(&mut self) {
        if self.players_alive.is_empty() {
            self.game_over("no players alive");
            return;
        }
        if !self
            .players_alive
            .iter()
            .all(|p| self.has_moved[*p].is_some())
        {
            return;
        }
        let mut mvs = HashMap::<usize, usize>::new();
        for mv in self.has_moved.iter() {
            let Some(mv) = mv else { continue };
            *mvs.entry(*mv).or_default() += 1;
        }
        let winning_len = mvs
            .iter()
            .map(|(_, players)| players)
            .cloned()
            .min()
            .unwrap();
        let (winning_num, winners_count) = mvs
            .iter()
            .filter(|(_, players)| **players == winning_len)
            .min()
            .unwrap();
        let winners = self
            .has_moved
            .iter()
            .cloned()
            .enumerate()
            .filter_map(|(p, mv)| {
                if mv == Some(*winning_num) {
                    Some(p)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        self.visualizer
            .as_mut()
            .map(|v| v.handle_move(&mut self.out, &self.has_moved, Some(*winning_num), &winners));

        let delta = 1.0 / (*winners_count as f64);
        for w in winners {
            self.scores[w] += delta;
        }
        w!(&mut self.out, "sendall move {winning_num}");
        for i in 1..=self.num_players {
            match self.has_moved[i] {
                Some(mv) => w!(&mut self.out, " {mv}"),
                None => w!(&mut self.out, " 0"),
            }
        }
        wln!(&mut self.out);
        self.next_turn();
    }
    fn next_turn(&mut self) {
        self.has_moved.iter_mut().for_each(|x| *x = None);
        self.turn += 1;
        if self.turn == self.max_turn {
            self.game_over("turn limit reached");
        } else {
            self.start_move();
        }
    }
    fn game_over(&mut self, reason: &str) {
        w!(&mut self.out, "over");
        for score in self.scores[1..].iter() {
            w!(&mut self.out, " {}", score);
        }
        wln!(&mut self.out, " {reason}");
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let mut stdin = std::io::stdin().lock();
    let stdout = std::io::stdout();
    let mut buf = String::new();
    let mut h = Handler::new(stdout);
    loop {
        buf.clear();
        match stdin.read_line(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                if !h.handle_line(&buf) {
                    break;
                }
            }
            Err(e) => panic!("Failed to read from stdin: {e}"),
        }
    }
}

mod visualizer {
    use crate::{w, wln};
    use std::io::Write;
    pub struct Visualizer {
        num_players: usize,
        num_options: usize,
        time: f32,
        next_id: u64,
        ids: Vec<u64>,
        positions: Vec<(f32, f32)>,
        player_color: Vec<(f32, f32, f32, f32)>,
        delta_x: f32,
        delta_y: f32,
        radius: f32,
    }

    impl Visualizer {
        pub fn new(num_players: usize, num_options: usize) -> Self {
            let player_color = vec![
                (0.5, 0.5, 0.5, 0.5),
                (0.8, 0.2, 0.2, 1.),
                (0.2, 0.8, 0.2, 1.),
                (0.2, 0.2, 0.8, 1.),
                (0.2, 0.6, 0.6, 1.),
                (0.6, 0.2, 0.6, 1.),
                (0.6, 0.6, 0.2, 1.),
                (0.7, 0.3, 0.1, 1.),
                (0.1, 0.7, 0.1, 1.),
                (0.1, 0.1, 0.7, 1.),
            ];
            let delta_x = 0.9 / (num_options as f32);
            let delta_y = 0.45 / (num_players as f32);
            let radius = (delta_x * 0.35).min(delta_y);
            Self {
                num_players,
                num_options,
                time: 0.0,
                next_id: 1,
                positions: vec![],
                ids: vec![],
                player_color,
                delta_x,
                delta_y,
                radius,
            }
        }
        pub fn handle_start<W: Write>(&mut self, out: &mut W) {
            self.positions.push((0., 0.));
            self.ids.push(0);
            for i in 1..=self.num_players {
                self.ids.push(self.next_id);
                self.positions
                    .push((0.05 + i as f32 * self.delta_x, self.radius + 0.05));
                let color = self
                    .player_color
                    .get(i as usize % self.player_color.len())
                    .unwrap()
                    .clone();
                w!(
                    out,
                    r#"vis {{t:{},create:{{id:{},z:1,p:[{},{}],geom:[{{circle:{{r:{},f:"{}"}}}},"#,
                    self.time,
                    self.next_id,
                    self.positions[i].0,
                    self.positions[i].1,
                    self.radius,
                    format_color(color)
                );
                wln!(
                    out,
                    r#"{{text:{{p:[{},{}],t:{},v:"{}"}}}}]}}}}"#,
                    -self.radius * 0.25,
                    self.radius * 0.25,
                    self.radius,
                    i
                );
                self.next_id += 1;
            }

            wln!(
                out,
                "vis {{t:{},create:{{id:{},z:1,geom:[{{line:{{p1:[0.05,0.5],p2:[0.95,0.5],t:0.004}}}}]}}}}",
                self.time,
                self.next_id
            );
            self.next_id += 1;
            for i in 0..=self.num_options {
                wln!(
                    out,
                    "vis {{t:{},create:{{id:{},z:1,p:[{},0.5],geom:[{{line:{{p1:[0,-0.05],p2:[0,0.05],t:0.004}}}}]}}}}",
                    self.time,
                    self.next_id,
                    i as f32 * self.delta_x + 0.05
                );
                self.next_id += 1;
            }
        }
        pub fn handle_move<W: Write>(
            &mut self,
            out: &mut W,
            moves: &[Option<usize>],
            winning_move: Option<usize>,
            winners: &[usize],
        ) {
            self.time += 0.4;
            let mut buckets = vec![0; 1 + self.num_options];
            for i in 1..moves.len() {
                let Some(x) = moves[i] else {
                    continue;
                };
                let newx = x as f32 * self.delta_x + 0.05 - 0.5 * self.delta_x;
                let newy = 0.55 + self.radius + buckets[x] as f32 * self.delta_y;
                buckets[x] += 1;
                let (x, y) = self.positions[i];
                let dx = newx - x;
                let dy = newy - y;
                wln!(
                    out,
                    "vis {{t:{},transform:{{id:{},d:0.3,mv:[{dx},{dy}]}}}}",
                    self.time,
                    self.ids[i]
                );
                self.positions[i] = (newx, newy);
            }
            self.time += 0.3;
            for w in winners {
                wln!(
                    out,
                    "vis {{t:{},transform:{{id:{},d:0.2,scale:1.6}}}}",
                    self.time,
                    self.ids[*w]
                );
            }
            for w in winners {
                wln!(
                    out,
                    "vis {{t:{},transform:{{id:{},d:0.2,scale:0.625}}}}",
                    self.time + 0.3,
                    self.ids[*w]
                );
            }
            self.time += 0.3;
        }
    }
    fn format_color((r, g, b, a): (f32, f32, f32, f32)) -> String {
        let u = |c: f32| -> u8 {
            (c.clamp(0., 1.) * 255.) as u8
        };
        format!("{:02x}{:02x}{:02x}{:02x}", u(r), u(g), u(b), u(a))
    }
}

#[macro_export]
macro_rules! wln {
    ($o:expr) => {{
        let _ = writeln!($o);
        let _ = $o.flush();
    }};
    ($o:expr, $($es:expr),+) => {{
        let _ = writeln!($o, $($es),+);
        let _ = $o.flush();
    }}
}

#[macro_export]
macro_rules! w {
    ($($es:expr),+) => {{
        let _ = write!($($es),+);
    }}
}
