use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, Write},
};

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
    fn distance_m(&self, c: Coord) -> i32 {
        (self.x - c.x).abs() + (self.y - c.y).abs()
    }
}

fn greedy_best_move(game: &Game) -> Vec<Coord> {
    let targ_corner = if game.current_player == 1 {
        Coord::new(16, 16)
    } else {
        Coord::new(1, 1)
    };
    let mut targ = targ_corner;
    for (h, c) in game.map.iter() {
        if c.get_home() == Some(game.current_player)
            && c.get_player().is_none()
            && targ.distance(targ_corner) > h.distance(targ_corner)
        {
            targ = *h;
        }
    }
    let distance_to_cost = |distance: f32| -> f32 { distance.powf(6.) };
    let mut scores_and_moves = game.possible_moves()
        .into_iter()
        .map(|v| {
            let from = v.first().unwrap();
            let to = v.last().unwrap();
            let mut new_c = distance_to_cost(to.distance_m(targ) as f32);
            let mut old_c = distance_to_cost(from.distance_m(targ) as f32);
            if game.map.get(from).unwrap().get_home() == Some(game.current_player) {
                // It's actually not easy to make this greedy player
                // complete the game.
                // This disincentivises moving pieces that are already at home.
                old_c /= 10.;
            }
            if game.map.get(to).unwrap().get_home() == Some(game.current_player) {
                new_c /= 10.;
            }
            (old_c - new_c, v)
        })
        .collect::<Vec<_>>();
    let (best_score, _) = scores_and_moves.iter().max_by(
        |x, y| x.partial_cmp(y).unwrap()).unwrap();
    let best_score = *best_score;
    scores_and_moves.retain(|(score, _)| *score == best_score);
    let i = (calculate_hash(&game.turn) % (scores_and_moves.len() as u64)) as usize;
    scores_and_moves[i].1.clone()
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut game = Game {
        map: create_map(),
        current_player: 0,
        turn: 1,
    };
    writeln!(&stdout, "ready").unwrap();
    stdout.flush().unwrap();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let mut it = line.split_whitespace();
        let cmd = it.next().unwrap();
        let args = it.map(|x| x.parse::<i32>().unwrap()).collect::<Vec<_>>();
        match cmd {
            "start" => game.current_player = args[0] as u8,
            "yourmove" => {
                let m = greedy_best_move(&game);
                write!(&mut stdout, "move").unwrap();
                for x in m {
                    write!(&mut stdout, " {} {}", x.x, x.y).unwrap();
                }
                writeln!(&mut stdout).unwrap();
                stdout.flush().unwrap();
            }
            "move" => {
                let from = Coord::new(args[0], args[1]);
                let to = Coord::new(args[args.len() - 2], args[args.len() - 1]);
                let c_from = game.map.get_mut(&from).unwrap();
                let p = c_from.get_player();
                c_from.set_player(None);
                game.map.get_mut(&to).unwrap().set_player(p);
                game.turn += 1;
            }
            "over" => {
                break;
            }
            _ => panic!("Unknown command: {:?}", cmd),
        }
    }
}

#[derive(Debug)]
struct Game {
    map: Map,
    current_player: u8,
    turn: usize,
}

impl Game {
    fn possible_moves(&self) -> Vec<Vec<Coord>> {
        let mut result = vec![];
        for (&pos, cell) in self.map.iter() {
            if cell.get_player() != Some(self.current_player) {
                continue;
            }
            let mut seen = HashSet::<Coord>::new();
            seen.insert(pos);

            for d in Direction::all() {
                let new = pos + Coord::from(d);
                if self.cell_free(new) {
                    result.push(vec![pos, new]);
                }
            }
            let mut stack = vec![(pos, 0)];
            let mut path = vec![pos];
            while let Some((top, depth)) = stack.pop() {
                path.truncate(depth);
                path.push(top);
                if path.len() > 1 {
                    result.push(path.clone());
                }
                for d in Direction::all() {
                    let mid = top + Coord::from(d);
                    if !self.cell_free(mid) {
                        let new = mid + Coord::from(d);
                        if !seen.contains(&new) && self.cell_free(new) {
                            stack.push((new, depth + 1));
                            seen.insert(new);
                        }
                    }
                }
            }
        }
        result
    }

    fn cell_free(&self, pos: Coord) -> bool {
        self.map
            .get(&pos)
            .map_or(false, |h| h.get_player().is_none())
    }
}

type Map = HashMap<Coord, Cell>;

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

#[derive(Copy, Clone, Default, Debug)]
struct Direction(u8);

impl From<Direction> for Coord {
    fn from(value: Direction) -> Self {
        match value.0 {
            0 => Coord::new(1, 0),
            1 => Coord::new(1, 1),
            2 => Coord::new(0, 1),
            3 => Coord::new(-1, 1),
            4 => Coord::new(-1, 0),
            5 => Coord::new(-1, -1),
            6 => Coord::new(0, -1),
            7 => Coord::new(1, -1),
            _ => panic!(),
        }
    }
}

impl Direction {
    fn new(x: i32) -> Self {
        Self(x.rem_euclid(8) as u8)
    }
    fn all() -> impl Iterator<Item = Self> {
        (0..8).map(Self::new)
    }
}


fn create_map() -> Map {
    let side_size = 16;
    let zero = Coord::new(0, 0);
    let mut m = Map::new();
    for x in 1..=side_size {
        for y in 1..=side_size {
            m.insert(Coord::new(x, y), Cell::new());
        }
    }
    for x in 1..=5 {
        for y in 1..=(7-x).min(5) {
            let c = Coord::new(x, y);
            m.get_mut(&c).unwrap().set_home(Some(2));
            m.get_mut(&c).unwrap().set_player(Some(1));

            let c = Coord::new(side_size + 1 - x, side_size + 1 - y);

            m.get_mut(&c).unwrap().set_home(Some(1));
            m.get_mut(&c).unwrap().set_player(Some(2));
        }
    }
    for x in 1..=7 {
        for y in 1..=(9-x).min(7) {
            m.get_mut(&Coord::new(x, y)).unwrap().set_block_zone(Some(2));
            let c = Coord::new(side_size + 1 - x, side_size + 1 - y);
            m.get_mut(&c).unwrap().set_block_zone(Some(1));
        }
    }
    m
}

use std::hash::Hasher;
fn calculate_hash<T: std::hash::Hash>(t: &T) -> u64 {
    let mut s = std::hash::DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}
