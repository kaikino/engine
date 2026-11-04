//! Native runtime for KAGAMI cartridges.
//!
//! A cartridge binary is three lines:
//!
//! ```no_run
//! use kagami::{Apu, Game, Gpu, StateHasher, UpdateCtx};
//! #[derive(Clone)]
//! struct MyGame;
//! impl Game for MyGame {
//!     fn new(_seed: u32) -> Self { MyGame }
//!     fn update(&mut self, _ctx: &mut UpdateCtx) {}
//!     fn draw(&self, _gpu: &mut Gpu) {}
//!     fn hash(&self, _h: &mut StateHasher) {}
//! }
//! fn main() -> std::process::ExitCode {
//!     kagami_runtime::main::<MyGame>()
//! }
//! ```
//!
//! The resulting binary accepts a full command line (`--help` lists it):
//! windowed play with one or two local players, headless simulation with
//! screenshots, replay recording and verification, and lockstep netplay over
//! UDP or a simulated in-process network.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod audio;
mod cli;
mod gamepad;
mod headless;
mod keys;
mod session;
mod window;

pub use audio::{AudioOutput, Resampler, SampleQueue};
pub use cli::{parse_args, Mode, NetplayTransport, Options, UsageError, USAGE};
pub use gamepad::{axis_to_dpad, PadState};
pub use headless::{run_headless, HeadlessReport, RandomInputs};
pub use keys::{Controllers, KeyMap};
pub use session::{NetStats, Session, SessionEvent};
pub use window::default_sram_path;

use kagami::Game;
use std::process::ExitCode;

/// Parses `std::env::args()`, runs the cartridge, and returns the exit code.
pub fn main<G: Game + 'static>() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = std::io::stdout();
    match run_with_args::<G>(&args, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kagami: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the cartridge with explicit arguments, writing reports to `out`.
///
/// # Errors
/// Returns an error for bad arguments, I/O failures, replay mismatches,
/// detected desyncs, or a window/audio backend that could not start.
pub fn run_with_args<G: Game + 'static>(
    args: &[String],
    out: &mut dyn std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = match parse_args(args) {
        Ok(o) => o,
        Err(UsageError::Help) => {
            writeln!(out, "{}", cli::USAGE)?;
            return Ok(());
        }
        Err(e) => return Err(Box::new(e)),
    };
    run_with_options::<G>(&opts, out)
}

/// Runs the cartridge with parsed options.
///
/// # Errors
/// See [`run_with_args`].
pub fn run_with_options<G: Game + 'static>(
    opts: &Options,
    out: &mut dyn std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match &opts.mode {
        Mode::Headless { .. } | Mode::Verify { .. } => {
            let report = run_headless::<G>(opts)?;
            writeln!(out, "{report}")?;
            if let Some(m) = report.mismatch {
                return Err(Box::new(m));
            }
            Ok(())
        }
        Mode::Window => window::run::<G>(opts, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagami::{Gpu, StateHasher, UpdateCtx};

    #[derive(Clone)]
    struct Tick(u32);
    impl Game for Tick {
        fn new(seed: u32) -> Self {
            Tick(seed)
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            self.0 = self.0.wrapping_add(1 + u32::from(ctx.input(0).bits()));
        }
        fn draw(&self, gpu: &mut Gpu) {
            gpu.clear(kagami::palette::KI);
        }
        fn hash(&self, h: &mut StateHasher) {
            h.write_u32(self.0);
        }
    }

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn sram_paths_are_slugs() {
        assert_eq!(default_sram_path("IPPON"), "kagami-ippon.sram");
        assert_eq!(
            default_sram_path("Hello, World!"),
            "kagami-hello--world.sram"
        );
        assert_eq!(default_sram_path("鏡"), "kagami-.sram");
    }

    #[test]
    fn help_prints_usage() {
        let mut out = Vec::new();
        run_with_args::<Tick>(&args("--help"), &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("HEADLESS"));
    }

    #[test]
    fn headless_runs_and_reports() {
        let mut out = Vec::new();
        run_with_args::<Tick>(&args("--headless --frames 3 --seed 5"), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("frames: 3"));
        assert!(text.contains("state hash:"));
    }

    #[test]
    fn errors_are_returned() {
        let mut out = Vec::new();
        assert!(run_with_args::<Tick>(&args("--bogus"), &mut out).is_err());
        assert!(run_with_args::<Tick>(&args("--verify /nonexistent/x.kgmr"), &mut out).is_err());
        // A replay whose checkpoint disagrees is an error, not a silent pass.
        let dir = std::env::temp_dir().join(format!("kagami-lib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.kgmr");
        let mut r = kagami::replay::Replay::record::<Tick>(1, 1, 10, 5, |_| {
            [kagami::Input::NONE; kagami::MAX_PLAYERS]
        });
        r.checkpoints[1].1 ^= 1;
        std::fs::write(&path, r.encode()).unwrap();
        let err = run_with_args::<Tick>(
            &["--verify".to_string(), path.to_string_lossy().into_owned()],
            &mut out,
        )
        .unwrap_err();
        assert!(err.to_string().contains("diverged"));
    }
}
