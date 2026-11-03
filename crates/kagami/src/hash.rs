//! Deterministic state hashing.
//!
//! Every game exposes its state through [`crate::Game::hash`]. The lockstep
//! layer compares hashes between peers to detect desyncs, and replays store a
//! hash per checkpoint so a diverging engine build is caught at the exact frame.
//!
//! The hash is 64-bit FNV-1a over the little-endian bytes fed to it. It is not
//! cryptographic; it only needs to be fast, stable across platforms and
//! sensitive to any single-bit change.

use crate::fx::{Fx, Vec2};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// An incremental 64-bit FNV-1a hasher.
///
/// ```
/// use kagami::StateHasher;
/// let mut a = StateHasher::new();
/// a.write_i32(7);
/// let mut b = StateHasher::new();
/// b.write_i32(8);
/// assert_ne!(a.finish(), b.finish());
/// ```
#[derive(Clone, Debug)]
pub struct StateHasher {
    state: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHasher {
    /// Starts a new hash.
    pub const fn new() -> StateHasher {
        StateHasher { state: FNV_OFFSET }
    }

    /// Feeds raw bytes.
    #[inline]
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    /// Feeds a `u8`.
    #[inline]
    pub fn write_u8(&mut self, v: u8) {
        self.write_bytes(&[v]);
    }
    /// Feeds a `bool`.
    #[inline]
    pub fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }
    /// Feeds a `u16`.
    #[inline]
    pub fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_le_bytes());
    }
    /// Feeds a `u32`.
    #[inline]
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }
    /// Feeds an `i32`.
    #[inline]
    pub fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_le_bytes());
    }
    /// Feeds a `u64`.
    #[inline]
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }
    /// Feeds an [`Fx`].
    #[inline]
    pub fn write_fx(&mut self, v: Fx) {
        self.write_i32(v.raw());
    }
    /// Feeds a [`Vec2`].
    #[inline]
    pub fn write_vec2(&mut self, v: Vec2) {
        self.write_fx(v.x);
        self.write_fx(v.y);
    }
    /// Feeds a `usize` as a `u64` so the hash does not depend on pointer width.
    #[inline]
    pub fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    /// Returns the hash so far without consuming the hasher.
    #[inline]
    pub const fn finish(&self) -> u64 {
        self.state
    }
}

/// Convenience: hash a closure's writes in one call.
pub fn hash_with(f: impl FnOnce(&mut StateHasher)) -> u64 {
    let mut h = StateHasher::new();
    f(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_fnv1a() {
        // FNV-1a 64 of "a" is 0xaf63dc4c8601ec8c.
        let mut h = StateHasher::new();
        h.write_bytes(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
        let mut h = StateHasher::new();
        h.write_bytes(b"foobar");
        assert_eq!(h.finish(), 0x85944171f73967e8);
        assert_eq!(StateHasher::default().finish(), FNV_OFFSET);
    }

    type Writer = Box<dyn Fn(&mut StateHasher)>;

    #[test]
    fn all_writers_change_state() {
        let base = StateHasher::new().finish();
        let writers: Vec<Writer> = vec![
            Box::new(|h| h.write_u8(2)),
            Box::new(|h| h.write_bool(true)),
            Box::new(|h| h.write_u16(1)),
            Box::new(|h| h.write_u32(1)),
            Box::new(|h| h.write_i32(-1)),
            Box::new(|h| h.write_u64(1)),
            Box::new(|h| h.write_fx(Fx::ONE)),
            Box::new(|h| h.write_vec2(Vec2::from_ints(1, 2))),
            Box::new(|h| h.write_usize(3)),
        ];
        let mut seen = vec![base];
        for w in &writers {
            let v = hash_with(|h| w(h));
            assert!(!seen.contains(&v));
            seen.push(v);
        }
    }

    #[test]
    fn order_sensitive() {
        let a = hash_with(|h| {
            h.write_u8(1);
            h.write_u8(2);
        });
        let b = hash_with(|h| {
            h.write_u8(2);
            h.write_u8(1);
        });
        assert_ne!(a, b);
    }
}
