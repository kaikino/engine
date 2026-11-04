//! Measures how fast the console runs headlessly: simulation only, and
//! simulation plus a full picture and sound render, for both cartridges.
//!
//! ```text
//! cargo run --release -p kagami-runtime --example bench
//! ```
//!
//! Timing is the only thing here that touches a wall clock; the simulation
//! itself stays deterministic.

use kagami::apu::SAMPLES_PER_FRAME;
use kagami::{Apu, Game, Gpu, UpdateCtx};
use kagami_runtime::RandomInputs;
use std::time::Instant;

fn bench<G: Game>(name: &str, frames: u32) {
    let mut game = G::new(42);
    let mut apu = Apu::new();
    let mut gpu = Gpu::new();
    game.setup(&mut gpu);
    let mut inputs = RandomInputs::new(42);
    let mut audio = [0i16; SAMPLES_PER_FRAME];

    let t = Instant::now();
    for f in 0..frames {
        let mut ctx = UpdateCtx::new(inputs.frame(f, 2), f, 2, &mut apu);
        game.update(&mut ctx);
    }
    let sim = t.elapsed();

    let mut game = G::new(42);
    let mut apu = Apu::new();
    let mut inputs = RandomInputs::new(42);
    let t = Instant::now();
    for f in 0..frames {
        let mut ctx = UpdateCtx::new(inputs.frame(f, 2), f, 2, &mut apu);
        game.update(&mut ctx);
        apu.render_frame(&mut audio);
        gpu.begin_frame();
        game.draw(&mut gpu);
        gpu.render();
    }
    let full = t.elapsed();

    let per = |d: std::time::Duration| d.as_secs_f64() * 1e6 / f64::from(frames);
    println!(
        "{name:8} {frames} frames | update {:7.1} us/frame ({:6.0}x realtime) | update+audio+render {:7.1} us/frame ({:5.0}x realtime)",
        per(sim),
        16_666.7 / per(sim),
        per(full),
        16_666.7 / per(full)
    );
}

fn main() {
    let frames = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3_000);
    bench::<ippon::Ippon>("IPPON", frames);
    bench::<hanabi::Hanabi>("HANABI", frames);
}
