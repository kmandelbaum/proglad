// It is OK to panic if the format of the data supplied by controller is wrong.
use std::io::Write;

fn main() {
    let mut stdin = std::io::stdin().lock();
    let stdout = std::io::stdout();
    let mut buf = String::new();
    stdin.read_line(&mut buf).unwrap();
    let mut parts = buf.split_ascii_whitespace();
    let mut visualize = false;
    let mut firstline = None;
    match (parts.next(), parts.next()) {
        (Some("param"), Some("inlinevisualize")) => {
            visualize = true;
        }
        (Some("param"), Some("visualize")) => {
            visualizer::visualize();
        }
        _ => firstline = Some(&buf),
    }
    let mut h = Handler::new(stdout, visualize);
    firstline.map(|line| h.handle_line(line));
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

struct Handler<W> {
    out: W,
    game: Game,
    max_time_per_move_ms: u32,
    timer_id: u32,
    visualizer: Option<visualizer::VHandler>,
}

impl<W: Write> Handler<W> {
    fn new(out: W, visualize: bool) -> Self {
        let visualizer = if visualize {
            Some(visualizer::VHandler::new())
        } else {
            None
        };
        Self {
            out,
            game: Game::default(),
            max_time_per_move_ms: 100,
            timer_id: 0,
            visualizer,
        }
    }
    fn handle_line(&mut self, line: &str) -> bool {
        let mut it = line.split_ascii_whitespace();
        match it.next().unwrap() {
            "start" => {
                self.visualizer
                    .as_mut()
                    .map(|v| v.handle_start(&mut self.out));
                for p in self.game.players_alive.iter() {
                    wln!(self.out, "send {p} start {p}");
                }
                wln!(self.out, "send 1 yourmove");
                self.timer_id += 1;
                wln!(
                    self.out,
                    "timer {} {}ms",
                    self.timer_id,
                    self.max_time_per_move_ms
                );
            }
            "recv" => {
                let from = it.next().unwrap().parse::<u8>().unwrap();
                self.handle_recv(from, it);
            }
            "timeout" => {
                let id = it.next().unwrap().parse::<u32>().unwrap();
                self.handle_timeout(id);
            }
            "dropped" => {
                let player = it.next().unwrap().parse::<u8>().unwrap();
                self.handle_dropped(player);
            }
            "over" => {
                return false;
            }
            cmd => panic!("Unrecognized command {cmd}"),
        }
        true
    }
    fn handle_recv<'a, I: Iterator<Item = &'a str>>(&mut self, player: u8, mut it: I) {
        if player != self.game.current_player {
            self.handle_player_error(player, "not your move");
            self.game.remove_player(player);
            self.check_gameover();
            return;
        }
        match it.next() {
            None => wln!(self.out, "playererror {player} no command"),
            Some("move") => {
                let parts: Result<Vec<i32>, std::num::ParseIntError> =
                    it.map(|s| s.parse::<i32>()).collect();
                let Ok(parts) = parts else {
                    self.handle_player_error(player, "failed to parse int");
                    return;
                };
                if parts.len() % 2 != 0 {
                    self.handle_player_error(player, "odd number of coordinates");
                    return;
                }
                let hops: Vec<Coord> = parts
                    .iter()
                    .step_by(2)
                    .zip(parts.iter().skip(1).step_by(2))
                    .map(|(x, y)| Coord::new(*x, *y))
                    .collect();
                if !self.game.try_move(&hops) {
                    self.handle_player_error(player, "invalid move");
                    return;
                }
                self.visualizer
                    .as_mut()
                    .map(|v| v.handle_move_hops(&mut self.out, hops.iter().cloned()));
                w!(self.out, "sendall move");
                for hop in hops {
                    w!(self.out, " {} {}", hop.x, hop.y);
                }
                wln!(self.out);
                let blockers = self.game.check_and_remove_blockers();
                for b in blockers {
                    wln!(self.out, "playererror {b} blocking another players home");
                }
                if self.check_gameover() {
                    return;
                }
                self.timer_id += 1;
                wln!(
                    self.out,
                    "timer {} {}ms",
                    self.timer_id,
                    self.max_time_per_move_ms
                );
                wln!(self.out, "send {} yourmove", self.game.current_player);
            }
            Some(_) => self.handle_player_error(player, "invalid command"),
        }
    }
    fn handle_player_error(&mut self, player: u8, error: &str) {
        wln!(self.out, "playererror {player} {error}");
        self.game.remove_player(player);
        self.check_gameover();
    }
    fn handle_timeout(&mut self, id: u32) {
        if id == self.timer_id {
            self.game.remove_player(self.game.current_player);
            self.check_gameover();
        }
    }
    fn handle_dropped(&mut self, player: u8) {
        self.game.remove_player(player);
        self.check_gameover();
    }
    fn check_gameover(&mut self) -> bool {
        match &self.game.status {
            GameStatus::Ongoing => false,
            GameStatus::Won(p, reason) => {
                if *p == 1 {
                    wln!(self.out, "over 2 0 {reason}");
                } else {
                    wln!(self.out, "over 0 2 {reason}");
                }
                true
            }
            GameStatus::Drawn(reason) => {
                wln!(self.out, "over 1 1 {reason}");
                true
            }
        }
    }
}

use std::{collections::HashMap, io::BufRead};

type Map = HashMap<Coord, Cell>;

fn create_map() -> Map {
    // TODO: multiplayer
    let side_size = 16;
    let zero = Coord::new(0, 0);
    let mut m = Map::new();
    for x in 1..=side_size {
        for y in 1..=side_size {
            m.insert(Coord::new(x, y), Cell::new());
        }
    }
    for x in 1..=5 {
        for y in 1..=(7 - x).min(5) {
            let c = Coord::new(x, y);
            m.get_mut(&c).unwrap().set_home(Some(2));
            m.get_mut(&c).unwrap().set_player(Some(1));

            let c = Coord::new(side_size + 1 - x, side_size + 1 - y);

            m.get_mut(&c).unwrap().set_home(Some(1));
            m.get_mut(&c).unwrap().set_player(Some(2));
        }
    }
    for x in 1..=7 {
        for y in 1..=(9 - x).min(7) {
            m.get_mut(&Coord::new(x, y))
                .unwrap()
                .set_block_zone(Some(2));
            let c = Coord::new(side_size + 1 - x, side_size + 1 - y);
            m.get_mut(&c).unwrap().set_block_zone(Some(1));
        }
    }
    m
}

// 4 msb - player number (if > 0), 4 lsb flags.
// 4 lsb - player number (if > 0) for the home point.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
struct Cell(u8);

impl Cell {
    fn new() -> Self {
        Self(0)
    }
    fn get_player(&self) -> Option<u8> {
        if self.0 & 0xf0 != 0 {
            Some(self.0 >> 4)
        } else {
            None
        }
    }
    fn set_player(&mut self, player: Option<u8>) {
        if let Some(p) = player {
            self.0 = (self.0 & 0x0f) | (p << 4);
        } else {
            self.0 &= 0x0f;
        }
    }
    fn get_home(&self) -> Option<u8> {
        if self.0 & 0x3 != 0 {
            Some(self.0 & 0x3)
        } else {
            None
        }
    }
    fn set_home(&mut self, player: Option<u8>) {
        if let Some(p) = player {
            self.0 = (self.0 & 0xfc) | p;
        } else {
            self.0 &= 0xfc;
        }
    }
    fn get_block_zone(&self) -> Option<u8> {
        if self.0 & 0xc != 0 {
            Some((self.0 & 0xc) >> 2)
        } else {
            None
        }
    }
    fn set_block_zone(&mut self, player: Option<u8>) {
        if let Some(p) = player {
            self.0 = (self.0 & 0xf3) | (p << 2);
        } else {
            self.0 &= 0xf3;
        }
    }
}

#[derive(Debug, Clone)]
enum GameStatus {
    Ongoing,
    Won(u8, String),
    Drawn(String),
}

#[derive(Debug, Clone)]
struct Game {
    map: Map,
    current_player: u8,
    status: GameStatus,
    players_alive: Vec<u8>,
    num_move: usize,
    max_home_blocking_moves: usize,
    max_moves: usize,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            map: create_map(),
            current_player: 1,
            status: GameStatus::Ongoing,
            players_alive: vec![1, 2],
            num_move: 1,
            max_home_blocking_moves: 100,
            max_moves: 1000,
        }
    }
}

fn winner(map: &Map) -> GameStatus {
    let mut home_count = [0; 4];
    let mut piece_count = [0; 4];
    for c in map.values() {
        if let Some(p) = c.get_player() {
            piece_count[p as usize] += 1;
            if c.get_home() == Some(p) {
                home_count[p as usize] += 1;
            }
        }
    }
    let mut res = GameStatus::Ongoing;
    for p in 1..4 {
        let p = p as usize;
        if piece_count[p] != 0 && home_count[p] == piece_count[p] {
            let GameStatus::Ongoing = res else {
                return GameStatus::Drawn("multiple players reached home".to_owned());
            };
            res = GameStatus::Won(p as u8, "all pieces home".to_owned());
        }
    }
    res
}

impl Game {
    fn move_piece(&mut self, from: Coord, to: Coord) {
        let player = self.map.get(&from).unwrap().get_player();
        self.map.entry(to).and_modify(|x| x.set_player(player));
        self.map.entry(from).and_modify(|x| x.set_player(None));
    }

    fn next_player(&self, player: u8) -> u8 {
        self.players_alive
            .iter()
            .cloned()
            .chain(self.players_alive.iter().cloned())
            .skip_while(|x| *x != player)
            .nth(1)
            .unwrap_or(player)
    }

    fn remove_player(&mut self, player: u8) {
        let Some(i) = self.players_alive.iter().position(|x| *x == player) else {
            return;
        };
        self.players_alive.remove(i);
        for (_, cell) in self.map.iter_mut() {
            if cell.get_player() == Some(player) {
                cell.set_player(None);
            }
            if cell.get_home() == Some(player) {
                cell.set_home(None);
            }
            if cell.get_block_zone() == Some(player) {
                cell.set_block_zone(None);
            }
        }
        if self.players_alive.len() == 1 {
            self.status =
                GameStatus::Won(self.players_alive[0], "other players dropped".to_owned());
        }
    }

    fn try_move(&mut self, hops: &[Coord]) -> bool {
        let GameStatus::Ongoing = self.status else {
            return false;
        };
        if hops.len() < 2 {
            return false;
        }
        let mut prev = hops[0];
        if self.map.get(&prev).and_then(|c| c.get_player()) != Some(self.current_player) {
            return false;
        }
        for i in 1..hops.len() {
            if !self.cell_free(hops[i]) {
                return false;
            }
            if hops[0..i].contains(&hops[i]) {
                return false;
            }
            match prev.distance(hops[i]) {
                1 => {
                    if hops.len() > 2 {
                        return false;
                    }
                }
                2 => {
                    let mut d = hops[i] - prev;
                    d.x /= 2;
                    d.y /= 2;
                    let mid = d + prev;
                    if self.map.get(&mid).and_then(|x| x.get_player()).is_none() {
                        return false;
                    }
                }
                _ => return false,
            }
            prev = hops[i];
        }
        self.move_piece(hops[0], hops[hops.len() - 1]);
        self.status = winner(&self.map);
        let np = self.next_player(self.current_player);
        if np <= self.current_player {
            self.num_move += 1;
        }
        self.current_player = np;
        if let GameStatus::Ongoing = self.status {
            if self.num_move >= self.max_moves {
                self.status = GameStatus::Drawn("turn limit reached".to_owned());
            }
        }
        true
    }

    fn check_and_remove_blockers(&mut self) -> Vec<u8> {
        if self.num_move <= self.max_home_blocking_moves {
            return vec![];
        }
        let mut blockers = vec![];
        for c in self.map.values() {
            let Some(player) = c.get_player() else {
                continue;
            };
            if c.get_block_zone().map_or(false, |h| h != player) && !blockers.contains(&player) {
                blockers.push(player);
            }
        }
        for b in blockers.iter() {
            self.remove_player(*b);
        }
        if self.players_alive.is_empty() {
            self.status = GameStatus::Drawn("all players dropped".to_owned());
        }
        blockers
    }

    fn cell_free(&self, pos: Coord) -> bool {
        self.map
            .get(&pos)
            .map_or(false, |h| h.get_player().is_none())
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
struct Coord {
    x: i32,
    y: i32,
}

impl Coord {
    fn new(x: i32, y: i32) -> Self {
        Coord { x, y }
    }
    fn distance(&self, c: Coord) -> i32 {
        (self.x - c.x).abs().max((self.y - c.y).abs())
    }
    pub fn to_pixel(&self, s: f32) -> (f32, f32) {
        (
            0.5 + (-8.5 + self.x as f32) * s,
            0.5 + (-8.5 + self.y as f32) * s,
        )
    }
}

impl std::ops::Add for Coord {
    type Output = Coord;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for Coord {
    type Output = Coord;
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Div<i32> for Coord {
    type Output = Coord;
    fn div(self, rhs: i32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl std::ops::Mul<i32> for Coord {
    type Output = Coord;
    fn mul(self, rhs: i32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

mod visualizer {
    use super::{create_map, w, wln, Coord};
    use std::collections::HashMap;
    use std::io::{BufRead, Write};
    pub struct VHandler {
        quad_size: f32,
        map: Vec<Coord>,
        piece_id: HashMap<Coord, u64>,
        piece_player: HashMap<u64, u8>,
        player_color: Vec<(f32, f32, f32, f32)>,
        hop_duration: f32,
        time: f32,
    }
    impl VHandler {
        pub fn new() -> Self {
            let map = create_map();
            let pieces = map
                .iter()
                .filter_map(|(&h, &c)| c.get_player().map(|p| (h, p)))
                .enumerate()
                .map(|(i, x)| (1 + i as u64, x));
            let map = map.iter().map(|(h, _)| *h).collect::<Vec<_>>();
            let piece_id =
                HashMap::<Coord, u64>::from_iter(pieces.clone().map(|(i, (h, _))| (h, i)));
            let piece_player = HashMap::<u64, u8>::from_iter(pieces.map(|(i, (_, p))| (i, p)));
            let player_color = vec![
                (0.5, 0.5, 0.5, 0.5),
                (0.8, 0.2, 0.2, 1.),
                (0.2, 0.8, 0.2, 1.),
            ];
            Self {
                map,
                piece_id,
                piece_player,
                player_color,
                quad_size: 0.06, // 1/16 is the limit.
                hop_duration: 0.3,
                time: 0.0,
            }
        }
        fn handle_line<W: Write>(&mut self, out: &mut W, line: &str) -> bool {
            let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
            if parts.len() < 3 {
                return true;
            }
            match (parts[1], parts[2]) {
                (">", "start") => self.handle_start(out),
                ("<", "sendall") => {
                    if parts[3] == "move" {
                        self.handle_move_str(out, parts);
                    }
                }
                _ => {}
            }
            true
        }
        pub fn handle_move_hops<W: Write, I: Iterator<Item = Coord>>(
            &mut self,
            out: &mut W,
            mut hops: I,
        ) {
            let first = hops.next().unwrap();
            let id = self.piece_id.get(&first).unwrap();
            let mut prev = first.to_pixel(self.quad_size);
            let mut last = first;
            for h in hops {
                last = h;
                let cur = h.to_pixel(self.quad_size);
                let (dx, dy) = (cur.0 - prev.0, cur.1 - prev.1);

                wln!(
                    out,
                    r#"vis {{"t":{},"transform":{{"id":{id},"d":{},"mv":[{dx},{dy}]}}}}"#,
                    self.time,
                    self.hop_duration
                );
                self.time += self.hop_duration;
                prev = cur;
            }
            self.piece_id.insert(last, *id);
            self.piece_id.remove(&first);
        }
        fn handle_move_str<W: Write>(&mut self, out: &mut W, parts: Vec<&str>) {
            let coords = parts[4..]
                .iter()
                .map(|p| p.parse().unwrap())
                .collect::<Vec<_>>();
            let hops = coords
                .iter()
                .step_by(2)
                .zip(coords.iter().skip(1).step_by(2))
                .map(|(x, y)| Coord::new(*x, *y));
            self.handle_move_hops(out, hops);
        }
        fn format_color((r, g, b, a): (f32, f32, f32, f32)) -> String {
            let u = |c: f32| -> u8 {
                (c.clamp(0., 1.) * 255.) as u8
            };
            format!("{:02x}{:02x}{:02x}{:02x}", u(r), u(g), u(b), u(a))
        }
        pub fn handle_start<W: Write>(&mut self, out: &mut W) {
            for (h, id) in self.piece_id.iter() {
                let p = self.piece_player.get(id).unwrap();
                let position = h.to_pixel(self.quad_size);
                let (px, py) = (position.0, position.1);
                let color = self.player_color.get(*p as usize).unwrap().clone();
                w!(out, r#"vis {{"t":{},"create":{{"id":{id},"z":2,"p":[{px},{py}],"geom":["#, self.time);
                wln!(
                    out,
                    r#"{{"circle":{{"r":{},"f":"{}","t":{}}}}}]}}}}"#,
                    self.quad_size * 0.4,
                    Self::format_color(color),
                    self.quad_size * 0.05
                );
            }
            let mut lines = vec![];

            let x0 = 0.5 - self.quad_size * 8.0;
            let y0 = x0;
            let x1 = 0.5 + self.quad_size * 8.0;
            let y1 = x1;

            for i in 0..=16 {
                let c = x0 + i as f32 * self.quad_size;
                lines.push(((x0, c), (x1, c)));
                lines.push(((c, y0), (c, y1)));
            }
            write_batched_lines(out, 10000, lines, 0.002, "000000ff", self.time);

            let mut lines = vec![];
            lines.push(((x0, y0), (x0 + 5. * self.quad_size, y0)));
            lines.push(((x0, y0), (x0, y0 + 5. * self.quad_size)));
            let mut prev = (x0, y0 + 5. * self.quad_size);
            for i in 0..4 {
                let mut cur = (prev.0 + self.quad_size, prev.1 - self.quad_size);
                if i == 0 {
                    cur.0 += self.quad_size;
                } else if i == 3 {
                    cur.1 -= self.quad_size;
                }
                lines.push((prev, (cur.0, prev.1)));
                lines.push(((cur.0, prev.1), cur));
                prev = cur;
            }
            let inv_lines = lines.iter().copied().map(|((x0, y0), (x1, y1))| ((1.0-x0, 1.0-y0), (1.0-x1, 1.0-y1))).collect::<Vec<_>>();
            write_batched_lines(out, 20000, lines, 0.008, "00ff007f", self.time);
            write_batched_lines(out, 30000, inv_lines, 0.008, "ff0000ff", self.time);

            let mut lines = vec![];
            lines.push(((x0, y0), (x0 + 7. * self.quad_size, y0)));
            lines.push(((x0, y0), (x0, y0 + 7. * self.quad_size)));
            let mut prev = (x0, y0 + 7. * self.quad_size);
            for i in 0..6 {
                let mut cur = (prev.0 + self.quad_size, prev.1 - self.quad_size);
                if i == 0 {
                    cur.0 += self.quad_size;
                } else if i == 5 {
                    cur.1 -= self.quad_size;
                }
                lines.push((prev, (cur.0, prev.1)));
                lines.push(((cur.0, prev.1), cur));
                prev = cur;
            }
            let inv_lines = lines.iter().copied().map(|((x0, y0), (x1, y1))| ((1.0-x0, 1.0-y0), (1.0-x1, 1.0-y1))).collect::<Vec<_>>();
            write_batched_lines(out, 40000, lines, 0.008, "0000007f", self.time);
            write_batched_lines(out, 50000, inv_lines, 0.008, "0000007f", self.time);
        }
    }
    pub fn visualize() {
        let mut stdin = std::io::stdin().lock();
        let mut stdout = std::io::stdout();
        let mut buf = String::new();
        let mut h = VHandler::new();
        loop {
            buf.clear();
            match stdin.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    if !h.handle_line(&mut stdout, &buf) {
                        break;
                    }
                }
                Err(e) => panic!("Failed to read from stdin: {e}"),
            }
        }
    }
    fn write_batched_lines<W: Write>(
        out: &mut W,
        base_id: usize,
        lines: Vec<((f32, f32), (f32, f32))>,
        thickness: f32,
        color: &str,
        time: f32,
    ) {
        const LINES_PER_BATCH: usize = 10;
        for batch in 0..(lines.len() + LINES_PER_BATCH - 1) / LINES_PER_BATCH {
            let batch_start = batch * LINES_PER_BATCH;
            let batch_end = ((batch + 1) * LINES_PER_BATCH).min(lines.len());
            w!(out, r#"vis {{"t":{time},"create":{{"id":{},"geom":["#, base_id + batch);
            for (idx, ((x1, y1), (x2, y2))) in lines[batch_start..batch_end].iter().enumerate() {
                if idx > 0 {
                    w!(out, ",");
                }
                w!(out, r#"{{"line":{{"p1":[{x1},{y1}],"p2":[{x2},{y2}],"t":{thickness},"s":"{color}"}}}}"#);
            }
            wln!(out, "]}}}}");
        }
    }
}
