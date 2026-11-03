//! 16.16 fixed-point arithmetic.
//!
//! All simulation math on KAGAMI uses [`Fx`]: a signed 32-bit integer with 16
//! fractional bits. Fixed-point arithmetic is bit-for-bit identical on every
//! machine and every compiler, which is what makes lockstep possible without
//! ever sending state over the wire.
//!
//! ```
//! use kagami::fx::Fx;
//! let a = Fx::from_int(3);
//! let b = Fx::from_ratio(1, 2);
//! assert_eq!((a * b).to_int(), 1);
//! assert_eq!((a * b).frac_bits(), 0x8000);
//! ```

use core::fmt;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// Number of fractional bits in an [`Fx`].
pub const FRAC_BITS: u32 = 16;
const ONE_RAW: i32 = 1 << FRAC_BITS;
const FRAC_MASK: i32 = ONE_RAW - 1;

/// A 16.16 signed fixed-point number.
///
/// Arithmetic wraps on overflow in release builds exactly like the underlying
/// `i32`, and that wrapping is itself deterministic. Games should keep
/// magnitudes below 32767.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fx(i32);

impl Fx {
    /// Zero.
    pub const ZERO: Fx = Fx(0);
    /// One.
    pub const ONE: Fx = Fx(ONE_RAW);
    /// One half.
    pub const HALF: Fx = Fx(ONE_RAW / 2);
    /// Largest representable value.
    pub const MAX: Fx = Fx(i32::MAX);
    /// Smallest representable value.
    pub const MIN: Fx = Fx(i32::MIN);
    /// π.
    pub const PI: Fx = Fx(205_887);
    /// 2π.
    pub const TAU: Fx = Fx(411_775);

    /// Builds a value from its raw 16.16 bit pattern.
    #[inline]
    pub const fn from_raw(raw: i32) -> Fx {
        Fx(raw)
    }

    /// Returns the raw 16.16 bit pattern.
    #[inline]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Converts an integer.
    #[inline]
    pub const fn from_int(v: i32) -> Fx {
        Fx(v.wrapping_shl(FRAC_BITS))
    }

    /// Builds `num / den` rounded toward zero. Panics if `den == 0`.
    #[inline]
    pub const fn from_ratio(num: i32, den: i32) -> Fx {
        Fx((((num as i64) << FRAC_BITS) / den as i64) as i32)
    }

    /// Integer part, rounding toward negative infinity.
    #[inline]
    pub const fn to_int(self) -> i32 {
        self.0 >> FRAC_BITS
    }

    /// Rounds to the nearest integer, halves away from zero.
    #[inline]
    pub const fn round(self) -> i32 {
        if self.0 >= 0 {
            (self.0 + ONE_RAW / 2) >> FRAC_BITS
        } else {
            -((-self.0 + ONE_RAW / 2) >> FRAC_BITS)
        }
    }

    /// Fractional bits (0..65535), always non-negative.
    #[inline]
    pub const fn frac_bits(self) -> u32 {
        (self.0 & FRAC_MASK) as u32
    }

    /// Largest integer not greater than `self`, as an [`Fx`].
    #[inline]
    pub const fn floor(self) -> Fx {
        Fx(self.0 & !FRAC_MASK)
    }

    /// Smallest integer not less than `self`, as an [`Fx`].
    #[inline]
    pub const fn ceil(self) -> Fx {
        if self.0 & FRAC_MASK == 0 {
            self
        } else {
            Fx((self.0 & !FRAC_MASK).wrapping_add(ONE_RAW))
        }
    }

    /// Absolute value (saturating at [`Fx::MAX`]).
    #[inline]
    pub const fn abs(self) -> Fx {
        Fx(self.0.saturating_abs())
    }

    /// `-1`, `0` or `1`.
    #[inline]
    pub const fn signum(self) -> Fx {
        Fx::from_int(self.0.signum())
    }

    /// Returns `true` if negative.
    #[inline]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Smaller of two values.
    #[inline]
    pub const fn min(self, other: Fx) -> Fx {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    /// Larger of two values.
    #[inline]
    pub const fn max(self, other: Fx) -> Fx {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    /// Clamps into `[lo, hi]`.
    #[inline]
    pub const fn clamp(self, lo: Fx, hi: Fx) -> Fx {
        self.max(lo).min(hi)
    }

    /// Saturating multiply.
    #[inline]
    pub const fn saturating_mul(self, other: Fx) -> Fx {
        let r = ((self.0 as i64) * (other.0 as i64)) >> FRAC_BITS;
        if r > i32::MAX as i64 {
            Fx::MAX
        } else if r < i32::MIN as i64 {
            Fx::MIN
        } else {
            Fx(r as i32)
        }
    }

    /// Linear interpolation: `a + (b - a) * t`.
    #[inline]
    pub fn lerp(a: Fx, b: Fx, t: Fx) -> Fx {
        a + (b - a) * t
    }

    /// Square root, rounded down to 16.16 precision. Negative input yields
    /// zero.
    pub fn sqrt(self) -> Fx {
        if self.0 <= 0 {
            return Fx::ZERO;
        }
        // sqrt(x * 2^16) in 16.16 is isqrt(raw << 16).
        let n = (self.0 as u64) << FRAC_BITS;
        Fx(isqrt_u64(n) as i32)
    }

    /// Sine of an angle expressed in *turns* (`0..=1` is a full circle).
    ///
    /// Uses a 256-entry quarter-wave table so results are identical everywhere.
    pub fn sin_turns(turns: Fx) -> Fx {
        // 1024 steps per turn; keep the fractional turn only.
        let idx = ((turns.0 as i64 * 1024) >> FRAC_BITS) as i32 & 1023;
        sin_table_1024(idx)
    }

    /// Cosine of an angle in turns. See [`Fx::sin_turns`].
    pub fn cos_turns(turns: Fx) -> Fx {
        Fx::sin_turns(turns + Fx::from_ratio(1, 4))
    }
}

/// Integer square root (floor).
const fn isqrt_u64(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = 1u64 << ((64 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Quarter-wave sine table with 256 entries for 0..=1/4 turn (inclusive 257th
/// value is ONE). Generated once at compile time with integer-only math via a
/// Bhaskara-style rational approximation refined by a CORDIC-free series.
const SIN_QUARTER: [i32; 257] = build_sin_quarter();

const fn build_sin_quarter() -> [i32; 257] {
    // sin(x) for x in [0, π/2] via a Taylor series evaluated in 32-bit
    // fixed point with 128-bit intermediates, so every entry is exact to
    // 16.16 precision on every platform.
    const SCALE: u32 = 32;
    const HALF_PI: i128 = 6_746_518_852; // π/2 * 2^32
    let mut t = [0i32; 257];
    let mut i = 0;
    while i <= 256 {
        let x: i128 = (HALF_PI * i as i128) >> 8;
        let x2 = (x * x) >> SCALE;
        let x3 = (x2 * x) >> SCALE;
        let x5 = (x3 * x2) >> SCALE;
        let x7 = (x5 * x2) >> SCALE;
        let x9 = (x7 * x2) >> SCALE;
        let x11 = (x9 * x2) >> SCALE;
        let s = x - x3 / 6 + x5 / 120 - x7 / 5040 + x9 / 362_880 - x11 / 39_916_800;
        // 32 fractional bits -> 16 fractional bits, rounded.
        let v = (s + (1 << 15)) >> 16;
        t[i] = if v > ONE_RAW as i128 {
            ONE_RAW
        } else if v < 0 {
            0
        } else {
            v as i32
        };
        i += 1;
    }
    t[256] = ONE_RAW;
    t
}

fn sin_table_1024(idx: i32) -> Fx {
    let q = idx & 255;
    match idx >> 8 {
        0 => Fx(SIN_QUARTER[q as usize]),
        1 => Fx(SIN_QUARTER[(256 - q) as usize]),
        2 => Fx(-SIN_QUARTER[q as usize]),
        _ => Fx(-SIN_QUARTER[(256 - q) as usize]),
    }
}

impl From<i32> for Fx {
    fn from(v: i32) -> Fx {
        Fx::from_int(v)
    }
}

impl Add for Fx {
    type Output = Fx;
    #[inline]
    fn add(self, o: Fx) -> Fx {
        Fx(self.0.wrapping_add(o.0))
    }
}
impl Sub for Fx {
    type Output = Fx;
    #[inline]
    fn sub(self, o: Fx) -> Fx {
        Fx(self.0.wrapping_sub(o.0))
    }
}
impl Mul for Fx {
    type Output = Fx;
    #[inline]
    fn mul(self, o: Fx) -> Fx {
        Fx((((self.0 as i64) * (o.0 as i64)) >> FRAC_BITS) as i32)
    }
}
impl Div for Fx {
    type Output = Fx;
    /// Division. Panics on division by zero.
    #[inline]
    fn div(self, o: Fx) -> Fx {
        Fx((((self.0 as i64) << FRAC_BITS) / (o.0 as i64)) as i32)
    }
}
impl Mul<i32> for Fx {
    type Output = Fx;
    #[inline]
    fn mul(self, o: i32) -> Fx {
        Fx(self.0.wrapping_mul(o))
    }
}
impl Div<i32> for Fx {
    type Output = Fx;
    #[inline]
    fn div(self, o: i32) -> Fx {
        Fx(self.0 / o)
    }
}
impl Neg for Fx {
    type Output = Fx;
    #[inline]
    fn neg(self) -> Fx {
        Fx(self.0.wrapping_neg())
    }
}
impl AddAssign for Fx {
    fn add_assign(&mut self, o: Fx) {
        *self = *self + o;
    }
}
impl SubAssign for Fx {
    fn sub_assign(&mut self, o: Fx) {
        *self = *self - o;
    }
}
impl MulAssign for Fx {
    fn mul_assign(&mut self, o: Fx) {
        *self = *self * o;
    }
}
impl DivAssign for Fx {
    fn div_assign(&mut self, o: Fx) {
        *self = *self / o;
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fx({})", self)
    }
}

impl fmt::Display for Fx {
    /// Formats as a decimal with up to four fractional digits, without floats.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let neg = self.0 < 0;
        let a = (self.0 as i64).abs();
        let int = a >> FRAC_BITS;
        let frac = ((a & FRAC_MASK as i64) * 10_000 + ONE_RAW as i64 / 2) >> FRAC_BITS;
        let (int, frac) = if frac >= 10_000 {
            (int + 1, 0)
        } else {
            (int, frac)
        };
        if neg {
            write!(f, "-")?;
        }
        if frac == 0 {
            write!(f, "{int}")
        } else {
            let s = format!("{frac:04}");
            write!(f, "{int}.{}", s.trim_end_matches('0'))
        }
    }
}

/// A two-dimensional fixed-point vector.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Vec2 {
    /// Horizontal component.
    pub x: Fx,
    /// Vertical component (positive is down, matching screen space).
    pub y: Fx,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Vec2 = Vec2 {
        x: Fx::ZERO,
        y: Fx::ZERO,
    };

    /// Builds a vector.
    #[inline]
    pub const fn new(x: Fx, y: Fx) -> Vec2 {
        Vec2 { x, y }
    }

    /// Builds a vector from integers.
    #[inline]
    pub const fn from_ints(x: i32, y: i32) -> Vec2 {
        Vec2 {
            x: Fx::from_int(x),
            y: Fx::from_int(y),
        }
    }

    /// Dot product.
    #[inline]
    pub fn dot(self, o: Vec2) -> Fx {
        self.x * o.x + self.y * o.y
    }

    /// Squared length.
    #[inline]
    pub fn len_sq(self) -> Fx {
        self.dot(self)
    }

    /// Length.
    #[inline]
    pub fn len(self) -> Fx {
        self.len_sq().sqrt()
    }

    /// Unit-length copy, or zero if the vector is zero.
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l == Fx::ZERO {
            Vec2::ZERO
        } else {
            Vec2::new(self.x / l, self.y / l)
        }
    }

    /// Component-wise scale.
    #[inline]
    pub fn scale(self, s: Fx) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }

    /// Manhattan distance to another point.
    #[inline]
    pub fn manhattan(self, o: Vec2) -> Fx {
        (self.x - o.x).abs() + (self.y - o.y).abs()
    }

    /// Returns the unit vector pointing at `turns` (0 = +x, 1/4 = +y).
    pub fn from_turns(turns: Fx) -> Vec2 {
        Vec2::new(Fx::cos_turns(turns), Fx::sin_turns(turns))
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    #[inline]
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    #[inline]
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<Fx> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn mul(self, s: Fx) -> Vec2 {
        self.scale(s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    #[inline]
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

/// An axis-aligned rectangle in fixed-point coordinates.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Rect {
    /// Top-left corner.
    pub pos: Vec2,
    /// Width and height.
    pub size: Vec2,
}

impl Rect {
    /// Builds a rectangle from integer coordinates.
    pub const fn from_ints(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect {
            pos: Vec2::from_ints(x, y),
            size: Vec2::from_ints(w, h),
        }
    }

    /// Builds a rectangle.
    pub const fn new(pos: Vec2, size: Vec2) -> Rect {
        Rect { pos, size }
    }

    /// Right edge (exclusive).
    pub fn right(&self) -> Fx {
        self.pos.x + self.size.x
    }

    /// Bottom edge (exclusive).
    pub fn bottom(&self) -> Fx {
        self.pos.y + self.size.y
    }

    /// Returns `true` if the two rectangles overlap (touching edges do not count).
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.pos.x < o.right()
            && o.pos.x < self.right()
            && self.pos.y < o.bottom()
            && o.pos.y < self.bottom()
    }

    /// Returns `true` if the point is inside.
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.pos.x && p.x < self.right() && p.y >= self.pos.y && p.y < self.bottom()
    }

    /// Returns a copy moved by `d`.
    pub fn translated(&self, d: Vec2) -> Rect {
        Rect::new(self.pos + d, self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_arithmetic() {
        let a = Fx::from_int(5);
        let b = Fx::from_ratio(3, 2);
        assert_eq!((a + b).to_int(), 6);
        assert_eq!((a - b).to_int(), 3);
        assert_eq!((a * b).to_int(), 7);
        assert_eq!((a / b), Fx::from_ratio(10, 3));
        assert_eq!(-a, Fx::from_int(-5));
        assert_eq!(a * 2, Fx::from_int(10));
        assert_eq!(a / 2, Fx::from_ratio(5, 2));
        let mut c = a;
        c += b;
        c -= b;
        c *= Fx::from_int(2);
        c /= Fx::from_int(2);
        assert_eq!(c, a);
        assert_eq!(Fx::from(7), Fx::from_int(7));
    }

    #[test]
    fn rounding_and_parts() {
        let v = Fx::from_ratio(-7, 2); // -3.5
        assert_eq!(v.to_int(), -4);
        assert_eq!(v.round(), -4);
        assert_eq!(v.floor(), Fx::from_int(-4));
        assert_eq!(v.ceil(), Fx::from_int(-3));
        assert_eq!(v.abs(), Fx::from_ratio(7, 2));
        assert_eq!(v.signum(), Fx::from_int(-1));
        assert!(v.is_negative());
        assert_eq!(Fx::from_ratio(5, 2).round(), 3);
        assert_eq!(Fx::from_int(4).ceil(), Fx::from_int(4));
        assert_eq!(Fx::from_ratio(1, 4).frac_bits(), 0x4000);
        assert_eq!(Fx::from_int(3).clamp(Fx::ZERO, Fx::ONE), Fx::ONE);
        assert_eq!(Fx::from_int(-3).clamp(Fx::ZERO, Fx::ONE), Fx::ZERO);
        assert_eq!(Fx::MIN.abs(), Fx::MAX);
        assert_eq!(Fx::MAX.saturating_mul(Fx::from_int(2)), Fx::MAX);
        assert_eq!(Fx::MIN.saturating_mul(Fx::from_int(2)), Fx::MIN);
        assert_eq!(
            Fx::from_int(3).saturating_mul(Fx::from_int(2)),
            Fx::from_int(6)
        );
        assert_eq!(
            Fx::lerp(Fx::ZERO, Fx::from_int(10), Fx::HALF),
            Fx::from_int(5)
        );
    }

    #[test]
    fn sqrt_is_exact_for_squares() {
        // 181^2 = 32761 is the largest perfect square that fits in 16.16.
        for i in 0..=181 {
            let v = Fx::from_int(i * i);
            assert_eq!(v.sqrt(), Fx::from_int(i), "sqrt({})", i * i);
        }
        assert_eq!(Fx::from_int(2).sqrt().raw(), 92_681); // 1.41421 * 65536 = 92681.9
        assert_eq!(Fx::from_int(-1).sqrt(), Fx::ZERO);
    }

    #[test]
    fn trig_table_matches_known_values() {
        assert_eq!(Fx::sin_turns(Fx::ZERO), Fx::ZERO);
        assert_eq!(Fx::sin_turns(Fx::from_ratio(1, 4)), Fx::ONE);
        assert_eq!(Fx::sin_turns(Fx::from_ratio(1, 2)), Fx::ZERO);
        assert_eq!(Fx::sin_turns(Fx::from_ratio(3, 4)), -Fx::ONE);
        assert_eq!(Fx::cos_turns(Fx::ZERO), Fx::ONE);
        assert_eq!(Fx::cos_turns(Fx::from_ratio(1, 2)), -Fx::ONE);
        // sin(1/12 turn) = sin(30°) = 0.5 within table precision.
        let s = Fx::sin_turns(Fx::from_ratio(1, 12));
        assert!((s - Fx::HALF).abs() < Fx::from_ratio(1, 200), "{s}");
        // Wraps past a full turn.
        assert_eq!(Fx::sin_turns(Fx::from_ratio(5, 4)), Fx::ONE);
        assert_eq!(Fx::sin_turns(Fx::from_ratio(-1, 4)), -Fx::ONE);
        // Monotonic in the first quadrant.
        for i in 1..256 {
            assert!(SIN_QUARTER[i] >= SIN_QUARTER[i - 1]);
        }
    }

    #[test]
    fn display_formats_without_floats() {
        assert_eq!(Fx::from_int(3).to_string(), "3");
        assert_eq!(Fx::from_ratio(-7, 2).to_string(), "-3.5");
        assert_eq!(Fx::from_ratio(1, 3).to_string(), "0.3333");
        assert_eq!(format!("{:?}", Fx::from_ratio(1, 4)), "Fx(0.25)");
        assert_eq!(Fx::from_raw(65_535).to_string(), "1");
    }

    #[test]
    fn vectors() {
        let a = Vec2::from_ints(3, 4);
        assert_eq!(a.len(), Fx::from_int(5));
        assert_eq!(a.len_sq(), Fx::from_int(25));
        let n = a.normalized();
        assert!((n.len() - Fx::ONE).abs() < Fx::from_ratio(1, 1000));
        assert_eq!(Vec2::ZERO.normalized(), Vec2::ZERO);
        assert_eq!(a + a, Vec2::from_ints(6, 8));
        assert_eq!(a - a, Vec2::ZERO);
        assert_eq!(a * Fx::from_int(2), Vec2::from_ints(6, 8));
        assert_eq!(-a, Vec2::from_ints(-3, -4));
        assert_eq!(a.dot(Vec2::from_ints(1, 1)), Fx::from_int(7));
        assert_eq!(a.manhattan(Vec2::ZERO), Fx::from_int(7));
        let mut b = a;
        b += a;
        b -= a;
        assert_eq!(b, a);
        let r = Vec2::from_turns(Fx::ZERO);
        assert_eq!(r, Vec2::new(Fx::ONE, Fx::ZERO));
    }

    #[test]
    fn rects() {
        let a = Rect::from_ints(0, 0, 10, 10);
        let b = Rect::from_ints(5, 5, 10, 10);
        let c = Rect::from_ints(10, 0, 5, 5);
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
        assert!(a.contains(Vec2::from_ints(9, 9)));
        assert!(!a.contains(Vec2::from_ints(10, 9)));
        assert_eq!(
            a.translated(Vec2::from_ints(1, 1)).pos,
            Vec2::from_ints(1, 1)
        );
        assert_eq!(a.right(), Fx::from_int(10));
        assert_eq!(a.bottom(), Fx::from_int(10));
        assert_eq!(Rect::new(Vec2::ZERO, Vec2::ZERO), Rect::default());
    }
}
