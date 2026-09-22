//! Graphics for Zelduh: the built-in art, plus importers that can replace it
//! with tiles from a Game Boy ROM or an image file the player supplies.
//!
//! The split matters. [`pack::AssetPack`] holds *structure* — which cells make
//! up a bush, which make up the hero walking left — separately from pixels, so
//! importing a tileset redraws the world without changing how it plays. No
//! game data ships here; a cartridge is only ever read from the player's own
//! file.

pub mod builtin;
pub mod image;
pub mod pack;
pub mod palette;
pub mod profile;
pub mod rom;
pub mod tile;

pub use pack::{AssetPack, Source, Sprite, SpriteId};
pub use palette::{pal, Palette};
pub use tile::{Cell, Tile8};

/// What sort of file was handed to [`load`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    /// A Game Boy or Game Boy Color cartridge image.
    Rom,
    /// A raw 2bpp tile dump, as produced by tile rippers (`.chr`, `.2bpp`).
    RawTiles,
    /// A bitmap image.
    Bmp,
    /// Already-decoded RGBA, as the browser's image decoder produces.
    Rgba { width: usize, height: usize },
    /// A text profile describing where a ROM keeps its graphics.
    Profile,
}

/// Guesses what a file is from its name and contents.
pub fn sniff(name: &str, data: &[u8]) -> Option<FileKind> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zprofile") || lower.ends_with(".txt") {
        return Some(FileKind::Profile);
    }
    if data.starts_with(b"BM") {
        return Some(FileKind::Bmp);
    }
    if rom::info(data).looks_like_gameboy() {
        return Some(FileKind::Rom);
    }
    if lower.ends_with(".gb") || lower.ends_with(".gbc") {
        return Some(FileKind::Rom);
    }
    if lower.ends_with(".chr") || lower.ends_with(".2bpp") || lower.ends_with(".bin") {
        return Some(FileKind::RawTiles);
    }
    // A text file is probably a profile -- but only if it really is text.
    // Plenty of binary files are technically all-ASCII.
    let printable = data
        .iter()
        .all(|b| b.is_ascii_graphic() || matches!(b, b' ' | b'\n' | b'\r' | b'\t'));
    if printable && data.iter().any(|b| b.is_ascii_alphabetic()) {
        return Some(FileKind::Profile);
    }
    None
}

/// What happened when a file was loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadReport {
    pub kind: &'static str,
    pub detail: String,
    pub tiles_imported: usize,
    pub warnings: Vec<String>,
}

/// Loads a file into a pack, replacing its pixels.
///
/// The pack keeps its structure, so whatever comes in is immediately visible
/// as terrain and sprites.
pub fn load(pack: &mut AssetPack, name: &str, data: &[u8], kind: FileKind) -> LoadReport {
    let mut warnings = Vec::new();
    match kind {
        FileKind::Rom => {
            let info = rom::info(data);
            if !info.looks_like_gameboy() {
                warnings.push("this does not look like a Game Boy ROM".to_string());
            }
            // Pick the stretch that scores highest as artwork.
            let regions = rom::scan(data, 256);
            let offset = regions.first().map(|r| r.offset).unwrap_or(0);
            let tiles = rom::decode_tiles(data, offset, pack.tile_count());
            let n = pack.import_tiles(0, &tiles);
            pack.source = Source::Rom {
                title: info.title.clone(),
                offset,
            };
            pack.name = if info.title.is_empty() {
                name.to_string()
            } else {
                info.title.clone()
            };
            LoadReport {
                kind: "rom",
                detail: format!(
                    "{} ({} KiB, {} banks) tiles from 0x{offset:05x}",
                    if info.title.is_empty() {
                        name
                    } else {
                        &info.title
                    },
                    info.actual_size / 1024,
                    info.banks
                ),
                tiles_imported: n,
                warnings,
            }
        }
        FileKind::RawTiles => {
            let tiles = rom::decode_tiles(data, 0, data.len() / rom::TILE_BYTES);
            let n = pack.import_tiles(0, &tiles);
            pack.source = Source::File {
                name: name.to_string(),
            };
            LoadReport {
                kind: "tiles",
                detail: format!("{n} tiles from {name}"),
                tiles_imported: n,
                warnings,
            }
        }
        FileKind::Bmp => match image::decode_bmp(data) {
            Some(img) => {
                let (tiles, palette) = image::tiles_from_image(&img);
                let n = pack.import_tiles(0, &tiles);
                pack.import_palettes(0, &[palette]);
                pack.source = Source::File {
                    name: name.to_string(),
                };
                LoadReport {
                    kind: "bmp",
                    detail: format!("{}x{} image, {n} tiles", img.width, img.height),
                    tiles_imported: n,
                    warnings,
                }
            }
            None => LoadReport {
                kind: "bmp",
                detail: "could not decode this bitmap".to_string(),
                tiles_imported: 0,
                warnings: vec!["only uncompressed 24 or 32 bit BMP is supported".to_string()],
            },
        },
        FileKind::Rgba { width, height } => {
            let img = image::Image {
                width,
                height,
                rgba: data.to_vec(),
            };
            let (tiles, palette) = image::tiles_from_image(&img);
            let n = pack.import_tiles(0, &tiles);
            pack.import_palettes(0, &[palette]);
            pack.source = Source::File {
                name: name.to_string(),
            };
            LoadReport {
                kind: "image",
                detail: format!("{width}x{height} image, {n} tiles"),
                tiles_imported: n,
                warnings,
            }
        }
        FileKind::Profile => {
            let text = String::from_utf8_lossy(data);
            let profile = profile::parse(&text);
            for (line, msg) in &profile.errors {
                warnings.push(format!("line {line}: {msg}"));
            }
            let bound = profile.terrain.len() + profile.sprites.len();
            profile.apply(pack);
            LoadReport {
                kind: "profile",
                detail: format!("{bound} bindings from {name}"),
                tiles_imported: 0,
                warnings,
            }
        }
    }
}

/// Applies a profile's `import` directives against ROM data.
///
/// Kept separate from [`load`] because a profile and the cartridge it
/// describes arrive as two different files.
pub fn apply_profile_imports(
    pack: &mut AssetPack,
    profile: &profile::Profile,
    rom_data: &[u8],
) -> usize {
    let mut total = 0;
    for d in &profile.imports {
        let profile::Directive::Import {
            offset,
            count,
            base,
        } = d;
        let tiles = rom::decode_tiles(rom_data, *offset, *count);
        total += pack.import_tiles(*base, &tiles);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing_common_files() {
        assert_eq!(sniff("game.gb", &[0u8; 64]), Some(FileKind::Rom));
        assert_eq!(sniff("tiles.chr", &[0u8; 64]), Some(FileKind::RawTiles));
        assert_eq!(sniff("art.bmp", b"BM\0\0"), Some(FileKind::Bmp));
        assert_eq!(sniff("map.zprofile", b"name x"), Some(FileKind::Profile));
        assert_eq!(
            sniff("notes", b"terrain BUSH 1 1 1 1"),
            Some(FileKind::Profile)
        );
    }

    #[test]
    fn binary_rubbish_is_not_mistaken_for_text() {
        assert_eq!(sniff("mystery", &[0u8, 1, 2, 3]), None);
        assert_eq!(sniff("mystery", &[]), None);
    }

    #[test]
    fn loading_raw_tiles_replaces_pixels_but_not_structure() {
        let mut pack = builtin::pack();
        let before = *pack.metatile(zelduh_core::tiles::tile::BUSH);
        // 64 tiles of solid colour 3.
        let data = vec![0xffu8; 64 * rom::TILE_BYTES];
        let report = load(&mut pack, "ripped.chr", &data, FileKind::RawTiles);
        assert_eq!(report.tiles_imported, 64);
        assert_eq!(*pack.metatile(zelduh_core::tiles::tile::BUSH), before);
        assert!(pack.tile(1).iter().all(|p| *p == 3));
    }

    #[test]
    fn loading_a_profile_reports_its_mistakes() {
        let mut pack = builtin::pack();
        let report = load(
            &mut pack,
            "x.zprofile",
            b"terrain BUSH 1 1 1 1\nnonsense here",
            FileKind::Profile,
        );
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("line 2"));
    }

    #[test]
    fn a_profile_import_pulls_tiles_out_of_a_rom() {
        let mut pack = builtin::pack();
        let p = profile::parse("import 0x10 4 0");
        let mut data = vec![0u8; 0x100];
        for b in data[0x10..0x50].iter_mut() {
            *b = 0xff;
        }
        let n = apply_profile_imports(&mut pack, &p, &data);
        assert_eq!(n, 4);
        assert!(pack.tile(0).iter().all(|v| *v == 3));
    }
}
