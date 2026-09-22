//! Overworld generation.
//!
//! The land itself comes from noise, so coastlines and forests look like they
//! grew rather than being laid out on a grid. Getting around is guaranteed
//! separately: a room graph decides which screens connect, and paths are then
//! carved along those connections, bridging water and cutting through trees.
//! Terrain is free to be rugged because the paths are not negotiable.

use zelduh_core::entity::Kind;
use zelduh_core::geom::{Dir, V2};
use zelduh_core::level::{Level, LevelKind, Room, RoomKind, Spawn, ROOM_H, ROOM_W, TILE_PX};
use zelduh_core::rng::Rng;
use zelduh_core::tiles::{self, flag, tile, Tile};

use crate::graph::{carve, RoomGraph};
use crate::noise;

/// Where a staircase leads out of the overworld.
#[derive(Clone, Copy, Debug)]
pub struct Entrance {
    /// Tile coordinates of the staircase.
    pub tx: i32,
    pub ty: i32,
    /// Which room it sits in.
    pub room: (i32, i32),
}

/// A generated overworld and the staircases waiting to be hooked up.
pub struct Overworld {
    pub level: Level,
    pub graph: RoomGraph,
    pub dungeon_entrances: Vec<Entrance>,
}

/// Generates an overworld of `rooms_w` by `rooms_h` screens.
pub fn generate(seed: u64, rooms_w: i32, rooms_h: i32, dungeons: usize) -> Overworld {
    let mut rng = Rng::new(seed ^ OVERWORLD_TAG);
    let mut level = Level::new(LevelKind::Overworld, rooms_w, rooms_h, tile::GRASS);

    paint_terrain(&mut level, seed);

    // The hero starts in the middle, which keeps the first walk in any
    // direction interesting.
    let start = (rooms_w / 2, rooms_h / 2);
    let graph = carve(&mut rng, rooms_w, rooms_h, start, (rooms_w * rooms_h / 4) as u32);
    carve_paths(&mut level, &graph, seed);

    // Flatten the starting screen so nobody wakes up inside a lake.
    clear_area(&mut level, room_center_tile(start), 3);
    let entrance_tile = room_center_tile(start);
    level.entrance = V2::from_px(
        entrance_tile.0 * TILE_PX + TILE_PX / 2,
        entrance_tile.1 * TILE_PX + TILE_PX / 2,
    );

    let dungeon_entrances = place_dungeon_entrances(&mut level, &graph, &mut rng, dungeons, start);
    decorate(&mut level, &mut rng, seed);
    populate(&mut level, &graph, &mut rng, start);

    for y in 0..rooms_h {
        for x in 0..rooms_w {
            if let Some(r) = level.room_mut(x, y) {
                *r = Room {
                    kind: if (x, y) == start {
                        RoomKind::Start
                    } else {
                        RoomKind::Normal
                    },
                    region: 0,
                    visited: false,
                    cleared: false,
                    exits: Dir::ALL
                        .into_iter()
                        .filter(|d| graph.linked(x, y, *d))
                        .fold(0u8, |acc, d| acc | 1 << d as u8),
                };
            }
        }
    }

    Overworld {
        level,
        graph,
        dungeon_entrances,
    }
}

/// Mixed into the seed so the overworld and the dungeons draw different
/// numbers from the same world seed.
const OVERWORLD_TAG: u64 = 0xa1b2_c3d4_e5f6_0717;

/// Centre tile of a room.
pub fn room_center_tile(room: (i32, i32)) -> (i32, i32) {
    (room.0 * ROOM_W + ROOM_W / 2, room.1 * ROOM_H + ROOM_H / 2)
}

/// Paints terrain from elevation and moisture noise.
fn paint_terrain(level: &mut Level, seed: u64) {
    let (w, h) = (level.map.w(), level.map.h());
    for ty in 0..h {
        for tx in 0..w {
            let elev = noise::fractal(seed ^ 0x11, tx, ty, 22);
            let moist = noise::fractal(seed ^ 0x22, tx, ty, 15);
            let detail = noise::value(seed ^ 0x33, tx, ty, 3);

            // The map is ringed by water so the world has an edge.
            let edge = (tx.min(w - 1 - tx)).min(ty.min(h - 1 - ty));
            let elev = if edge < 3 { elev - (3 - edge) * 260 } else { elev };

            // Elevation decides sea, shore and mountain; moisture decides what
            // grows in between, with the detail octave breaking up the edges so
            // biomes interlock rather than sitting in clean bands.
            let t = if elev < 250 {
                tile::WATER
            } else if elev < 305 {
                tile::WATER_SHALLOW
            } else if elev < 345 {
                tile::SAND
            } else if elev > 820 {
                tile::CLIFF
            } else if elev > 770 {
                tile::ROCK
            } else if moist > 660 {
                // Woodland, with clearings where the detail noise dips.
                if detail > 280 {
                    tile::TREE
                } else {
                    tile::GRASS_TALL
                }
            } else if moist > 520 {
                if detail > 600 {
                    tile::GRASS_TALL
                } else if detail < 60 {
                    tile::BUSH
                } else {
                    tile::GRASS
                }
            } else if moist < 300 {
                // Dry ground: patchy sand rather than an unbroken desert.
                if detail > 520 {
                    tile::SAND
                } else if detail < 70 {
                    tile::ROCK
                } else {
                    tile::GRASS
                }
            } else if (820..870).contains(&detail) {
                tile::FLOWERS
            } else if detail > 960 {
                tile::BUSH
            } else if detail < 40 {
                tile::ROCK
            } else {
                tile::GRASS
            };
            level.map.set(tx, ty, t);
        }
    }
}

/// Cuts walkable paths along every connection in the room graph.
fn carve_paths(level: &mut Level, graph: &RoomGraph, seed: u64) {
    for ry in 0..graph.h {
        for rx in 0..graph.w {
            let from = room_center_tile((rx, ry));
            // Widen the middle of each room so there is somewhere to stand.
            clear_area(level, from, 2);
            for dir in graph.exits(rx, ry) {
                // Only carve each connection once.
                if matches!(dir, Dir::Up | Dir::Left) {
                    continue;
                }
                let (dx, dy) = dir.step();
                let to = room_center_tile((rx + dx, ry + dy));
                // Bend the corridor at a point that varies per connection, so
                // paths do not all meet in straight lines.
                let bend = noise::value(seed ^ 0x44, rx * 7 + dx, ry * 7 + dy, 2) % 2 == 0;
                carve_corridor(level, from, to, bend);
            }
        }
    }
}

/// Carves an L-shaped path two tiles wide between two points.
fn carve_corridor(level: &mut Level, from: (i32, i32), to: (i32, i32), horizontal_first: bool) {
    let corner = if horizontal_first {
        (to.0, from.1)
    } else {
        (from.0, to.1)
    };
    carve_line(level, from, corner);
    carve_line(level, corner, to);
}

fn carve_line(level: &mut Level, a: (i32, i32), b: (i32, i32)) {
    let (mut x, mut y) = a;
    loop {
        for oy in 0..2 {
            for ox in 0..2 {
                path_tile(level, x + ox, y + oy);
            }
        }
        if (x, y) == b {
            break;
        }
        if x != b.0 {
            x += (b.0 - x).signum();
        } else if y != b.1 {
            y += (b.1 - y).signum();
        } else {
            break;
        }
    }
}

/// Lays one tile of path, bridging water rather than draining it.
fn path_tile(level: &mut Level, tx: i32, ty: i32) {
    if !level.map.in_bounds(tx, ty) {
        return;
    }
    let cur = level.map.get(tx, ty);
    let next = match cur {
        tile::WATER | tile::LAVA => tile::BRIDGE,
        tile::WATER_SHALLOW => tile::SAND,
        _ => tile::PATH,
    };
    level.map.set(tx, ty, next);
}

/// Flattens a square of terrain to plain walkable ground.
fn clear_area(level: &mut Level, center: (i32, i32), radius: i32) {
    for ty in center.1 - radius..=center.1 + radius {
        for tx in center.0 - radius..=center.0 + radius {
            if !level.map.in_bounds(tx, ty) {
                continue;
            }
            let cur = level.map.get(tx, ty);
            if tiles::any(cur, flag::SOLID | flag::WATER | flag::PIT | flag::HARMFUL) {
                level.map.set(tx, ty, tile::GRASS);
            }
        }
    }
}

/// Puts a staircase in a handful of rooms away from the start.
fn place_dungeon_entrances(
    level: &mut Level,
    graph: &RoomGraph,
    rng: &mut Rng,
    count: usize,
    start: (i32, i32),
) -> Vec<Entrance> {
    let mut candidates: Vec<(i32, i32)> = Vec::new();
    for y in 0..graph.h {
        for x in 0..graph.w {
            if (x, y) != start && graph.depth_at(x, y) >= 1 {
                candidates.push((x, y));
            }
        }
    }
    // Prefer rooms far from the start, then take a random spread of them.
    candidates.sort_by_key(|(x, y)| -graph.depth_at(*x, *y));
    let pool = candidates.len().min(count * 3).max(count.min(candidates.len()));
    let mut pool: Vec<(i32, i32)> = candidates.into_iter().take(pool).collect();
    rng.shuffle(&mut pool);

    let mut out = Vec::new();
    for room in pool.into_iter().take(count) {
        let c = room_center_tile(room);
        // Stand the staircase clear of the path so it is visible.
        let spot = (c.0 + rng.range(-2, 2), c.1 - 2);
        let spot = level.find_free_near(spot.0, spot.1, 4).unwrap_or(c);
        clear_area(level, spot, 1);
        level.map.set(spot.0, spot.1, tile::STAIRS_DOWN);
        out.push(Entrance {
            tx: spot.0,
            ty: spot.1,
            room,
        });
    }
    out
}

/// Scatters the small stuff: bushes along paths, rocks, the odd sign.
fn decorate(level: &mut Level, rng: &mut Rng, seed: u64) {
    let (w, h) = (level.map.w(), level.map.h());
    for ty in 1..h - 1 {
        for tx in 1..w - 1 {
            if level.map.get(tx, ty) != tile::GRASS {
                continue;
            }
            let n = noise::value(seed ^ 0x55, tx, ty, 2);
            // Bushes like to grow beside paths, which is also where a hero
            // wants something to cut.
            let beside_path = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| level.map.get(tx + dx, ty + dy) == tile::PATH);
            if beside_path && n > 620 && rng.chance(1, 2) {
                level.map.set(tx, ty, tile::BUSH);
            } else if n > 880 && rng.chance(1, 3) {
                level.map.set(tx, ty, tile::BUSH);
            } else if n < 110 && rng.chance(1, 3) {
                level.map.set(tx, ty, tile::ROCK);
            } else if (400..430).contains(&n) && rng.chance(1, 4) {
                level.map.set(tx, ty, tile::FLOWERS);
            }
        }
    }
}

/// Fills rooms with monsters, more of them further from home.
fn populate(level: &mut Level, graph: &RoomGraph, rng: &mut Rng, start: (i32, i32)) {
    for ry in 0..graph.h {
        for rx in 0..graph.w {
            if (rx, ry) == start {
                continue;
            }
            let Some(room_index) = level.room_index(rx, ry) else {
                continue;
            };
            let depth = graph.depth_at(rx, ry).max(0);
            let count = (1 + depth / 3).min(4);
            for _ in 0..rng.range(1, count) {
                let tx = rx * ROOM_W + rng.range(1, ROOM_W - 2);
                let ty = ry * ROOM_H + rng.range(1, ROOM_H - 2);
                let Some((tx, ty)) = level.find_free_near(tx, ty, 3) else {
                    continue;
                };
                let kind = pick_monster(rng, depth);
                level.spawns.push(Spawn {
                    room: room_index as u16,
                    kind: kind as u8,
                    tx,
                    ty,
                    param: 0,
                });
            }
            // The occasional chest of rupees, tucked away from the middle.
            if rng.chance(1, 8) {
                let tx = rx * ROOM_W + rng.range(1, ROOM_W - 2);
                let ty = ry * ROOM_H + rng.range(1, ROOM_H - 2);
                if let Some((tx, ty)) = level.find_free_near(tx, ty, 3) {
                    level.spawns.push(Spawn {
                        room: room_index as u16,
                        kind: Kind::Chest as u8,
                        tx,
                        ty,
                        param: 0,
                    });
                }
            }
        }
    }
}

fn pick_monster(rng: &mut Rng, depth: i32) -> Kind {
    let roll = rng.below(100) as i32 + depth * 4;
    match roll {
        0..=40 => Kind::Octorok,
        41..=60 => Kind::Keese,
        61..=78 => Kind::Zol,
        79..=95 => Kind::Moblin,
        96..=115 => Kind::Tektite,
        _ => Kind::Stalfos,
    }
}

/// Every tile reachable on foot from a starting tile.
pub fn reachable(level: &Level, from: (i32, i32)) -> Vec<bool> {
    let (w, h) = (level.map.w(), level.map.h());
    let mut seen = vec![false; (w * h) as usize];
    let passable = |t: Tile| !tiles::blocks_walk(t) && !tiles::any(t, flag::WATER | flag::HARMFUL);
    if !level.map.in_bounds(from.0, from.1) || !passable(level.map.get(from.0, from.1)) {
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
    fn every_room_can_be_walked_to() {
        for seed in [1u64, 2, 3, 17, 4242] {
            let ow = generate(seed, 6, 5, 2);
            let start = room_center_tile(ow.graph.start);
            let seen = reachable(&ow.level, start);
            let w = ow.level.map.w();
            for ry in 0..ow.level.rooms_h() {
                for rx in 0..ow.level.rooms_w() {
                    let c = room_center_tile((rx, ry));
                    assert!(
                        seen[(c.1 * w + c.0) as usize],
                        "seed {seed}: room {rx},{ry} is cut off"
                    );
                }
            }
        }
    }

    #[test]
    fn the_hero_does_not_start_in_water_or_a_wall() {
        for seed in 0..20u64 {
            let ow = generate(seed, 5, 5, 1);
            let t = ow.level.map.at_world(ow.level.entrance);
            assert!(
                !tiles::blocks_walk(t) && !tiles::any(t, flag::WATER | flag::PIT | flag::HARMFUL),
                "seed {seed}: started on tile {t}"
            );
        }
    }

    #[test]
    fn staircases_are_reachable() {
        for seed in [5u64, 6, 7] {
            let ow = generate(seed, 6, 5, 3);
            assert_eq!(ow.dungeon_entrances.len(), 3);
            let seen = reachable(&ow.level, room_center_tile(ow.graph.start));
            let w = ow.level.map.w();
            for e in &ow.dungeon_entrances {
                assert_eq!(ow.level.map.get(e.tx, e.ty), tile::STAIRS_DOWN);
                // The staircase tile itself is walkable, so it must be in the
                // flood fill.
                assert!(
                    seen[(e.ty * w + e.tx) as usize],
                    "seed {seed}: a staircase is unreachable"
                );
            }
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate(12345, 5, 4, 2);
        let b = generate(12345, 5, 4, 2);
        assert_eq!(a.level.map.raw(), b.level.map.raw());
        assert_eq!(a.level.spawns.len(), b.level.spawns.len());
        assert_eq!(a.level.entrance, b.level.entrance);
    }

    #[test]
    fn different_seeds_make_different_worlds() {
        let a = generate(1, 5, 4, 1);
        let b = generate(2, 5, 4, 1);
        assert_ne!(a.level.map.raw(), b.level.map.raw());
    }

    #[test]
    fn monsters_are_not_spawned_inside_walls() {
        let ow = generate(77, 6, 5, 2);
        assert!(!ow.level.spawns.is_empty());
        for s in &ow.level.spawns {
            let t = ow.level.map.get(s.tx, s.ty);
            assert!(
                !tiles::blocks_walk(t),
                "spawn at {},{} is inside tile {t}",
                s.tx,
                s.ty
            );
        }
    }

    #[test]
    fn the_world_has_a_shoreline_at_its_edge() {
        let ow = generate(3, 6, 5, 1);
        let w = ow.level.map.w();
        let h = ow.level.map.h();
        let mut water = 0;
        for x in 0..w {
            if ow.level.map.get(x, 0) == tile::WATER {
                water += 1;
            }
            if ow.level.map.get(x, h - 1) == tile::WATER {
                water += 1;
            }
        }
        assert!(water > w, "the map should be ringed by water, saw {water}");
    }

    #[test]
    fn the_map_holds_a_mix_of_terrain() {
        let ow = generate(9, 6, 5, 1);
        let mut kinds = std::collections::HashSet::new();
        for t in ow.level.map.raw() {
            kinds.insert(*t);
        }
        assert!(
            kinds.len() >= 6,
            "expected varied terrain, got {:?}",
            kinds
        );
    }
}
