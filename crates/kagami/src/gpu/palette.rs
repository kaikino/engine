//! The fixed 32-color KAGAMI palette.
//!
//! Every color is a traditional Japanese pigment (日本の伝統色). The values are
//! close approximations of the commonly published sRGB renderings of each
//! name. The palette is fixed hardware: games pick indices, they do not define
//! colors. Index 0 is the ink black that doubles as the default color key.

/// Number of colors in the palette.
pub const PALETTE_SIZE: usize = 32;

/// 墨 — ink black. Index 0; the default color key and backdrop.
pub const SUMI: u8 = 0;
/// 胡粉 — shell white.
pub const GOFUN: u8 = 1;
/// 象牙 — ivory.
pub const ZOGE: u8 = 2;
/// 鼠 — mouse gray.
pub const NEZUMI: u8 = 3;
/// 鉛 — lead gray.
pub const NAMARI: u8 = 4;
/// 藍 — indigo.
pub const AI: u8 = 5;
/// 紺 — navy.
pub const KON: u8 = 6;
/// 浅葱 — pale teal.
pub const ASAGI: u8 = 7;
/// 空 — sky blue.
pub const SORA: u8 = 8;
/// 朱 — vermilion.
pub const SHU: u8 = 9;
/// 紅 — crimson.
pub const BENI: u8 = 10;
/// 茜 — madder red.
pub const AKANE: u8 = 11;
/// 桜 — cherry blossom.
pub const SAKURA: u8 = 12;
/// 桃 — peach pink.
pub const MOMO: u8 = 13;
/// 山吹 — golden yellow.
pub const YAMABUKI: u8 = 14;
/// 黄 — yellow.
pub const KI: u8 = 15;
/// 抹茶 — matcha green.
pub const MATCHA: u8 = 16;
/// 若草 — young grass.
pub const WAKAKUSA: u8 = 17;
/// 松葉 — pine needle.
pub const MATSUBA: u8 = 18;
/// 常磐 — evergreen.
pub const TOKIWA: u8 = 19;
/// 萌黄 — sprout green.
pub const MOEGI: u8 = 20;
/// 藤 — wisteria.
pub const FUJI: u8 = 21;
/// 菖蒲 — iris purple.
pub const AYAME: u8 = 22;
/// 江戸紫 — Edo purple.
pub const EDOMURASAKI: u8 = 23;
/// 栗 — chestnut.
pub const KURI: u8 = 24;
/// 琥珀 — amber.
pub const KOHAKU: u8 = 25;
/// 柿 — persimmon.
pub const KAKI: u8 = 26;
/// 砂 — sand.
pub const SUNA: u8 = 27;
/// 檜皮 — cypress bark.
pub const HIWADA: u8 = 28;
/// 利休鼠 — Rikyū gray-green.
pub const RIKYUNEZUMI: u8 = 29;
/// 金 — gold.
pub const KIN: u8 = 30;
/// 瑠璃 — lapis blue.
pub const RURI: u8 = 31;

/// sRGB values, indexed by palette index.
pub const RGB: [[u8; 3]; PALETTE_SIZE] = [
    [0x1c, 0x1c, 0x1c], // 0 sumi
    [0xff, 0xff, 0xfc], // 1 gofun
    [0xf8, 0xf4, 0xe6], // 2 zoge
    [0x94, 0x94, 0x95], // 3 nezumi
    [0x7b, 0x7c, 0x7d], // 4 namari
    [0x16, 0x5e, 0x83], // 5 ai
    [0x22, 0x3a, 0x70], // 6 kon
    [0x00, 0xa3, 0xaf], // 7 asagi
    [0xa0, 0xd8, 0xef], // 8 sora
    [0xeb, 0x61, 0x01], // 9 shu
    [0xc9, 0x17, 0x1e], // 10 beni
    [0xb7, 0x28, 0x2e], // 11 akane
    [0xfe, 0xf4, 0xf4], // 12 sakura
    [0xf0, 0x91, 0x99], // 13 momo
    [0xf8, 0xb5, 0x00], // 14 yamabuki
    [0xff, 0xd9, 0x00], // 15 ki
    [0xc5, 0xc5, 0x6a], // 16 matcha
    [0xc3, 0xd8, 0x25], // 17 wakakusa
    [0x83, 0x9b, 0x5c], // 18 matsuba
    [0x00, 0x7b, 0x43], // 19 tokiwa
    [0xaa, 0xcf, 0x53], // 20 moegi
    [0xbb, 0xbc, 0xde], // 21 fuji
    [0x67, 0x41, 0x96], // 22 ayame
    [0x74, 0x53, 0x99], // 23 edomurasaki
    [0x76, 0x2f, 0x07], // 24 kuri
    [0xbf, 0x78, 0x3a], // 25 kohaku
    [0xed, 0x6d, 0x3d], // 26 kaki
    [0xdc, 0xd3, 0xb2], // 27 suna
    [0x96, 0x50, 0x42], // 28 hiwada
    [0x88, 0x8e, 0x7e], // 29 rikyunezumi
    [0xe6, 0xb4, 0x22], // 30 kin
    [0x1e, 0x50, 0xa2], // 31 ruri
];

/// Romanized names, indexed by palette index.
pub const NAMES: [&str; PALETTE_SIZE] = [
    "sumi",
    "gofun",
    "zoge",
    "nezumi",
    "namari",
    "ai",
    "kon",
    "asagi",
    "sora",
    "shu",
    "beni",
    "akane",
    "sakura",
    "momo",
    "yamabuki",
    "ki",
    "matcha",
    "wakakusa",
    "matsuba",
    "tokiwa",
    "moegi",
    "fuji",
    "ayame",
    "edomurasaki",
    "kuri",
    "kohaku",
    "kaki",
    "suna",
    "hiwada",
    "rikyunezumi",
    "kin",
    "ruri",
];

/// Kanji names, indexed by palette index.
pub const KANJI: [&str; PALETTE_SIZE] = [
    "墨",
    "胡粉",
    "象牙",
    "鼠",
    "鉛",
    "藍",
    "紺",
    "浅葱",
    "空",
    "朱",
    "紅",
    "茜",
    "桜",
    "桃",
    "山吹",
    "黄",
    "抹茶",
    "若草",
    "松葉",
    "常磐",
    "萌黄",
    "藤",
    "菖蒲",
    "江戸紫",
    "栗",
    "琥珀",
    "柿",
    "砂",
    "檜皮",
    "利休鼠",
    "金",
    "瑠璃",
];

/// Looks up the sRGB triple for a palette index. Out-of-range indices wrap.
#[inline]
pub const fn rgb(index: u8) -> [u8; 3] {
    RGB[(index as usize) % PALETTE_SIZE]
}

/// Finds a palette index by romanized name.
pub fn by_name(name: &str) -> Option<u8> {
    NAMES
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_consistent() {
        assert_eq!(RGB.len(), PALETTE_SIZE);
        assert_eq!(NAMES.len(), PALETTE_SIZE);
        assert_eq!(KANJI.len(), PALETTE_SIZE);
        assert_eq!(by_name("AI"), Some(AI));
        assert_eq!(by_name("ruri"), Some(RURI));
        assert_eq!(by_name("nope"), None);
        assert_eq!(rgb(SUMI), [0x1c, 0x1c, 0x1c]);
        assert_eq!(rgb(32), rgb(0));
        // All colors are distinct.
        for i in 0..PALETTE_SIZE {
            for j in (i + 1)..PALETTE_SIZE {
                assert_ne!(RGB[i], RGB[j], "{} and {}", NAMES[i], NAMES[j]);
            }
        }
    }
}
