//! Vectors, directions and axis-aligned boxes in fixed-point pixels.

use crate::fixed::{px, to_px, Fx, ONE};

/// A 2D fixed-point vector.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct V2 {
    pub x: Fx,
    pub y: Fx,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0, y: 0 };

    #[inline]
    pub const fn new(x: Fx, y: Fx) -> Self {
        V2 { x, y }
    }

    /// Builds a vector from whole pixel coordinates.
    #[inline]
    pub const fn from_px(x: i32, y: i32) -> Self {
        V2 { x: px(x), y: px(y) }
    }

    // Named rather than operators: fixed-point vectors are added all over the
    // simulation, and `a.add(b)` reads unambiguously next to `a.scale(n, d)`.
    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }

    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }

    #[inline]
    pub fn scale(self, n: i32, d: i32) -> V2 {
        if d == 0 {
            return V2::ZERO;
        }
        V2::new(
            ((self.x as i64 * n as i64) / d as i64) as Fx,
            ((self.y as i64 * n as i64) / d as i64) as Fx,
        )
    }

    /// Length of the vector, in fixed point.
    #[inline]
    pub fn length(self) -> Fx {
        crate::fixed::length(self.x, self.y)
    }

    /// Returns the vector rescaled to `len`, or zero if it has no direction.
    pub fn with_length(self, len: Fx) -> V2 {
        let l = self.length();
        if l == 0 {
            V2::ZERO
        } else {
            self.scale(len, l)
        }
    }

    #[inline]
    pub fn px_x(self) -> i32 {
        to_px(self.x)
    }

    #[inline]
    pub fn px_y(self) -> i32 {
        to_px(self.y)
    }
}

/// One of the four facings. The discriminants are part of the wire format for
/// animation, so they must stay stable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Dir {
    #[default]
    Down = 0,
    Up = 1,
    Left = 2,
    Right = 3,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Down, Dir::Up, Dir::Left, Dir::Right];

    /// Unit vector for this direction, one pixel long.
    pub fn unit(self) -> V2 {
        match self {
            Dir::Down => V2::new(0, ONE),
            Dir::Up => V2::new(0, -ONE),
            Dir::Left => V2::new(-ONE, 0),
            Dir::Right => V2::new(ONE, 0),
        }
    }

    /// Whole-pixel step for this direction.
    pub fn step(self) -> (i32, i32) {
        match self {
            Dir::Down => (0, 1),
            Dir::Up => (0, -1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }

    pub fn from_u8(v: u8) -> Dir {
        match v & 3 {
            0 => Dir::Down,
            1 => Dir::Up,
            2 => Dir::Left,
            _ => Dir::Right,
        }
    }

    /// Picks the facing that best matches a delta, preferring the dominant axis.
    pub fn from_delta(dx: Fx, dy: Fx) -> Dir {
        if dx.abs() > dy.abs() {
            if dx < 0 {
                Dir::Left
            } else {
                Dir::Right
            }
        } else if dy < 0 {
            Dir::Up
        } else {
            Dir::Down
        }
    }
}

/// An axis-aligned box in fixed-point world space, anchored at its top-left.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: Fx,
    pub y: Fx,
    pub w: Fx,
    pub h: Fx,
}

impl Rect {
    #[inline]
    pub const fn new(x: Fx, y: Fx, w: Fx, h: Fx) -> Self {
        Rect { x, y, w, h }
    }

    /// Builds a box centred on `c` with the given pixel size.
    pub fn centered(c: V2, w_px: i32, h_px: i32) -> Rect {
        Rect::new(c.x - px(w_px) / 2, c.y - px(h_px) / 2, px(w_px), px(h_px))
    }

    #[inline]
    pub fn right(&self) -> Fx {
        self.x + self.w
    }

    #[inline]
    pub fn bottom(&self) -> Fx {
        self.y + self.h
    }

    #[inline]
    pub fn center(&self) -> V2 {
        V2::new(self.x + self.w / 2, self.y + self.h / 2)
    }

    /// True when the two boxes share any area.
    #[inline]
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    /// True when the point lies inside the box.
    #[inline]
    pub fn contains(&self, p: V2) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Grows the box by `n` pixels on every side.
    pub fn inflate(&self, n_px: i32) -> Rect {
        Rect::new(
            self.x - px(n_px),
            self.y - px(n_px),
            self.w + px(n_px) * 2,
            self.h + px(n_px) * 2,
        )
    }

    pub fn offset(&self, d: V2) -> Rect {
        Rect::new(self.x + d.x, self.y + d.y, self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_overlap() {
        let a = Rect::new(px(0), px(0), px(16), px(16));
        let b = Rect::new(px(8), px(8), px(16), px(16));
        let c = Rect::new(px(16), px(0), px(16), px(16));
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c), "edge contact is not an overlap");
    }

    #[test]
    fn dir_roundtrip() {
        for d in Dir::ALL {
            assert_eq!(d.opposite().opposite(), d);
            assert_eq!(Dir::from_u8(d as u8), d);
        }
    }

    #[test]
    fn dir_from_delta_prefers_dominant_axis() {
        assert_eq!(Dir::from_delta(px(-10), px(3)), Dir::Left);
        assert_eq!(Dir::from_delta(px(2), px(-9)), Dir::Up);
    }

    #[test]
    fn with_length_normalises() {
        let v = V2::from_px(3, 4).with_length(px(10));
        assert!((v.length() - px(10)).abs() < px(1), "{v:?}");
    }
}
