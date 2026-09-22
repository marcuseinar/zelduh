//! Turning ordinary images into four-colour tiles.
//!
//! The web build hands decoded RGBA straight from the browser's image decoder,
//! so PNG and GIF come for free. BMP is handled here as well, because it is the
//! one common format that is trivial to read without pulling in a decoder.

use crate::palette::{Palette, Rgb};
use crate::tile::{Tile8, BLANK};

/// Decoded pixels, eight bits per channel.
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// `width * height * 4` bytes, RGBA.
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: usize, y: usize) -> (u8, u8, u8, u8) {
        if x >= self.width || y >= self.height {
            return (0, 0, 0, 0);
        }
        let i = (y * self.width + x) * 4;
        (
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        )
    }
}

/// Perceived brightness of a colour, 0..=255.
#[inline]
fn luma(r: u8, g: u8, b: u8) -> u32 {
    (r as u32 * 54 + g as u32 * 183 + b as u32 * 19) >> 8
}

/// Quantises an image to four brightness levels and slices it into 8x8 tiles.
///
/// Returns the tiles in reading order along with a palette built from the mean
/// colour of each level, so the result keeps the source's hues.
pub fn tiles_from_image(img: &Image) -> (Vec<Tile8>, Palette) {
    let cols = img.width / 8;
    let rows = img.height / 8;
    if cols == 0 || rows == 0 {
        return (Vec::new(), Palette::new(0, 0x555555, 0xaaaaaa, 0xffffff));
    }

    // Split the brightness range into quartiles so that flat, low-contrast
    // source art still uses all four colours.
    let mut hist = [0u32; 256];
    for y in 0..rows * 8 {
        for x in 0..cols * 8 {
            let (r, g, b, a) = img.pixel(x, y);
            if a >= 128 {
                hist[luma(r, g, b) as usize] += 1;
            }
        }
    }
    let total: u32 = hist.iter().sum();
    let mut cuts = [64u32, 128, 192];
    if total > 0 {
        let mut seen = 0u32;
        let mut next = 0;
        for (v, count) in hist.iter().enumerate() {
            seen += count;
            while next < 3 && seen * 4 >= total * (next as u32 + 1) {
                cuts[next] = v as u32;
                next += 1;
            }
        }
    }

    let level = |r: u8, g: u8, b: u8| -> u8 {
        let l = luma(r, g, b);
        if l <= cuts[0] {
            0
        } else if l <= cuts[1] {
            1
        } else if l <= cuts[2] {
            2
        } else {
            3
        }
    };

    // Average the colours that land in each level to build the palette.
    let mut sums = [[0u64; 3]; 4];
    let mut counts = [0u64; 4];
    for y in 0..rows * 8 {
        for x in 0..cols * 8 {
            let (r, g, b, a) = img.pixel(x, y);
            if a < 128 {
                continue;
            }
            let l = level(r, g, b) as usize;
            sums[l][0] += r as u64;
            sums[l][1] += g as u64;
            sums[l][2] += b as u64;
            counts[l] += 1;
        }
    }
    let mut colors = [0 as Rgb; 4];
    for (i, color) in colors.iter_mut().enumerate() {
        *color = match counts[i] {
            // A level no pixel landed in still needs a colour, so spread the
            // unused ones evenly along the greys.
            0 => {
                let v = (i as u32 * 85) & 0xff;
                (v << 16) | (v << 8) | v
            }
            n => {
                let r = (sums[i][0] / n) as u32;
                let g = (sums[i][1] / n) as u32;
                let b = (sums[i][2] / n) as u32;
                (r << 16) | (g << 8) | b
            }
        };
    }

    let mut tiles = Vec::with_capacity(cols * rows);
    for ty in 0..rows {
        for tx in 0..cols {
            let mut t = BLANK;
            for y in 0..8 {
                for x in 0..8 {
                    let (r, g, b, a) = img.pixel(tx * 8 + x, ty * 8 + y);
                    // Fully transparent pixels become colour 0, which sprites
                    // treat as see-through.
                    t[y * 8 + x] = if a < 128 { 0 } else { level(r, g, b) };
                }
            }
            tiles.push(t);
        }
    }
    (tiles, Palette(colors))
}

/// Decodes an uncompressed 24 or 32 bit BMP.
pub fn decode_bmp(data: &[u8]) -> Option<Image> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return None;
    }
    let read_u32 = |at: usize| -> u32 {
        u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    };
    let read_i32 = |at: usize| -> i32 { read_u32(at) as i32 };
    let pixel_offset = read_u32(10) as usize;
    let header_size = read_u32(14);
    if header_size < 40 {
        return None;
    }
    let width = read_i32(18);
    let height = read_i32(22);
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    let compression = read_u32(30);
    if compression != 0 || !(bpp == 24 || bpp == 32) || width <= 0 || height == 0 {
        return None;
    }
    let (w, flip) = (width as usize, height > 0);
    let h = height.unsigned_abs() as usize;
    let bytes_per_px = (bpp / 8) as usize;
    let row_bytes = (w * bytes_per_px + 3) & !3;
    if pixel_offset + row_bytes * h > data.len() {
        return None;
    }

    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        // BMP rows run bottom to top unless the height is negative.
        let src_row = if flip { h - 1 - y } else { y };
        let base = pixel_offset + src_row * row_bytes;
        for x in 0..w {
            let p = base + x * bytes_per_px;
            let d = (y * w + x) * 4;
            rgba[d] = data[p + 2];
            rgba[d + 1] = data[p + 1];
            rgba[d + 2] = data[p];
            rgba[d + 3] = if bytes_per_px == 4 { data[p + 3] } else { 255 };
        }
    }
    Some(Image {
        width: w,
        height: h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Image {
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = ((x * 255) / w.max(1)) as u8;
                let i = (y * w + x) * 4;
                rgba[i] = v;
                rgba[i + 1] = v;
                rgba[i + 2] = v;
                rgba[i + 3] = 255;
            }
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn a_gradient_uses_all_four_levels() {
        let (tiles, pal) = tiles_from_image(&gradient(64, 8));
        assert_eq!(tiles.len(), 8);
        let mut seen = [false; 4];
        for t in &tiles {
            for p in t.iter() {
                seen[*p as usize] = true;
            }
        }
        assert!(seen.iter().all(|s| *s), "all four levels should be used");
        assert!(pal.0[3] > pal.0[0], "palette should run dark to light");
    }

    #[test]
    fn transparent_pixels_become_colour_zero() {
        let mut img = gradient(8, 8);
        for i in 0..8 {
            img.rgba[i * 4 + 3] = 0;
        }
        let (tiles, _) = tiles_from_image(&img);
        assert_eq!(&tiles[0][0..8], &[0u8; 8]);
    }

    #[test]
    fn images_smaller_than_a_tile_produce_nothing() {
        let (tiles, _) = tiles_from_image(&gradient(4, 4));
        assert!(tiles.is_empty());
    }

    #[test]
    fn bmp_roundtrip() {
        // A 2x2 24-bit BMP: red, green / blue, white.
        let w = 2usize;
        let h = 2usize;
        let row = (w * 3 + 3) & !3;
        let mut data = vec![0u8; 54 + row * h];
        data[0..2].copy_from_slice(b"BM");
        data[10..14].copy_from_slice(&54u32.to_le_bytes());
        data[14..18].copy_from_slice(&40u32.to_le_bytes());
        data[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        data[22..26].copy_from_slice(&(h as i32).to_le_bytes());
        data[28..30].copy_from_slice(&24u16.to_le_bytes());
        // Bottom row first: blue, white.
        data[54..57].copy_from_slice(&[255, 0, 0]);
        data[57..60].copy_from_slice(&[255, 255, 255]);
        // Top row: red, green.
        data[54 + row..54 + row + 3].copy_from_slice(&[0, 0, 255]);
        data[54 + row + 3..54 + row + 6].copy_from_slice(&[0, 255, 0]);

        let img = decode_bmp(&data).expect("should decode");
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.pixel(0, 0), (255, 0, 0, 255), "top left is red");
        assert_eq!(img.pixel(0, 1), (0, 0, 255, 255), "bottom left is blue");
    }

    #[test]
    fn rubbish_is_not_a_bmp() {
        assert!(decode_bmp(b"not a bitmap at all, honestly, no").is_none());
        assert!(decode_bmp(&[]).is_none());
    }
}
