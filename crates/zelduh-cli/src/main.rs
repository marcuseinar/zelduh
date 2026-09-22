//! A desktop harness for the engine.
//!
//! The game itself runs in a browser, but a headless build that can render a
//! frame to a PNG, dump a generated map as text and time the simulation makes
//! it possible to see and measure what changed without opening one.

mod png;

use std::fmt::Write as _;
use std::process::ExitCode;
use std::time::Instant;

use zelduh_assets::pack::SpriteId;
use zelduh_assets::{builtin, AssetPack};
use zelduh_core::button;
use zelduh_core::level::{ROOM_H, ROOM_W};
use zelduh_core::tiles::tile;
use zelduh_core::World;
use zelduh_gen::Config;
use zelduh_render::Framebuffer;

const USAGE: &str = "\
zelduh - a top-down action adventure engine

USAGE:
    zelduh <COMMAND> [OPTIONS]

COMMANDS:
    shot      Render one frame to a PNG
    sheet     Render every tile and sprite in the asset pack to a PNG
    icon      Render an app icon to a PNG
    map       Print a generated level as text
    rom       Report what is inside a Game Boy ROM
    bench     Time the simulation
    help      Show this message

OPTIONS:
    --seed <N>      World seed                  (default 1)
    --level <N>     Which level to look at      (default 0, the overworld)
    --frames <N>    Frames to simulate first    (default 1)
    --scale <N>     Pixel scale for PNG output  (default 4)
    --out <PATH>    Where to write the PNG      (default zelduh.png)
    --walk <DIRS>   Buttons to hold while simulating, e.g. rrrduu
    --rom <PATH>    A ROM to take graphics from
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let opts = Options::parse(&args[1..]);
    let result = match args[0].as_str() {
        "shot" => cmd_shot(&opts),
        "sheet" => cmd_sheet(&opts),
        "icon" => cmd_icon(&opts),
        "map" => cmd_map(&opts),
        "rom" => cmd_rom(&opts),
        "bench" => cmd_bench(&opts),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    seed: u64,
    level: u16,
    frames: u32,
    scale: usize,
    out: String,
    walk: String,
    rom: Option<String>,
    path: Option<String>,
}

impl Options {
    fn parse(args: &[String]) -> Options {
        let mut o = Options {
            seed: 1,
            level: 0,
            frames: 1,
            scale: 4,
            out: "zelduh.png".to_string(),
            walk: String::new(),
            rom: None,
            path: None,
        };
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            let mut value = || {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                i += 1;
                v
            };
            match arg {
                "--seed" => o.seed = value().parse().unwrap_or(1),
                "--level" => o.level = value().parse().unwrap_or(0),
                "--frames" => o.frames = value().parse().unwrap_or(1),
                "--scale" => o.scale = value().parse().unwrap_or(4),
                "--out" => o.out = value(),
                "--walk" => o.walk = value(),
                "--rom" => o.rom = Some(value()),
                other if !other.starts_with("--") => o.path = Some(other.to_string()),
                other => eprintln!("warning: ignoring unknown option {other}"),
            }
            i += 1;
        }
        o
    }

    /// Builds the asset pack, importing a ROM's graphics when one was given.
    fn pack(&self) -> Result<AssetPack, String> {
        let mut pack = builtin::pack();
        if let Some(path) = &self.rom {
            let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            let kind = zelduh_assets::sniff(path, &data)
                .ok_or_else(|| format!("{path}: not a file this engine can read"))?;
            let report = zelduh_assets::load(&mut pack, path, &data, kind);
            eprintln!("loaded {}: {}", report.kind, report.detail);
            for w in &report.warnings {
                eprintln!("  warning: {w}");
            }
        }
        Ok(pack)
    }
}

/// Turns a string like "rrrdduu" into one button mask per frame.
fn walk_script(s: &str) -> Vec<u16> {
    s.chars()
        .map(|c| match c {
            'l' => button::LEFT,
            'r' => button::RIGHT,
            'u' => button::UP,
            'd' => button::DOWN,
            'a' => button::A,
            'b' => button::B,
            _ => 0,
        })
        .collect()
}

fn run_world(opts: &Options) -> World {
    let mut w = zelduh_gen::new_world(Config::from_seed(opts.seed), 4);
    w.join(0);
    let script = walk_script(&opts.walk);
    for f in 0..opts.frames {
        // The script loops so a short string can drive a long run.
        let buttons = if script.is_empty() {
            0
        } else {
            script[(f as usize) % script.len()]
        };
        w.set_input(0, buttons);
        w.step();
    }
    w
}

fn cmd_shot(opts: &Options) -> Result<(), String> {
    let pack = opts.pack()?;
    let mut world = run_world(opts);

    // Looking at a dungeon means going there.
    if opts.level > 0 && (opts.level as usize) < world.levels.len() {
        let pos = world.levels[opts.level as usize].entrance;
        world.warp_player(0, opts.level, pos, zelduh_core::Dir::Down);
        world.step();
    }

    let mut fb = Framebuffer::new();
    zelduh_render::render(&mut fb, &world, &pack, 0);
    let data = png::encode(
        &fb.pixels,
        fb.width as usize,
        fb.height as usize,
        opts.scale,
    );
    std::fs::write(&opts.out, &data).map_err(|e| format!("{}: {e}", opts.out))?;
    println!(
        "wrote {} ({}x{}, seed {}, frame {})",
        opts.out,
        fb.width as usize * opts.scale,
        fb.height as usize * opts.scale,
        opts.seed,
        world.frame
    );
    Ok(())
}

/// Lays every piece of art out on one sheet, which is the quickest way to see
/// that an imported tileset landed where it should.
fn cmd_sheet(opts: &Options) -> Result<(), String> {
    let pack = opts.pack()?;
    let cell = 40;
    let cols = 12;
    let terrain: Vec<u8> = (0..tile::COUNT as u16)
        .map(|t| t as u8)
        .filter(|t| pack.metatile(*t).iter().any(|c| !c.is_blank()))
        .collect();
    let sprite_count = SpriteId::N;
    let rows = terrain.len().div_ceil(cols) + sprite_count.div_ceil(cols) + 2;

    let mut fb = Framebuffer {
        width: (cols * cell) as i32,
        height: (rows * cell) as i32,
        pixels: vec![0xff20_2020; cols * cell * rows * cell],
    };

    zelduh_render::draw_text(&mut fb, "TERRAIN", 4, 4, 0xffff_ffff);
    let mut y = 14;
    for (i, t) in terrain.iter().enumerate() {
        let x = (i % cols) * cell;
        let row = i / cols;
        let meta = *pack.metatile(*t);
        for (c, cellv) in meta.iter().enumerate() {
            zelduh_render::draw_cell(
                &mut fb,
                &pack,
                *cellv,
                (x + 8 + (c % 2) * 8) as i32,
                (y + row * cell + 8 + (c / 2) * 8) as i32,
                false,
            );
        }
        let label = format!("{t}");
        zelduh_render::draw_text(
            &mut fb,
            &label,
            (x + 8) as i32,
            (y + row * cell) as i32,
            0xffb0_b0b0,
        );
    }

    y += terrain.len().div_ceil(cols) * cell + 10;
    zelduh_render::draw_text(&mut fb, "SPRITES", 4, y as i32, 0xffff_ffff);
    y += 10;
    for i in 0..sprite_count {
        let id = sprite_id(i);
        let x = (i % cols) * cell;
        let row = i / cols;
        let s = pack.sprite(id).clone();
        zelduh_render::draw_sprite(
            &mut fb,
            &pack,
            &s,
            (x + 4) as i32,
            (y + row * cell + 6) as i32,
            0,
        );
        zelduh_render::draw_text(
            &mut fb,
            &format!("{i}"),
            (x + 4) as i32,
            (y + row * cell) as i32,
            0xffb0_b0b0,
        );
    }

    let data = png::encode(
        &fb.pixels,
        fb.width as usize,
        fb.height as usize,
        opts.scale.min(3),
    );
    std::fs::write(&opts.out, &data).map_err(|e| format!("{}: {e}", opts.out))?;
    println!(
        "wrote {} ({} terrain tiles, {} sprites, {} unique 8x8 tiles)",
        opts.out,
        terrain.len(),
        sprite_count,
        pack.tile_count()
    );
    Ok(())
}

/// [`SpriteId`] has no `from_usize`, so this walks the list the profile
/// parser already keeps in enum order.
fn sprite_id(i: usize) -> SpriteId {
    zelduh_assets::profile::SPRITE_NAMES[i].1
}

/// Renders the app icon: the hero on a plain background, scaled up.
///
/// The icon is drawn from the same art the game uses rather than being a
/// separate file to keep in step with it.
fn cmd_icon(opts: &Options) -> Result<(), String> {
    let pack = opts.pack()?;
    // A 32x32 board gives the sprite a margin on every side.
    const SIZE: i32 = 32;
    let mut fb = Framebuffer {
        width: SIZE,
        height: SIZE,
        pixels: vec![0; (SIZE * SIZE) as usize],
    };
    let background =
        zelduh_assets::palette::rgb_to_abgr(pack.palette(zelduh_assets::palette::pal::GRASS).0[2]);
    fb.clear(background);
    let sprite = pack.sprite(SpriteId::HeroDown0).clone();
    zelduh_render::draw_sprite(
        &mut fb,
        &pack,
        &sprite,
        (SIZE - sprite.width()) / 2,
        (SIZE - sprite.height()) / 2,
        0,
    );
    // The scale option doubles as the icon's size in units of 32 pixels.
    let scale = opts.scale.max(1);
    let data = png::encode(&fb.pixels, SIZE as usize, SIZE as usize, scale);
    std::fs::write(&opts.out, &data).map_err(|e| format!("{}: {e}", opts.out))?;
    println!(
        "wrote {} ({}x{})",
        opts.out,
        SIZE as usize * scale,
        SIZE as usize * scale
    );
    Ok(())
}

fn cmd_map(opts: &Options) -> Result<(), String> {
    let levels = zelduh_gen::generate(Config::from_seed(opts.seed));
    let level = levels
        .get(opts.level as usize)
        .ok_or_else(|| format!("no level {}", opts.level))?;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "level {} - {:?}, {}x{} rooms, {} spawns, {} links",
        opts.level,
        level.kind,
        level.rooms_w(),
        level.rooms_h(),
        level.spawns.len(),
        level.links.len()
    );
    for ty in 0..level.map.h() {
        if ty % ROOM_H == 0 {
            let _ = writeln!(
                out,
                "{}",
                "-".repeat(level.map.w() as usize + level.rooms_w() as usize)
            );
        }
        for tx in 0..level.map.w() {
            if tx % ROOM_W == 0 {
                out.push('|');
            }
            out.push(glyph(level.map.get(tx, ty)));
        }
        out.push('\n');
    }
    let e = level.entrance;
    let _ = writeln!(
        out,
        "entrance at {},{}",
        zelduh_core::fixed::to_px(e.x),
        zelduh_core::fixed::to_px(e.y)
    );
    print!("{out}");
    Ok(())
}

/// One character per tile, chosen to make a map readable in a terminal.
fn glyph(t: u8) -> char {
    match t {
        tile::VOID => ' ',
        tile::GRASS => '.',
        tile::GRASS_TALL => ',',
        tile::FLOWERS => '*',
        tile::SAND => ':',
        tile::PATH => '=',
        tile::FLOOR => '.',
        tile::CARPET => '%',
        tile::BRIDGE => 'H',
        tile::WATER => '~',
        tile::WATER_SHALLOW => '-',
        tile::LAVA => '!',
        tile::PIT => 'o',
        tile::BUSH => 'w',
        tile::ROCK => 'O',
        tile::TREE => 'T',
        tile::WALL | tile::WALL_DUNGEON => '#',
        tile::CLIFF => 'A',
        tile::WALL_CRACKED => 'x',
        tile::BLOCK => 'B',
        tile::STATUE => 'S',
        tile::POT => 'p',
        tile::SIGN => 'i',
        tile::DOOR_OPEN => '+',
        tile::DOOR_SHUT => 'D',
        tile::DOOR_LOCKED => 'L',
        tile::DOOR_BOSS => '&',
        tile::STAIRS_DOWN => '>',
        tile::STAIRS_UP => '<',
        tile::WARP => '@',
        tile::LEDGE_DOWN | tile::LEDGE_UP | tile::LEDGE_LEFT | tile::LEDGE_RIGHT => '^',
        _ => '?',
    }
}

fn cmd_rom(opts: &Options) -> Result<(), String> {
    let path = opts
        .path
        .clone()
        .or_else(|| opts.rom.clone())
        .ok_or("usage: zelduh rom <FILE>")?;
    let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let info = zelduh_assets::rom::info(&data);
    println!("file            {path}");
    println!(
        "size            {} KiB ({} banks)",
        data.len() / 1024,
        info.banks
    );
    println!("title           {:?}", info.title);
    println!("nintendo logo   {}", yes_no(info.has_logo));
    println!("header checksum {}", yes_no(info.header_checksum_ok));
    println!("colour          {}", yes_no(info.color));
    println!("cartridge type  0x{:02x}", info.cart_type);
    println!("looks like a gb {}", yes_no(info.looks_like_gameboy()));
    println!();
    println!("most graphics-like regions:");
    for r in zelduh_assets::rom::scan(&data, 256).into_iter().take(8) {
        println!(
            "  0x{:06x}  {:>4} KiB  score {}",
            r.offset,
            r.len / 1024,
            r.score
        );
    }
    Ok(())
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn cmd_bench(opts: &Options) -> Result<(), String> {
    let frames = opts.frames.max(1000);
    let start = Instant::now();
    let mut w = zelduh_gen::new_world(Config::from_seed(opts.seed), 4);
    let built = start.elapsed();
    for i in 0..4 {
        w.join(i);
    }

    let start = Instant::now();
    for f in 0..frames {
        // Keep everyone moving so the work is representative.
        let buttons = match f % 240 {
            0..=59 => button::RIGHT,
            60..=119 => button::DOWN | button::A,
            120..=179 => button::LEFT,
            _ => button::UP,
        };
        for p in 0..4 {
            w.set_input(p, buttons);
        }
        w.step();
    }
    let sim = start.elapsed();

    let pack = builtin::pack();
    let mut fb = Framebuffer::new();
    let start = Instant::now();
    for _ in 0..frames {
        zelduh_render::render(&mut fb, &w, &pack, 0);
    }
    let draw = start.elapsed();

    println!("world built in    {built:?}");
    println!(
        "{frames} sim frames   {sim:?}  ({:.1} us/frame, {:.0}x real time)",
        sim.as_secs_f64() * 1e6 / frames as f64,
        1.0 / (sim.as_secs_f64() * 60.0 / frames as f64)
    );
    println!(
        "{frames} draw frames  {draw:?}  ({:.1} us/frame)",
        draw.as_secs_f64() * 1e6 / frames as f64
    );
    println!("entities at end   {}", w.entities.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_take_their_defaults() {
        let o = Options::parse(&[]);
        assert_eq!(o.seed, 1);
        assert_eq!(o.scale, 4);
        assert_eq!(o.out, "zelduh.png");
    }

    #[test]
    fn options_parse_flags() {
        let args: Vec<String> = "--seed 9 --frames 30 --out x.png --walk rrd"
            .split(' ')
            .map(String::from)
            .collect();
        let o = Options::parse(&args);
        assert_eq!(o.seed, 9);
        assert_eq!(o.frames, 30);
        assert_eq!(o.out, "x.png");
        assert_eq!(o.walk, "rrd");
    }

    #[test]
    fn a_walk_script_becomes_button_masks() {
        assert_eq!(
            walk_script("rda"),
            vec![button::RIGHT, button::DOWN, button::A]
        );
        assert!(walk_script("").is_empty());
    }

    #[test]
    fn every_tile_has_a_glyph() {
        for t in 0..=255u8 {
            let g = glyph(t);
            assert!(g.is_ascii());
        }
        assert_eq!(glyph(tile::WATER), '~');
    }

    #[test]
    fn sprite_ids_are_listed_in_order() {
        assert_eq!(sprite_id(0), SpriteId::HeroDown0);
        assert_eq!(zelduh_assets::profile::SPRITE_NAMES.len(), SpriteId::N);
    }

    #[test]
    fn a_short_run_produces_a_live_world() {
        let o = Options {
            seed: 3,
            level: 0,
            frames: 30,
            scale: 1,
            out: String::new(),
            walk: "r".to_string(),
            rom: None,
            path: None,
        };
        let w = run_world(&o);
        assert_eq!(w.frame, 30);
        assert!(w.players[0].active);
    }
}
