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

/// True when any pixel in the image is see-through.
pub fn has_transparency(img: &Image) -> bool {
    img.rgba.as_chunks::<4>().0.iter().any(|px| px[3] < 128)
}

/// An image quantised into 8x8 cells, each fitted to one of several palettes.
pub struct Fitted {
    /// The cells, in reading order.
    pub tiles: Vec<Tile8>,
    /// Which palette each cell was fitted to, parallel to `tiles`.
    pub palettes: Vec<u8>,
    /// The palettes themselves, built from the image's own colours.
    pub table: Vec<Palette>,
}

/// How to quantise an image.
#[derive(Clone, Copy, Debug)]
pub struct FitOptions {
    /// How many palettes to build. The engine has sixteen.
    pub slots: usize,
    /// True when colour 0 means "see through".
    ///
    /// Sprites are drawn over the world, so their first colour is not a
    /// colour at all. That leaves three to draw with, and it matters: fitting
    /// a sprite as though it had four turns its dark outline transparent and
    /// eats the edges of everything.
    pub transparent: bool,
}

impl Default for FitOptions {
    fn default() -> FitOptions {
        FitOptions {
            slots: 16,
            transparent: false,
        }
    }
}

#[inline]
fn channel(c: Rgb, ch: usize) -> u32 {
    (c >> (16 - ch * 8)) & 0xff
}

#[inline]
fn pack_rgb(r: u32, g: u32, b: u32) -> Rgb {
    ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff)
}

/// Squared distance between two colours, in plain RGB.
#[inline]
fn distance(a: Rgb, b: Rgb) -> u32 {
    let mut sum = 0u32;
    for ch in 0..3 {
        let d = channel(a, ch).abs_diff(channel(b, ch));
        sum += d * d;
    }
    sum
}

/// Squared distance between two palettes, over the slots that are in use.
fn palette_distance(a: &[Rgb; 4], b: &[Rgb; 4], first: usize) -> u64 {
    (first..4).map(|i| distance(a[i], b[i]) as u64).sum()
}

/// The best four colours for one cell, darkest first.
///
/// Sorting by brightness and splitting into equal shares of the cell's pixels
/// puts the four colours where the cell actually has detail, rather than at
/// the ends of its range where a single stray highlight would drag them.
fn cell_palette(colors: &mut [(Rgb, u32)], first: usize) -> [Rgb; 4] {
    let mut out = [0 as Rgb; 4];
    if colors.is_empty() {
        return out;
    }
    colors.sort_unstable_by_key(|(c, _)| {
        (
            luma(
                channel(*c, 0) as u8,
                channel(*c, 1) as u8,
                channel(*c, 2) as u8,
            ),
            *c,
        )
    });
    let total: u64 = colors.iter().map(|(_, n)| *n as u64).sum();
    let levels = (4 - first) as u64;
    let mut at = 0usize;
    let mut seen = 0u64;
    for slot in first..4 {
        let until = total * (slot as u64 - first as u64 + 1) / levels;
        let (mut sums, mut count) = ([0u64; 3], 0u64);
        while at < colors.len() && (seen < until || count == 0) {
            let (c, n) = colors[at];
            for (ch, sum) in sums.iter_mut().enumerate() {
                *sum += channel(c, ch) as u64 * n as u64;
            }
            count += n as u64;
            seen += n as u64;
            at += 1;
        }
        out[slot] = match count {
            // Nothing left to average: repeat the shade below rather than
            // inventing one.
            0 => out[slot.saturating_sub(1).max(first)],
            n => pack_rgb(
                (sums[0] / n) as u32,
                (sums[1] / n) as u32,
                (sums[2] / n) as u32,
            ),
        };
    }
    out
}

/// Groups the cells' own palettes into `slots` shared ones.
///
/// Clustering what each cell wants, rather than the image's raw colours, is
/// what makes this work: a sheet that is nine tenths red tunic would
/// otherwise spend every palette on reds and leave nothing for a face.
fn group_palettes(wanted: &[([Rgb; 4], u32)], slots: usize, first: usize) -> Vec<[Rgb; 4]> {
    // Start from the palettes the most cells asked for, skipping any too
    // close to one already chosen so the first guess covers the range.
    let mut tally: Vec<([Rgb; 4], u32)> = Vec::new();
    for (palette, weight) in wanted {
        match tally.iter_mut().find(|(p, _)| p == palette) {
            Some((_, n)) => *n += weight,
            None => tally.push((*palette, *weight)),
        }
    }
    tally.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut centers: Vec<[Rgb; 4]> = Vec::with_capacity(slots);
    for spread in [4000u64, 1000, 200, 0] {
        for (palette, _) in &tally {
            if centers.len() >= slots {
                break;
            }
            if centers
                .iter()
                .all(|c| palette_distance(c, palette, first) > spread)
            {
                centers.push(*palette);
            }
        }
    }
    if centers.is_empty() {
        centers.push([0; 4]);
    }

    // A handful of rounds of Lloyd's algorithm. It converges quickly on data
    // this shape, and stopping early costs a little accuracy and nothing else.
    for _ in 0..8 {
        let mut sums = vec![[[0u64; 3]; 4]; centers.len()];
        let mut counts = vec![0u64; centers.len()];
        let mut moved = false;
        for (palette, weight) in &tally {
            let pick = (0..centers.len())
                .min_by_key(|i| palette_distance(&centers[*i], palette, first))
                .unwrap_or(0);
            counts[pick] += *weight as u64;
            for slot in first..4 {
                for (ch, sum) in sums[pick][slot].iter_mut().enumerate() {
                    *sum += channel(palette[slot], ch) as u64 * *weight as u64;
                }
            }
        }
        for (i, center) in centers.iter_mut().enumerate() {
            if counts[i] == 0 {
                continue;
            }
            for slot in first..4 {
                let c = pack_rgb(
                    (sums[i][slot][0] / counts[i]) as u32,
                    (sums[i][slot][1] / counts[i]) as u32,
                    (sums[i][slot][2] / counts[i]) as u32,
                );
                if c != center[slot] {
                    moved = true;
                }
                center[slot] = c;
            }
        }
        if !moved {
            break;
        }
    }
    centers
}

/// Quantises an image into cells, each fitted to the palette that suits it.
///
/// The engine draws four colours per 8x8 cell out of a table of sixteen
/// palettes, which is the Game Boy Color's arrangement rather than the Game
/// Boy's one palette for everything. Fitting each cell separately is what
/// makes that worth having: grass keeps its greens and the path beside it
/// keeps its browns, where quantising a whole picture at once turns both to
/// mud.
pub fn fit_image(img: &Image, options: FitOptions) -> Fitted {
    let slots = options.slots.clamp(1, 64);
    let first = usize::from(options.transparent);
    let cols = img.width / 8;
    let rows = img.height / 8;
    if cols == 0 || rows == 0 {
        return Fitted {
            tiles: Vec::new(),
            palettes: Vec::new(),
            table: vec![Palette::new(0, 0x555555, 0xaaaaaa, 0xffffff); slots],
        };
    }

    // What each cell would ask for if it had a palette to itself.
    let mut wanted: Vec<([Rgb; 4], u32)> = Vec::with_capacity(cols * rows);
    let mut colors: Vec<(Rgb, u32)> = Vec::with_capacity(64);
    let read_cell = |tx: usize, ty: usize, colors: &mut Vec<(Rgb, u32)>| {
        colors.clear();
        for y in 0..8 {
            for x in 0..8 {
                let (r, g, b, a) = img.pixel(tx * 8 + x, ty * 8 + y);
                if a < 128 {
                    continue;
                }
                // Rounded to five bits a channel, so near-identical shades
                // count as one and a gradient does not outvote a flat area.
                let c = pack_rgb(r as u32 & 0xf8, g as u32 & 0xf8, b as u32 & 0xf8);
                match colors.iter_mut().find(|(k, _)| *k == c) {
                    Some((_, n)) => *n += 1,
                    None => colors.push((c, 1)),
                }
            }
        }
    };
    for ty in 0..rows {
        for tx in 0..cols {
            read_cell(tx, ty, &mut colors);
            let weight = colors.iter().map(|(_, n)| *n).sum::<u32>().max(1);
            wanted.push((cell_palette(&mut colors, first), weight));
        }
    }

    let mut table: Vec<Palette> = group_palettes(&wanted, slots, first)
        .into_iter()
        .map(Palette)
        .collect();
    // A stable order, so the same image always produces the same table.
    table.sort_unstable_by_key(|p| p.0);
    while table.len() < slots {
        table.push(Palette::new(0, 0x555555, 0xaaaaaa, 0xffffff));
    }

    let mut tiles = Vec::with_capacity(cols * rows);
    let mut palettes = Vec::with_capacity(cols * rows);
    for ty in 0..rows {
        for tx in 0..cols {
            // Pick the palette this cell loses the least to, measured on the
            // pixels themselves rather than on the palette it asked for.
            let mut best = (0usize, u64::MAX);
            for (i, palette) in table.iter().enumerate() {
                let mut error = 0u64;
                for y in 0..8 {
                    for x in 0..8 {
                        let (r, g, b, a) = img.pixel(tx * 8 + x, ty * 8 + y);
                        if a < 128 {
                            continue;
                        }
                        let px = pack_rgb(r as u32, g as u32, b as u32);
                        let nearest = (first..4)
                            .map(|j| distance(px, palette.0[j]))
                            .min()
                            .unwrap_or(0);
                        error += nearest as u64;
                    }
                    if error >= best.1 {
                        break;
                    }
                }
                if error < best.1 {
                    best = (i, error);
                }
            }

            let palette = table[best.0];
            let mut tile = BLANK;
            for y in 0..8 {
                for x in 0..8 {
                    let (r, g, b, a) = img.pixel(tx * 8 + x, ty * 8 + y);
                    tile[y * 8 + x] = if a < 128 {
                        0
                    } else {
                        let px = pack_rgb(r as u32, g as u32, b as u32);
                        (first..4)
                            .min_by_key(|j| distance(px, palette.0[*j]))
                            .unwrap_or(first) as u8
                    };
                }
            }
            tiles.push(tile);
            palettes.push(best.0 as u8);
        }
    }
    Fitted {
        tiles,
        palettes,
        table,
    }
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
        let fitted = fit_image(&gradient(64, 8), FitOptions::default());
        assert_eq!(fitted.tiles.len(), 8);
        let mut seen = [false; 4];
        for t in &fitted.tiles {
            for p in t.iter() {
                seen[*p as usize] = true;
            }
        }
        assert!(seen.iter().all(|s| *s), "all four levels should be used");
    }

    #[test]
    fn transparent_pixels_become_colour_zero() {
        let mut img = gradient(8, 8);
        for i in 0..8 {
            img.rgba[i * 4 + 3] = 0;
        }
        let fitted = fit_image(&img, FitOptions::default());
        assert_eq!(&fitted.tiles[0][0..8], &[0u8; 8]);
    }

    #[test]
    fn images_smaller_than_a_tile_produce_nothing() {
        let fitted = fit_image(&gradient(4, 4), FitOptions::default());
        assert!(fitted.tiles.is_empty());
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
