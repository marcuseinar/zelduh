//! Room graphs: the skeleton both the overworld and dungeons are built on.

use zelduh_core::geom::Dir;
use zelduh_core::rng::Rng;

/// A grid of rooms with connections between neighbours.
#[derive(Clone, Debug)]
pub struct RoomGraph {
    pub w: i32,
    pub h: i32,
    /// Bit per [`Dir`] for each room: a connection leads that way.
    links: Vec<u8>,
    /// Distance from the start room along the connections.
    pub depth: Vec<i32>,
    pub start: (i32, i32),
}

impl RoomGraph {
    pub fn new(w: i32, h: i32) -> RoomGraph {
        let w = w.max(1);
        let h = h.max(1);
        RoomGraph {
            w,
            h,
            links: vec![0; (w * h) as usize],
            depth: vec![-1; (w * h) as usize],
            start: (0, 0),
        }
    }

    #[inline]
    pub fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some((y * self.w + x) as usize)
        }
    }

    /// True when the two adjacent rooms are connected.
    pub fn linked(&self, x: i32, y: i32, dir: Dir) -> bool {
        self.index(x, y)
            .map(|i| self.links[i] & (1 << dir as u8) != 0)
            .unwrap_or(false)
    }

    /// Connects two adjacent rooms, in both directions.
    pub fn link(&mut self, x: i32, y: i32, dir: Dir) {
        let (dx, dy) = dir.step();
        let (nx, ny) = (x + dx, y + dy);
        if let (Some(a), Some(b)) = (self.index(x, y), self.index(nx, ny)) {
            self.links[a] |= 1 << dir as u8;
            self.links[b] |= 1 << dir.opposite() as u8;
        }
    }

    /// All connections out of a room.
    pub fn exits(&self, x: i32, y: i32) -> Vec<Dir> {
        Dir::ALL
            .into_iter()
            .filter(|d| self.linked(x, y, *d))
            .collect()
    }

    /// Distance from the start room, or -1 when unreachable.
    pub fn depth_at(&self, x: i32, y: i32) -> i32 {
        self.index(x, y).map(|i| self.depth[i]).unwrap_or(-1)
    }

    /// The room furthest from the start, which is where a boss belongs.
    pub fn deepest(&self) -> (i32, i32) {
        let mut best = (self.start, -1);
        for y in 0..self.h {
            for x in 0..self.w {
                let d = self.depth_at(x, y);
                if d > best.1 {
                    best = ((x, y), d);
                }
            }
        }
        best.0
    }

    /// Recomputes [`RoomGraph::depth`] by breadth-first search from the start.
    pub fn recompute_depth(&mut self) {
        self.depth.iter_mut().for_each(|d| *d = -1);
        let Some(start) = self.index(self.start.0, self.start.1) else {
            return;
        };
        self.depth[start] = 0;
        let mut queue = vec![self.start];
        let mut head = 0;
        while head < queue.len() {
            let (x, y) = queue[head];
            head += 1;
            let d = self.depth_at(x, y);
            for dir in self.exits(x, y) {
                let (dx, dy) = dir.step();
                let (nx, ny) = (x + dx, y + dy);
                if let Some(i) = self.index(nx, ny) {
                    if self.depth[i] < 0 {
                        self.depth[i] = d + 1;
                        queue.push((nx, ny));
                    }
                }
            }
        }
    }

    /// The path from the start room to a room, following decreasing depth.
    pub fn path_to(&self, to: (i32, i32)) -> Vec<(i32, i32)> {
        let mut path = Vec::new();
        let mut cur = to;
        if self.depth_at(cur.0, cur.1) < 0 {
            return path;
        }
        path.push(cur);
        while cur != self.start {
            let d = self.depth_at(cur.0, cur.1);
            let mut moved = false;
            for dir in self.exits(cur.0, cur.1) {
                let (dx, dy) = dir.step();
                let n = (cur.0 + dx, cur.1 + dy);
                if self.depth_at(n.0, n.1) == d - 1 {
                    cur = n;
                    path.push(cur);
                    moved = true;
                    break;
                }
            }
            if !moved {
                break;
            }
        }
        path.reverse();
        path
    }

    /// True when every room can be reached from the start.
    pub fn is_fully_connected(&self) -> bool {
        self.depth.iter().all(|d| *d >= 0)
    }
}

/// Grows a spanning tree over the room grid by randomised depth-first search,
/// then adds a few extra links so the map has loops rather than being a pure
/// tree, which makes for better exploration.
pub fn carve(rng: &mut Rng, w: i32, h: i32, start: (i32, i32), extra_links: u32) -> RoomGraph {
    let mut g = RoomGraph::new(w, h);
    g.start = (start.0.clamp(0, w - 1), start.1.clamp(0, h - 1));

    let mut visited = vec![false; (g.w * g.h) as usize];
    let mut stack = vec![g.start];
    visited[g.index(g.start.0, g.start.1).unwrap()] = true;

    while let Some(&(x, y)) = stack.last() {
        let mut options: Vec<Dir> = Dir::ALL
            .into_iter()
            .filter(|d| {
                let (dx, dy) = d.step();
                g.index(x + dx, y + dy)
                    .map(|i| !visited[i])
                    .unwrap_or(false)
            })
            .collect();
        if options.is_empty() {
            stack.pop();
            continue;
        }
        rng.shuffle(&mut options);
        let dir = options[0];
        let (dx, dy) = dir.step();
        g.link(x, y, dir);
        let n = (x + dx, y + dy);
        visited[g.index(n.0, n.1).unwrap()] = true;
        stack.push(n);
    }

    for _ in 0..extra_links {
        let x = rng.below(g.w as u32) as i32;
        let y = rng.below(g.h as u32) as i32;
        let dir = Dir::from_u8(rng.below(4) as u8);
        let (dx, dy) = dir.step();
        if g.index(x + dx, y + dy).is_some() {
            g.link(x, y, dir);
        }
    }

    g.recompute_depth();
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carving_connects_every_room() {
        for seed in 0..25u64 {
            let mut rng = Rng::new(seed);
            let g = carve(&mut rng, 6, 5, (0, 0), 3);
            assert!(g.is_fully_connected(), "seed {seed} left a room stranded");
        }
    }

    #[test]
    fn links_are_symmetric() {
        let mut rng = Rng::new(4);
        let g = carve(&mut rng, 4, 4, (1, 1), 2);
        for y in 0..g.h {
            for x in 0..g.w {
                for d in g.exits(x, y) {
                    let (dx, dy) = d.step();
                    assert!(
                        g.linked(x + dx, y + dy, d.opposite()),
                        "one-way link at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_deepest_room_is_far_from_the_start() {
        let mut rng = Rng::new(11);
        let g = carve(&mut rng, 5, 5, (0, 0), 0);
        let deep = g.deepest();
        assert!(g.depth_at(deep.0, deep.1) >= 4, "a 5x5 maze should be deep");
    }

    #[test]
    fn a_path_runs_from_the_start_to_its_target() {
        let mut rng = Rng::new(21);
        let g = carve(&mut rng, 5, 4, (2, 2), 1);
        let target = g.deepest();
        let path = g.path_to(target);
        assert_eq!(path.first(), Some(&g.start));
        assert_eq!(path.last(), Some(&target));
        // Consecutive rooms must be neighbours.
        for w in path.windows(2) {
            let d = (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs();
            assert_eq!(d, 1);
        }
    }

    #[test]
    fn a_single_room_graph_is_valid() {
        let mut rng = Rng::new(1);
        let g = carve(&mut rng, 1, 1, (0, 0), 5);
        assert!(g.is_fully_connected());
        assert_eq!(g.deepest(), (0, 0));
    }

    #[test]
    fn carving_is_deterministic() {
        let a = carve(&mut Rng::new(99), 6, 6, (0, 0), 4);
        let b = carve(&mut Rng::new(99), 6, 6, (0, 0), 4);
        assert_eq!(a.depth, b.depth);
    }
}
