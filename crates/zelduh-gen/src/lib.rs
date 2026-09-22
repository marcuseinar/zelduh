//! World generation for Zelduh.
//!
//! [`generate`] builds a whole game world from one seed: an overworld, a set of
//! dungeons, and the staircases that join them. Everything is drawn from a
//! seeded generator and integer noise, so a seed is a shareable world — two
//! players who type the same number get the same island.

pub mod dungeon;
pub mod graph;
pub mod noise;
pub mod overworld;

use zelduh_core::geom::Dir;
use zelduh_core::level::{Level, Link};

pub use dungeon::Dungeon;
pub use graph::RoomGraph;
pub use overworld::Overworld;

/// How big a world to build.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub seed: u64,
    /// Overworld size in screens.
    pub overworld: (i32, i32),
    /// How many dungeons to build.
    pub dungeons: usize,
    /// Dungeon size in rooms.
    pub dungeon_size: (i32, i32),
}

impl Default for Config {
    fn default() -> Self {
        Config {
            seed: 1,
            overworld: (8, 6),
            dungeons: 3,
            dungeon_size: (4, 4),
        }
    }
}

impl Config {
    /// A world of the given seed at default size.
    pub fn from_seed(seed: u64) -> Config {
        Config {
            seed,
            ..Config::default()
        }
    }
}

/// Builds a complete world. Level 0 is always the overworld.
pub fn generate(cfg: Config) -> Vec<Level> {
    let dungeons_wanted = cfg.dungeons.min(8);
    let mut ow = overworld::generate(
        cfg.seed,
        cfg.overworld.0,
        cfg.overworld.1,
        dungeons_wanted,
    );

    let mut levels = Vec::with_capacity(1 + dungeons_wanted);
    let mut dungeons = Vec::new();
    for i in 0..dungeons_wanted {
        dungeons.push(dungeon::generate(
            cfg.seed,
            i as u8,
            cfg.dungeon_size.0,
            cfg.dungeon_size.1,
        ));
    }

    // Join each staircase to its dungeon and back again.
    for (i, d) in dungeons.iter_mut().enumerate() {
        let level_index = (i + 1) as u16;
        let Some(entrance) = ow.dungeon_entrances.get(i) else {
            continue;
        };
        ow.level.links.push(Link {
            from: (entrance.tx, entrance.ty),
            to_level: level_index,
            to_pos: d.arrival,
            to_dir: Dir::Up,
        });
        // Arriving back on the overworld puts you just below the staircase, so
        // you do not immediately step back into it.
        let back = zelduh_core::level::tile_center(entrance.tx, entrance.ty + 1);
        d.level.links.push(Link {
            from: d.exit,
            to_level: 0,
            to_pos: back,
            to_dir: Dir::Down,
        });
    }

    levels.push(ow.level);
    for d in dungeons {
        levels.push(d.level);
    }
    levels
}

/// Builds a world and wraps it in a ready-to-run [`zelduh_core::World`].
pub fn new_world(cfg: Config, players: usize) -> zelduh_core::World {
    let levels = generate(cfg);
    zelduh_core::World::new(cfg.seed, levels, players)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zelduh_core::tiles::tile;

    #[test]
    fn a_world_has_an_overworld_and_its_dungeons() {
        let levels = generate(Config::from_seed(42));
        assert_eq!(levels.len(), 4);
        assert_eq!(levels[0].kind, zelduh_core::LevelKind::Overworld);
        assert!(levels[1..]
            .iter()
            .all(|l| l.kind == zelduh_core::LevelKind::Dungeon));
    }

    #[test]
    fn staircases_lead_both_ways() {
        let levels = generate(Config::from_seed(7));
        assert_eq!(levels[0].links.len(), 3);
        for link in &levels[0].links {
            let to = &levels[link.to_level as usize];
            assert_eq!(levels[0].map.get(link.from.0, link.from.1), tile::STAIRS_DOWN);
            assert!(
                to.links.iter().any(|back| back.to_level == 0),
                "a dungeon with no way out"
            );
        }
    }

    #[test]
    fn a_world_is_reproducible_from_its_seed() {
        let a = generate(Config::from_seed(1234));
        let b = generate(Config::from_seed(1234));
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.map.raw(), y.map.raw());
            assert_eq!(x.spawns.len(), y.spawns.len());
        }
    }

    #[test]
    fn a_world_can_be_stepped() {
        let mut w = new_world(Config::from_seed(5), 2);
        w.join(0);
        for _ in 0..240 {
            w.set_input(0, zelduh_core::button::RIGHT);
            w.step();
        }
        assert!(w.players[0].is_alive() || w.players[0].respawn > 0);
    }

    #[test]
    fn asking_for_no_dungeons_is_allowed() {
        let levels = generate(Config {
            dungeons: 0,
            ..Config::from_seed(3)
        });
        assert_eq!(levels.len(), 1);
        assert!(levels[0].links.is_empty());
    }

    #[test]
    fn a_tiny_world_still_generates() {
        let levels = generate(Config {
            overworld: (1, 1),
            dungeons: 1,
            dungeon_size: (1, 1),
            seed: 9,
        });
        assert_eq!(levels.len(), 2);
    }
}
