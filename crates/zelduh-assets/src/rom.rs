//! Reading graphics out of a Game Boy ROM the player supplies.
//!
//! No game data ships with this engine. A cartridge image is parsed only as a
//! container: the header is read to identify it, and stretches of the ROM are
//! decoded as 2bpp tiles so they can be dropped into an asset pack. Which
//! stretches hold graphics is not recorded anywhere in a ROM, so the scanner
//! scores candidate regions and lets the player pick.

use crate::tile::{decode_2bpp, is_flat, Tile8};

/// Bytes per 2bpp tile.
pub const TILE_BYTES: usize = 16;
/// Bytes in one ROM bank.
pub const BANK_BYTES: usize = 16 * 1024;
/// The Nintendo logo bitmap every real cartridge carries at 0x104.
const LOGO: [u8; 16] = [
    0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0c, 0x00, 0x0d,
];

/// What the cartridge header says about a ROM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomInfo {
    /// The internal title, trimmed of padding.
    pub title: String,
    /// True when the first bytes of the logo area match a real cartridge.
    pub has_logo: bool,
    /// True when the header declares Game Boy Color support.
    pub color: bool,
    /// Cartridge type byte, which implies the mapper.
    pub cart_type: u8,
    /// ROM size in bytes as declared by the header.
    pub declared_size: usize,
    /// Actual size of the data handed in.
    pub actual_size: usize,
    /// True when the header checksum matches.
    pub header_checksum_ok: bool,
    /// Number of 16 KiB banks in the data.
    pub banks: usize,
}

impl RomInfo {
    /// True when this really looks like a Game Boy ROM rather than some other
    /// file that happened to be dropped on the page.
    pub fn looks_like_gameboy(&self) -> bool {
        self.has_logo && self.header_checksum_ok && self.actual_size >= 32 * 1024
    }
}

/// Reads the cartridge header.
pub fn info(data: &[u8]) -> RomInfo {
    let title_bytes = data.get(0x134..0x144).unwrap_or(&[]);
    let title: String = title_bytes
        .iter()
        .take_while(|b| **b != 0)
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|b| *b as char)
        .collect();
    let has_logo = data
        .get(0x104..0x104 + LOGO.len())
        .map(|s| s == LOGO)
        .unwrap_or(false);
    let color = matches!(data.get(0x143), Some(0x80) | Some(0xc0));
    let cart_type = data.get(0x147).copied().unwrap_or(0);
    let size_code = data.get(0x148).copied().unwrap_or(0);
    let declared_size = if size_code <= 8 {
        (32 * 1024usize) << size_code
    } else {
        0
    };

    // The header checksum covers 0x134..=0x14c.
    let mut sum: u8 = 0;
    for b in data.get(0x134..=0x14c).unwrap_or(&[]) {
        sum = sum.wrapping_sub(*b).wrapping_sub(1);
    }
    let header_checksum_ok = data.get(0x14d).map(|c| *c == sum).unwrap_or(false);

    RomInfo {
        title: title.trim().to_string(),
        has_logo,
        color,
        cart_type,
        declared_size,
        actual_size: data.len(),
        header_checksum_ok,
        banks: data.len() / BANK_BYTES,
    }
}

/// Decodes `count` tiles starting at a byte offset.
pub fn decode_tiles(data: &[u8], offset: usize, count: usize) -> Vec<Tile8> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = offset + i * TILE_BYTES;
        let end = start + TILE_BYTES;
        if end > data.len() {
            break;
        }
        out.push(decode_2bpp(&data[start..end]));
    }
    out
}

/// A stretch of ROM that looks like it holds tile graphics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub offset: usize,
    pub len: usize,
    /// Higher is more likely to be graphics. Roughly 0..1000.
    pub score: u32,
}

/// Scores how much a block of tiles looks like artwork rather than code.
///
/// Graphics have a lot of structure: neighbouring pixels agree, rows repeat
/// vertically, and few tiles are completely flat. Code and tables look like
/// noise under the same measurements.
pub fn score_block(data: &[u8], offset: usize, tiles: usize) -> u32 {
    let decoded = decode_tiles(data, offset, tiles);
    if decoded.len() < tiles / 2 || decoded.is_empty() {
        return 0;
    }
    let mut flat = 0usize;
    let mut runs = 0u64;
    let mut vertical = 0u64;
    let mut samples = 0u64;
    for t in &decoded {
        if is_flat(t) {
            flat += 1;
            continue;
        }
        for y in 0..8 {
            for x in 0..7 {
                if t[y * 8 + x] == t[y * 8 + x + 1] {
                    runs += 1;
                }
                samples += 1;
            }
        }
        for y in 0..7 {
            for x in 0..8 {
                if t[y * 8 + x] == t[(y + 1) * 8 + x] {
                    vertical += 1;
                }
            }
        }
    }
    if samples == 0 {
        return 0;
    }
    let horizontal_ratio = runs * 500 / samples;
    let vertical_ratio = vertical * 500 / (samples + 1);
    let flat_penalty = (flat * 1000 / decoded.len().max(1)) as u64;
    (horizontal_ratio + vertical_ratio).saturating_sub(flat_penalty) as u32
}

/// Finds the stretches of a ROM most likely to hold tile graphics.
///
/// Returns regions sorted best first. `window_tiles` is how many tiles each
/// candidate covers; 128 tiles is one 2 KiB block, about the size of one set
/// of character graphics.
pub fn scan(data: &[u8], window_tiles: usize) -> Vec<Region> {
    let window = window_tiles * TILE_BYTES;
    if data.len() < window {
        return Vec::new();
    }
    let mut regions = Vec::new();
    let step = window;
    let mut offset = 0;
    while offset + window <= data.len() {
        let score = score_block(data, offset, window_tiles);
        regions.push(Region {
            offset,
            len: window,
            score,
        });
        offset += step;
    }
    regions.sort_by(|a, b| b.score.cmp(&a.score).then(a.offset.cmp(&b.offset)));
    regions.truncate(64);
    regions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::encode_2bpp;

    /// Builds a minimal but valid-looking ROM image.
    fn fake_rom(title: &str) -> Vec<u8> {
        let mut rom = vec![0u8; 32 * 1024];
        rom[0x104..0x104 + LOGO.len()].copy_from_slice(&LOGO);
        for (i, b) in title.bytes().take(15).enumerate() {
            rom[0x134 + i] = b;
        }
        rom[0x143] = 0x80; // Game Boy Color aware.
        rom[0x147] = 0x1b;
        rom[0x148] = 0x00; // 32 KiB.
        let mut sum: u8 = 0;
        for b in &rom[0x134..=0x14c] {
            sum = sum.wrapping_sub(*b).wrapping_sub(1);
        }
        rom[0x14d] = sum;
        rom
    }

    #[test]
    fn reads_the_header() {
        let rom = fake_rom("ZELDUH");
        let i = info(&rom);
        assert_eq!(i.title, "ZELDUH");
        assert!(i.has_logo);
        assert!(i.color);
        assert!(i.header_checksum_ok);
        assert_eq!(i.declared_size, 32 * 1024);
        assert_eq!(i.banks, 2);
        assert!(i.looks_like_gameboy());
    }

    #[test]
    fn rejects_a_file_that_is_not_a_rom() {
        let junk = b"this is a text file, not a cartridge".repeat(100);
        let i = info(&junk);
        assert!(!i.looks_like_gameboy());
    }

    #[test]
    fn a_truncated_file_does_not_panic() {
        for n in [0usize, 1, 16, 0x140, 0x14d] {
            let _ = info(&vec![0u8; n]);
        }
    }

    #[test]
    fn decoding_stops_at_the_end_of_the_data() {
        let data = vec![0xffu8; TILE_BYTES * 3 + 4];
        let tiles = decode_tiles(&data, 0, 10);
        assert_eq!(tiles.len(), 3);
    }

    #[test]
    fn graphics_score_higher_than_noise() {
        // A block of smooth, structured tiles.
        let mut art = Vec::new();
        for n in 0..128 {
            let mut t = [0u8; 64];
            for y in 0..8 {
                for x in 0..8 {
                    t[y * 8 + x] = if (x + n / 16) % 8 < 4 { 1 } else { 2 };
                }
            }
            art.extend_from_slice(&encode_2bpp(&t));
        }
        // A block of pseudo-random bytes standing in for code.
        let mut noise = Vec::new();
        let mut state = 12345u32;
        for _ in 0..128 * TILE_BYTES {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            noise.push((state >> 16) as u8);
        }
        let art_score = score_block(&art, 0, 128);
        let noise_score = score_block(&noise, 0, 128);
        assert!(
            art_score > noise_score,
            "art {art_score} should beat noise {noise_score}"
        );
    }

    #[test]
    fn flat_blocks_score_nothing() {
        let empty = vec![0u8; 128 * TILE_BYTES];
        assert_eq!(score_block(&empty, 0, 128), 0);
    }

    #[test]
    fn scan_returns_regions_best_first() {
        let mut rom = fake_rom("ART");
        // Drop recognisable graphics in the middle of the ROM.
        let mut t = [0u8; 64];
        for y in 0..8 {
            for x in 0..8 {
                t[y * 8 + x] = if x < 4 { 3 } else { 1 };
            }
        }
        let enc = encode_2bpp(&t);
        for i in 0..128 {
            let at = 0x4000 + i * TILE_BYTES;
            rom[at..at + TILE_BYTES].copy_from_slice(&enc);
        }
        let regions = scan(&rom, 128);
        assert!(!regions.is_empty());
        assert!(regions.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(regions[0].offset, 0x4000);
    }
}
