//! Asset packs: the bridge between what the simulation means and what you see.
//!
//! A pack holds three things: a flat array of 8x8 tiles, a palette table, and
//! *structure* — which cells make up the bush tile, which make up the hero
//! walking left. The structure comes from the built-in pack; the pixels can be
//! replaced wholesale from a ROM or an image, which is what lets you point the
//! engine at a cartridge you own and see the world redrawn with its graphics.

use std::collections::HashMap;

use crate::palette::{Palette, DEFAULT_PALETTES};
use crate::tile::{Cell, Tile8, BLANK};
use zelduh_core::tiles::tile;

/// A 16x16 terrain tile: four 8x8 cells in reading order.
pub type MetaTile = [Cell; 4];

/// A composed image made of 8x8 cells.
#[derive(Clone, Debug, Default)]
pub struct Sprite {
    /// Width in cells.
    pub cols: u8,
    /// Height in cells.
    pub rows: u8,
    /// `cols * rows` cells in reading order.
    pub cells: Vec<Cell>,
    /// Offset in pixels from the draw anchor to the sprite's top-left.
    pub ox: i8,
    pub oy: i8,
}

impl Sprite {
    pub fn width(&self) -> i32 {
        self.cols as i32 * 8
    }

    pub fn height(&self) -> i32 {
        self.rows as i32 * 8
    }

    pub fn cell(&self, cx: usize, cy: usize) -> Cell {
        self.cells
            .get(cy * self.cols as usize + cx)
            .copied()
            .unwrap_or(Cell::BLANK)
    }
}

/// Every image the renderer knows how to draw.
///
/// The discriminants are used as array indices, so entries may be appended but
/// not reordered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum SpriteId {
    HeroDown0 = 0,
    HeroDown1,
    HeroUp0,
    HeroUp1,
    HeroSide0,
    HeroSide1,
    HeroAttackDown,
    HeroAttackUp,
    HeroAttackSide,
    HeroCarryDown,
    HeroCarryUp,
    HeroCarrySide,
    HeroSwim,
    HeroFall,
    SwordUp,
    SwordSide,
    Octorok0,
    Octorok1,
    Moblin0,
    Moblin1,
    Zol0,
    Zol1,
    Keese0,
    Keese1,
    Tektite0,
    Tektite1,
    Stalfos0,
    Stalfos1,
    Boss0,
    Boss1,
    Rock,
    ArrowVert,
    ArrowSide,
    BeamVert,
    BeamSide,
    Boomerang,
    Bomb,
    Explosion,
    Fireball,
    Rupee,
    Heart,
    Key,
    BombPickup,
    ArrowPickup,
    Fairy,
    HeartPiece,
    Triforce,
    ChestClosed,
    ChestOpen,
    Poof,
    Sparkle,
    Shadow,
    HudHeart4,
    HudHeart2,
    HudHeart1,
    HudHeart0,
    IconSword,
    IconShield,
    IconBomb,
    IconBow,
    IconBoomerang,
    IconFeather,
    IconBracelet,
    IconBoots,
    IconFlippers,
    IconKey,
    IconRupee,
    Count,
}

impl SpriteId {
    pub const N: usize = SpriteId::Count as usize;
}

/// Where a pack's pixels came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The art that ships with the engine.
    BuiltIn,
    /// Tiles lifted out of a Game Boy ROM the player supplied.
    Rom { title: String, offset: usize },
    /// Tiles decoded from an image or raw tile dump.
    File { name: String },
}

/// A complete set of graphics.
#[derive(Clone, Debug)]
pub struct AssetPack {
    pub name: String,
    pub source: Source,
    pub tiles: Vec<Tile8>,
    pub palettes: Vec<Palette>,
    /// One entry per logical terrain tile id.
    pub metatiles: Vec<MetaTile>,
    /// True for terrain that is an object standing on the ground rather than
    /// the ground itself. These are drawn over the level's ground tile, with
    /// colour 0 left transparent.
    pub overlay: Vec<bool>,
    /// One entry per [`SpriteId`].
    pub sprites: Vec<Sprite>,
}

impl AssetPack {
    /// An empty pack with the default palettes and nothing drawn.
    pub fn empty() -> AssetPack {
        AssetPack {
            name: "empty".to_string(),
            source: Source::BuiltIn,
            tiles: vec![BLANK],
            palettes: DEFAULT_PALETTES.to_vec(),
            metatiles: vec![[Cell::BLANK; 4]; tile::COUNT],
            overlay: vec![false; tile::COUNT],
            sprites: vec![Sprite::default(); SpriteId::N],
        }
    }

    /// Pixels for a tile index, or a blank tile when out of range.
    #[inline]
    pub fn tile(&self, index: u16) -> &Tile8 {
        self.tiles.get(index as usize).unwrap_or(&BLANK)
    }

    /// The palette at an index, falling back to the first one.
    #[inline]
    pub fn palette(&self, index: u8) -> &Palette {
        self.palettes
            .get(index as usize)
            .unwrap_or(&DEFAULT_PALETTES[0])
    }

    /// The art for a logical terrain tile.
    #[inline]
    pub fn metatile(&self, t: u8) -> &MetaTile {
        const FALLBACK: MetaTile = [Cell::BLANK; 4];
        self.metatiles.get(t as usize).unwrap_or(&FALLBACK)
    }

    /// True when this terrain is drawn over the level's ground tile.
    #[inline]
    pub fn is_overlay(&self, t: u8) -> bool {
        self.overlay.get(t as usize).copied().unwrap_or(false)
    }

    /// The art for a sprite.
    #[inline]
    pub fn sprite(&self, id: SpriteId) -> &Sprite {
        &self.sprites[id as usize]
    }

    /// Replaces tile pixels starting at `base`, growing the array if needed.
    ///
    /// Structure is untouched, so an imported tileset lands under the existing
    /// arrangement of metatiles and sprites.
    pub fn import_tiles(&mut self, base: usize, tiles: &[Tile8]) -> usize {
        if self.tiles.len() < base + tiles.len() {
            self.tiles.resize(base + tiles.len(), BLANK);
        }
        self.tiles[base..base + tiles.len()].copy_from_slice(tiles);
        tiles.len()
    }

    /// Replaces the palette table, keeping any palettes the new set omits.
    pub fn import_palettes(&mut self, base: usize, palettes: &[Palette]) {
        if self.palettes.len() < base + palettes.len() {
            self.palettes
                .resize(base + palettes.len(), DEFAULT_PALETTES[0]);
        }
        self.palettes[base..base + palettes.len()].copy_from_slice(palettes);
    }

    /// Number of distinct 8x8 tiles in the pack.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
}

/// Builds a pack from string art, reusing identical tiles.
pub struct PackBuilder {
    tiles: Vec<Tile8>,
    lookup: HashMap<Tile8, u16>,
    pub pack: AssetPack,
}

impl PackBuilder {
    pub fn new(name: &str) -> PackBuilder {
        let mut b = PackBuilder {
            tiles: Vec::new(),
            lookup: HashMap::new(),
            pack: AssetPack::empty(),
        };
        b.pack.name = name.to_string();
        // Tile 0 is always blank so that an unset cell draws nothing.
        b.intern(BLANK);
        b
    }

    /// Adds a tile, returning the index of an identical one if there is one.
    pub fn intern(&mut self, t: Tile8) -> u16 {
        if let Some(i) = self.lookup.get(&t) {
            return *i;
        }
        let i = self.tiles.len() as u16;
        self.tiles.push(t);
        self.lookup.insert(t, i);
        i
    }

    /// Converts string art into a sprite.
    ///
    /// Rows must be a multiple of 8 tall and 8 wide. `.` and a space are
    /// colour 0 (transparent in sprites), `-` is 1, `+` is 2 and `#` is 3.
    pub fn art(&mut self, rows: &[&str], palette: u8) -> Sprite {
        let h = rows.len();
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        assert!(h % 8 == 0 && h > 0, "art must be a multiple of 8 rows tall");
        assert!(w % 8 == 0 && w > 0, "art must be a multiple of 8 columns wide");
        let (cols, nrows) = (w / 8, h / 8);
        let mut cells = Vec::with_capacity(cols * nrows);
        for cy in 0..nrows {
            for cx in 0..cols {
                let mut t = BLANK;
                let mut used = false;
                for y in 0..8 {
                    let row: Vec<char> = rows[cy * 8 + y].chars().collect();
                    for x in 0..8 {
                        let ch = row.get(cx * 8 + x).copied().unwrap_or('.');
                        let v = match ch {
                            '-' | '1' => 1,
                            '+' | '2' => 2,
                            '#' | '3' => 3,
                            _ => 0,
                        };
                        t[y * 8 + x] = v;
                        used |= v != 0;
                    }
                }
                cells.push(if used {
                    Cell::new(self.intern(t), palette)
                } else {
                    Cell::BLANK
                });
            }
        }
        Sprite {
            cols: cols as u8,
            rows: nrows as u8,
            cells,
            ox: 0,
            oy: 0,
        }
    }

    /// Registers a 16x16 terrain tile from art. Art that is only 8x8 is
    /// repeated across the whole 16x16 square, which is how most ground
    /// textures are built.
    pub fn terrain(&mut self, id: u8, rows: &[&str], palette: u8) {
        let s = self.art(rows, palette);
        let meta: MetaTile = match (s.cols, s.rows) {
            (2, 2) => [s.cell(0, 0), s.cell(1, 0), s.cell(0, 1), s.cell(1, 1)],
            (1, 1) => {
                let c = s.cell(0, 0);
                [c; 4]
            }
            _ => panic!("terrain art must be 8x8 or 16x16"),
        };
        self.pack.metatiles[id as usize] = meta;
    }

    /// Registers a 16x16 object that stands on the ground: colour 0 is left
    /// transparent and the level's ground shows through.
    pub fn object(&mut self, id: u8, rows: &[&str], palette: u8) {
        self.terrain(id, rows, palette);
        self.pack.overlay[id as usize] = true;
    }

    /// Registers a sprite from art.
    pub fn sprite(&mut self, id: SpriteId, rows: &[&str], palette: u8) {
        let s = self.art(rows, palette);
        self.pack.sprites[id as usize] = s;
    }

    /// Registers a sprite and shifts where it is drawn relative to its anchor.
    pub fn sprite_off(&mut self, id: SpriteId, rows: &[&str], palette: u8, ox: i8, oy: i8) {
        let mut s = self.art(rows, palette);
        s.ox = ox;
        s.oy = oy;
        self.pack.sprites[id as usize] = s;
    }

    /// Finishes the pack.
    pub fn build(mut self) -> AssetPack {
        self.pack.tiles = self.tiles;
        self.pack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_interns_identical_tiles_once() {
        let mut b = PackBuilder::new("t");
        let rows = [
            "--------", "--------", "--------", "--------", "--------", "--------", "--------",
            "--------",
        ];
        let a = b.art(&rows, 0);
        let c = b.art(&rows, 1);
        assert_eq!(a.cells[0].tile, c.cells[0].tile, "same pixels, same tile");
        assert_ne!(a.cells[0].palette, c.cells[0].palette);
    }

    #[test]
    fn art_maps_characters_to_colours() {
        let mut b = PackBuilder::new("t");
        let rows = [
            ".-+#....", "........", "........", "........", "........", "........", "........",
            "........",
        ];
        let s = b.art(&rows, 0);
        let pack = b.build();
        let t = pack.tile(s.cells[0].tile);
        assert_eq!(&t[0..4], &[0, 1, 2, 3]);
    }

    #[test]
    fn blank_art_produces_no_cell() {
        let mut b = PackBuilder::new("t");
        let rows = ["........"; 8];
        let s = b.art(&rows, 0);
        assert!(s.cells[0].is_blank());
    }

    #[test]
    fn eight_by_eight_terrain_is_repeated() {
        let mut b = PackBuilder::new("t");
        let rows = ["-------#"; 8];
        b.terrain(tile::GRASS, &rows, 0);
        let pack = b.build();
        let m = pack.metatile(tile::GRASS);
        assert_eq!(m[0], m[3], "one tile fills all four cells");
    }

    #[test]
    fn importing_tiles_keeps_structure() {
        let mut b = PackBuilder::new("t");
        let rows = ["########"; 8];
        b.terrain(tile::GRASS, &rows, 0);
        let mut pack = b.build();
        let idx = pack.metatile(tile::GRASS)[0].tile;
        let mut replacement = BLANK;
        replacement[0] = 2;
        pack.import_tiles(idx as usize, &[replacement]);
        assert_eq!(pack.metatile(tile::GRASS)[0].tile, idx, "structure held");
        assert_eq!(pack.tile(idx)[0], 2, "pixels changed");
    }

    #[test]
    fn importing_past_the_end_grows_the_array() {
        let mut pack = AssetPack::empty();
        pack.import_tiles(40, &[BLANK; 4]);
        assert_eq!(pack.tile_count(), 44);
    }

    #[test]
    fn unknown_tiles_and_sprites_are_safe_to_ask_for() {
        let pack = AssetPack::empty();
        assert!(pack.metatile(255)[0].is_blank());
        assert_eq!(pack.tile(9999), &BLANK);
    }
}
