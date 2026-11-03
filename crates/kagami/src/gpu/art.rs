//! Tile art authored as text.
//!
//! Cartridges embed their tiles as ASCII art so the whole game lives in one
//! readable source tree with no binary assets:
//!
//! ```
//! use kagami::gpu::art::parse_tiles;
//! let tiles = parse_tiles(
//!     "........\n\
//!      .111111.\n\
//!      .1....1.\n\
//!      .1.22.1.\n\
//!      .1.22.1.\n\
//!      .1....1.\n\
//!      .111111.\n\
//!      ........",
//!     ".12",
//!     &[0, 9, 15],
//! ).unwrap();
//! assert_eq!(tiles.len(), 64);
//! assert_eq!(tiles[9], 9);
//! ```

use super::TILE_PIXELS;

/// Errors from [`parse_tiles`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtError {
    /// A character in the art is not in the key string.
    UnknownChar {
        /// The offending character.
        ch: char,
        /// Zero-based line number.
        line: usize,
    },
    /// The art's width is not a multiple of 8.
    BadWidth(usize),
    /// The art's height is not a multiple of 8.
    BadHeight(usize),
    /// A line is shorter or longer than the first line.
    RaggedLine(usize),
    /// `key` and `colors` have different lengths.
    KeyMismatch,
}

impl core::fmt::Display for ArtError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ArtError::UnknownChar { ch, line } => {
                write!(f, "unknown art character {ch:?} on line {line}")
            }
            ArtError::BadWidth(w) => write!(f, "art width {w} is not a multiple of 8"),
            ArtError::BadHeight(h) => write!(f, "art height {h} is not a multiple of 8"),
            ArtError::RaggedLine(l) => write!(f, "line {l} has a different length"),
            ArtError::KeyMismatch => write!(f, "key and colors must have the same length"),
        }
    }
}

impl std::error::Error for ArtError {}

/// Parses a block of ASCII art into 8×8 tiles, row-major across the sheet.
///
/// `key` lists the characters used in the art and `colors` the palette index
/// for each, by position. The art must be a multiple of 8 wide and tall; a
/// sheet 16 tiles wide maps 1:1 onto tile memory for multi-tile sprites.
/// Leading and trailing whitespace on each line is ignored, as are blank
/// lines.
pub fn parse_tiles(art: &str, key: &str, colors: &[u8]) -> Result<Vec<u8>, ArtError> {
    let key: Vec<char> = key.chars().collect();
    if key.len() != colors.len() {
        return Err(ArtError::KeyMismatch);
    }
    let lines: Vec<&str> = art
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(ArtError::BadHeight(0));
    }
    let width = lines[0].chars().count();
    if width == 0 || width % 8 != 0 {
        return Err(ArtError::BadWidth(width));
    }
    if lines.len() % 8 != 0 {
        return Err(ArtError::BadHeight(lines.len()));
    }
    let mut grid: Vec<u8> = Vec::with_capacity(width * lines.len());
    for (ln, line) in lines.iter().enumerate() {
        if line.chars().count() != width {
            return Err(ArtError::RaggedLine(ln));
        }
        for ch in line.chars() {
            let idx = key
                .iter()
                .position(|&k| k == ch)
                .ok_or(ArtError::UnknownChar { ch, line: ln })?;
            grid.push(colors[idx]);
        }
    }
    let tiles_w = width / 8;
    let tiles_h = lines.len() / 8;
    let mut out = vec![0u8; tiles_w * tiles_h * TILE_PIXELS];
    for ty in 0..tiles_h {
        for tx in 0..tiles_w {
            let base = (ty * tiles_w + tx) * TILE_PIXELS;
            for py in 0..8 {
                for px in 0..8 {
                    out[base + py * 8 + px] = grid[(ty * 8 + py) * width + tx * 8 + px];
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: &str =
        "11111111\n1......1\n1......1\n1......1\n1......1\n1......1\n1......1\n11111111";

    #[test]
    fn parses_single_tile() {
        let t = parse_tiles(BOX, ".1", &[0, 5]).unwrap();
        assert_eq!(t.len(), 64);
        assert_eq!(t[0], 5);
        assert_eq!(t[9], 0);
        assert_eq!(t[63], 5);
    }

    #[test]
    fn parses_sheet_row_major() {
        let mut art = String::new();
        for _ in 0..8 {
            art.push_str("aaaaaaaabbbbbbbb\n");
        }
        for _ in 0..8 {
            art.push_str("ccccccccdddddddd\n");
        }
        let t = parse_tiles(&art, "abcd", &[1, 2, 3, 4]).unwrap();
        assert_eq!(t.len(), 256);
        assert_eq!(t[0], 1);
        assert_eq!(t[64], 2);
        assert_eq!(t[128], 3);
        assert_eq!(t[192], 4);
    }

    #[test]
    fn errors() {
        assert_eq!(parse_tiles(BOX, ".1", &[0]), Err(ArtError::KeyMismatch));
        assert_eq!(parse_tiles("", "", &[]), Err(ArtError::BadHeight(0)));
        assert_eq!(
            parse_tiles("1234567", "1234567", &[0; 7]),
            Err(ArtError::BadWidth(7))
        );
        assert_eq!(
            parse_tiles("12345678", "12345678", &[0; 8]),
            Err(ArtError::BadHeight(1))
        );
        let ragged = BOX.replacen("1......1", "1.....1", 1);
        assert_eq!(
            parse_tiles(&ragged, ".1", &[0, 1]),
            Err(ArtError::RaggedLine(1))
        );
        let unknown = BOX.replacen("1......1", "1..x...1", 1);
        assert_eq!(
            parse_tiles(&unknown, ".1", &[0, 1]),
            Err(ArtError::UnknownChar { ch: 'x', line: 1 })
        );
        for e in [
            ArtError::KeyMismatch,
            ArtError::BadHeight(0),
            ArtError::BadWidth(7),
            ArtError::RaggedLine(1),
            ArtError::UnknownChar { ch: 'x', line: 1 },
        ] {
            assert_ne!(e.to_string(), "");
        }
    }
}
