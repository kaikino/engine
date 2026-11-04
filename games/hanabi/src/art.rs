//! HANABI graphics as text. Key: `.` transparent, `k` wood (kuri), `w` white,
//! `y` gold, `r` vermilion, `b` lapis, `s` water indigo, `n` night navy,
//! `o` orange (kaki), `p` pink, `g` gray, `t` teal (asagi), `m` momo,
//! `d` dark ridge (tokiwa).

use kagami::gpu::art::parse_tiles;
use kagami::gpu::{Gpu, SHEET_W, TILE_PIXELS};
use kagami::palette as pal;

const KEY: &str = ".kwyrbsnopgtmd";
const COLORS: [u8; 14] = [
    pal::SUMI,
    pal::KURI,
    pal::GOFUN,
    pal::KIN,
    pal::SHU,
    pal::RURI,
    pal::AI,
    pal::KON,
    pal::KAKI,
    pal::SAKURA,
    pal::NEZUMI,
    pal::ASAGI,
    pal::MOMO,
    pal::TOKIWA,
];

/// Background and effect tile indices.
pub mod tile {
    /// Blank.
    pub const EMPTY: u16 = 0;
    /// Night sky.
    pub const SKY: u16 = 1;
    /// Night sky with a star.
    pub const SKY_STAR: u16 = 2;
    /// Night sky with a dim star.
    pub const SKY_DIM: u16 = 3;
    /// Mountain silhouette, slope up to the right.
    pub const MOUNTAIN_L: u16 = 4;
    /// Mountain body.
    pub const MOUNTAIN: u16 = 5;
    /// Mountain silhouette, slope down to the right.
    pub const MOUNTAIN_R: u16 = 6;
    /// River water.
    pub const RIVER: u16 = 7;
    /// River with a highlight.
    pub const RIVER_GLINT: u16 = 8;
    /// Riverbank stones.
    pub const BANK: u16 = 9;
    /// Festival lantern string.
    pub const LANTERN_STRING: u16 = 10;
    /// Bullet (player firework shell).
    pub const SHOT: u16 = 11;
    /// Enemy shot.
    pub const ENEMY_SHOT: u16 = 12;
    /// Spark particle.
    pub const SPARK: u16 = 13;
    /// Life icon.
    pub const LIFE: u16 = 14;
    /// Bomb icon.
    pub const BOMB: u16 = 15;
}

/// Sprite base tiles (multi-tile objects on the sheet).
pub mod sprite {
    /// Player boat, 2×2, player 1 colors.
    pub const BOAT: u16 = 16;
    /// Player boat, 2×2, player 2 colors.
    pub const BOAT2: u16 = 18;
    /// Lantern enemy frame 1, 1×1.
    pub const LANTERN_A: u16 = 20;
    /// Lantern enemy frame 2, 1×1.
    pub const LANTERN_B: u16 = 21;
    /// Oni mask enemy, 2×2.
    pub const ONI: u16 = 22;
    /// Firework burst frame 1, 2×2.
    pub const BURST_A: u16 = 24;
    /// Firework burst frame 2, 2×2.
    pub const BURST_B: u16 = 26;
    /// Big bomb bloom, 4×4.
    pub const BLOOM: u16 = 28;
}

const BG_TILES: [(u16, &str); 16] = [
    (
        tile::EMPTY,
        "........|........|........|........|........|........|........|........",
    ),
    (
        tile::SKY,
        "nnnnnnnn|nnnnnnnn|nnnnnnnn|nnnnnnnn|nnnnnnnn|nnnnnnnn|nnnnnnnn|nnnnnnnn",
    ),
    (
        tile::SKY_STAR,
        "nnnnnnnn|nnnnnwnn|nnnnwwwn|nnnnnwnn|nnnnnnnn|nwnnnnnn|nnnnnnnn|nnnnnnnn",
    ),
    (
        tile::SKY_DIM,
        "nnnnnnnn|nnnnnnnn|nngnnnnn|nnnnnnnn|nnnnnngn|nnnnnnnn|nnnnnnnn|nnnnnnnn",
    ),
    (
        tile::MOUNTAIN_L,
        "........|.......d|......dd|.....ddd|....dddd|...ddddd|..dddddd|.ddddddd",
    ),
    (
        tile::MOUNTAIN,
        "dddddddd|dddddddd|dddddddd|dddddddd|dddddddd|dddddddd|dddddddd|dddddddd",
    ),
    (
        tile::MOUNTAIN_R,
        "........|d.......|dd......|ddd.....|dddd....|ddddd...|dddddd..|ddddddd.",
    ),
    (
        tile::RIVER,
        "ssssssss|ssssssss|ssssssss|ssssssss|ssssssss|ssssssss|ssssssss|ssssssss",
    ),
    (
        tile::RIVER_GLINT,
        "ssssssss|ssttssss|ssssssss|ssssssss|sssssstt|ssssssss|tsssssss|ssssssss",
    ),
    (
        tile::BANK,
        "dddddddd|dgdddgdd|dddddddd|dddgdddd|ddddddgd|dgdddddd|ddddgddd|dddddddd",
    ),
    (
        tile::LANTERN_STRING,
        "........|kkkkkkkk|.r...r..|roroorok|roroorok|.o...o..|........|........",
    ),
    (
        tile::SHOT,
        "........|...y....|..yyy...|..yyy...|...y....|...w....|...w....|........",
    ),
    (
        tile::ENEMY_SHOT,
        "........|........|...r....|..rrr...|..rrr...|...r....|........|........",
    ),
    (
        tile::SPARK,
        "........|........|........|...y....|..y.y...|...y....|........|........",
    ),
    (
        tile::LIFE,
        "........|..www...|.wwwww..|.wwwww..|..www...|...w....|........|........",
    ),
    (
        tile::BOMB,
        "...y....|..yy....|.rrrr...|rrrrrr..|rrrrrr..|rrrrrr..|.rrrr...|........",
    ),
];

const BOAT: &str = "
.......ww.......
.......ww.......
......wwww......
.....wyyyyw.....
.....wyyyyw.....
......wwww......
.......kk.......
kkkkkkkkkkkkkkkk
.kkkkkkkkkkkkkk.
.ooooooooooooo..
..oooooooooooo..
..ssssssssssss..
...ssssssssss...
................
................
................
";

const LANTERN_A: &str = "
...k....
..rrr...
.rrrrr..
.ryyyr..
.ryyyr..
.rrrrr..
..rrr...
...k....
";

const LANTERN_B: &str = "
...k....
..rrr...
.ryyyr..
.ryyyr..
.rrrrr..
.rrrrr..
..rrr...
...k....
";

const ONI: &str = "
..r..........r..
..rr........rr..
..rrrrrrrrrrrr..
.rrrrrrrrrrrrrr.
.rrwwrrrrrrwwrr.
.rrkwrrrrrrwkrr.
.rrrrrrrrrrrrrr.
.rrrrkrrrrkrrrr.
.rrrrrkrrrkrrrr.
..rrrrwwwwrrrr..
..rrrwkwwkwrrr..
...rrrwwwwrrr...
....rrrrrrrr....
.....rrrrrr.....
.......rr.......
................
";

const BURST_A: &str = "
................
.......y........
......y.y.......
.....y...y......
....y..w..y.....
...y..www..y....
......w.w.......
.....w...w......
......w.w.......
...y..www..y....
....y..w..y.....
.....y...y......
......y.y.......
.......y........
................
................
";

const BURST_B: &str = "
.p.....p.....p..
..p...........p.
...p.........p..
....p.......p...
.....p..p..p....
......p.p.p.....
.......ppp......
..pppppp.pppppp.
.......ppp......
......p.p.p.....
.....p..p..p....
....p.......p...
...p.........p..
..p...........p.
.p.....p.....p..
................
";

const BLOOM: &str = "
...............y................
..............yyy...............
.............y.y.y..............
....y.......y..y..y.......y.....
.....y.....y...y...y.....y......
......y...y....y....y...y.......
.......y.y.....y.....y.y........
........y......y......y.........
.........y.....w.....y..........
..........y...www...y...........
...........y.wwwww.y............
............ywwwwwy.............
.............wwwww..............
..............www...............
yyyyyyyyyyyyyywwwyyyyyyyyyyyyyyy
..............www...............
.............wwwww..............
............ywwwwwy.............
...........y.wwwww.y............
..........y...www...y...........
.........y.....w.....y..........
........y......y......y.........
.......y.y.....y.....y.y........
......y...y....y....y...y.......
.....y.....y...y...y.....y......
....y.......y..y..y.......y.....
.............y.y.y..............
..............yyy...............
...............y................
................................
................................
................................
";

fn place(gpu: &mut Gpu, base: u16, art: &str, colors: &[u8]) {
    let tiles = parse_tiles(art, KEY, colors).expect("hanabi art is valid");
    let width_tiles = art
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map_or(1, |l| l.chars().count() / 8);
    for (i, chunk) in tiles.chunks(TILE_PIXELS).enumerate() {
        let col = i % width_tiles;
        let row = i / width_tiles;
        gpu.load_tiles(base as usize + col + row * SHEET_W, chunk);
    }
}

/// Loads every tile the game uses.
pub fn load(gpu: &mut Gpu) {
    for (index, art) in BG_TILES {
        place(gpu, index, &art.replace('|', "\n"), &COLORS);
    }
    place(gpu, sprite::BOAT, BOAT, &COLORS);
    // Player 2's boat: pink sail instead of gold, teal hull.
    let mut p2 = COLORS;
    p2[3] = pal::MOMO;
    p2[8] = pal::ASAGI;
    place(gpu, sprite::BOAT2, BOAT, &p2);
    place(gpu, sprite::LANTERN_A, LANTERN_A, &COLORS);
    place(gpu, sprite::LANTERN_B, LANTERN_B, &COLORS);
    place(gpu, sprite::ONI, ONI, &COLORS);
    place(gpu, sprite::BURST_A, BURST_A, &COLORS);
    place(gpu, sprite::BURST_B, BURST_B, &COLORS);
    place(gpu, sprite::BLOOM, BLOOM, &COLORS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_art_parses() {
        for (_, art) in BG_TILES {
            assert_eq!(
                parse_tiles(&art.replace('|', "\n"), KEY, &COLORS)
                    .unwrap()
                    .len(),
                TILE_PIXELS,
                "{art}"
            );
        }
        assert_eq!(
            parse_tiles(BOAT, KEY, &COLORS).unwrap().len(),
            4 * TILE_PIXELS
        );
        assert_eq!(
            parse_tiles(LANTERN_A, KEY, &COLORS).unwrap().len(),
            TILE_PIXELS
        );
        assert_eq!(
            parse_tiles(LANTERN_B, KEY, &COLORS).unwrap().len(),
            TILE_PIXELS
        );
        assert_eq!(
            parse_tiles(ONI, KEY, &COLORS).unwrap().len(),
            4 * TILE_PIXELS
        );
        assert_eq!(
            parse_tiles(BURST_A, KEY, &COLORS).unwrap().len(),
            4 * TILE_PIXELS
        );
        assert_eq!(
            parse_tiles(BURST_B, KEY, &COLORS).unwrap().len(),
            4 * TILE_PIXELS
        );
        assert_eq!(
            parse_tiles(BLOOM, KEY, &COLORS).unwrap().len(),
            16 * TILE_PIXELS
        );
    }

    #[test]
    fn load_places_sprites() {
        let mut gpu = Gpu::new();
        load(&mut gpu);
        assert_eq!(gpu.tile_pixel(sprite::BOAT, 7, 0), pal::GOFUN);
        assert_eq!(
            gpu.tile_pixel(sprite::BOAT + SHEET_W as u16, 1, 0),
            pal::KURI
        );
        assert_eq!(gpu.tile_pixel(sprite::BOAT2, 6, 3), pal::MOMO);
        assert_eq!(
            gpu.tile_pixel(sprite::BLOOM + 1 + SHEET_W as u16, 7, 7),
            pal::GOFUN
        );
        assert_eq!(gpu.tile_pixel(tile::RIVER, 0, 0), pal::AI);
        assert_eq!(gpu.tile_pixel(tile::SKY_STAR, 5, 1), pal::GOFUN);
    }
}
