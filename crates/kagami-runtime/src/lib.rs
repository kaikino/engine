//! Native runtime for KAGAMI cartridges.
//!
//! A cartridge binary is three lines:
//!
//! ```no_run
//! use kagami::{Apu, Game, Gpu, StateHasher, UpdateCtx};
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
mod headless;
mod keys;
mod session;
mod window;

pub use audio::{AudioOutput, Resampler, SampleQueue};
pub use cli::{parse_args, Mode, NetplayTransport, Options, UsageError, USAGE};
pub use headless::{run_headless, HeadlessReport, RandomInputs};
pub use keys::{Controllers, KeyMap};
pub use session::{Session, SessionEvent};

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
