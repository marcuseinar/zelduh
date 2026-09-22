//! The software renderer.
//!
//! Everything is drawn into a 160x144 framebuffer of packed RGBA words, at the
//! Game Boy's resolution, and scaled up by whoever displays it. Drawing by
//! hand rather than with a GPU keeps the pixel grid exact: no filtering, no
//! half-pixel sprite positions, no surprises between platforms.

pub mod font;
pub mod sprites;

use zelduh_assets::pack::{AssetPack, Sprite, SpriteId};
use zelduh_assets::palette::{pal, rgb_to_abgr};
use zelduh_assets::tile::{self as at, Cell};
use zelduh_core::entity::{eflag, Entity, Kind};
use zelduh_core::fixed::to_px;
use zelduh_core::items::Item;
use zelduh_core::level::{HUD_H, SCREEN_H, SCREEN_W, TILE_PX};
use zelduh_core::world::{pstate, World};

/// A packed RGBA pixel buffer.
pub struct Framebuffer {
    pub width: i32,
    pub height: i32,
    /// `width * height` pixels, each `0xAABBGGRR`.
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    /// Creates a framebuffer at the Game Boy's resolution.
    pub fn new() -> Framebuffer {
        Framebuffer {
            width: SCREEN_W,
            height: SCREEN_H,
            pixels: vec![0xff00_0000; (SCREEN_W * SCREEN_H) as usize],
        }
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, color: u32) {
        if x >= 0 && y >= 0 && x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = color;
        }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x >= 0 && y >= 0 && x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize]
        } else {
            0
        }
    }

    /// Fills a rectangle, clipped to the buffer.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        for py in y.max(0)..(y + h).min(self.height) {
            for px in x.max(0)..(x + w).min(self.width) {
                self.pixels[(py * self.width + px) as usize] = color;
            }
        }
    }

    /// The pixels as bytes, for handing to a canvas.
    pub fn as_bytes(&self) -> &[u8] {
        // Safe: u32 and u8 have no padding or invalid bit patterns.
        unsafe {
            std::slice::from_raw_parts(
                self.pixels.as_ptr() as *const u8,
                self.pixels.len() * 4,
            )
        }
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Framebuffer::new()
    }
}

/// Draws one 8x8 cell.
///
/// `transparent` decides whether colour 0 is a hole (sprites) or a real
/// colour (terrain).
pub fn draw_cell(fb: &mut Framebuffer, pack: &AssetPack, cell: Cell, x: i32, y: i32, transparent: bool) {
    if cell.is_blank() {
        return;
    }
    // Skip cells that are entirely off screen before touching any pixels.
    if x + 8 <= 0 || y + 8 <= 0 || x >= fb.width || y >= fb.height {
        return;
    }
    let tile = pack.tile(cell.tile);
    let palette = pack.palette(cell.palette);
    let colors = palette.to_abgr();
    for py in 0..8 {
        let ty = y + py;
        if ty < 0 || ty >= fb.height {
            continue;
        }
        for px in 0..8 {
            let tx = x + px;
            if tx < 0 || tx >= fb.width {
                continue;
            }
            let v = at::pixel(tile, px as usize, py as usize, cell.flip);
            if transparent && v == 0 {
                continue;
            }
            fb.pixels[(ty * fb.width + tx) as usize] = colors[v as usize];
        }
    }
}

/// Draws a whole sprite with an optional flip applied to the whole image.
pub fn draw_sprite(fb: &mut Framebuffer, pack: &AssetPack, sprite: &Sprite, x: i32, y: i32, flip: u8) {
    let cols = sprite.cols as i32;
    let rows = sprite.rows as i32;
    for cy in 0..rows {
        for cx in 0..cols {
            // Flipping the image means both mirroring each cell and swapping
            // which cell sits where.
            let (sx, sy) = (
                if flip & at::FLIP_X != 0 { cols - 1 - cx } else { cx },
                if flip & at::FLIP_Y != 0 { rows - 1 - cy } else { cy },
            );
            let mut cell = sprite.cell(sx as usize, sy as usize);
            cell.flip ^= flip;
            draw_cell(
                fb,
                pack,
                cell,
                x + sprite.ox as i32 + cx * 8,
                y + sprite.oy as i32 + cy * 8,
                true,
            );
        }
    }
}

/// Draws a sprite by id.
pub fn draw(fb: &mut Framebuffer, pack: &AssetPack, id: SpriteId, x: i32, y: i32, flip: u8) {
    let sprite = pack.sprite(id).clone();
    draw_sprite(fb, pack, &sprite, x, y, flip);
}

/// Draws text in the given palette colour.
pub fn draw_text(fb: &mut Framebuffer, text: &str, x: i32, y: i32, color: u32) {
    let mut cx = x;
    for ch in text.chars() {
        if ch != ' ' {
            for gy in 0..font::HEIGHT {
                for gx in 0..5 {
                    if font::pixel(ch, gx, gy) {
                        fb.set(cx + gx, y + gy, color);
                    }
                }
            }
        }
        cx += font::ADVANCE;
    }
}

/// Draws text with a one-pixel shadow so it stays readable over any terrain.
pub fn draw_text_shadowed(fb: &mut Framebuffer, text: &str, x: i32, y: i32, color: u32, shadow: u32) {
    draw_text(fb, text, x + 1, y + 1, shadow);
    draw_text(fb, text, x, y, color);
}

/// Renders the world from one player's point of view.
pub fn render(fb: &mut Framebuffer, world: &World, pack: &AssetPack, player: usize) {
    let Some(p) = world.players.get(player) else {
        fb.clear(0xff00_0000);
        return;
    };
    let level = world.level(p.level);

    // Screen shake nudges the camera by a pixel or two.
    let shake = if p.camera.shake > 0 {
        let n = (world.frame % 4) as i32;
        (n % 2 * 2 - 1, (n / 2) * 2 - 1)
    } else {
        (0, 0)
    };
    let cam_x = to_px(p.camera.x) + shake.0;
    let cam_y = to_px(p.camera.y) + shake.1;

    fb.clear(rgb_to_abgr(pack.palette(pal::DUNGEON).0[3]));

    // Terrain: only the tiles that touch the viewport.
    let first_tx = cam_x.div_euclid(TILE_PX);
    let first_ty = cam_y.div_euclid(TILE_PX);
    let cols = SCREEN_W / TILE_PX + 2;
    let rows = (SCREEN_H - HUD_H) / TILE_PX + 2;
    for ty in first_ty..first_ty + rows {
        for tx in first_tx..first_tx + cols {
            let t = level.map.get(tx, ty);
            let meta = pack.metatile(t);
            let sx = tx * TILE_PX - cam_x;
            let sy = ty * TILE_PX - cam_y + HUD_H;
            for (i, cell) in meta.iter().enumerate() {
                let ox = (i as i32 % 2) * 8;
                let oy = (i as i32 / 2) * 8;
                draw_cell(fb, pack, *cell, sx + ox, sy + oy, false);
            }
        }
    }

    // Entities, sorted by their feet so that nearer things overlap farther
    // ones, which is what sells the top-down perspective.
    let mut order: Vec<(i32, usize)> = Vec::new();
    for idx in 0..world.entities.capacity() {
        let Some(e) = world.entities.at(idx) else {
            continue;
        };
        if e.level != p.level {
            continue;
        }
        let sx = to_px(e.pos.x) - cam_x;
        let sy = to_px(e.pos.y) - cam_y;
        // A generous margin so big sprites do not pop in at the edges.
        if sx < -40 || sy < -40 || sx > SCREEN_W + 40 || sy > SCREEN_H + 40 {
            continue;
        }
        order.push((to_px(e.pos.y), idx));
    }
    order.sort_by_key(|(y, idx)| (*y, *idx));

    for (_, idx) in order {
        let Some(e) = world.entities.at(idx) else {
            continue;
        };
        draw_entity(fb, pack, world, e, cam_x, cam_y);
    }

    draw_hud(fb, world, pack, player);
}

/// Draws one entity, with its shadow if it is off the ground.
fn draw_entity(fb: &mut Framebuffer, pack: &AssetPack, world: &World, e: &Entity, cam_x: i32, cam_y: i32) {
    let Some((id, flip)) = sprites::for_entity(world, e) else {
        return;
    };
    let sprite = pack.sprite(id);
    let z = to_px(e.z).max(0);
    // Sprites are anchored at the feet: bottom-centre of the image sits on the
    // bottom of the collision body.
    let x = to_px(e.pos.x) - cam_x - sprite.width() / 2;
    let foot = to_px(e.pos.y) + e.body_h / 2;
    let y = foot - cam_y + HUD_H - sprite.height();

    if z > 0 && !e.has(eflag::FLYING) || (e.has(eflag::FLYING) && e.kind.is_enemy()) {
        draw(fb, pack, SpriteId::Shadow, x, y + sprite.height() - 16, 0);
    }

    // A hurt entity flashes: skip drawing on alternate frames.
    if e.iframes > 0 && (world.frame / 2) % 2 == 0 {
        return;
    }
    let sprite = pack.sprite(id).clone();
    draw_sprite(fb, pack, &sprite, x, y - z, flip);

    // A carried tile is drawn as its terrain art.
    if e.kind == Kind::Carried {
        let meta = *pack.metatile(e.data[0] as u8);
        for (i, cell) in meta.iter().enumerate() {
            let ox = (i as i32 % 2) * 8;
            let oy = (i as i32 / 2) * 8;
            draw_cell(fb, pack, *cell, x + ox, y - z + oy, true);
        }
    }
}

/// Draws the status bar.
pub fn draw_hud(fb: &mut Framebuffer, world: &World, pack: &AssetPack, player: usize) {
    let hud_bg = rgb_to_abgr(pack.palette(pal::HUD).0[3]);
    let text = rgb_to_abgr(pack.palette(pal::HUD).0[1]);
    let dim = rgb_to_abgr(pack.palette(pal::HUD).0[2]);
    fb.fill_rect(0, 0, SCREEN_W, HUD_H, hud_bg);

    let Some(p) = world.players.get(player) else {
        return;
    };

    // Equipped items, in their buttons.
    draw(fb, pack, sprites::item_icon(p.inv.equipped[1]), 2, 4, 0);
    draw_text(fb, "B", 2, 0, dim);
    draw(fb, pack, sprites::item_icon(p.inv.equipped[0]), 14, 4, 0);
    draw_text(fb, "A", 14, 0, dim);

    // Rupees and keys.
    draw(fb, pack, SpriteId::IconRupee, 28, 0, 0);
    draw_text(fb, &format!("{:03}", p.inv.rupees), 36, 1, text);
    draw(fb, pack, SpriteId::IconKey, 28, 8, 0);
    draw_text(fb, &format!("{:02}", p.inv.keys), 36, 9, text);

    // Hearts, a quarter at a time.
    if let Some(e) = world.entities.get(p.entity) {
        let hearts = (e.max_hp as i32 + 3) / 4;
        for i in 0..hearts.min(14) {
            let left = e.hp as i32 - i * 4;
            let id = match left {
                l if l >= 4 => SpriteId::HudHeart4,
                3 | 2 => SpriteId::HudHeart2,
                1 => SpriteId::HudHeart1,
                _ => SpriteId::HudHeart0,
            };
            let (hx, hy) = (64 + (i % 7) * 9, if i < 7 { 1 } else { 9 });
            draw(fb, pack, id, hx, hy, 0);
        }
    }

    // Bomb and arrow counts, when the player has them.
    if p.inv.has(Item::Bombs) {
        draw_text(fb, &format!("{:02}", p.inv.bombs), 130, 1, text);
    }
    if p.inv.has(Item::Bow) {
        draw_text(fb, &format!("{:02}", p.inv.arrows), 146, 1, text);
    }

    // A message sits across the bottom of the bar.
    if let Some((msg, _)) = p.message {
        let w = font::width(msg);
        let x = (SCREEN_W - w) / 2;
        fb.fill_rect(x - 3, SCREEN_H - 14, w + 6, 11, hud_bg);
        draw_text(fb, msg, x, SCREEN_H - 12, text);
    }

    // Waiting to respawn.
    if p.respawn > 0 {
        let msg = "YOU DIED";
        let x = (SCREEN_W - font::width(msg)) / 2;
        draw_text_shadowed(fb, msg, x, 70, text, hud_bg);
    }
}

/// True when the player's own entity is in a state that hides the sprite.
pub fn player_is_hidden(e: &Entity) -> bool {
    e.state == pstate::DEAD
}

#[cfg(test)]
mod tests {
    use super::*;
    use zelduh_core::level::{Level, LevelKind};
    use zelduh_core::tiles::tile;
    use zelduh_core::V2;

    fn test_world() -> World {
        let mut lv = Level::new(LevelKind::Overworld, 2, 2, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(1, vec![lv], 1);
        w.join(0);
        w
    }

    #[test]
    fn the_framebuffer_is_a_game_boy_screen() {
        let fb = Framebuffer::new();
        assert_eq!((fb.width, fb.height), (160, 144));
        assert_eq!(fb.pixels.len(), 160 * 144);
        assert_eq!(fb.as_bytes().len(), 160 * 144 * 4);
    }

    #[test]
    fn rendering_fills_the_screen() {
        let w = test_world();
        let pack = zelduh_assets::builtin::pack();
        let mut fb = Framebuffer::new();
        fb.clear(0);
        render(&mut fb, &w, &pack, 0);
        assert!(
            fb.pixels.iter().all(|p| *p != 0),
            "every pixel should have been written"
        );
    }

    #[test]
    fn the_hud_sits_at_the_top() {
        let w = test_world();
        let pack = zelduh_assets::builtin::pack();
        let mut fb = Framebuffer::new();
        render(&mut fb, &w, &pack, 0);
        // The status bar background differs from the grass below it.
        let bar = fb.get(2, 14);
        let field = fb.get(2, HUD_H + 40);
        assert_ne!(bar, field);
    }

    #[test]
    fn drawing_off_screen_is_harmless() {
        let pack = zelduh_assets::builtin::pack();
        let mut fb = Framebuffer::new();
        for (x, y) in [(-100, -100), (500, 500), (-4, 70), (158, 2)] {
            draw(&mut fb, &pack, SpriteId::HeroDown0, x, y, 0);
            draw_text(&mut fb, "EDGE", x, y, 0xffffffff);
        }
    }

    #[test]
    fn a_flipped_sprite_mirrors() {
        let pack = zelduh_assets::builtin::pack();
        let mut a = Framebuffer::new();
        let mut b = Framebuffer::new();
        a.clear(0);
        b.clear(0);
        draw(&mut a, &pack, SpriteId::HeroSide0, 0, 0, 0);
        draw(&mut b, &pack, SpriteId::HeroSide0, 0, 0, at::FLIP_X);
        let w = pack.sprite(SpriteId::HeroSide0).width();
        for y in 0..16 {
            for x in 0..w {
                assert_eq!(a.get(x, y), b.get(w - 1 - x, y), "mismatch at {x},{y}");
            }
        }
    }

    #[test]
    fn text_draws_something() {
        let mut fb = Framebuffer::new();
        fb.clear(0);
        draw_text(&mut fb, "ZELDUH", 10, 10, 0xffff_ffff);
        assert!(fb.pixels.iter().any(|p| *p == 0xffff_ffff));
    }
}
