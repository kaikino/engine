//! Renders the whole system font (ASCII and katakana) to a PNG.
//!
//! ```text
//! cargo run -p kagami --example font_sheet -- font.png
//! ```

use kagami::gpu::{font, kana};
use kagami::{palette, Gpu};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "font.png".to_string());
    let mut gpu = Gpu::new();
    gpu.clear(palette::SUMI);
    gpu.begin_frame();
    let mut y = 4;
    let ascii: String = (font::FIRST_CHAR..font::FIRST_CHAR + font::GLYPH_COUNT as u8)
        .map(char::from)
        .collect();
    for line in ascii.as_bytes().chunks(32) {
        gpu.text(0, y, std::str::from_utf8(line).unwrap(), palette::GOFUN);
        y += 10;
    }
    y += 6;
    let rows = [
        "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホ",
        "マミムメモヤユヨラリルレロワヲンーァィゥェォッャュョ・〜",
        "ガギグゲゴザジズゼゾダヂヅデドバビブベボパピプペポヴ",
        "「カガミ」、ロックステップ。ヰヱヮヵヶ",
    ];
    for row in rows {
        gpu.text(0, y, row, palette::SAKURA);
        y += 10;
    }
    let covered = (0x30A1..=0x30FC)
        .filter(|c| char::from_u32(*c).is_some_and(kana::covers))
        .count();
    gpu.text(
        0,
        y + 4,
        &format!("{covered} KATAKANA CODE POINTS"),
        palette::SORA,
    );
    gpu.render();
    std::fs::write(&path, gpu.screenshot_png()).expect("write png");
    println!("wrote {path}");
}
