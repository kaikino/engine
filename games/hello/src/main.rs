//! The smallest KAGAMI cartridge: a greeting that drifts with the d-pad.
//!
//! ```text
//! cargo run -p hello
//! cargo run -p hello -- --headless --frames 60 --screenshot hello.png
//! ```
#![deny(clippy::float_arithmetic)]

use kagami::{palette, Game, Gpu, StateHasher, UpdateCtx};

struct Hello {
    x: i32,
    y: i32,
    frame: u32,
}

impl Game for Hello {
    fn new(_seed: u32) -> Self {
        Hello {
            x: 104,
            y: 108,
            frame: 0,
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx) {
        let input = ctx.input(0);
        self.x = (self.x + input.axis_x()).clamp(0, 208);
        self.y = (self.y + input.axis_y()).clamp(0, 216);
        if input.a() && self.frame % 10 == 0 {
            ctx.apu.sfx(0, &kagami::Sfx::blip());
        }
        self.frame += 1;
    }

    fn draw(&self, gpu: &mut Gpu) {
        gpu.clear(palette::AI);
        gpu.rect(self.x - 4, self.y - 4, 56, 16, palette::KIN);
        gpu.text(self.x, self.y, "KAGAMI", palette::GOFUN);
        gpu.text_centered(200, "ARROWS MOVE  Z BEEPS", palette::SORA);
    }

    fn hash(&self, h: &mut StateHasher) {
        h.write_i32(self.x);
        h.write_i32(self.y);
        h.write_u32(self.frame);
    }

    fn title(&self) -> &'static str {
        "Hello"
    }
}

fn main() -> std::process::ExitCode {
    kagami_runtime::main::<Hello>()
}
