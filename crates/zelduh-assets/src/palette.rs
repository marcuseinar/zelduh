//! Four-colour palettes, in the spirit of the Game Boy Color.
//!
//! Every tile is two bits per pixel, so it can only ever show four colours.
//! Which four is decided by the palette attached to the cell that uses it,
//! which is what lets one grey rock tile be reused as a mossy one.

/// A colour as `0xRRGGBB`. Alpha is implied: index 0 of a sprite palette is
/// transparent, everything else is opaque.
pub type Rgb = u32;

/// Four colours, indexed by the two bits stored per pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette(pub [Rgb; 4]);

impl Palette {
    pub const fn new(a: Rgb, b: Rgb, c: Rgb, d: Rgb) -> Palette {
        Palette([a, b, c, d])
    }

    #[inline]
    pub fn color(&self, index: u8) -> Rgb {
        self.0[(index & 3) as usize]
    }

    /// Converts to the 0xAABBGGRR words a browser canvas wants.
    pub fn to_abgr(&self) -> [u32; 4] {
        let mut out = [0u32; 4];
        for i in 0..4 {
            out[i] = rgb_to_abgr(self.0[i]);
        }
        out
    }
}

/// Converts `0xRRGGBB` to the little-endian RGBA word used by `ImageData`.
#[inline]
pub const fn rgb_to_abgr(c: Rgb) -> u32 {
    let r = (c >> 16) & 0xff;
    let g = (c >> 8) & 0xff;
    let b = c & 0xff;
    0xff00_0000 | (b << 16) | (g << 8) | r
}

/// Indices into the built-in palette table. Art refers to palettes by name so
/// that an imported tileset can recolour everything at once.
pub mod pal {
    pub const GRASS: u8 = 0;
    pub const EARTH: u8 = 1;
    pub const WATER: u8 = 2;
    pub const STONE: u8 = 3;
    pub const DUNGEON: u8 = 4;
    pub const HERO: u8 = 5;
    pub const ENEMY_RED: u8 = 6;
    pub const ENEMY_BLUE: u8 = 7;
    pub const ENEMY_GREEN: u8 = 8;
    pub const BONE: u8 = 9;
    pub const GOLD: u8 = 10;
    pub const HEART: u8 = 11;
    pub const HUD: u8 = 12;
    pub const SHADE: u8 = 13;
    pub const FIRE: u8 = 14;
    pub const BOSS: u8 = 15;
    pub const COUNT: usize = 16;
}

/// The default palette table.
pub const DEFAULT_PALETTES: [Palette; pal::COUNT] = [
    // GRASS: the overworld's greens.
    Palette::new(0xa8d048, 0x78b840, 0x3c7828, 0x183818),
    // EARTH: paths, sand and wooden things.
    Palette::new(0xf0d8a0, 0xd8b070, 0xa07840, 0x503818),
    // WATER.
    Palette::new(0x98e0f8, 0x50a8e0, 0x2868b8, 0x102848),
    // STONE: cliffs, rocks and walls.
    Palette::new(0xd8d8c8, 0xa8a898, 0x686860, 0x282828),
    // DUNGEON: cold indoor stone.
    Palette::new(0xc8c0e0, 0x8880b0, 0x504870, 0x201c30),
    // HERO: index 0 is transparent for sprites.
    Palette::new(0x000000, 0xf8d8a8, 0x58b048, 0x183018),
    // ENEMY_RED.
    Palette::new(0x000000, 0xf8b090, 0xd84838, 0x481010),
    // ENEMY_BLUE.
    Palette::new(0x000000, 0xb0d0f8, 0x4868c8, 0x101838),
    // ENEMY_GREEN.
    Palette::new(0x000000, 0xc0e878, 0x58a038, 0x143010),
    // BONE: skeletons and skulls.
    Palette::new(0x000000, 0xf8f8e8, 0xb0b098, 0x383830),
    // GOLD: rupees, keys and chests.
    Palette::new(0x000000, 0xf8e878, 0xe0a828, 0x604010),
    // HEART.
    Palette::new(0x000000, 0xf8b0b8, 0xe04048, 0x501018),
    // HUD: the status bar.
    Palette::new(0x000000, 0xf8f8f8, 0x909090, 0x181818),
    // SHADE: shadows and smoke.
    Palette::new(0x000000, 0xb8b8b8, 0x606060, 0x202020),
    // FIRE: explosions and fireballs.
    Palette::new(0x000000, 0xf8e068, 0xf07020, 0x902000),
    // BOSS.
    Palette::new(0x000000, 0xe0a8f8, 0x9840c0, 0x300848),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abgr_conversion_puts_red_first() {
        assert_eq!(rgb_to_abgr(0xff0000), 0xff0000ff);
        assert_eq!(rgb_to_abgr(0x0000ff), 0xffff0000);
        assert_eq!(rgb_to_abgr(0x000000), 0xff000000);
    }

    #[test]
    fn palette_indices_wrap() {
        let p = DEFAULT_PALETTES[pal::GRASS as usize];
        assert_eq!(p.color(4), p.color(0));
    }

    #[test]
    fn sprite_palettes_reserve_index_zero() {
        for i in [pal::HERO, pal::ENEMY_RED, pal::GOLD, pal::HEART] {
            assert_eq!(
                DEFAULT_PALETTES[i as usize].0[0],
                0,
                "sprite palette {i} must keep index 0 for transparency"
            );
        }
    }
}
