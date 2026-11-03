//! The KAGAMI controller: a d-pad and four buttons packed into one byte.

use core::fmt;
use core::ops::{BitAnd, BitOr, BitOrAssign, Not};

/// One frame of controller state. Each bit is a button that is held.
///
/// ```
/// use kagami::Input;
/// let i = Input::LEFT | Input::A;
/// assert!(i.left() && i.a() && !i.right());
/// assert_eq!(i.bits(), Input::LEFT.bits() | Input::A.bits());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Input(u8);

impl Input {
    /// Nothing held.
    pub const NONE: Input = Input(0);
    /// D-pad up.
    pub const UP: Input = Input(1);
    /// D-pad down.
    pub const DOWN: Input = Input(2);
    /// D-pad left.
    pub const LEFT: Input = Input(4);
    /// D-pad right.
    pub const RIGHT: Input = Input(8);
    /// Primary action button (丸, *maru*).
    pub const A: Input = Input(16);
    /// Secondary action button (罰, *batsu*).
    pub const B: Input = Input(32);
    /// Third action button (三角, *sankaku*).
    pub const C: Input = Input(64);
    /// Start / pause.
    pub const START: Input = Input(128);

    /// Builds from raw bits.
    #[inline]
    pub const fn from_bits(b: u8) -> Input {
        Input(b)
    }

    /// Raw bits.
    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns `true` if every button in `other` is held.
    #[inline]
    pub const fn contains(self, other: Input) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns `true` if nothing is held.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Up held.
    #[inline]
    pub const fn up(self) -> bool {
        self.contains(Input::UP)
    }
    /// Down held.
    #[inline]
    pub const fn down(self) -> bool {
        self.contains(Input::DOWN)
    }
    /// Left held.
    #[inline]
    pub const fn left(self) -> bool {
        self.contains(Input::LEFT)
    }
    /// Right held.
    #[inline]
    pub const fn right(self) -> bool {
        self.contains(Input::RIGHT)
    }
    /// A held.
    #[inline]
    pub const fn a(self) -> bool {
        self.contains(Input::A)
    }
    /// B held.
    #[inline]
    pub const fn b(self) -> bool {
        self.contains(Input::B)
    }
    /// C held.
    #[inline]
    pub const fn c(self) -> bool {
        self.contains(Input::C)
    }
    /// Start held.
    #[inline]
    pub const fn start(self) -> bool {
        self.contains(Input::START)
    }

    /// Horizontal axis: `-1`, `0` or `1`. Opposite directions cancel.
    #[inline]
    pub const fn axis_x(self) -> i32 {
        (self.right() as i32) - (self.left() as i32)
    }

    /// Vertical axis: `-1` (up), `0` or `1` (down).
    #[inline]
    pub const fn axis_y(self) -> i32 {
        (self.down() as i32) - (self.up() as i32)
    }

    /// Buttons that are held now but were not held in `prev`.
    #[inline]
    pub const fn pressed_since(self, prev: Input) -> Input {
        Input(self.0 & !prev.0)
    }

    /// Buttons that were held in `prev` but are not held now.
    #[inline]
    pub const fn released_since(self, prev: Input) -> Input {
        Input(prev.0 & !self.0)
    }
}

impl BitOr for Input {
    type Output = Input;
    fn bitor(self, o: Input) -> Input {
        Input(self.0 | o.0)
    }
}
impl BitOrAssign for Input {
    fn bitor_assign(&mut self, o: Input) {
        self.0 |= o.0;
    }
}
impl BitAnd for Input {
    type Output = Input;
    fn bitand(self, o: Input) -> Input {
        Input(self.0 & o.0)
    }
}
impl Not for Input {
    type Output = Input;
    fn not(self) -> Input {
        Input(!self.0)
    }
}

impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [&str; 8] = ["UP", "DOWN", "LEFT", "RIGHT", "A", "B", "C", "START"];
        write!(f, "Input(")?;
        let mut first = true;
        for (i, name) in NAMES.iter().enumerate() {
            if self.0 & (1 << i) != 0 {
                if !first {
                    write!(f, "|")?;
                }
                write!(f, "{name}")?;
                first = false;
            }
        }
        if first {
            write!(f, "NONE")?;
        }
        write!(f, ")")
    }
}

/// Tracks the previous frame's input to expose press/release edges.
///
/// ```
/// use kagami::input::{Edge, Input};
/// let mut e = Edge::default();
/// e.update(Input::A);
/// assert!(e.pressed(Input::A));
/// e.update(Input::A);
/// assert!(!e.pressed(Input::A) && e.held(Input::A));
/// e.update(Input::NONE);
/// assert!(e.released(Input::A));
/// ```
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    prev: Input,
    cur: Input,
}

impl Edge {
    /// Feeds the current frame's input.
    pub fn update(&mut self, now: Input) {
        self.prev = self.cur;
        self.cur = now;
    }
    /// Current input.
    pub const fn current(&self) -> Input {
        self.cur
    }
    /// Buttons held this frame.
    pub const fn held(&self, b: Input) -> bool {
        self.cur.contains(b)
    }
    /// Buttons that went down this frame.
    pub const fn pressed(&self, b: Input) -> bool {
        self.cur.pressed_since(self.prev).contains(b)
    }
    /// Buttons that went up this frame.
    pub const fn released(&self, b: Input) -> bool {
        self.cur.released_since(self.prev).contains(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_and_queries() {
        let i = Input::UP | Input::B;
        assert!(i.up() && i.b());
        assert!(!i.down() && !i.left() && !i.right() && !i.a() && !i.c() && !i.start());
        assert_eq!(i.bits(), 33);
        assert_eq!(Input::from_bits(33), i);
        assert!(Input::NONE.is_empty());
        assert!(!i.is_empty());
        assert_eq!(i & Input::UP, Input::UP);
        assert_eq!(!Input::NONE, Input::from_bits(255));
        let mut j = Input::NONE;
        j |= Input::START;
        assert!(j.start());
    }

    #[test]
    fn axes_cancel() {
        assert_eq!((Input::LEFT | Input::RIGHT).axis_x(), 0);
        assert_eq!(Input::LEFT.axis_x(), -1);
        assert_eq!(Input::RIGHT.axis_x(), 1);
        assert_eq!(Input::UP.axis_y(), -1);
        assert_eq!(Input::DOWN.axis_y(), 1);
        assert_eq!(Input::NONE.axis_y(), 0);
    }

    #[test]
    fn edges() {
        let now = Input::A | Input::B;
        let prev = Input::B | Input::C;
        assert_eq!(now.pressed_since(prev), Input::A);
        assert_eq!(now.released_since(prev), Input::C);
    }

    #[test]
    fn debug_format() {
        assert_eq!(format!("{:?}", Input::NONE), "Input(NONE)");
        assert_eq!(
            format!("{:?}", Input::LEFT | Input::START),
            "Input(LEFT|START)"
        );
    }
}
