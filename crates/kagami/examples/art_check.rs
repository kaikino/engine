//! Parses a Rust art snippet (as produced by `web/tools/tiles.html`) and
//! reports how many tiles it holds. Exit status 1 if it does not parse.
//!
//! ```text
//! cargo run -p kagami --example art_check -- art.rs
//! ```

use kagami::gpu::art::parse_tiles;
use kagami::gpu::TILE_PIXELS;
use kagami::palette;

fn main() {
    let path = std::env::args().nth(1).expect("usage: art_check <file.rs>");
    let text = std::fs::read_to_string(&path).expect("read file");
    let key = capture(&text, "KEY: &str = \"", "\"").expect("KEY constant");
    let colors_src = capture(&text, "COLORS: [u8; ", "];").expect("COLORS constant");
    let colors: Vec<u8> = colors_src
        .split_once('[')
        .map_or("", |(_, rest)| rest)
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|name| {
            let name = name.trim_start_matches("pal::");
            palette::by_name(name).unwrap_or_else(|| panic!("unknown color {name}"))
        })
        .collect();
    let art = capture(&text, "ART: &str = \"", "\"").expect("ART constant");
    match parse_tiles(art, key, &colors) {
        Ok(tiles) => {
            let n = tiles.len() / TILE_PIXELS;
            let opaque = tiles.iter().filter(|&&c| c != palette::SUMI).count();
            println!("{path}: {n} tiles, {opaque} opaque pixels, key {key:?}");
        }
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    }
}

fn capture<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = text.find(start)? + start.len();
    let j = text[i..].find(end)? + i;
    Some(&text[i..j])
}
