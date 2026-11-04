//! # KAGAMI (鏡)
//!
//! A deterministic, lockstep-multiplayer fantasy console with a retro Japanese
//! aesthetic. The name means *mirror*: every player's console runs an identical
//! simulation, so each screen is a mirror of the others.
//!
//! This crate is the console itself. It has **zero dependencies** and contains:
//!
//! - [`fx`]: 16.16 fixed-point math. The simulation never touches floats; the
//!   crate denies float arithmetic at compile time outside the audio synth.
//! - [`rng`]: a seeded xoshiro128\*\* generator.
//! - [`input`]: the eight-button controller.
//! - [`hash`]: state hashing used for desync detection and replays.
//! - [`gpu`]: the picture processor. 256×224, 32-color palette of traditional
//!   Japanese pigments, 8×8 tiles, two scrolling background layers, 128 sprites
//!   with a 16-per-scanline limit, and an overlay for text and primitives.
//! - [`apu`]: the sound processor. Four chiptune channels (two pulse, one
//!   triangle, one noise) driven by tracker-style songs and sound effects.
//! - [`net`]: input-delay lockstep networking with desync detection, over a
//!   pluggable [`net::Transport`] (in-memory loopback and UDP are provided).
//! - [`replay`]: inputs-only replays. A whole match is a few kilobytes.
//! - [`Game`]: the trait a cartridge implements.
//!
//! See the repository `README.md` and `docs/MANUAL.md` for the hardware spec.
//!
//! ```
//! use kagami::{Apu, Game, Gpu, Input, StateHasher, UpdateCtx, MAX_PLAYERS};
//!
//! #[derive(Clone)]
//! struct Pong { x: i32 }
//!
//! impl Game for Pong {
//!     fn new(_seed: u32) -> Self { Pong { x: 0 } }
//!     fn update(&mut self, ctx: &mut UpdateCtx) {
//!         if ctx.inputs[0].right() { self.x += 1; }
//!     }
//!     fn draw(&self, gpu: &mut Gpu) {
//!         gpu.clear(kagami::palette::AI);
//!         gpu.text(self.x, 100, "KAGAMI", kagami::palette::GOFUN);
//!     }
//!     fn hash(&self, h: &mut StateHasher) { h.write_i32(self.x); }
//! }
//!
//! let mut game = Pong::new(1);
//! let mut apu = Apu::new();
//! let mut ctx = UpdateCtx::new([Input::RIGHT; MAX_PLAYERS], 0, 1, &mut apu);
//! game.update(&mut ctx);
//! assert_eq!(game.x, 1);
//! ```
#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::float_cmp)]
#![warn(missing_docs)]

pub mod apu;
pub mod fx;
pub mod game;
pub mod gpu;
pub mod hash;
pub mod input;
pub mod net;
pub mod replay;
pub mod rng;

pub use apu::{Apu, Sfx, SfxStep, Song, Waveform};
pub use fx::{Fx, Vec2};
pub use game::{Game, UpdateCtx, MAX_PLAYERS, TICK_RATE};
pub use gpu::palette;
pub use gpu::{Gpu, Sprite, SCREEN_H, SCREEN_W};
pub use hash::StateHasher;
pub use input::Input;
pub use rng::Rng;
