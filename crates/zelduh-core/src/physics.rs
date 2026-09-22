//! Movement against terrain.
//!
//! Movement is resolved one axis at a time in small steps: that keeps fast
//! projectiles from tunnelling through walls, and gives the classic feel where
//! walking diagonally into a wall slides you along it.

use crate::entity::{eflag, Entity};
use crate::fixed::{px, to_px, Fx, ONE};
use crate::geom::{Rect, V2};
use crate::level::{Map, TILE_PX};
use crate::tiles::{self, Tile};

/// Which axes a move was blocked on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hit {
    pub x: bool,
    pub y: bool,
}

impl Hit {
    pub fn any(self) -> bool {
        self.x || self.y
    }
}

/// True when this entity is stopped by the given terrain.
pub fn blocks(e: &Entity, t: Tile) -> bool {
    if e.has(eflag::GHOST) {
        false
    } else if e.has(eflag::FLYING) || e.has(eflag::AIRBORNE) {
        tiles::blocks_flight(t)
    } else {
        tiles::blocks_walk(t)
    }
}

/// True when the entity's body would be clear at `pos`.
pub fn fits_at(map: &Map, e: &Entity, pos: V2) -> bool {
    let r = Rect::centered(pos, e.body_w, e.body_h);
    map.rect_is_clear(&r, |t| blocks(e, t))
}

/// Moves an entity by `delta`, stopping against terrain.
///
/// Returns which axes were blocked. The entity's position is updated in place.
pub fn move_by(map: &Map, e: &mut Entity, delta: V2) -> Hit {
    let mut hit = Hit::default();
    if e.has(eflag::GHOST) {
        e.pos = e.pos.add(delta);
        return hit;
    }

    // Step in slices no larger than half a tile so nothing tunnels.
    let max_step = px(TILE_PX / 2);
    let steps = ((delta.x.abs().max(delta.y.abs()) + max_step - 1) / max_step).max(1);
    let step = V2::new(delta.x / steps, delta.y / steps);
    // Integer division drops a remainder; add it back on the final step so a
    // slow-moving entity never stalls short of its target.
    let rem = V2::new(delta.x - step.x * steps, delta.y - step.y * steps);

    for i in 0..steps {
        let mut s = step;
        if i == steps - 1 {
            s = s.add(rem);
        }
        if s.x != 0 {
            let want = V2::new(e.pos.x + s.x, e.pos.y);
            if fits_at(map, e, want) {
                e.pos = want;
            } else if let Some(p) = slide(map, e, want, true) {
                e.pos = p;
            } else {
                hit.x = true;
            }
        }
        if s.y != 0 {
            let want = V2::new(e.pos.x, e.pos.y + s.y);
            if fits_at(map, e, want) {
                e.pos = want;
            } else if let Some(p) = slide(map, e, want, false) {
                e.pos = p;
            } else {
                hit.y = true;
            }
        }
    }
    hit
}

/// Nudges an entity around a corner it has clipped, the way Link slips past
/// the edge of a wall instead of catching on it.
fn slide(map: &Map, e: &Entity, want: V2, horizontal: bool) -> Option<V2> {
    // Only walking entities get corner assistance.
    if e.has(eflag::FLYING) || e.has(eflag::AIRBORNE) {
        return None;
    }
    const NUDGE: Fx = ONE;
    let max = px(5);
    let mut off = NUDGE;
    while off <= max {
        for sign in [-1, 1] {
            let p = if horizontal {
                V2::new(want.x, want.y + off * sign)
            } else {
                V2::new(want.x + off * sign, want.y)
            };
            if fits_at(map, e, p) {
                // Move only along the assisting axis; the blocked axis still
                // advances because `want` already carried it.
                return Some(p);
            }
        }
        off += NUDGE;
    }
    None
}

/// Separates two overlapping solid bodies by pushing them apart along the
/// shallower axis. Returns the offset applied to `a` (the other body takes the
/// opposite offset).
pub fn separate(a: &Rect, b: &Rect, strength: Fx) -> V2 {
    if !a.overlaps(b) {
        return V2::ZERO;
    }
    let ac = a.center();
    let bc = b.center();
    let dx = ac.x - bc.x;
    let dy = ac.y - bc.y;
    let overlap_x = (a.w + b.w) / 2 - dx.abs();
    let overlap_y = (a.h + b.h) / 2 - dy.abs();
    if overlap_x <= 0 || overlap_y <= 0 {
        return V2::ZERO;
    }
    if overlap_x < overlap_y {
        V2::new(if dx < 0 { -strength } else { strength }, 0)
    } else {
        V2::new(0, if dy < 0 { -strength } else { strength })
    }
}

/// Tile coordinates under a world position.
#[inline]
pub fn tile_coords(p: V2) -> (i32, i32) {
    (
        to_px(p.x).div_euclid(TILE_PX),
        to_px(p.y).div_euclid(TILE_PX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Entities, Kind};
    use crate::tiles::tile;

    fn octorok_at(x: i32, y: i32) -> Entity {
        let mut es = Entities::new();
        let id = es.spawn(Kind::Octorok, 0, V2::from_px(x, y));
        es.get(id).unwrap().clone()
    }

    #[test]
    fn walking_into_a_wall_stops() {
        let mut m = Map::new(1, 1, tile::GRASS);
        m.fill_rect(4, 0, 1, 8, tile::WALL);
        let mut e = octorok_at(40, 40);
        let hit = move_by(&mut m.clone(), &mut e, V2::new(px(40), 0));
        assert!(hit.x, "should be blocked by the wall");
        assert!(to_px(e.pos.x) <= 64 - 6, "stopped short of x=64: {}", to_px(e.pos.x));
    }

    #[test]
    fn fast_projectiles_do_not_tunnel() {
        let mut m = Map::new(2, 1, tile::GRASS);
        m.fill_rect(6, 0, 1, 8, tile::WALL);
        let mut e = octorok_at(16, 40);
        e.flags |= eflag::FLYING;
        let hit = move_by(&m, &mut e, V2::new(px(200), 0));
        assert!(hit.x, "a 200px step must still notice the wall");
        assert!(to_px(e.pos.x) < 96);
    }

    #[test]
    fn ghosts_pass_through_everything() {
        let m = Map::new(1, 1, tile::WALL);
        let mut e = octorok_at(8, 8);
        e.flags |= eflag::GHOST;
        let hit = move_by(&m, &mut e, V2::new(px(50), px(50)));
        assert!(!hit.any());
        assert_eq!(to_px(e.pos.x), 58);
    }

    #[test]
    fn small_moves_are_not_lost_to_rounding() {
        let m = Map::new(1, 1, tile::GRASS);
        let mut e = octorok_at(40, 40);
        let before = e.pos.x;
        move_by(&m, &mut e, V2::new(3, 0));
        assert_eq!(e.pos.x - before, 3, "a sub-pixel step must still apply");
    }

    #[test]
    fn corner_slide_slips_past_an_edge() {
        let mut m = Map::new(1, 1, tile::GRASS);
        // A wall in tile column 3 (pixels 48..64). A body at x=68 clips its
        // right edge by two pixels, which should be nudged clear rather than
        // catching.
        m.fill_rect(3, 0, 1, 4, tile::WALL);
        let mut e = octorok_at(68, 40);
        move_by(&m, &mut e, V2::new(0, px(6)));
        assert!(to_px(e.pos.y) > 40, "the body should have slipped past");
        assert!(to_px(e.pos.x) >= 70, "and been nudged clear of the corner");
    }

    #[test]
    fn separate_pushes_along_the_shallow_axis() {
        let a = Rect::centered(V2::from_px(10, 10), 12, 12);
        let b = Rect::centered(V2::from_px(12, 18), 12, 12);
        let d = separate(&a, &b, px(1));
        assert_eq!(d.x, 0, "vertical overlap is shallower");
        assert!(d.y < 0, "a is above b so it is pushed up");
    }

    #[test]
    fn separate_ignores_non_overlapping() {
        let a = Rect::centered(V2::from_px(0, 0), 8, 8);
        let b = Rect::centered(V2::from_px(100, 100), 8, 8);
        assert_eq!(separate(&a, &b, px(1)), V2::ZERO);
    }
}
