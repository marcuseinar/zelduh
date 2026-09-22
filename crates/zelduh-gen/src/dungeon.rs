//! Dungeon generation.
//!
//! A dungeon is a grid of walled rooms joined by doorways. One door on the way
//! to the boss is locked, and its key is always placed somewhere you can reach
//! without it — that ordering is checked by a flood fill rather than assumed,
//! because a dungeon you cannot finish is worse than no dungeon at all.

use zelduh_core::entity::Kind;
use zelduh_core::geom::{Dir, V2};
use zelduh_core::level::{Level, LevelKind, Room, RoomKind, Spawn, ROOM_H, ROOM_W, TILE_PX};
use zelduh_core::rng::Rng;
use zelduh_core::tiles::{self, flag, tile};

use crate::graph::{carve, RoomGraph};

/// A generated dungeon floor.
pub struct Dungeon {
    pub level: Level,
    pub graph: RoomGraph,
    /// Tile coordinates of the staircase back out.
    pub exit: (i32, i32),
    /// Where an arriving hero should appear.
    pub arrival: V2,
}

/// Generates dungeon number `index`.
pub fn generate(seed: u64, index: u8, rooms_w: i32, rooms_h: i32) -> Dungeon {
    let mut rng = Rng::new(seed ^ 0xd0_0d_0000_0000 ^ (index as u64) << 32);
    let mut level = Level::new(LevelKind::Dungeon, rooms_w, rooms_h, tile::WALL_DUNGEON);
    level.dungeon = index + 1;

    // Enter from the bottom middle, the way a dungeon usually opens.
    let start = (rooms_w / 2, rooms_h - 1);
    let graph = carve(&mut rng, rooms_w, rooms_h, start, (rooms_w * rooms_h / 6) as u32);

    for ry in 0..rooms_h {
        for rx in 0..rooms_w {
            carve_room(&mut level, rx, ry);
        }
    }
    for ry in 0..rooms_h {
        for rx in 0..rooms_w {
            for dir in graph.exits(rx, ry) {
                if matches!(dir, Dir::Up | Dir::Left) {
                    continue;
                }
                open_doorway(&mut level, rx, ry, dir, tile::FLOOR);
            }
        }
    }

    let boss_room = graph.deepest();
    let path = graph.path_to(boss_room);

    // Lock the last door before the boss, and guard the boss room itself.
    let locked = lock_a_door(&mut level, &graph, &path);
    if let Some((rx, ry, dir)) = door_into(&graph, &path, boss_room) {
        open_doorway(&mut level, rx, ry, dir, tile::DOOR_BOSS);
    }

    let mut level = level;
    let exit = place_exit(&mut level, start);
    let arrival = V2::from_px(
        exit.0 * TILE_PX + TILE_PX / 2,
        exit.1 * TILE_PX + TILE_PX / 2 + TILE_PX,
    );
    level.entrance = arrival;

    decorate(&mut level, &graph, &mut rng, start, boss_room);
    populate(
        &mut level,
        &graph,
        &mut rng,
        start,
        boss_room,
        locked,
        index,
    );

    for ry in 0..rooms_h {
        for rx in 0..rooms_w {
            let kind = if (rx, ry) == start {
                RoomKind::Start
            } else if (rx, ry) == boss_room {
                RoomKind::Boss
            } else {
                RoomKind::Normal
            };
            if let Some(r) = level.room_mut(rx, ry) {
                *r = Room {
                    kind,
                    region: index,
                    visited: false,
                    cleared: false,
                    exits: Dir::ALL
                        .into_iter()
                        .filter(|d| graph.linked(rx, ry, *d))
                        .fold(0u8, |acc, d| acc | 1 << d as u8),
                };
            }
        }
    }

    Dungeon {
        level,
        graph,
        exit,
        arrival,
    }
}

/// Hollows out one room, leaving a one-tile wall all round.
fn carve_room(level: &mut Level, rx: i32, ry: i32) {
    let x = rx * ROOM_W;
    let y = ry * ROOM_H;
    level
        .map
        .fill_rect(x + 1, y + 1, ROOM_W - 2, ROOM_H - 2, tile::FLOOR);
}

/// Opens (or blocks) the doorway between a room and its neighbour.
fn open_doorway(level: &mut Level, rx: i32, ry: i32, dir: Dir, with: u8) {
    let x = rx * ROOM_W;
    let y = ry * ROOM_H;
    match dir {
        Dir::Right => {
            for i in 0..2 {
                level.map.set(x + ROOM_W - 1, y + 3 + i, with);
                level.map.set(x + ROOM_W, y + 3 + i, with);
            }
        }
        Dir::Left => {
            for i in 0..2 {
                level.map.set(x, y + 3 + i, with);
                level.map.set(x - 1, y + 3 + i, with);
            }
        }
        Dir::Down => {
            for i in 0..2 {
                level.map.set(x + 4 + i, y + ROOM_H - 1, with);
                level.map.set(x + 4 + i, y + ROOM_H, with);
            }
        }
        Dir::Up => {
            for i in 0..2 {
                level.map.set(x + 4 + i, y, with);
                level.map.set(x + 4 + i, y - 1, with);
            }
        }
    }
}

/// The room and direction of the door leading into `target` along the path.
fn door_into(
    _graph: &RoomGraph,
    path: &[(i32, i32)],
    target: (i32, i32),
) -> Option<(i32, i32, Dir)> {
    let pos = path.iter().position(|r| *r == target)?;
    if pos == 0 {
        return None;
    }
    let from = path[pos - 1];
    let (dx, dy) = (target.0 - from.0, target.1 - from.1);
    let dir = match (dx, dy) {
        (1, 0) => Dir::Right,
        (-1, 0) => Dir::Left,
        (0, 1) => Dir::Down,
        (0, -1) => Dir::Up,
        _ => return None,
    };
    Some((from.0, from.1, dir))
}

/// Locks one door partway along the route to the boss.
///
/// Returns the depth at which the lock sits, so the key can be placed in a
/// room shallower than that.
fn lock_a_door(level: &mut Level, graph: &RoomGraph, path: &[(i32, i32)]) -> Option<i32> {
    if path.len() < 3 {
        return None;
    }
    // Two thirds of the way in: far enough to matter, near enough that the key
    // hunt does not span the whole floor.
    let at = (path.len() * 2 / 3).clamp(1, path.len() - 2);
    let room = path[at];
    let (rx, ry, dir) = door_into(graph, path, room)?;
    open_doorway(level, rx, ry, dir, tile::DOOR_LOCKED);
    Some(graph.depth_at(room.0, room.1))
}

/// Puts the staircase out in the starting room.
fn place_exit(level: &mut Level, start: (i32, i32)) -> (i32, i32) {
    let tx = start.0 * ROOM_W + ROOM_W / 2;
    let ty = start.1 * ROOM_H + 2;
    level.map.set(tx, ty, tile::STAIRS_UP);
    (tx, ty)
}

/// True when a tile lies on the cross of corridors joining a room's four
/// doorways. Nothing is ever placed here, which is what keeps a decorated
/// room walkable no matter what the dice say.
fn is_corridor(rx: i32, ry: i32, tx: i32, ty: i32) -> bool {
    let x = rx * ROOM_W;
    let y = ry * ROOM_H;
    (tx == x + 4 || tx == x + 5) || (ty == y + 3 || ty == y + 4)
}

/// Places a decoration, unless it would stand in a doorway corridor.
fn decorate_tile(level: &mut Level, rx: i32, ry: i32, tx: i32, ty: i32, t: u8) {
    if is_corridor(rx, ry, tx, ty) {
        return;
    }
    level.map.set(tx, ty, t);
}

/// Adds pots, blocks and the occasional pit.
fn decorate(level: &mut Level, graph: &RoomGraph, rng: &mut Rng, start: (i32, i32), boss: (i32, i32)) {
    for ry in 0..graph.h {
        for rx in 0..graph.w {
            if (rx, ry) == start {
                continue;
            }
            let x = rx * ROOM_W;
            let y = ry * ROOM_H;
            // The boss needs a clear floor to charge around on.
            if (rx, ry) == boss {
                continue;
            }
            match rng.below(6) {
                0 => {
                    // Pits either side of the middle, with the corridors left
                    // open so the room is still crossable on foot.
                    for i in 0..5 {
                        decorate_tile(level, rx, ry, x + 2 + i, y + 2, tile::PIT);
                        decorate_tile(level, rx, ry, x + 2 + i, y + ROOM_H - 3, tile::PIT);
                    }
                }
                1 => {
                    // Pushable blocks in the corners.
                    for (dx, dy) in [
                        (2, 2),
                        (ROOM_W - 3, 2),
                        (2, ROOM_H - 3),
                        (ROOM_W - 3, ROOM_H - 3),
                    ] {
                        decorate_tile(level, rx, ry, x + dx, y + dy, tile::BLOCK);
                    }
                }
                2 => {
                    for _ in 0..3 {
                        let tx = x + rng.range(2, ROOM_W - 3);
                        let ty = y + rng.range(2, ROOM_H - 3);
                        decorate_tile(level, rx, ry, tx, ty, tile::POT);
                    }
                }
                3 => {
                    // A crack in an outer wall, worth a bomb.
                    let side = rng.below(2);
                    let (tx, ty) = if side == 0 {
                        (x + rng.range(2, ROOM_W - 3), y)
                    } else {
                        (x, y + rng.range(2, ROOM_H - 3))
                    };
                    if level.map.get(tx, ty) == tile::WALL_DUNGEON {
                        level.map.set(tx, ty, tile::WALL_CRACKED);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Places monsters, the boss, the key and the treasure.
fn populate(
    level: &mut Level,
    graph: &RoomGraph,
    rng: &mut Rng,
    start: (i32, i32),
    boss: (i32, i32),
    lock_depth: Option<i32>,
    index: u8,
) {
    // The boss, in the deepest room.
    if let Some(room_index) = level.room_index(boss.0, boss.1) {
        let c = (boss.0 * ROOM_W + ROOM_W / 2, boss.1 * ROOM_H + ROOM_H / 2);
        level.spawns.push(Spawn {
            room: room_index as u16,
            kind: Kind::Boss as u8,
            tx: c.0,
            ty: c.1,
            param: index as i32,
        });
    }

    // The key goes in a room you can reach before the locked door.
    if let Some(depth) = lock_depth {
        let mut candidates: Vec<(i32, i32)> = Vec::new();
        for ry in 0..graph.h {
            for rx in 0..graph.w {
                let d = graph.depth_at(rx, ry);
                if d >= 0 && d < depth && (rx, ry) != start {
                    candidates.push((rx, ry));
                }
            }
        }
        if candidates.is_empty() {
            candidates.push(start);
        }
        rng.shuffle(&mut candidates);
        let room = candidates[0];
        if let Some(room_index) = level.room_index(room.0, room.1) {
            let c = (room.0 * ROOM_W + ROOM_W / 2, room.1 * ROOM_H + 3);
            if let Some((tx, ty)) = level.find_free_near(c.0, c.1, 3) {
                level.spawns.push(Spawn {
                    room: room_index as u16,
                    kind: Kind::Chest as u8,
                    tx,
                    ty,
                    // A chest holding a small key.
                    param: 9,
                });
            }
        }
    }

    // One treasure chest with a real item, in a dead end if there is one.
    let mut leaves: Vec<(i32, i32)> = Vec::new();
    for ry in 0..graph.h {
        for rx in 0..graph.w {
            if graph.exits(rx, ry).len() == 1 && (rx, ry) != start && (rx, ry) != boss {
                leaves.push((rx, ry));
            }
        }
    }
    if leaves.is_empty() {
        leaves.push(boss);
    }
    rng.shuffle(&mut leaves);
    let treasure_room = leaves[0];
    if let Some(room_index) = level.room_index(treasure_room.0, treasure_room.1) {
        let c = (
            treasure_room.0 * ROOM_W + ROOM_W / 2,
            treasure_room.1 * ROOM_H + ROOM_H / 2,
        );
        if let Some((tx, ty)) = level.find_free_near(c.0, c.1, 3) {
            level.spawns.push(Spawn {
                room: room_index as u16,
                kind: Kind::Chest as u8,
                tx,
                ty,
                // Each dungeon holds a different item.
                param: 1 + (index as i32 % 8),
            });
        }
    }

    // Monsters everywhere but the entrance.
    for ry in 0..graph.h {
        for rx in 0..graph.w {
            if (rx, ry) == start || (rx, ry) == boss {
                continue;
            }
            let Some(room_index) = level.room_index(rx, ry) else {
                continue;
            };
            let count = rng.range(1, 3 + index.min(3) as i32);
            for _ in 0..count {
                let tx = rx * ROOM_W + rng.range(2, ROOM_W - 3);
                let ty = ry * ROOM_H + rng.range(2, ROOM_H - 3);
                let Some((tx, ty)) = level.find_free_near(tx, ty, 3) else {
                    continue;
                };
                let kind = match rng.below(100) + index as u32 * 8 {
                    0..=25 => Kind::Keese,
                    26..=45 => Kind::Zol,
                    46..=65 => Kind::Octorok,
                    66..=85 => Kind::Moblin,
                    86..=105 => Kind::Tektite,
                    _ => Kind::Stalfos,
                };
                level.spawns.push(Spawn {
                    room: room_index as u16,
                    kind: kind as u8,
                    tx,
                    ty,
                    param: 0,
                });
            }
        }
    }
}

/// Flood fill that treats locked doors as walls, for checking that a dungeon
/// can actually be finished.
pub fn reachable_without_keys(level: &Level, from: (i32, i32)) -> Vec<bool> {
    let (w, h) = (level.map.w(), level.map.h());
    let mut seen = vec![false; (w * h) as usize];
    let passable = |t: u8| {
        !tiles::blocks_walk(t) && !tiles::any(t, flag::WATER | flag::HARMFUL | flag::PIT)
    };
    if !level.map.in_bounds(from.0, from.1) {
        return seen;
    }
    let mut queue = vec![from];
    seen[(from.1 * w + from.0) as usize] = true;
    let mut head = 0;
    while head < queue.len() {
        let (x, y) = queue[head];
        head += 1;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if !level.map.in_bounds(nx, ny) {
                continue;
            }
            let i = (ny * w + nx) as usize;
            if seen[i] || !passable(level.map.get(nx, ny)) {
                continue;
            }
            seen[i] = true;
            queue.push((nx, ny));
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_are_walled_and_hollow() {
        let d = generate(1, 0, 4, 3);
        // The very corner of a room is always wall.
        assert!(tiles::blocks_walk(d.level.map.get(0, 0)));
        // The middle of a room is always floor.
        let c = (ROOM_W / 2, ROOM_H / 2);
        assert!(!tiles::blocks_walk(d.level.map.get(c.0, c.1)));
    }

    #[test]
    fn the_key_can_always_be_got_before_the_lock() {
        for seed in 0..30u64 {
            let d = generate(seed, 0, 4, 4);
            let has_lock = d
                .level
                .map
                .raw()
                .iter()
                .any(|t| *t == tile::DOOR_LOCKED);
            if !has_lock {
                continue;
            }
            let seen = reachable_without_keys(&d.level, d.exit);
            let w = d.level.map.w();
            let key = d
                .level
                .spawns
                .iter()
                .find(|s| s.kind == Kind::Chest as u8 && s.param == 9);
            let key = key.unwrap_or_else(|| panic!("seed {seed}: a locked door with no key"));
            assert!(
                seen[(key.ty * w + key.tx) as usize],
                "seed {seed}: the key is behind its own lock"
            );
        }
    }

    #[test]
    fn there_is_exactly_one_boss() {
        let d = generate(5, 1, 4, 4);
        let bosses: Vec<_> = d
            .level
            .spawns
            .iter()
            .filter(|s| s.kind == Kind::Boss as u8)
            .collect();
        assert_eq!(bosses.len(), 1);
    }

    #[test]
    fn the_boss_door_is_locked() {
        let mut found = 0;
        for seed in 0..10u64 {
            let d = generate(seed, 0, 4, 4);
            if d.level.map.raw().iter().any(|t| *t == tile::DOOR_BOSS) {
                found += 1;
            }
        }
        assert!(found >= 8, "most dungeons should gate their boss: {found}/10");
    }

    #[test]
    fn the_way_out_is_a_staircase() {
        let d = generate(2, 0, 3, 3);
        assert_eq!(d.level.map.get(d.exit.0, d.exit.1), tile::STAIRS_UP);
        assert!(!tiles::blocks_walk(d.level.map.at_world(d.arrival)));
    }

    #[test]
    fn nothing_spawns_inside_a_wall() {
        for seed in 0..10u64 {
            let d = generate(seed, 0, 4, 4);
            for s in &d.level.spawns {
                assert!(
                    !tiles::blocks_walk(d.level.map.get(s.tx, s.ty)),
                    "seed {seed}: spawn inside a wall at {},{}",
                    s.tx,
                    s.ty
                );
            }
        }
    }

    #[test]
    fn every_room_is_reachable_once_you_hold_the_keys() {
        for seed in 0..10u64 {
            let mut d = generate(seed, 0, 4, 4);
            // Unlock every door, then check the whole floor opens up.
            let (w, h) = (d.level.map.w(), d.level.map.h());
            for ty in 0..h {
                for tx in 0..w {
                    let t = d.level.map.get(tx, ty);
                    if t == tile::DOOR_LOCKED || t == tile::DOOR_BOSS {
                        d.level.map.set(tx, ty, tile::FLOOR);
                    }
                }
            }
            let seen = reachable_without_keys(&d.level, d.exit);
            for ry in 0..d.level.rooms_h() {
                for rx in 0..d.level.rooms_w() {
                    let c = (rx * ROOM_W + ROOM_W / 2, ry * ROOM_H + ROOM_H / 2);
                    assert!(
                        seen[(c.1 * w + c.0) as usize],
                        "seed {seed}: room {rx},{ry} is walled off"
                    );
                }
            }
        }
    }

    #[test]
    fn dungeons_differ_from_one_another() {
        let a = generate(3, 0, 4, 4);
        let b = generate(3, 1, 4, 4);
        assert_ne!(a.level.map.raw(), b.level.map.raw());
    }
}
