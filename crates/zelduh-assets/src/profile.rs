//! Profiles: text files that say where a ROM keeps its graphics.
//!
//! Nothing in a cartridge says "the bushes live here", so pointing the engine
//! at a ROM alone gives a remix rather than a faithful tileset. A profile is
//! how that knowledge gets written down and shared: a few lines of text that
//! import tiles from an offset and then bind them to terrain and sprites.
//!
//! ```text
//! name       My Cartridge
//! palette    GRASS 0f380f 306230 8bac0f 9bbc0f
//! import     0x30000 256          # 256 tiles from this offset
//! terrain    GRASS 12 12 12 12 pal=GRASS
//! sprite     HeroDown0 2 2  40 41 48 49 pal=HERO
//! ```

use crate::pack::{AssetPack, Sprite, SpriteId};
use crate::palette::Palette;
use crate::tile::Cell;
use zelduh_core::tiles::tile;

include!("names.rs");

/// Looks up a sprite by the name used in profiles.
pub fn sprite_by_name(name: &str) -> Option<SpriteId> {
    SPRITE_NAMES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, id)| *id)
}

/// Looks up a terrain tile id by name.
pub fn tile_by_name(name: &str) -> Option<u8> {
    TILE_NAMES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, id)| *id)
}

/// Looks up a palette slot by name.
pub fn palette_by_name(name: &str) -> Option<u8> {
    use crate::palette::pal;
    Some(match name.to_ascii_uppercase().as_str() {
        "GRASS" => pal::GRASS,
        "EARTH" => pal::EARTH,
        "WATER" => pal::WATER,
        "STONE" => pal::STONE,
        "DUNGEON" => pal::DUNGEON,
        "HERO" => pal::HERO,
        "ENEMY_RED" => pal::ENEMY_RED,
        "ENEMY_BLUE" => pal::ENEMY_BLUE,
        "ENEMY_GREEN" => pal::ENEMY_GREEN,
        "BONE" => pal::BONE,
        "GOLD" => pal::GOLD,
        "HEART" => pal::HEART,
        "HUD" => pal::HUD,
        "SHADE" => pal::SHADE,
        "FIRE" => pal::FIRE,
        "BOSS" => pal::BOSS,
        other => return other.parse::<u8>().ok(),
    })
}

/// Parses a number written as decimal, or hex with a `0x` prefix.
fn number(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

/// A cell written as `12`, `12:x` (flipped horizontally) or `-` for nothing.
fn cell(token: &str, palette: u8) -> Option<Cell> {
    if token == "-" || token == "." {
        return Some(Cell::BLANK);
    }
    let (num, flips) = match token.split_once(':') {
        Some((n, f)) => (n, f),
        None => (token, ""),
    };
    let tile = number(num)? as u16;
    let mut flip = 0;
    if flips.contains('x') {
        flip |= crate::tile::FLIP_X;
    }
    if flips.contains('y') {
        flip |= crate::tile::FLIP_Y;
    }
    Some(Cell::flipped(tile, palette, flip))
}

/// Something a profile asks for that the loader has to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Directive {
    /// Import `count` tiles from a ROM offset into the pack at `base`.
    Import {
        offset: usize,
        count: usize,
        base: usize,
    },
}

/// The palette a cell gets when its line does not name one: whichever the
/// quantiser fitted that tile to when its image was imported.
pub const AUTO_PALETTE: u8 = u8::MAX;

/// The result of reading a profile.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub name: String,
    pub imports: Vec<Directive>,
    pub palettes: Vec<(u8, Palette)>,
    pub terrain: Vec<(u8, [Cell; 4])>,
    pub sprites: Vec<(SpriteId, Sprite)>,
    /// Lines that could not be understood, with their line numbers.
    pub errors: Vec<(usize, String)>,
}

impl Profile {
    /// Applies everything except imports, which need the ROM bytes.
    pub fn apply(&self, pack: &mut AssetPack) {
        if !self.name.is_empty() {
            pack.name = self.name.clone();
        }
        for (slot, p) in &self.palettes {
            pack.import_palettes(*slot as usize, &[*p]);
        }
        for (id, meta) in &self.terrain {
            if (*id as usize) < pack.metatiles.len() {
                let mut meta = *meta;
                for c in meta.iter_mut() {
                    resolve(pack, c);
                }
                pack.metatiles[*id as usize] = meta;
            }
        }
        for (id, s) in &self.sprites {
            let mut s = s.clone();
            for c in s.cells.iter_mut() {
                resolve(pack, c);
            }
            pack.sprites[*id as usize] = s;
        }
    }
}

/// Fills in a cell's palette from the tile it points at, when the profile
/// left the choice open.
fn resolve(pack: &AssetPack, cell: &mut Cell) {
    if cell.palette != AUTO_PALETTE {
        return;
    }
    cell.palette = if cell.is_blank() {
        0
    } else {
        pack.fitted_palette(cell.tile).unwrap_or(0)
    };
}

/// Reads a profile. Unknown lines are collected as errors rather than
/// stopping the load, so one typo cannot cost you the rest of the file.
pub fn parse(text: &str) -> Profile {
    let mut p = Profile::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words: Vec<&str> = line.split_whitespace().collect();

        // A trailing `pal=NAME` sets the palette for this line's cells.
        // Without one, each cell keeps whatever palette it was fitted to when
        // its image was imported.
        let mut palette = AUTO_PALETTE;
        if let Some(pos) = words.iter().position(|w| w.starts_with("pal=")) {
            let want = words.remove(pos)[4..].to_string();
            match palette_by_name(&want) {
                Some(v) => palette = v,
                None => p.errors.push((n + 1, format!("unknown palette {want}"))),
            }
        }

        let keyword = words[0].to_ascii_lowercase();
        let args = &words[1..];
        match keyword.as_str() {
            "name" => p.name = args.join(" "),
            "palette" => match (
                args.first().and_then(|a| palette_by_name(a)),
                args.len() >= 5,
            ) {
                (Some(slot), true) => {
                    let mut colors = [0u32; 4];
                    let mut ok = true;
                    for i in 0..4 {
                        match u32::from_str_radix(args[i + 1].trim_start_matches('#'), 16) {
                            Ok(v) => colors[i] = v,
                            Err(_) => ok = false,
                        }
                    }
                    if ok {
                        p.palettes.push((slot, Palette(colors)));
                    } else {
                        p.errors.push((n + 1, "bad colour".to_string()));
                    }
                }
                _ => p
                    .errors
                    .push((n + 1, "palette needs a slot and 4 colours".into())),
            },
            "import" => {
                let offset = args.first().and_then(|a| number(a));
                let count = args.get(1).and_then(|a| number(a));
                let base = args.get(2).and_then(|a| number(a)).unwrap_or(0);
                match (offset, count) {
                    (Some(o), Some(c)) if o >= 0 && c > 0 => p.imports.push(Directive::Import {
                        offset: o as usize,
                        count: c as usize,
                        base: base.max(0) as usize,
                    }),
                    _ => p
                        .errors
                        .push((n + 1, "import needs an offset and a count".into())),
                }
            }
            "terrain" => {
                let id = args
                    .first()
                    .and_then(|a| tile_by_name(a).or_else(|| number(a).map(|v| v as u8)));
                match id {
                    Some(id) if args.len() >= 5 => {
                        let mut cells = [Cell::BLANK; 4];
                        let mut ok = true;
                        for i in 0..4 {
                            match cell(args[i + 1], palette) {
                                Some(c) => cells[i] = c,
                                None => ok = false,
                            }
                        }
                        if ok {
                            p.terrain.push((id, cells));
                        } else {
                            p.errors.push((n + 1, "bad cell".to_string()));
                        }
                    }
                    Some(_) => p.errors.push((n + 1, "terrain needs 4 cells".into())),
                    None => p
                        .errors
                        .push((n + 1, format!("unknown terrain {}", args[0]))),
                }
            }
            "sprite" => {
                let id = args.first().and_then(|a| sprite_by_name(a));
                let cols = args.get(1).and_then(|a| number(a)).unwrap_or(0);
                let rows = args.get(2).and_then(|a| number(a)).unwrap_or(0);
                let want = (cols * rows) as usize;
                match id {
                    Some(id) if want > 0 && args.len() >= 3 + want => {
                        let mut cells = Vec::with_capacity(want);
                        let mut ok = true;
                        for i in 0..want {
                            match cell(args[3 + i], palette) {
                                Some(c) => cells.push(c),
                                None => ok = false,
                            }
                        }
                        if ok {
                            p.sprites.push((
                                id,
                                Sprite {
                                    cols: cols as u8,
                                    rows: rows as u8,
                                    cells,
                                    ox: 0,
                                    oy: 0,
                                },
                            ));
                        } else {
                            p.errors.push((n + 1, "bad cell".to_string()));
                        }
                    }
                    Some(_) => p
                        .errors
                        .push((n + 1, "sprite needs cols, rows and that many cells".into())),
                    None => p
                        .errors
                        .push((n + 1, format!("unknown sprite {}", args[0]))),
                }
            }
            other => p.errors.push((n + 1, format!("unknown keyword {other}"))),
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_both_ways() {
        assert_eq!(tile_by_name("BUSH"), Some(tile::BUSH));
        assert_eq!(tile_by_name("bush"), Some(tile::BUSH));
        assert_eq!(tile_by_name("nope"), None);
        assert_eq!(sprite_by_name("HeroDown0"), Some(SpriteId::HeroDown0));
        assert_eq!(palette_by_name("HERO"), Some(crate::palette::pal::HERO));
        assert_eq!(palette_by_name("7"), Some(7));
    }

    #[test]
    fn every_sprite_has_a_name() {
        assert_eq!(SPRITE_NAMES.len(), SpriteId::N);
        for (i, (_, id)) in SPRITE_NAMES.iter().enumerate() {
            assert_eq!(*id as usize, i, "sprite names must stay in enum order");
        }
    }

    #[test]
    fn parses_a_whole_profile() {
        let p = parse(
            r#"
            # a comment, ignored
            name       Test Cartridge
            palette    GRASS 0f380f 306230 8bac0f 9bbc0f
            import     0x30000 256
            terrain    GRASS 12 13 14 15 pal=GRASS
            sprite     HeroDown0 2 2  40 41 48 49 pal=HERO
            "#,
        );
        assert_eq!(p.name, "Test Cartridge");
        assert_eq!(p.palettes.len(), 1);
        assert_eq!(
            p.imports[0],
            Directive::Import {
                offset: 0x30000,
                count: 256,
                base: 0
            }
        );
        assert_eq!(p.terrain[0].0, tile::GRASS);
        assert_eq!(p.terrain[0].1[0].tile, 12);
        assert_eq!(p.sprites[0].1.cells.len(), 4);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
    }

    #[test]
    fn flips_and_blanks_are_understood() {
        let p = parse("terrain GRASS 5:x 5:y 5:xy - pal=GRASS");
        let cells = p.terrain[0].1;
        assert_eq!(cells[0].flip, crate::tile::FLIP_X);
        assert_eq!(cells[1].flip, crate::tile::FLIP_Y);
        assert_eq!(cells[2].flip, crate::tile::FLIP_X | crate::tile::FLIP_Y);
        assert!(cells[3].is_blank());
    }

    #[test]
    fn a_bad_line_does_not_sink_the_file() {
        let p = parse("name Good\nwobble 1 2 3\nterrain BUSH 1 1 1 1");
        assert_eq!(p.name, "Good");
        assert_eq!(p.terrain.len(), 1);
        assert_eq!(p.errors.len(), 1);
        assert_eq!(p.errors[0].0, 2);
    }

    #[test]
    fn applying_a_profile_changes_the_pack() {
        let mut pack = crate::builtin::pack();
        let before = *pack.metatile(tile::BUSH);
        let p = parse("terrain BUSH 3 3 3 3 pal=STONE");
        p.apply(&mut pack);
        assert_ne!(*pack.metatile(tile::BUSH), before);
        assert_eq!(pack.metatile(tile::BUSH)[0].tile, 3);
    }
}
