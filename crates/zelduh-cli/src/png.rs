//! A minimal PNG writer.
//!
//! Only enough to look at what the renderer produced: 8-bit RGBA, and deflate
//! "stored" blocks so no compressor is needed. Files are bigger than they need
//! to be, which does not matter for screenshots.

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
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

/// Encodes RGBA pixels as a PNG.
///
/// `pixels` is `width * height` packed `0xAABBGGRR` words, the same layout the
/// renderer produces.
pub fn encode(pixels: &[u32], width: usize, height: usize, scale: usize) -> Vec<u8> {
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

    // A zlib stream of stored deflate blocks.
    let mut z = vec![0x78, 0x01];
    let mut offset = 0;
    while offset < raw.len() {
        let n = (raw.len() - offset).min(65535);
        let last = offset + n >= raw.len();
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[offset..offset + n]);
        offset += n;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, no interlace.
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
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
        assert_eq!(&png[0..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
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
    fn large_images_are_split_into_several_blocks() {
        // 400x400 RGBA is far more than one 65535 byte stored block.
        let px = vec![0u32; 400 * 400];
        let png = encode(&px, 400, 400, 1);
        assert!(png.len() > 65535);
    }
}
