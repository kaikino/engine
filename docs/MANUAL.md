# KAGAMI 鏡 — Operator's Manual

*Thank you for choosing the KAGAMI home entertainment console. Please read
this manual carefully before building your first cartridge, and keep it for
future reference.*

---

## 1. Specifications

| Unit | Specification |
|---|---|
| Picture processor | 256 × 224 dots, 60 frames per second |
| Palette | 32 fixed colors (see §4) |
| Tile memory | 512 tiles of 8 × 8 dots, arranged as a sheet 16 tiles wide |
| Background | 2 layers, 32 × 32 tiles each, independent wrap-around scroll, per-tile horizontal flip, vertical flip and priority |
| Objects | 128 per frame, 16 per scanline, 1 × 1 to 4 × 4 tiles, flip |
| Overlay | pixel, line, rectangle and text drawing above all layers |
| Sound processor | 4 voices: pulse (4 duty cycles), pulse, triangle, noise; 44.1 kHz output |
| Controller | 8 buttons: ↑ ↓ ← → A B C START, up to 4 controllers |
| Arithmetic | 32-bit signed fixed point, 16 fractional bits |
| Communication | lockstep link, 1–4 consoles, UDP or in-process |

## 2. The cartridge

A cartridge is a Rust type implementing `kagami::Game`:

```rust
pub trait Game {
    fn new(seed: u32) -> Self;                   // build a fresh game
    fn setup(&self, gpu: &mut Gpu) {}            // load tiles (optional)
    fn update(&mut self, ctx: &mut UpdateCtx);   // simulate one frame
    fn draw(&self, gpu: &mut Gpu);               // present the current state
    fn hash(&self, h: &mut StateHasher);         // feed all state to the hasher
    fn title(&self) -> &'static str { .. }       // window title (optional)
    fn default_players(&self) -> u8 { 1 }        // (optional)
}
```

`UpdateCtx` gives `update` everything it may use:

| Field | Meaning |
|---|---|
| `inputs: [Input; 4]` | every controller this frame; unused slots are `Input::NONE` |
| `frame: u32` | frame number from zero |
| `players: u8` | active players |
| `apu: &mut Apu` | the sound processor, to start effects and songs |

The runtime calls `update` exactly 60 times per second of game time, `draw`
whenever it wants a picture, and `hash` whenever it needs to compare notes
with another console. `setup` runs once, before the first `draw`.

### 2.1 The three rules

1. **`update` depends only on `self` and `ctx`.** No clocks, no random
   sources other than `kagami::Rng`, no floats, no iteration over hash maps,
   no reading files.
2. **`draw` does not change the game.** It takes `&self`; the compiler
   enforces this. Draw whatever you like, as many times as you like.
3. **`hash` covers every field that affects `update`.** The hash is how a
   desync is detected. A field left out is a bug that hides.

Put this at the top of your crate and rule 1's hardest part is enforced by
the compiler:

```rust
#![deny(clippy::float_arithmetic)]
```

### 2.2 The binary

```rust
fn main() -> std::process::ExitCode {
    kagami_runtime::main::<MyGame>()
}
```

That gives your cartridge the complete command line described in §7.

## 3. Picture processor (`kagami::Gpu`)

A frame is composed in this order:

```
backdrop → BG0 → BG1 → objects (16 per line) → priority tiles → overlay → remap → RGB
```

### 3.1 Tiles

Tiles are 64 bytes of palette indices. Author them as text and load them in
`setup`:

```rust
use kagami::gpu::art::parse_tiles;

const BOX: &str = "
11111111
1......1
1..22..1
1..22..1
1..22..1
1..22..1
1......1
11111111";

fn setup(&self, gpu: &mut Gpu) {
    let tiles = parse_tiles(BOX, ".12", &[palette::SUMI, palette::AI, palette::SHU]).unwrap();
    gpu.load_tiles(1, &tiles);
}
```

A block of art wider than 8 columns yields several tiles, row-major. A block
exactly 128 columns wide maps 1:1 onto the tile sheet, which is how
multi-tile objects find their second row: tile *t*'s lower neighbour is
*t + 16*.

### 3.2 Color key

One palette index is transparent in tiles and objects. It is index 0 (墨
sumi) by default; change it with `gpu.set_color_key(c)`. The overlay has no
color key: every overlay color is opaque, including index 0.

### 3.3 Backgrounds

```rust
let bg = &mut gpu.bg[0];
bg.enabled = true;
bg.scroll_x = camera_x;
bg.set(tx, ty, tile_index);                 // plain
bg.set(tx, ty, tile_index | kagami::gpu::FLIP_X);
bg.set(tx, ty, tile_index | kagami::gpu::PRIORITY);  // drawn above objects
bg.fill_rect(0, 20, 32, 8, FLOOR);
```

Maps are 32 × 32 tiles (256 × 256 dots) and wrap in both directions, so a
scrolling world is built by rewriting tiles at the edge as the camera moves.

### 3.4 Objects

```rust
gpu.sprite(Sprite::new(x, y, tile).size(2, 3).flip_x(facing_left));
```

Objects are pushed fresh every `draw`; object memory is cleared before each
frame. The first object pushed is drawn on top. The 17th object on any
scanline is not drawn on that line. `gpu.dropped_sprite_lines()` reports how
often that happened so you can tune, or flicker on purpose the way 1990
games did.

### 3.5 Overlay

```rust
gpu.pixel(x, y, c);
gpu.line(x0, y0, x1, y1, c);
gpu.rect(x, y, w, h, c);        // outline
gpu.rect_fill(x, y, w, h, c);
gpu.text(x, y, "SCORE 1200", palette::GOFUN);
gpu.text_centered(y, "GAME OVER", palette::BENI);
gpu.blit_tile(x, y, tile, flip_x, flip_y);  // a tile on the overlay, no object budget
```

The system font covers printable ASCII in 8 × 8 cells: 32 columns by 28 rows.

### 3.6 Remap

`gpu.pal(from, to)` makes every pixel of one color display as another, at
scan-out, without touching tiles. Use it for hit flashes, night palettes
and fades. `gpu.pal_reset()` restores identity.

## 4. Palette

| # | Name | 漢字 | RGB | # | Name | 漢字 | RGB |
|---|---|---|---|---|---|---|---|
| 0 | sumi | 墨 | `#1c1c1c` | 16 | matcha | 抹茶 | `#c5c56a` |
| 1 | gofun | 胡粉 | `#fffffc` | 17 | wakakusa | 若草 | `#c3d825` |
| 2 | zoge | 象牙 | `#f8f4e6` | 18 | matsuba | 松葉 | `#839b5c` |
| 3 | nezumi | 鼠 | `#949495` | 19 | tokiwa | 常磐 | `#007b43` |
| 4 | namari | 鉛 | `#7b7c7d` | 20 | moegi | 萌黄 | `#aacf53` |
| 5 | ai | 藍 | `#165e83` | 21 | fuji | 藤 | `#bbbcde` |
| 6 | kon | 紺 | `#223a70` | 22 | ayame | 菖蒲 | `#674196` |
| 7 | asagi | 浅葱 | `#00a3af` | 23 | edomurasaki | 江戸紫 | `#745399` |
| 8 | sora | 空 | `#a0d8ef` | 24 | kuri | 栗 | `#762f07` |
| 9 | shu | 朱 | `#eb6101` | 25 | kohaku | 琥珀 | `#bf783a` |
| 10 | beni | 紅 | `#c9171e` | 26 | kaki | 柿 | `#ed6d3d` |
| 11 | akane | 茜 | `#b7282e` | 27 | suna | 砂 | `#dcd3b2` |
| 12 | sakura | 桜 | `#fef4f4` | 28 | hiwada | 檜皮 | `#965042` |
| 13 | momo | 桃 | `#f09199` | 29 | rikyunezumi | 利休鼠 | `#888e7e` |
| 14 | yamabuki | 山吹 | `#f8b500` | 30 | kin | 金 | `#e6b422` |
| 15 | ki | 黄 | `#ffd900` | 31 | ruri | 瑠璃 | `#1e50a2` |

All 32 are constants in `kagami::palette`. The values approximate the
commonly published renderings of each traditional name. The palette is
fixed hardware; cartridges cannot define colors.

## 5. Sound processor (`kagami::Apu`)

Sound is started from `update`, so it belongs to the deterministic timeline
and replays sound identical.

```rust
// Effects: a waveform and a few steps of (note, volume, frames).
ctx.apu.sfx(0, &Sfx::coin());
ctx.apu.sfx(3, &Sfx::new(Waveform::Noise, &[SfxStep::new(40, 15, 3)]));

// Songs: tracker rows, one track per voice.
ctx.apu.play_song(Song {
    frames_per_row: 8,
    tracks: vec![
        Track { wave: Waveform::Pulse25, rows: vec![Row::on(62, 10), Row::hold(), Row::off(), Row::on(67, 10)] },
        Track { wave: Waveform::Triangle, rows: vec![Row::on(50, 12), Row::hold(), Row::hold(), Row::hold()] },
    ],
    looped: true,
});
ctx.apu.stop_song();
```

Notes are MIDI numbers (69 = A4 = 440 Hz). Volumes are 0–15. An effect
takes over its voice until it finishes, then the song resumes there. The
synthesizer uses integer arithmetic only, so even the sample stream is
bit-identical everywhere.

## 6. Arithmetic (`kagami::Fx`)

```rust
use kagami::{Fx, Vec2};
let speed = Fx::from_ratio(5, 4);        // 1.25
let angle = Fx::from_ratio(1, 8);        // 1/8 turn = 45°
let v = Vec2::from_turns(angle) * speed; // unit vector scaled
let len = v.len();                       // integer sqrt, exact for squares
let px = v.x.round();                    // back to screen pixels
```

`Fx` is a 16.16 signed fixed-point number: range ±32767, resolution
1/65536. Trigonometry takes *turns* (0..1 is a full circle) and reads a
compile-time table, so every machine gets the same bits. `Fx` implements
`Display` without floats, for debugging.

## 7. Runtime command line

```
<cartridge> [OPTIONS]

PLAY (default):
  --players N        local players 1..=4 on one keyboard
  --seed S           match seed (default 1)
  --scale N          window scale 1..=8 (default 3)
  --mute             no audio
  --delay D          netplay input delay in frames (default 3, or 1 with --rollback)
  --rollback         predict remote inputs and correct later instead of waiting
  --prediction N     rollback window in frames (default 8)
  --record FILE      save the match as a replay when the window closes
  --replay FILE      play a replay instead of reading the keyboard
  --exit-after N     close the window after N frames

NETPLAY:
  --netplay udp --player I --bind ADDR --peer ADDR[,ADDR...]
  --netplay loopback [--latency F] [--jitter F] [--loss PERMILLE]

HEADLESS:
  --headless --frames N [--screenshot FILE] [--record FILE] [--random-inputs]
  --verify FILE      re-simulate a replay and check every checkpoint
```

Keys: P1 arrows, Z = A, X = B, C = C, Enter = START. P2 W A S D, J = A,
K = B, L = C, Tab = START. F1 toggles the stats overlay, F2 saves a
screenshot, F3 mutes, F4 saves a replay, Esc quits.

## 7.1 Browser

Add to your cartridge's library crate:

```rust
#[cfg(target_arch = "wasm32")]
kagami_web::cartridge!(MyGame);
```

with `crate-type = ["rlib", "cdylib"]`, a `kagami-web` dependency under
`[target.'cfg(target_arch = "wasm32")'.dependencies]`, and the runtime
dependency under `cfg(not(target_arch = "wasm32"))`. Build with
`cargo build --target wasm32-unknown-unknown --release` and point
`web/index.html?cart=<name>` at the `.wasm`. The exports are documented on
`kagami_web::cartridge!`.

## 8. Netplay

Every console runs the same game on the same inputs. An input pressed on
frame *f* takes effect on frame *f + delay* on every console. If a remote
input has not arrived, the console shows WAITING and holds the frame until
it does. Every 30 frames the consoles exchange state hashes; a mismatch
ends the match with a DESYNC message naming the frame. Add `--rollback`
and the console instead predicts late inputs and corrects itself when they
arrive, with an input delay of one frame. See [NETCODE.md](NETCODE.md).

To play over a LAN, each player runs the cartridge with their own
`--player` index, a `--bind` port, and the other players' addresses in
index order under `--peer`. All players must use the same `--seed`,
`--players` and `--delay`.

## 9. Replays

A replay (`.kgmr`) is the seed, the player count and one byte per player
per frame, plus a state hash every 60 frames. `--record` writes one,
`--replay` plays it, `--verify` re-simulates it and checks every hash, which
is how you prove a build has not changed the simulation. `docs/replays/demo.kgmr`
is such a proof and CI runs it.

## 10. Care and handling

The limits in §1 are not configuration. The sprite-per-line limit, the
palette and the resolution are what make a KAGAMI game look and behave like
a KAGAMI game, and they will not be raised in a future revision. Design
within them; the best 1990 games did.
