//! Fixed-point math.
//!
//! The simulation never uses floating point: every player in a multiplayer
//! session must reach bit-identical state from the same inputs, and `f32`
//! rounding is not guaranteed to match across platforms. Positions are 24.8
//! fixed point, i.e. one pixel is [`ONE`] units.

/// Number of fractional bits in a [`Fx`] value.
pub const SHIFT: u32 = 8;
/// One whole pixel in fixed-point units.
pub const ONE: Fx = 1 << SHIFT;
/// Half a pixel.
pub const HALF: Fx = ONE / 2;

/// A 24.8 fixed-point scalar, measured in pixels.
pub type Fx = i32;

/// Converts whole pixels to fixed point.
#[inline]
pub const fn px(v: i32) -> Fx {
    v << SHIFT
}

/// Truncates fixed point to whole pixels, rounding towards negative infinity.
#[inline]
pub const fn to_px(v: Fx) -> i32 {
    v >> SHIFT
}

/// Rounds fixed point to the nearest whole pixel.
#[inline]
pub const fn round_px(v: Fx) -> i32 {
    (v + HALF) >> SHIFT
}

/// Multiplies two fixed-point values.
#[inline]
pub fn fmul(a: Fx, b: Fx) -> Fx {
    ((a as i64 * b as i64) >> SHIFT) as Fx
}

/// Divides two fixed-point values. Returns 0 when `b` is 0.
#[inline]
pub fn fdiv(a: Fx, b: Fx) -> Fx {
    if b == 0 {
        0
    } else {
        (((a as i64) << SHIFT) / b as i64) as Fx
    }
}

/// Moves `v` towards `target` by at most `step`.
#[inline]
pub fn approach(v: Fx, target: Fx, step: Fx) -> Fx {
    if v < target {
        (v + step).min(target)
    } else {
        (v - step).max(target)
    }
}

/// Integer square root, used for distance comparisons in AI code.
pub fn isqrt(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// Length of a fixed-point vector.
#[inline]
pub fn length(x: Fx, y: Fx) -> Fx {
    isqrt(x as i64 * x as i64 + y as i64 * y as i64) as Fx
}

/// A 256-entry sine table in fixed point (one turn = 256 units).
static SIN_TABLE: [i16; 256] = build_sin_table();

const fn build_sin_table() -> [i16; 256] {
    // Generated with a const-evaluable CORDIC-free polynomial approximation:
    // sin(x) via the classic Bhaskara approximation on [0, pi], mirrored.
    let mut t = [0i16; 256];
    let mut i = 0;
    while i < 256 {
        // Map i to degrees * 1000 to stay in integers.
        let deg = (i as i64) * 360_000 / 256;
        let (d, sign) = if deg <= 180_000 {
            (deg, 1i64)
        } else {
            (deg - 180_000, -1i64)
        };
        // Bhaskara I: sin(d) ~= 4d(180-d) / (40500 - d(180-d)) for d in degrees.
        let dd = d / 1000;
        let num = 4 * dd * (180 - dd);
        let den = 40500 - dd * (180 - dd);
        let v = if den == 0 { 0 } else { sign * num * 256 / den };
        t[i] = v as i16;
        i += 1;
    }
    t
}

/// Sine of an angle where a full turn is 256 units. Result is fixed point.
#[inline]
pub fn sin(angle: i32) -> Fx {
    SIN_TABLE[(angle & 0xff) as usize] as Fx
}

/// Cosine of an angle where a full turn is 256 units. Result is fixed point.
#[inline]
pub fn cos(angle: i32) -> Fx {
    sin(angle + 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px_roundtrip() {
        assert_eq!(to_px(px(7)), 7);
        assert_eq!(to_px(px(-7)), -7);
        assert_eq!(round_px(px(3) + HALF), 4);
    }

    #[test]
    fn fmul_is_pixel_scaled() {
        assert_eq!(fmul(px(3), px(4)), px(12));
        assert_eq!(fmul(px(3), HALF), px(1) + HALF);
    }

    #[test]
    fn isqrt_matches() {
        for n in [0i64, 1, 2, 3, 4, 15, 16, 17, 100, 12345, 1 << 30] {
            let r = isqrt(n);
            assert!(r * r <= n, "{n}");
            assert!((r + 1) * (r + 1) > n, "{n}");
        }
    }

    #[test]
    fn trig_is_bounded_and_periodic() {
        for a in 0..256 {
            assert!(sin(a).abs() <= ONE + 2, "sin({a}) = {}", sin(a));
            assert_eq!(sin(a), sin(a + 256));
        }
        assert!((sin(0)).abs() <= 2);
        assert!((sin(64) - ONE).abs() <= 6);
        assert!((sin(192) + ONE).abs() <= 6);
    }
}
