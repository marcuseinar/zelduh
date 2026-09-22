//! Eight by eight pixel tiles, two bits per pixel.

/// One 8x8 tile as 64 pixel indices in the range 0..=3.
pub type Tile8 = [u8; 64];

/// An empty tile.
pub const BLANK: Tile8 = [0; 64];

/// Horizontal flip bit for a [`Cell`].
pub const FLIP_X: u8 = 1;
/// Vertical flip bit for a [`Cell`].
pub const FLIP_Y: u8 = 2;

/// One 8x8 cell of a composed image: which tile, in which palette, flipped how.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cell {
    pub tile: u16,
    pub palette: u8,
    pub flip: u8,
}

impl Cell {
    pub const fn new(tile: u16, palette: u8) -> Cell {
        Cell {
            tile,
            palette,
            flip: 0,
        }
    }

    pub const fn flipped(tile: u16, palette: u8, flip: u8) -> Cell {
        Cell {
            tile,
            palette,
            flip,
        }
    }

    /// True when the cell refers to no tile at all.
    pub fn is_blank(&self) -> bool {
        self.tile == u16::MAX
    }

    /// A cell that draws nothing.
    pub const BLANK: Cell = Cell {
        tile: u16::MAX,
        palette: 0,
        flip: 0,
    };
}

/// Reads a pixel from a tile, applying flips.
#[inline]
pub fn pixel(t: &Tile8, x: usize, y: usize, flip: u8) -> u8 {
    let x = if flip & FLIP_X != 0 { 7 - x } else { x };
    let y = if flip & FLIP_Y != 0 { 7 - y } else { y };
    t[y * 8 + x]
}

/// Decodes one Game Boy 2bpp tile: 16 bytes, two bitplanes interleaved by row.
pub fn decode_2bpp(bytes: &[u8]) -> Tile8 {
    let mut t = BLANK;
    for y in 0..8 {
        let lo = bytes.get(y * 2).copied().unwrap_or(0);
        let hi = bytes.get(y * 2 + 1).copied().unwrap_or(0);
        for x in 0..8 {
            let bit = 7 - x;
            let v = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);
            t[y * 8 + x] = v;
        }
    }
    t
}

/// Encodes a tile back to the Game Boy's 16-byte 2bpp form.
pub fn encode_2bpp(t: &Tile8) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (y, row) in out.chunks_mut(2).enumerate() {
        let (mut lo, mut hi) = (0u8, 0u8);
        for x in 0..8 {
            let v = t[y * 8 + x] & 3;
            let bit = 7 - x;
            lo |= (v & 1) << bit;
            hi |= ((v >> 1) & 1) << bit;
        }
        row[0] = lo;
        row[1] = hi;
    }
    out
}

/// True when every pixel in the tile is the same. Used when guessing whether a
/// stretch of a ROM holds graphics or something else.
pub fn is_flat(t: &Tile8) -> bool {
    t.iter().all(|p| *p == t[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bpp_roundtrips() {
        let mut t = BLANK;
        for (i, px) in t.iter_mut().enumerate() {
            *px = (i % 4) as u8;
        }
        assert_eq!(decode_2bpp(&encode_2bpp(&t)), t);
    }

    #[test]
    fn two_bpp_decodes_a_known_pattern() {
        // Low plane all set, high plane clear: every pixel is colour 1.
        let bytes = [0xff, 0x00].repeat(8);
        let t = decode_2bpp(&bytes);
        assert!(t.iter().all(|p| *p == 1));
        // Both planes set: colour 3.
        let bytes = [0xff, 0xff].repeat(8);
        assert!(decode_2bpp(&bytes).iter().all(|p| *p == 3));
    }

    #[test]
    fn decode_tolerates_short_input() {
        let t = decode_2bpp(&[0xff, 0xff]);
        assert_eq!(t[0], 3);
        assert_eq!(t[63], 0);
    }

    #[test]
    fn flips_mirror_pixels() {
        let mut t = BLANK;
        t[0] = 3;
        assert_eq!(pixel(&t, 0, 0, 0), 3);
        assert_eq!(pixel(&t, 7, 0, FLIP_X), 3);
        assert_eq!(pixel(&t, 0, 7, FLIP_Y), 3);
        assert_eq!(pixel(&t, 7, 7, FLIP_X | FLIP_Y), 3);
    }

    #[test]
    fn flat_detection() {
        assert!(is_flat(&BLANK));
        let mut t = BLANK;
        t[5] = 1;
        assert!(!is_flat(&t));
    }
}
