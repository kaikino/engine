//! All IPPON graphics, as text. Tiles are 8×8; fighter poses are 16×24 or
//! 24×24 and are placed on the 16-tile-wide sheet so multi-tile sprites work.
//!
//! Art key: `.` transparent, `k` armor (kon), `a` hakama (ai), `w` white,
//! `n` grille gray, `b` shinai bamboo, `g` dark gray, `f` floor, `d` dark
//! floor seam, `p` pillar, `s` shoji paper, `l` shoji lattice, `r` red,
//! `y` gold, `m` petal pink.

use kagami::gpu::art::parse_tiles;
use kagami::gpu::{Gpu, SHEET_W, TILE_PIXELS};
use kagami::palette as pal;

const KEY: &str = ".kawnbgfdpslrym";

fn colors(hakama: u8, armor: u8) -> [u8; 15] {
    [
        pal::SUMI,
        armor,
        hakama,
        pal::GOFUN,
        pal::NEZUMI,
        pal::KOHAKU,
        pal::NAMARI,
        pal::SUNA,
        pal::KURI,
        pal::HIWADA,
        pal::ZOGE,
        pal::KURI,
        pal::BENI,
        pal::KIN,
        pal::MOMO,
    ]
}

// ----- background tiles (row 0 of the sheet) --------------------------------

/// Tile indices for background and HUD tiles.
pub mod tile {
    /// Blank.
    pub const EMPTY: u16 = 0;
    /// Floor plank.
    pub const FLOOR: u16 = 1;
    /// Floor plank with a seam.
    pub const FLOOR_SEAM: u16 = 2;
    /// Plain wall.
    pub const WALL: u16 = 3;
    /// Shoji screen panel.
    pub const SHOJI: u16 = 4;
    /// Wooden pillar.
    pub const PILLAR: u16 = 5;
    /// Hanging scroll, top.
    pub const SCROLL_TOP: u16 = 6;
    /// Hanging scroll, middle.
    pub const SCROLL_MID: u16 = 7;
    /// Hanging scroll, bottom.
    pub const SCROLL_BOTTOM: u16 = 8;
    /// Falling petal.
    pub const PETAL: u16 = 9;
    /// White judge's flag.
    pub const FLAG_WHITE: u16 = 10;
    /// Red judge's flag.
    pub const FLAG_RED: u16 = 11;
    /// Wall skirting.
    pub const SKIRT: u16 = 12;
}

const BG_TILES: [(u16, &str); 13] = [
    (
        tile::EMPTY,
        "........|........|........|........|........|........|........|........",
    ),
    (
        tile::FLOOR,
        "ffffffff|ffffffff|ffffffff|ffffffdf|ffffffff|ffffffff|ffffffff|dddddddd",
    ),
    (
        tile::FLOOR_SEAM,
        "ffffffff|ffffffff|ffffffff|ffffffff|ffffffff|ffffffff|ffffffff|ffffffff",
    ),
    (
        tile::WALL,
        "gggggggg|gggggggg|gggggggg|gggggggg|gggggggg|gggggggg|gggggggg|gggggggg",
    ),
    (
        tile::SHOJI,
        "lslslsls|lslslsls|ssssssss|ssssssss|lslslsls|lslslsls|ssssssss|ssssssss",
    ),
    (
        tile::PILLAR,
        "ppkppkpp|ppkppkpp|ppkppkpp|ppkppkpp|ppkppkpp|ppkppkpp|ppkppkpp|ppkppkpp",
    ),
    (
        tile::SCROLL_TOP,
        "gggggggg|ggrrrrrg|grssssrg|grssssrg|grsskssg|grssssrg|grssssrg|grssssrg",
    ),
    (
        tile::SCROLL_MID,
        "grssssrg|grskkssg|grssssrg|grsskssg|grssssrg|grskkksg|grssssrg|grssssrg",
    ),
    (
        tile::SCROLL_BOTTOM,
        "grssssrg|grssssrg|grsskssg|grssssrg|grrrrrrg|gggggggg|gggggggg|gggggggg",
    ),
    (
        tile::PETAL,
        "........|........|...m.m..|...mmm..|....m...|........|........|........",
    ),
    (
        tile::FLAG_WHITE,
        "wwwwww..|wwwwww..|wwwwww..|wwwwww..|wwwwww..|b.......|b.......|b.......",
    ),
    (
        tile::FLAG_RED,
        "rrrrrr..|rrrrrr..|rrrrrr..|rrrrrr..|rrrrrr..|b.......|b.......|b.......",
    ),
    (
        tile::SKIRT,
        "kkkkkkkk|kkkkkkkk|gkkkkkkk|ggkkkkkk|gggggggg|ffffffff|ffffffff|ffffffff",
    ),
];

// ----- fighter poses ----------------------------------------------------------

const IDLE: &str = "
.....kkkkkk.....
....kkkkkkkk....
....kwnnnnwk....
....kwnnnnwk....
....kknnnnkk....
.....kkkkkk.....
....kkkkkkkk....
...kkkkkkkkkk...
...kkkkkkkkk.b..
..kkkkkkkkkkbb..
..k.kkkkkkkbb...
..k.kkkkkkbbk...
..k.kkkkkbbkk...
....kkkkkkkk....
....aaaaaaaa....
....aaaaaaaa....
...aaaaaaaaaa...
...aaaaaaaaaa...
...aaaaaaaaaa...
..aaaaaaaaaaaa..
..aaaaaaaaaaaa..
..aaaaa..aaaaa..
..aaaaa..aaaaa..
..wwwww..wwwww..
";

const WALK1: &str = "
.....kkkkkk.....
....kkkkkkkk....
....kwnnnnwk....
....kwnnnnwk....
....kknnnnkk....
.....kkkkkk.....
....kkkkkkkk....
...kkkkkkkkkk...
...kkkkkkkkk.b..
..kkkkkkkkkkbb..
..k.kkkkkkkbb...
..k.kkkkkkbbk...
..k.kkkkkbbkk...
....kkkkkkkk....
....aaaaaaaa....
....aaaaaaaa....
...aaaaaaaaaa...
...aaaaaaaaaa...
..aaaaaaaaaaaa..
..aaaaaa.aaaaaa.
.aaaaaa...aaaaa.
.aaaaa.....aaaa.
.aaaaa.....aaaa.
.wwwww.....wwww.
";

const WALK2: &str = "
.....kkkkkk.....
....kkkkkkkk....
....kwnnnnwk....
....kwnnnnwk....
....kknnnnkk....
.....kkkkkk.....
....kkkkkkkk....
...kkkkkkkkkk...
...kkkkkkkkk.b..
..kkkkkkkkkkbb..
..k.kkkkkkkbb...
..k.kkkkkkbbk...
..k.kkkkkbbkk...
....kkkkkkkk....
....aaaaaaaa....
....aaaaaaaa....
...aaaaaaaaaa...
...aaaaaaaaaa...
...aaaaaaaaaa...
...aaaaaaaaaa...
....aaaaaaaa....
....aaaaaaaa....
....aaaaaaaa....
....wwwwwwww....
";

const WINDUP: &str = "
..........b.....
.........bb.....
.....kkkkbk.....
....kkkkbkkk....
....kwnbnnwk....
....kwnbnnwk....
....kkbnnnkk....
.....kbkkkk.....
....kkbkkkkk....
...kkkbkkkkkk...
...kkkbkkkkkk...
..kkkkbbkkkkkk..
..k.kkkbkkkk.k..
..k.kkkkkkkk.k..
....aaaaaaaa....
....aaaaaaaa....
...aaaaaaaaaa...
...aaaaaaaaaa...
...aaaaaaaaaa...
..aaaaaaaaaaaa..
..aaaaaaaaaaaa..
..aaaaa..aaaaa..
..aaaaa..aaaaa..
..wwwww..wwwww..
";

const STRIKE: &str = "
........................
.....kkkkkk.............
....kkkkkkkk............
....kwnnnnwk............
....kwnnnnwk............
....kknnnnkk............
.....kkkkkk.............
....kkkkkkkkkk..........
...kkkkkkkkkkkkbbbbbbbbb
...kkkkkkkkkkkkkbbbbbbbb
..kkkkkkkkkkkk..........
..k.kkkkkkkk............
..k.kkkkkkkk............
....kkkkkkkk............
....aaaaaaaaa...........
...aaaaaaaaaaa..........
...aaaaaaaaaaaa.........
..aaaaaaaaaaaaaa........
..aaaaaa..aaaaaa........
.aaaaaa....aaaaaa.......
.aaaaa......aaaaa.......
.aaaaa.......aaaa.......
.aaaaa.......aaaa.......
.wwwww.......wwww.......
";

const BLOCK: &str = "
.............b..
.....kkkkkk..b..
....kkkkkkkk.b..
....kwnnnnwk.b..
....kwnnnnwk.b..
....kknnnnkk.b..
.....kkkkkk..b..
....kkkkkkkk.b..
...kkkkkkkkkkb..
...kkkkkkkkkkb..
..kkkkkkkkkkkb..
..k.kkkkkkkkkb..
..k.kkkkkkkkkb..
....kkkkkkkk.b..
....aaaaaaaa....
....aaaaaaaa....
...aaaaaaaaaa...
...aaaaaaaaaa...
...aaaaaaaaaa...
..aaaaaaaaaaaa..
..aaaaaaaaaaaa..
..aaaaa..aaaaa..
..aaaaa..aaaaa..
..wwwww..wwwww..
";

const HIT: &str = "
................
..kkkkkk........
.kkkkkkkk.......
.kwnnnnwk.......
.kwnnnnwk.......
.kknnnnkk.......
..kkkkkk........
..kkkkkkkk......
.kkkkkkkkkk.....
.kkkkkkkkkkk....
.kkkkkkkkkkkk...
.k.kkkkkkkkkk...
.k.kkkkkkkkk.b..
...kkkkkkkk..b..
...aaaaaaaa.b...
...aaaaaaaa.b...
..aaaaaaaaaab...
..aaaaaaaaaab...
..aaaaaaaaaa....
.aaaaaaaaaaaa...
.aaaaaaaaaaaa...
.aaaaa..aaaaa...
.aaaaa..aaaaa...
.wwwww..wwwww...
";

/// Where each pose lives in tile memory (top-left tile index).
pub mod pose {
    /// Idle, 2×3 tiles.
    pub const IDLE: u16 = 16;
    /// Walk frame 1, 2×3.
    pub const WALK1: u16 = 18;
    /// Walk frame 2, 2×3.
    pub const WALK2: u16 = 20;
    /// Wind-up, 2×3.
    pub const WINDUP: u16 = 22;
    /// Strike, 3×3.
    pub const STRIKE: u16 = 24;
    /// Block, 2×3.
    pub const BLOCK: u16 = 27;
    /// Hit recoil, 2×3.
    pub const HIT: u16 = 29;
    /// Added to a pose index for player 2's recolored set.
    pub const P2_OFFSET: u16 = 48;
}

fn place(gpu: &mut Gpu, base: u16, art: &str, colors: &[u8]) {
    let tiles = parse_tiles(art, KEY, colors).expect("ippon art is valid");
    let width_tiles = art
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map_or(2, |l| l.len() / 8);
    for (i, chunk) in tiles.chunks(TILE_PIXELS).enumerate() {
        let col = i % width_tiles;
        let row = i / width_tiles;
        gpu.load_tiles(base as usize + col + row * SHEET_W, chunk);
    }
}

/// Loads every tile the game uses.
pub fn load(gpu: &mut Gpu) {
    let p1 = colors(pal::AI, pal::KON);
    let p2 = colors(pal::AKANE, pal::HIWADA);
    for (index, art) in BG_TILES {
        let lines = art.replace('|', "\n");
        place(gpu, index, &lines, &p1);
    }
    for (offset, c) in [(0u16, &p1), (pose::P2_OFFSET, &p2)] {
        place(gpu, pose::IDLE + offset, IDLE, c);
        place(gpu, pose::WALK1 + offset, WALK1, c);
        place(gpu, pose::WALK2 + offset, WALK2, c);
        place(gpu, pose::WINDUP + offset, WINDUP, c);
        place(gpu, pose::STRIKE + offset, STRIKE, c);
        place(gpu, pose::BLOCK + offset, BLOCK, c);
        place(gpu, pose::HIT + offset, HIT, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_art_parses() {
        let c = colors(pal::AI, pal::KON);
        for (_, art) in BG_TILES {
            let lines = art.replace('|', "\n");
            assert_eq!(
                parse_tiles(&lines, KEY, &c).unwrap().len(),
                TILE_PIXELS,
                "{art}"
            );
        }
        for a in [IDLE, WALK1, WALK2, WINDUP, BLOCK, HIT] {
            assert_eq!(
                parse_tiles(a, KEY, &c).unwrap().len(),
                6 * TILE_PIXELS,
                "{a}"
            );
        }
        assert_eq!(parse_tiles(STRIKE, KEY, &c).unwrap().len(), 9 * TILE_PIXELS);
    }

    #[test]
    fn load_places_poses_on_sheet() {
        let mut gpu = Gpu::new();
        load(&mut gpu);
        // Idle helmet: row 0 of the pose is in tile 16, pixel (5,0) is armor.
        assert_eq!(gpu.tile_pixel(pose::IDLE, 5, 0), pal::KON);
        // Second row of tiles sits one sheet row down.
        assert_eq!(gpu.tile_pixel(pose::IDLE + SHEET_W as u16, 4, 6), pal::AI);
        // P2 is recolored.
        assert_eq!(
            gpu.tile_pixel(pose::IDLE + pose::P2_OFFSET, 5, 0),
            pal::HIWADA
        );
        // Strike's third column carries the shinai.
        assert_eq!(
            gpu.tile_pixel(pose::STRIKE + 2 + SHEET_W as u16, 7, 0),
            pal::KOHAKU
        );
        // Background tiles.
        assert_eq!(gpu.tile_pixel(tile::FLOOR, 0, 0), pal::SUNA);
        assert_eq!(gpu.tile_pixel(tile::FLAG_RED, 1, 2), pal::BENI);
    }
}
