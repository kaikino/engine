//! Browser runtime for KAGAMI cartridges.
//!
//! The console has no platform code, so a browser build needs only a thin
//! C ABI that JavaScript can drive: create the game, step one frame with
//! packed inputs, and read the RGBA framebuffer and the frame's audio
//! samples straight out of WebAssembly memory. No `wasm-bindgen`, no JS
//! bundler, no dependencies: `web/kagami.js` is 150 lines of plain
//! JavaScript.
//!
//! A cartridge opts in with one line in its library crate:
//!
//! ```ignore
//! #[cfg(target_arch = "wasm32")]
//! kagami_web::cartridge!(MyGame);
//! ```
//!
//! and builds with `cargo build --target wasm32-unknown-unknown --release`
//! (the crate must be a `cdylib`). See `scripts/build-web.sh`.
//!
//! The exported functions and their contract are documented on
//! [`Console`], which the macro wraps in thread-local storage.
#![warn(missing_docs)]
// `#[no_mangle]` trips the `unsafe_code` lint although it contains no unsafe
// operations; the exports below are plain safe Rust. Allowed only here.
#![allow(unsafe_code)]

use kagami::apu::SAMPLES_PER_FRAME;
use kagami::{Apu, Game, Gpu, Input, StateHasher, UpdateCtx, MAX_PLAYERS, SCREEN_H, SCREEN_W};

/// A cartridge plus its picture and sound processors, driven one frame at a
/// time by the host.
pub struct Console<G: Game> {
    game: G,
    gpu: Gpu,
    apu: Apu,
    audio: [i16; SAMPLES_PER_FRAME],
    frame: u32,
    players: u8,
}

impl<G: Game> Console<G> {
    /// Boots a cartridge with a match seed and player count (`1..=4`).
    pub fn new(seed: u32, players: u8) -> Console<G> {
        let game = G::new(seed);
        let mut gpu = Gpu::new();
        game.setup(&mut gpu);
        Console {
            game,
            gpu,
            apu: Apu::new(),
            audio: [0; SAMPLES_PER_FRAME],
            frame: 0,
            players: players.clamp(1, MAX_PLAYERS as u8),
        }
    }

    /// Simulates one frame. `packed` holds one input byte per player,
    /// player 0 in the low byte. Renders the picture and the frame's audio.
    /// Returns the frame number that was simulated.
    pub fn step(&mut self, packed: u32) -> u32 {
        let mut inputs = [Input::NONE; MAX_PLAYERS];
        for (p, slot) in inputs.iter_mut().enumerate().take(self.players as usize) {
            *slot = Input::from_bits((packed >> (p * 8)) as u8);
        }
        let frame = self.frame;
        let mut ctx = UpdateCtx::new(inputs, frame, self.players, &mut self.apu);
        self.game.update(&mut ctx);
        self.frame += 1;
        self.apu.render_frame(&mut self.audio);
        self.gpu.begin_frame();
        self.game.draw(&mut self.gpu);
        self.gpu.render();
        frame
    }

    /// The last rendered frame as RGBA8, `SCREEN_W * SCREEN_H * 4` bytes.
    pub fn framebuffer(&self) -> &[u8] {
        self.gpu.rgba()
    }

    /// The last frame's 735 mono 16-bit samples at 44.1 kHz.
    pub fn audio(&self) -> &[i16; SAMPLES_PER_FRAME] {
        &self.audio
    }

    /// Hash of the game state.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.game.hash(&mut h);
        h.finish()
    }

    /// Frames simulated so far.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Active players.
    pub fn players(&self) -> u8 {
        self.players
    }

    /// Sets the master volume `0..=15`.
    pub fn set_volume(&mut self, v: u8) {
        self.apu.set_master_volume(v);
    }

    /// The cartridge title.
    pub fn title(&self) -> &'static str {
        self.game.title()
    }
}

/// Screen width, for hosts.
pub const WIDTH: u32 = SCREEN_W as u32;
/// Screen height, for hosts.
pub const HEIGHT: u32 = SCREEN_H as u32;

/// Defines the WebAssembly exports for a cartridge type.
///
/// Exports (all plain C ABI, no allocation by the host):
///
/// | Function | Meaning |
/// |---|---|
/// | `kagami_init(seed: u32, players: u32)` | boot (or reboot) the cartridge |
/// | `kagami_step(packed_inputs: u32) -> u32` | simulate and render one frame, returns its number |
/// | `kagami_fb_ptr() -> *const u8`, `kagami_fb_len() -> u32` | RGBA framebuffer in wasm memory |
/// | `kagami_audio_ptr() -> *const i16`, `kagami_audio_len() -> u32` | last frame's samples |
/// | `kagami_width() -> u32`, `kagami_height() -> u32` | screen size |
/// | `kagami_hash_lo() -> u32`, `kagami_hash_hi() -> u32` | state hash halves (no BigInt needed) |
/// | `kagami_frame() -> u32` | frames simulated |
/// | `kagami_set_volume(v: u32)` | master volume 0..=15 |
/// | `kagami_title_ptr() -> *const u8`, `kagami_title_len() -> u32` | UTF-8 title |
///
/// Pointers are valid until the next `kagami_init`; the framebuffer and
/// audio buffers are reused across frames, so copy them out after each step.
#[macro_export]
macro_rules! cartridge {
    ($game:ty) => {
        mod __kagami_web_exports {
            use super::*;
            use std::cell::RefCell;

            thread_local! {
                static CONSOLE: RefCell<Option<$crate::Console<$game>>> = const { RefCell::new(None) };
            }

            fn with<R>(f: impl FnOnce(&mut $crate::Console<$game>) -> R) -> R {
                CONSOLE.with(|c| {
                    let mut slot = c.borrow_mut();
                    let console = slot.get_or_insert_with(|| $crate::Console::new(1, 1));
                    f(console)
                })
            }

            #[no_mangle]
            pub extern "C" fn kagami_init(seed: u32, players: u32) {
                CONSOLE.with(|c| {
                    *c.borrow_mut() = Some($crate::Console::new(seed, players as u8));
                });
            }

            #[no_mangle]
            pub extern "C" fn kagami_step(packed: u32) -> u32 {
                with(|c| c.step(packed))
            }

            #[no_mangle]
            pub extern "C" fn kagami_fb_ptr() -> *const u8 {
                with(|c| c.framebuffer().as_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_fb_len() -> u32 {
                with(|c| c.framebuffer().len() as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_audio_ptr() -> *const i16 {
                with(|c| c.audio().as_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_audio_len() -> u32 {
                with(|c| c.audio().len() as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_width() -> u32 {
                $crate::WIDTH
            }

            #[no_mangle]
            pub extern "C" fn kagami_height() -> u32 {
                $crate::HEIGHT
            }

            #[no_mangle]
            pub extern "C" fn kagami_hash_lo() -> u32 {
                with(|c| c.state_hash() as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_hash_hi() -> u32 {
                with(|c| (c.state_hash() >> 32) as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_frame() -> u32 {
                with(|c| c.frame())
            }

            #[no_mangle]
            pub extern "C" fn kagami_set_volume(v: u32) {
                with(|c| c.set_volume(v as u8));
            }

            #[no_mangle]
            pub extern "C" fn kagami_title_ptr() -> *const u8 {
                with(|c| c.title().as_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_title_len() -> u32 {
                with(|c| c.title().len() as u32)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Blink {
        on: bool,
        n: u32,
    }
    impl Game for Blink {
        fn new(seed: u32) -> Self {
            Blink {
                on: seed % 2 == 1,
                n: 0,
            }
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            if ctx.input(0).a() {
                self.on = !self.on;
                ctx.apu.sfx(0, &kagami::Sfx::blip());
            }
            if ctx.input(1).b() {
                self.n += 100;
            }
            self.n += 1;
        }
        fn draw(&self, gpu: &mut Gpu) {
            gpu.clear(if self.on {
                kagami::palette::KI
            } else {
                kagami::palette::AI
            });
        }
        fn hash(&self, h: &mut StateHasher) {
            h.write_bool(self.on);
            h.write_u32(self.n);
        }
        fn title(&self) -> &'static str {
            "Blink"
        }
    }

    cartridge!(Blink);

    #[test]
    fn console_steps_and_exposes_buffers() {
        let mut c: Console<Blink> = Console::new(1, 2);
        assert_eq!(c.players(), 2);
        assert_eq!(c.title(), "Blink");
        assert_eq!(c.step(0), 0);
        assert_eq!(c.frame(), 1);
        assert_eq!(c.framebuffer().len(), (WIDTH * HEIGHT * 4) as usize);
        assert_eq!(
            &c.framebuffer()[..3],
            &kagami::palette::rgb(kagami::palette::KI)
        );
        // Player 0 presses A: color flips. Player 1 presses B: n jumps.
        c.step(u32::from(Input::A.bits()) | (u32::from(Input::B.bits()) << 8));
        assert_eq!(
            &c.framebuffer()[..3],
            &kagami::palette::rgb(kagami::palette::AI)
        );
        assert!(c.audio().iter().any(|&s| s != 0), "the blip rendered");
        let h = c.state_hash();
        let mut reference = Blink::new(1);
        let mut apu = Apu::new();
        let mut ctx = UpdateCtx::new([Input::NONE; MAX_PLAYERS], 0, 2, &mut apu);
        reference.update(&mut ctx);
        let mut ctx = UpdateCtx::new(
            [Input::A, Input::B, Input::NONE, Input::NONE],
            1,
            2,
            &mut apu,
        );
        reference.update(&mut ctx);
        assert_eq!(h, kagami::hash::hash_with(|hh| reference.hash(hh)));
        c.set_volume(0);
        c.step(0);
        assert!(c.audio().iter().all(|&s| s == 0));
        // Players clamp, and a second player's bits are ignored with 1 player.
        assert_eq!(Console::<Blink>::new(1, 9).players(), 4);
        let mut one: Console<Blink> = Console::new(1, 1);
        let mut two: Console<Blink> = Console::new(1, 2);
        one.step(u32::from(Input::B.bits()) << 8);
        two.step(u32::from(Input::B.bits()) << 8);
        assert_ne!(one.state_hash(), two.state_hash());
    }

    #[test]
    fn exports_work_through_the_macro() {
        __kagami_web_exports::kagami_init(3, 1);
        assert_eq!(__kagami_web_exports::kagami_width(), 256);
        assert_eq!(__kagami_web_exports::kagami_height(), 224);
        assert_eq!(__kagami_web_exports::kagami_fb_len(), 256 * 224 * 4);
        assert_eq!(__kagami_web_exports::kagami_audio_len(), 735);
        assert_eq!(__kagami_web_exports::kagami_step(0), 0);
        assert_eq!(__kagami_web_exports::kagami_frame(), 1);
        assert!(!__kagami_web_exports::kagami_fb_ptr().is_null());
        assert!(!__kagami_web_exports::kagami_audio_ptr().is_null());
        let lo = __kagami_web_exports::kagami_hash_lo();
        let hi = __kagami_web_exports::kagami_hash_hi();
        let direct: Console<Blink> = {
            let mut c = Console::new(3, 1);
            c.step(0);
            c
        };
        assert_eq!((u64::from(hi) << 32) | u64::from(lo), direct.state_hash());
        __kagami_web_exports::kagami_set_volume(3);
        assert_eq!(__kagami_web_exports::kagami_title_len(), 5);
        assert!(!__kagami_web_exports::kagami_title_ptr().is_null());
        // init again resets the frame counter.
        __kagami_web_exports::kagami_init(3, 1);
        assert_eq!(__kagami_web_exports::kagami_frame(), 0);
    }
}
