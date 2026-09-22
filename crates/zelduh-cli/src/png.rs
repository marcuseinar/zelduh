//! A minimal PNG writer.
//!
//! Only enough to look at what the renderer produced, with no compressor:
//! deflate "stored" blocks carry the data as-is. That would make files far
//! larger than they need to be, except that everything this engine draws comes
//! from four-colour palettes, so images are written as indexed PNGs at the
//! smallest bit depth their palette fits in. A 512x512 icon of four colours
//! lands at two bits a pixel rather than thirty-two.

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    let mut c = 0xffff_ffffu32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// The distinct colours in an image, in first-seen order, if there are few
/// enough for an indexed PNG.
fn palette_of(pixels: &[u32]) -> Option<Vec<u32>> {
    let mut palette: Vec<u32> = Vec::new();
    for p in pixels {
        if !palette.contains(p) {
            if palette.len() == 256 {
                return None;
            }
            palette.push(*p);
        }
    }
    Some(palette)
}

/// Bits per pixel for a palette of this size: 1, 2, 4 or 8.
fn bit_depth(colors: usize) -> u8 {
    match colors {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// Encodes RGBA pixels as a PNG.
///
/// `pixels` is `width * height` packed `0xAABBGGRR` words, the same layout the
/// renderer produces. Images with 256 colours or fewer are written as indexed
/// PNGs, which is every image this engine produces.
pub fn encode(pixels: &[u32], width: usize, height: usize, scale: usize) -> Vec<u8> {
    match palette_of(pixels) {
        Some(palette) => encode_indexed(pixels, width, height, scale, &palette),
        None => encode_rgba(pixels, width, height, scale),
    }
}

/// Writes an indexed PNG at the smallest bit depth the palette fits in.
fn encode_indexed(
    pixels: &[u32],
    width: usize,
    height: usize,
    scale: usize,
    palette: &[u32],
) -> Vec<u8> {
    let scale = scale.max(1);
    let (w, h) = (width * scale, height * scale);
    let depth = bit_depth(palette.len());
    let per_byte = 8 / depth as usize;
    let row_bytes = w.div_ceil(per_byte);

    let mut raw = Vec::with_capacity(h * (1 + row_bytes));
    for y in 0..h {
        raw.push(0); // filter: none
        let mut byte = 0u8;
        let mut filled = 0usize;
        for x in 0..w {
            let color = pixels[(y / scale) * width + (x / scale)];
            let index = palette.iter().position(|c| *c == color).unwrap_or(0) as u8;
            byte = (byte << depth) | (index & ((1 << depth) - 1));
            filled += 1;
            if filled == per_byte {
                raw.push(byte);
                byte = 0;
                filled = 0;
            }
        }
        if filled > 0 {
            // The last byte of a row is padded on the right.
            raw.push(byte << (depth as usize * (per_byte - filled)));
        }
    }

    let mut plte = Vec::with_capacity(palette.len() * 3);
    let mut trns = Vec::with_capacity(palette.len());
    let mut needs_trns = false;
    for c in palette {
        plte.push((c & 0xff) as u8);
        plte.push(((c >> 8) & 0xff) as u8);
        plte.push(((c >> 16) & 0xff) as u8);
        let alpha = ((c >> 24) & 0xff) as u8;
        trns.push(alpha);
        needs_trns |= alpha != 0xff;
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[depth, 3, 0, 0, 0]); // indexed colour
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"PLTE", &plte);
    if needs_trns {
        chunk(&mut out, b"tRNS", &trns);
    }
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Writes a plain 8-bit RGBA PNG, for images with too many colours to index.
fn encode_rgba(pixels: &[u32], width: usize, height: usize, scale: usize) -> Vec<u8> {
    let scale = scale.max(1);
    let (w, h) = (width * scale, height * scale);

    // Raw scanlines, each with a zero filter byte.
    let mut raw = Vec::with_capacity(h * (1 + w * 4));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = pixels[(y / scale) * width + (x / scale)];
            raw.push((p & 0xff) as u8);
            raw.push(((p >> 8) & 0xff) as u8);
            raw.push(((p >> 16) & 0xff) as u8);
            raw.push(((p >> 24) & 0xff) as u8);
        }
    }

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, no interlace.
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Wraps bytes in a zlib stream of stored deflate blocks.
fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let mut offset = 0;
    loop {
        let n = (raw.len() - offset).min(65535);
        let last = offset + n >= raw.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[offset..offset + n]);
        offset += n;
        if last {
            break;
        }
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_a_known_value() {
        // The CRC of "IEND" with no data, as it appears in every PNG.
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn adler_of_empty_is_one() {
        assert_eq!(adler32(&[]), 1);
    }

    #[test]
    fn output_starts_with_the_png_signature() {
        let png = encode(&[0xff00_00ff; 4], 2, 2, 1);
        assert_eq!(
            &png[0..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
        assert_eq!(&png[12..16], b"IHDR");
        assert!(png.ends_with(&crc32(b"IEND").to_be_bytes()));
    }

    #[test]
    fn scaling_multiplies_the_declared_size() {
        let png = encode(&[0; 4], 2, 2, 3);
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((w, h), (6, 6));
    }

    #[test]
    fn a_four_colour_image_is_indexed_at_two_bits_a_pixel() {
        let px: Vec<u32> = (0..64)
            .map(|i| [0xff000000u32, 0xff0000ff, 0xff00ff00, 0xffff0000][i % 4])
            .collect();
        let png = encode(&px, 8, 8, 1);
        assert_eq!(png[24], 2, "bit depth should be 2");
        assert_eq!(png[25], 3, "colour type should be indexed");
        assert_eq!(&png[37..41], b"PLTE");
        // Eight rows of eight two-bit pixels: two bytes plus a filter byte.
        assert!(png.len() < 200, "{} bytes", png.len());
    }

    #[test]
    fn indexing_survives_scaling_and_odd_widths() {
        // Five pixels wide at two bits each does not fill a whole byte.
        let px = vec![
            0xff112233u32,
            0xff445566,
            0xff112233,
            0xff445566,
            0xff112233,
        ];
        let png = encode(&px, 5, 1, 3);
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        assert_eq!(w, 15);
        assert_eq!(png[24], 1, "two colours need only one bit");
    }

    #[test]
    fn transparency_is_carried_into_the_palette() {
        let px = vec![0x00000000u32, 0xffffffff];
        let png = encode(&px, 2, 1, 1);
        assert!(
            png.windows(4).any(|w| w == b"tRNS"),
            "an image with alpha needs a tRNS chunk"
        );
    }

    #[test]
    fn many_coloured_images_fall_back_to_rgba() {
        let px: Vec<u32> = (0..1000).map(|i| 0xff000000 | i as u32).collect();
        let png = encode(&px, 1000, 1, 1);
        assert_eq!(png[24], 8, "bit depth 8");
        assert_eq!(png[25], 6, "colour type RGBA");
    }

    #[test]
    fn large_images_are_split_into_several_blocks() {
        // 400x400 RGBA is far more than one 65535 byte stored block.
        let px: Vec<u32> = (0..400 * 400)
            .map(|i| 0xff000000 | (i as u32 * 7919))
            .collect();
        let png = encode_rgba(&px, 400, 400, 1);
        assert!(png.len() > 65535);
    }
}
