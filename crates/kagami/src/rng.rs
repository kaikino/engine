//! Seeded pseudo-random numbers (xoshiro128\*\*).
//!
//! The generator is part of game state: seed it from the match seed, hash it
//! with the rest of the state, and never pull randomness from anywhere else.

use crate::fx::Fx;
use crate::hash::StateHasher;

/// A xoshiro128\*\* generator. Small, fast and fully deterministic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    s: [u32; 4],
}

impl Rng {
    /// Creates a generator from a 32-bit seed, expanding it with splitmix32.
    pub fn new(seed: u32) -> Rng {
        let mut x = seed;
        let mut s = [0u32; 4];
        for v in &mut s {
            // splitmix32
            x = x.wrapping_add(0x9E37_79B9);
            let mut z = x;
            z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
            z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
            *v = z ^ (z >> 16);
        }
        if s == [0; 4] {
            s[0] = 1;
        }
        Rng { s }
    }

    /// Next 32 random bits.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 9;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(11);
        result
    }

    /// Uniform integer in `[0, n)`. Returns 0 if `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Lemire's nearly-divisionless method, debiased.
        let mut m = u64::from(self.next_u32()) * u64::from(n);
        let mut l = m as u32;
        if l < n {
            let t = n.wrapping_neg() % n;
            while l < t {
                m = u64::from(self.next_u32()) * u64::from(n);
                l = m as u32;
            }
        }
        (m >> 32) as u32
    }

    /// Uniform integer in `[lo, hi]` (inclusive). Panics if `lo > hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        assert!(lo <= hi, "Rng::range lo > hi");
        let span = (i64::from(hi) - i64::from(lo) + 1) as u32;
        lo.wrapping_add(self.below(span) as i32)
    }

    /// Returns `true` with probability `num / den`.
    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        self.below(den) < num
    }

    /// Uniform fixed-point value in `[0, 1)`.
    pub fn fx(&mut self) -> Fx {
        Fx::from_raw((self.next_u32() >> 16) as i32)
    }

    /// Uniform fixed-point value in `[lo, hi)`.
    pub fn fx_range(&mut self, lo: Fx, hi: Fx) -> Fx {
        lo + (hi - lo) * self.fx()
    }

    /// Picks a random element, or `None` if the slice is empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            items.get(self.below(items.len() as u32) as usize)
        }
    }

    /// Shuffles a slice in place (Fisher–Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Feeds the generator state into a [`StateHasher`].
    pub fn hash_state(&self, h: &mut StateHasher) {
        for v in &self.s {
            h.write_u32(*v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_seed() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let mut c = Rng::new(43);
        assert_ne!(a.next_u32(), c.next_u32());
    }

    #[test]
    fn known_sequence() {
        // Pinned so any change to the algorithm is caught.
        let mut r = Rng::new(0);
        let seq: Vec<u32> = (0..4).map(|_| r.next_u32()).collect();
        assert_eq!(
            seq,
            vec![3_809_008_728, 1_133_695_204, 53_579_671, 2_891_528_803]
        );
    }

    #[test]
    fn ranges_are_in_bounds() {
        let mut r = Rng::new(7);
        let mut seen = [false; 6];
        for _ in 0..5000 {
            let v = r.range(-2, 3);
            assert!((-2..=3).contains(&v));
            seen[(v + 2) as usize] = true;
            assert!(r.below(10) < 10);
            let f = r.fx();
            assert!(f >= Fx::ZERO && f < Fx::ONE);
            let g = r.fx_range(Fx::from_int(2), Fx::from_int(4));
            assert!(g >= Fx::from_int(2) && g < Fx::from_int(4));
        }
        assert!(seen.iter().all(|&s| s));
        assert_eq!(r.below(0), 0);
        assert_eq!(r.range(5, 5), 5);
        assert!(r.chance(1, 1));
        assert!(!r.chance(0, 1));
    }

    #[test]
    fn pick_and_shuffle() {
        let mut r = Rng::new(1);
        assert_eq!(r.pick::<i32>(&[]), None);
        assert_eq!(r.pick(&[9]), Some(&9));
        let mut v: Vec<u32> = (0..20).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
        assert_ne!(v, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn zero_state_is_avoided() {
        // No seed may produce an all-zero state; smoke-test a few.
        for s in [0u32, 1, u32::MAX, 0xDEAD_BEEF] {
            let r = Rng::new(s);
            assert_ne!(r.s, [0; 4]);
        }
    }

    #[test]
    fn hashes_state() {
        let mut h1 = StateHasher::new();
        let mut h2 = StateHasher::new();
        Rng::new(1).hash_state(&mut h1);
        Rng::new(2).hash_state(&mut h2);
        assert_ne!(h1.finish(), h2.finish());
    }
}
