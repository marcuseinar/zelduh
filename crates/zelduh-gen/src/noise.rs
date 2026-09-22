//! Integer value noise.
//!
//! Terrain has to come out identical on every machine, so this is plain
//! integer hashing and interpolation rather than anything floating point.

/// Hashes a lattice point to a value in `0..=1023`.
fn hash(seed: u64, x: i32, y: i32) -> i32 {
    let mut h = seed
        ^ (x as i64 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as i64 as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 29;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 32;
    (h & 1023) as i32
}

/// Smooth step on a 0..=1024 fraction, so cells blend without visible seams.
#[inline]
fn smooth(t: i32) -> i32 {
    // 3t^2 - 2t^3, in fixed point with 1024 as one.
    let t2 = (t * t) >> 10;
    let t3 = (t2 * t) >> 10;
    (3 * t2 - 2 * t3).clamp(0, 1024)
}

/// Value noise at `(x, y)`, sampled on a lattice of `scale` tiles.
///
/// Returns 0..=1023.
pub fn value(seed: u64, x: i32, y: i32, scale: i32) -> i32 {
    let scale = scale.max(1);
    let (gx, gy) = (x.div_euclid(scale), y.div_euclid(scale));
    let (fx, fy) = (x.rem_euclid(scale), y.rem_euclid(scale));
    let (tx, ty) = (smooth(fx * 1024 / scale), smooth(fy * 1024 / scale));

    let a = hash(seed, gx, gy);
    let b = hash(seed, gx + 1, gy);
    let c = hash(seed, gx, gy + 1);
    let d = hash(seed, gx + 1, gy + 1);

    let top = a + (((b - a) * tx) >> 10);
    let bottom = c + (((d - c) * tx) >> 10);
    (top + (((bottom - top) * ty) >> 10)).clamp(0, 1023)
}

/// Two octaves of value noise, which gives large shapes with some detail.
pub fn fractal(seed: u64, x: i32, y: i32, scale: i32) -> i32 {
    let a = value(seed, x, y, scale);
    let b = value(seed ^ 0x5ad5_ad5a, x, y, (scale / 3).max(2));
    ((a * 3 + b) / 4).clamp(0, 1023)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_in_range() {
        for y in -50..50 {
            for x in -50..50 {
                let v = fractal(7, x, y, 16);
                assert!((0..1024).contains(&v), "{v}");
            }
        }
    }

    #[test]
    fn noise_is_deterministic() {
        assert_eq!(value(1, 5, 9, 16), value(1, 5, 9, 16));
        assert_ne!(value(1, 5, 9, 16), value(2, 5, 9, 16));
    }

    #[test]
    fn noise_is_smooth_between_neighbours() {
        // Adjacent samples on a coarse lattice should not jump the full range.
        let mut worst = 0;
        for x in 0..200 {
            let a = value(3, x, 0, 32);
            let b = value(3, x + 1, 0, 32);
            worst = worst.max((a - b).abs());
        }
        assert!(worst < 200, "noise jumped by {worst}");
    }

    #[test]
    fn negative_coordinates_work() {
        let a = value(9, -33, -17, 16);
        assert!((0..1024).contains(&a));
    }

    #[test]
    fn a_scale_of_zero_does_not_divide_by_zero() {
        let _ = value(1, 3, 3, 0);
    }
}
