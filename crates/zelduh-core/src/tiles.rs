//! Logical terrain tiles.
//!
//! The map stores *meaning* ("this is a bush"), never appearance. How a bush
//! looks is decided by the asset pack, so swapping in tiles ripped from a ROM
//! changes the look of a world without changing how it plays.

/// A logical tile id as stored in [`crate::level::Map`].
pub type Tile = u8;

/// Tile ids. These are persisted in save data and sent over the wire, so the
/// numeric values must not be reordered.
pub mod tile {
    use super::Tile;

    pub const VOID: Tile = 0;
    pub const GRASS: Tile = 1;
    pub const GRASS_TALL: Tile = 2;
    pub const FLOWERS: Tile = 3;
    pub const SAND: Tile = 4;
    pub const PATH: Tile = 5;
    pub const FLOOR: Tile = 6;
    pub const CARPET: Tile = 7;
    pub const BRIDGE: Tile = 8;

    pub const WATER: Tile = 16;
    pub const WATER_SHALLOW: Tile = 17;
    pub const LAVA: Tile = 18;
    pub const PIT: Tile = 19;

    pub const BUSH: Tile = 32;
    pub const ROCK: Tile = 33;
    pub const TREE: Tile = 34;
    pub const WALL: Tile = 35;
    pub const CLIFF: Tile = 36;
    pub const WALL_DUNGEON: Tile = 37;
    pub const WALL_CRACKED: Tile = 38;
    pub const BLOCK: Tile = 39;
    pub const STATUE: Tile = 40;
    pub const POT: Tile = 41;
    pub const SIGN: Tile = 42;

    pub const DOOR_OPEN: Tile = 48;
    pub const DOOR_SHUT: Tile = 49;
    pub const DOOR_LOCKED: Tile = 50;
    pub const DOOR_BOSS: Tile = 51;
    pub const STAIRS_DOWN: Tile = 52;
    pub const STAIRS_UP: Tile = 53;
    pub const WARP: Tile = 54;

    pub const LEDGE_DOWN: Tile = 64;
    pub const LEDGE_UP: Tile = 65;
    pub const LEDGE_LEFT: Tile = 66;
    pub const LEDGE_RIGHT: Tile = 67;

    /// One past the highest id, i.e. the size of the tile tables.
    pub const COUNT: usize = 128;
}

/// Terrain behaviour bits.
pub mod flag {
    /// Blocks walking entities.
    pub const SOLID: u16 = 1 << 0;
    /// Deep water: swimmable, drowns anyone without flippers.
    pub const WATER: u16 = 1 << 1;
    /// A hole to fall into.
    pub const PIT: u16 = 1 << 2;
    /// Can be cut down with a sword.
    pub const CUTTABLE: u16 = 1 << 3;
    /// Can be picked up with the power bracelet.
    pub const LIFTABLE: u16 = 1 << 4;
    /// Destroyed by a bomb blast.
    pub const BOMBABLE: u16 = 1 << 5;
    /// Can be pushed one square.
    pub const PUSHABLE: u16 = 1 << 6;
    /// Changes level when stepped on.
    pub const TRANSITION: u16 = 1 << 7;
    /// Hurts on contact.
    pub const HARMFUL: u16 = 1 << 8;
    /// Blocks flying and thrown things as well as walkers.
    pub const TALL: u16 = 1 << 9;
    /// Slows movement (shallow water, tall grass).
    pub const SLOW: u16 = 1 << 10;
    /// A one-way drop; walkable but you hop down it.
    pub const LEDGE: u16 = 1 << 11;
    /// Needs a small key.
    pub const LOCKED: u16 = 1 << 12;
    /// Projectiles pass over it even though walkers cannot pass.
    pub const LOW: u16 = 1 << 13;
}

const fn build_flags() -> [u16; tile::COUNT] {
    use flag::*;
    let mut f = [0u16; tile::COUNT];
    f[tile::VOID as usize] = SOLID | TALL;
    f[tile::GRASS_TALL as usize] = CUTTABLE | SLOW;
    f[tile::WATER as usize] = WATER;
    f[tile::WATER_SHALLOW as usize] = SLOW;
    f[tile::LAVA as usize] = HARMFUL | WATER;
    f[tile::PIT as usize] = PIT;

    f[tile::BUSH as usize] = SOLID | CUTTABLE | LIFTABLE | LOW;
    f[tile::ROCK as usize] = SOLID | LIFTABLE | LOW;
    f[tile::TREE as usize] = SOLID | TALL;
    f[tile::WALL as usize] = SOLID | TALL;
    f[tile::CLIFF as usize] = SOLID | TALL;
    f[tile::WALL_DUNGEON as usize] = SOLID | TALL;
    f[tile::WALL_CRACKED as usize] = SOLID | TALL | BOMBABLE;
    f[tile::BLOCK as usize] = SOLID | PUSHABLE | LOW;
    f[tile::STATUE as usize] = SOLID | LOW;
    f[tile::POT as usize] = SOLID | LIFTABLE | LOW;
    f[tile::SIGN as usize] = SOLID | LIFTABLE | LOW;

    f[tile::DOOR_SHUT as usize] = SOLID | TALL;
    f[tile::DOOR_LOCKED as usize] = SOLID | TALL | LOCKED;
    f[tile::DOOR_BOSS as usize] = SOLID | TALL | LOCKED;
    f[tile::STAIRS_DOWN as usize] = TRANSITION;
    f[tile::STAIRS_UP as usize] = TRANSITION;
    f[tile::WARP as usize] = TRANSITION;

    f[tile::LEDGE_DOWN as usize] = LEDGE;
    f[tile::LEDGE_UP as usize] = LEDGE;
    f[tile::LEDGE_LEFT as usize] = LEDGE;
    f[tile::LEDGE_RIGHT as usize] = LEDGE;
    f
}

static FLAGS: [u16; tile::COUNT] = build_flags();

/// Behaviour bits for a tile id.
#[inline]
pub fn flags(t: Tile) -> u16 {
    FLAGS[(t as usize) & (tile::COUNT - 1)]
}

/// True when the tile has every bit in `mask`.
#[inline]
pub fn has(t: Tile, mask: u16) -> bool {
    flags(t) & mask == mask
}

/// True when the tile has any bit in `mask`.
#[inline]
pub fn any(t: Tile, mask: u16) -> bool {
    flags(t) & mask != 0
}

/// True when a walking entity is blocked by this tile.
#[inline]
pub fn blocks_walk(t: Tile) -> bool {
    any(t, flag::SOLID)
}

/// True when a flying or thrown entity is blocked by this tile.
#[inline]
pub fn blocks_flight(t: Tile) -> bool {
    any(t, flag::TALL)
}

/// What a tile turns into once it is cut, lifted or blown up.
pub fn destroyed(t: Tile) -> Tile {
    match t {
        tile::BUSH | tile::GRASS_TALL | tile::POT | tile::SIGN => tile::GRASS,
        tile::ROCK => tile::PATH,
        tile::WALL_CRACKED => tile::FLOOR,
        other => other,
    }
}

/// The direction a ledge drops towards, if it is one.
pub fn ledge_dir(t: Tile) -> Option<crate::geom::Dir> {
    use crate::geom::Dir;
    match t {
        tile::LEDGE_DOWN => Some(Dir::Down),
        tile::LEDGE_UP => Some(Dir::Up),
        tile::LEDGE_LEFT => Some(Dir::Left),
        tile::LEDGE_RIGHT => Some(Dir::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_is_walkable_and_walls_are_not() {
        assert!(!blocks_walk(tile::GRASS));
        assert!(blocks_walk(tile::WALL));
        assert!(blocks_flight(tile::WALL));
        assert!(!blocks_flight(tile::BUSH), "arrows fly over bushes");
    }

    #[test]
    fn cutting_a_bush_leaves_grass() {
        assert_eq!(destroyed(tile::BUSH), tile::GRASS);
        assert_eq!(destroyed(tile::GRASS), tile::GRASS);
    }

    #[test]
    fn flags_never_index_out_of_bounds() {
        for t in 0..=255u8 {
            let _ = flags(t);
        }
    }

    #[test]
    fn locked_doors_are_solid() {
        assert!(blocks_walk(tile::DOOR_LOCKED));
        assert!(any(tile::DOOR_LOCKED, flag::LOCKED));
    }
}
