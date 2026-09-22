//! Maps, rooms and level geometry.
//!
//! The screen layout follows the Game Boy: a 160x144 screen with a 16 pixel
//! status bar, leaving a 160x128 playfield that holds exactly 10x8 tiles of
//! 16x16 pixels. One "room" is one screenful, and the camera snaps between
//! rooms the way Link's Awakening does rather than scrolling freely.

use crate::fixed::{px, to_px, Fx};
use crate::geom::{Dir, Rect, V2};
use crate::tiles::{self, flag, tile, Tile};

/// Width and height of one tile, in pixels.
pub const TILE_PX: i32 = 16;
/// Room width in tiles.
pub const ROOM_W: i32 = 10;
/// Room height in tiles.
pub const ROOM_H: i32 = 8;
/// Room width in pixels.
pub const ROOM_PX_W: i32 = ROOM_W * TILE_PX;
/// Room height in pixels.
pub const ROOM_PX_H: i32 = ROOM_H * TILE_PX;
/// Full screen width in pixels.
pub const SCREEN_W: i32 = 160;
/// Full screen height in pixels.
pub const SCREEN_H: i32 = 144;
/// Height of the status bar at the top of the screen.
pub const HUD_H: i32 = 16;

/// What sort of place a level is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum LevelKind {
    #[default]
    Overworld = 0,
    Dungeon = 1,
    Cave = 2,
    Interior = 3,
}

/// What a room is used for. Generation fills this in; gameplay reads it to
/// decide what to spawn and whether to lock the doors behind you.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum RoomKind {
    #[default]
    Normal = 0,
    Start = 1,
    Treasure = 2,
    Boss = 3,
    Shop = 4,
    Puzzle = 5,
    Corridor = 6,
    Empty = 7,
}

/// Per-room metadata.
#[derive(Clone, Copy, Debug, Default)]
pub struct Room {
    pub kind: RoomKind,
    /// Which dungeon region this room belongs to, for keys and maps.
    pub region: u8,
    /// Set once a player has entered the room at least once.
    pub visited: bool,
    /// Set once the room's encounter has been cleared.
    pub cleared: bool,
    /// Bit per [`Dir`]: a doorway leads that way.
    pub exits: u8,
}

/// A thing generation wants placed in a room the first time a player sees it.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    /// Index of the room this spawn belongs to.
    pub room: u16,
    /// Entity kind discriminant, see `crate::entity::Kind`.
    pub kind: u8,
    /// Tile coordinates within the level.
    pub tx: i32,
    pub ty: i32,
    /// Per-kind parameter, e.g. which boss or what a chest holds.
    pub param: i32,
}

/// A one-way link from a transition tile to somewhere else.
#[derive(Clone, Copy, Debug)]
pub struct Link {
    /// Tile coordinates of the transition tile in this level.
    pub from: (i32, i32),
    /// Index of the destination level.
    pub to_level: u16,
    /// Destination position in world pixels.
    pub to_pos: V2,
    /// Facing the traveller arrives with.
    pub to_dir: Dir,
}

/// A grid of levels' worth of tiles, addressed in tile coordinates.
#[derive(Clone, Debug)]
pub struct Map {
    /// Width in rooms.
    pub rooms_w: i32,
    /// Height in rooms.
    pub rooms_h: i32,
    tiles: Vec<Tile>,
}

impl Map {
    /// Creates a map of `rooms_w` by `rooms_h` rooms filled with one tile.
    pub fn new(rooms_w: i32, rooms_h: i32, fill: Tile) -> Map {
        let rooms_w = rooms_w.max(1);
        let rooms_h = rooms_h.max(1);
        let n = (rooms_w * ROOM_W * rooms_h * ROOM_H) as usize;
        Map {
            rooms_w,
            rooms_h,
            tiles: vec![fill; n],
        }
    }

    /// Width in tiles.
    #[inline]
    pub fn w(&self) -> i32 {
        self.rooms_w * ROOM_W
    }

    /// Height in tiles.
    #[inline]
    pub fn h(&self) -> i32 {
        self.rooms_h * ROOM_H
    }

    /// True when the tile coordinate is inside the map.
    #[inline]
    pub fn in_bounds(&self, tx: i32, ty: i32) -> bool {
        tx >= 0 && ty >= 0 && tx < self.w() && ty < self.h()
    }

    /// Tile at a tile coordinate. Out of bounds reads as [`tile::VOID`], which
    /// is solid, so the world is walled in without special-casing edges.
    #[inline]
    pub fn get(&self, tx: i32, ty: i32) -> Tile {
        if self.in_bounds(tx, ty) {
            self.tiles[(ty * self.w() + tx) as usize]
        } else {
            tile::VOID
        }
    }

    /// Writes a tile, ignoring out-of-bounds writes.
    #[inline]
    pub fn set(&mut self, tx: i32, ty: i32, t: Tile) {
        if self.in_bounds(tx, ty) {
            let w = self.w();
            self.tiles[(ty * w + tx) as usize] = t;
        }
    }

    /// Tile at a world pixel position.
    #[inline]
    pub fn at_world(&self, p: V2) -> Tile {
        self.get(to_px(p.x).div_euclid(TILE_PX), to_px(p.y).div_euclid(TILE_PX))
    }

    /// Fills a tile-space rectangle.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, t: Tile) {
        for ty in y..y + h {
            for tx in x..x + w {
                self.set(tx, ty, t);
            }
        }
    }

    /// Draws the outline of a tile-space rectangle.
    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, t: Tile) {
        for tx in x..x + w {
            self.set(tx, y, t);
            self.set(tx, y + h - 1, t);
        }
        for ty in y..y + h {
            self.set(x, ty, t);
            self.set(x + w - 1, ty, t);
        }
    }

    /// Raw tile storage, row major. Useful for rendering and snapshots.
    pub fn raw(&self) -> &[Tile] {
        &self.tiles
    }

    /// True when a walking body of `w_px` by `h_px` centred on `c` fits without
    /// touching solid terrain.
    pub fn body_fits(&self, c: V2, w_px: i32, h_px: i32) -> bool {
        self.rect_is_clear(&Rect::centered(c, w_px, h_px), tiles::blocks_walk)
    }

    /// True when no tile overlapping `r` satisfies `blocked`.
    pub fn rect_is_clear(&self, r: &Rect, blocked: impl Fn(Tile) -> bool) -> bool {
        let x0 = to_px(r.x).div_euclid(TILE_PX);
        let y0 = to_px(r.y).div_euclid(TILE_PX);
        // `r.right()` is exclusive, so a body that stops exactly on a tile
        // boundary must not be tested against the tile it is touching.
        let x1 = to_px(r.right() - 1).div_euclid(TILE_PX);
        let y1 = to_px(r.bottom() - 1).div_euclid(TILE_PX);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                if blocked(self.get(tx, ty)) {
                    return false;
                }
            }
        }
        true
    }

    /// Runs `f` for every tile overlapping `r`.
    pub fn for_each_tile_in(&self, r: &Rect, mut f: impl FnMut(i32, i32, Tile)) {
        let x0 = to_px(r.x).div_euclid(TILE_PX);
        let y0 = to_px(r.y).div_euclid(TILE_PX);
        let x1 = to_px(r.right() - 1).div_euclid(TILE_PX);
        let y1 = to_px(r.bottom() - 1).div_euclid(TILE_PX);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                f(tx, ty, self.get(tx, ty));
            }
        }
    }
}

/// One self-contained place: the overworld, a dungeon floor, a cave.
#[derive(Clone, Debug)]
pub struct Level {
    pub kind: LevelKind,
    pub map: Map,
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    /// Enemy placements, used to populate rooms as players walk into them.
    pub spawns: Vec<Spawn>,
    /// Where an arriving player appears if nothing else says otherwise.
    pub entrance: V2,
    /// Dungeon number, or 0 for the overworld.
    pub dungeon: u8,
    /// The tile drawn underneath anything that stands on the ground, so a bush
    /// on a dungeon floor is not drawn sitting on a patch of grass.
    pub ground: Tile,
}

impl Level {
    /// Creates an empty level of the given size in rooms.
    pub fn new(kind: LevelKind, rooms_w: i32, rooms_h: i32, fill: Tile) -> Level {
        Level {
            kind,
            map: Map::new(rooms_w, rooms_h, fill),
            rooms: vec![Room::default(); (rooms_w.max(1) * rooms_h.max(1)) as usize],
            links: Vec::new(),
            spawns: Vec::new(),
            entrance: V2::from_px(ROOM_PX_W / 2, ROOM_PX_H / 2),
            dungeon: 0,
            ground: match kind {
                LevelKind::Overworld => tile::GRASS,
                _ => tile::FLOOR,
            },
        }
    }

    #[inline]
    pub fn rooms_w(&self) -> i32 {
        self.map.rooms_w
    }

    #[inline]
    pub fn rooms_h(&self) -> i32 {
        self.map.rooms_h
    }

    /// Room index for room coordinates, if they are in bounds.
    pub fn room_index(&self, rx: i32, ry: i32) -> Option<usize> {
        if rx < 0 || ry < 0 || rx >= self.rooms_w() || ry >= self.rooms_h() {
            None
        } else {
            Some((ry * self.rooms_w() + rx) as usize)
        }
    }

    pub fn room(&self, rx: i32, ry: i32) -> Option<&Room> {
        self.room_index(rx, ry).map(|i| &self.rooms[i])
    }

    pub fn room_mut(&mut self, rx: i32, ry: i32) -> Option<&mut Room> {
        self.room_index(rx, ry).map(move |i| &mut self.rooms[i])
    }

    /// Room coordinates containing a world pixel position.
    pub fn room_at(&self, p: V2) -> (i32, i32) {
        (
            to_px(p.x).div_euclid(ROOM_PX_W),
            to_px(p.y).div_euclid(ROOM_PX_H),
        )
    }

    /// World-pixel bounds of a room.
    pub fn room_bounds(&self, rx: i32, ry: i32) -> Rect {
        Rect::new(
            px(rx * ROOM_PX_W),
            px(ry * ROOM_PX_H),
            px(ROOM_PX_W),
            px(ROOM_PX_H),
        )
    }

    /// Centre of a room in world pixels.
    pub fn room_center(&self, rx: i32, ry: i32) -> V2 {
        V2::from_px(
            rx * ROOM_PX_W + ROOM_PX_W / 2,
            ry * ROOM_PX_H + ROOM_PX_H / 2,
        )
    }

    /// The link starting at a tile, if any.
    pub fn link_at(&self, tx: i32, ty: i32) -> Option<&Link> {
        self.links.iter().find(|l| l.from == (tx, ty))
    }

    /// Bounds of the whole level in world pixels.
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, px(self.map.w() * TILE_PX), px(self.map.h() * TILE_PX))
    }

    /// Clamps a position to stay inside the level.
    pub fn clamp(&self, p: V2, half_w: Fx, half_h: Fx) -> V2 {
        let b = self.bounds();
        V2::new(
            p.x.clamp(b.x + half_w, b.right() - half_w),
            p.y.clamp(b.y + half_h, b.bottom() - half_h),
        )
    }

    /// Finds a walkable tile near `(tx, ty)`, searching outwards. Used to place
    /// things that generation put somewhere awkward.
    pub fn find_free_near(&self, tx: i32, ty: i32, radius: i32) -> Option<(i32, i32)> {
        for r in 0..=radius {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (x, y) = (tx + dx, ty + dy);
                    let t = self.map.get(x, y);
                    if self.map.in_bounds(x, y)
                        && !tiles::blocks_walk(t)
                        && !tiles::any(t, flag::WATER | flag::PIT | flag::HARMFUL)
                    {
                        return Some((x, y));
                    }
                }
            }
        }
        None
    }
}

/// Centre of the tile at tile coordinates, in world pixels.
#[inline]
pub fn tile_center(tx: i32, ty: i32) -> V2 {
    V2::from_px(tx * TILE_PX + TILE_PX / 2, ty * TILE_PX + TILE_PX / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_bounds_reads_as_void() {
        let m = Map::new(2, 2, tile::GRASS);
        assert_eq!(m.get(-1, 0), tile::VOID);
        assert_eq!(m.get(0, -1), tile::VOID);
        assert_eq!(m.get(m.w(), 0), tile::VOID);
        assert!(tiles::blocks_walk(m.get(-1, -1)));
    }

    #[test]
    fn map_size_is_rooms_times_room_size() {
        let m = Map::new(3, 4, tile::GRASS);
        assert_eq!(m.w(), 30);
        assert_eq!(m.h(), 32);
        assert_eq!(m.raw().len(), 30 * 32);
    }

    #[test]
    fn body_fits_respects_walls() {
        let mut m = Map::new(1, 1, tile::GRASS);
        m.set(2, 2, tile::WALL);
        // Tile (2,2) covers pixels 32..48.
        assert!(!m.body_fits(V2::from_px(40, 40), 12, 12));
        assert!(m.body_fits(V2::from_px(8, 8), 12, 12));
    }

    #[test]
    fn a_body_touching_the_edge_of_a_wall_still_fits() {
        let mut m = Map::new(1, 1, tile::GRASS);
        m.set(2, 0, tile::WALL);
        // Wall starts at x=32; a 12 wide body centred at 26 ends exactly at 32.
        assert!(m.body_fits(V2::from_px(26, 8), 12, 12));
        assert!(!m.body_fits(V2::from_px(27, 8), 12, 12));
    }

    #[test]
    fn room_lookup_matches_position() {
        let l = Level::new(LevelKind::Overworld, 4, 4, tile::GRASS);
        assert_eq!(l.room_at(V2::from_px(0, 0)), (0, 0));
        assert_eq!(l.room_at(V2::from_px(ROOM_PX_W, ROOM_PX_H)), (1, 1));
        assert_eq!(l.room_at(V2::from_px(ROOM_PX_W - 1, 0)), (0, 0));
        assert_eq!(l.room_index(4, 0), None);
    }

    #[test]
    fn find_free_near_skips_water() {
        let mut l = Level::new(LevelKind::Overworld, 1, 1, tile::WATER);
        l.map.set(5, 5, tile::GRASS);
        assert_eq!(l.find_free_near(3, 3, 8), Some((5, 5)));
    }
}
