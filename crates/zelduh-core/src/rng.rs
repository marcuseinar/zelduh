//! Deterministic pseudo-random numbers.
//!
//! Every random draw the simulation makes must be reproducible from the world
//! seed and the frame number, so this is a plain `splitmix64` generator with no
//! environmental input.

/// A small, fast, fully deterministic random number generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a generator from a seed.
    pub const fn new(seed: u64) -> Self {
        Rng {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    /// Returns the raw internal state, for snapshotting.
    pub const fn state(&self) -> u64 {
        self.state
    }

    /// Restores a generator from a snapshot taken with [`Rng::state`].
    pub const fn from_state(state: u64) -> Self {
        Rng { state }
    }

    /// Draws the next 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Draws the next 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Draws a value in `0..n`. Returns 0 when `n` is 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Multiply-shift: unbiased enough for gameplay and branch-free.
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// Draws a value in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// Returns true with probability `num / den`.
    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        self.below(den.max(1)) < num
    }

    /// Returns true half the time.
    pub fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// Picks a random element of `items`.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u32) as usize]
    }

    /// Shuffles a slice in place.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Derives an independent generator, so one subsystem drawing more numbers
    /// cannot shift another subsystem's sequence.
    pub fn fork(&mut self, tag: u64) -> Rng {
        Rng::new(self.next_u64() ^ tag.wrapping_mul(0xd6e8_feb8_6659_fd93))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn below_is_in_range() {
        let mut r = Rng::new(7);
        for n in 1..64u32 {
            for _ in 0..64 {
                assert!(r.below(n) < n);
            }
        }
        assert_eq!(r.below(0), 0);
    }

    #[test]
    fn shuffle_keeps_elements() {
        let mut r = Rng::new(9);
        let mut v: Vec<i32> = (0..32).collect();
        r.shuffle(&mut v);
        v.sort();
        assert_eq!(v, (0..32).collect::<Vec<_>>());
    }

    #[test]
    fn state_roundtrips() {
        let mut a = Rng::new(123);
        a.next_u64();
        let mut b = Rng::from_state(a.state());
        assert_eq!(a.next_u64(), b.next_u64());
    }
}
