//! Running cartridges without a window.

use crate::cli::{Mode, Options};
use crate::session::Session;
use kagami::apu::SAMPLES_PER_FRAME;
use kagami::replay::{Replay, ReplayMismatch};
use kagami::{Game, Gpu, Input, Rng, MAX_PLAYERS};
use std::fmt;

/// What a headless run produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadlessReport {
    /// Frames simulated.
    pub frames: u32,
    /// Final game-state hash.
    pub state_hash: u64,
    /// Hash of the final rendered frame, if one was rendered.
    pub frame_hash: Option<u64>,
    /// Sprite scanline drops in the final frame.
    pub dropped_sprite_lines: u32,
    /// Where the screenshot went.
    pub screenshot: Option<String>,
    /// Where the replay went.
    pub recorded: Option<String>,
    /// For `--verify`: the first failing checkpoint, if any.
    pub mismatch: Option<ReplayMismatch>,
    /// For `--verify`: checkpoints that were checked.
    pub checkpoints: usize,
}

impl fmt::Display for HeadlessReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "frames: {}", self.frames)?;
        writeln!(f, "state hash: {:016x}", self.state_hash)?;
        if let Some(h) = self.frame_hash {
            writeln!(f, "frame hash: {h:016x}")?;
            writeln!(f, "dropped sprite lines: {}", self.dropped_sprite_lines)?;
        }
        if let Some(p) = &self.screenshot {
            writeln!(f, "screenshot: {p}")?;
        }
        if let Some(p) = &self.recorded {
            writeln!(f, "replay: {p}")?;
        }
        if self.checkpoints > 0 || self.mismatch.is_some() {
            match &self.mismatch {
                None => writeln!(f, "verify: OK ({} checkpoints)", self.checkpoints)?,
                Some(m) => writeln!(f, "verify: FAILED ({m})")?,
            }
        }
        Ok(())
    }
}

/// Seeded pseudo-random controller input for soak tests and screenshots.
///
/// Each player holds a random input for a random 4–20 frames, which looks a
/// lot more like a human than per-frame noise. START is never random, but
/// player 0 presses it once on frame 5 so title screens advance.
pub struct RandomInputs {
    rng: Rng,
    held: [Input; MAX_PLAYERS],
    until: [u32; MAX_PLAYERS],
}

impl RandomInputs {
    /// Creates a generator.
    pub fn new(seed: u32) -> RandomInputs {
        RandomInputs {
            rng: Rng::new(seed ^ 0x5EED_1234),
            held: [Input::NONE; MAX_PLAYERS],
            until: [0; MAX_PLAYERS],
        }
    }

    /// Inputs for a frame.
    pub fn frame(&mut self, frame: u32, players: u8) -> [Input; MAX_PLAYERS] {
        let mut out = [Input::NONE; MAX_PLAYERS];
        let active = (players as usize).min(MAX_PLAYERS);
        for (p, slot) in out.iter_mut().enumerate().take(active) {
            if frame >= self.until[p] {
                self.held[p] = Input::from_bits(self.rng.next_u32() as u8 & 0x7F);
                self.until[p] = frame + 4 + self.rng.below(17);
            }
            *slot = self.held[p];
        }
        if frame == 5 {
            out[0] |= Input::START;
        }
        out
    }
}

/// Runs the headless or verify mode described by `opts`.
///
/// # Errors
/// I/O errors reading or writing replay and screenshot files, or a corrupt
/// replay file. A verify mismatch is reported in the result, not as an error.
pub fn run_headless<G: Game>(opts: &Options) -> Result<HeadlessReport, Box<dyn std::error::Error>> {
    match &opts.mode {
        Mode::Verify { path } => {
            let bytes = std::fs::read(path)?;
            let replay = Replay::decode(&bytes)?;
            let checkpoints = replay.checkpoints.len();
            let frames = replay.len() as u32;
            match replay.verify::<G>() {
                Ok(hash) => Ok(HeadlessReport {
                    frames,
                    state_hash: hash,
                    frame_hash: None,
                    dropped_sprite_lines: 0,
                    screenshot: None,
                    recorded: None,
                    mismatch: None,
                    checkpoints,
                }),
                Err(m) => Ok(HeadlessReport {
                    frames,
                    state_hash: m.actual,
                    frame_hash: None,
                    dropped_sprite_lines: 0,
                    screenshot: None,
                    recorded: None,
                    mismatch: Some(m),
                    checkpoints,
                }),
            }
        }
        Mode::Headless {
            frames,
            screenshot,
            random_inputs,
        } => {
            let source = match &opts.replay {
                Some(path) => Some(Replay::decode(&std::fs::read(path)?)?),
                None => None,
            };
            let seed = source.as_ref().map_or(opts.seed, |r| r.seed);
            let players = source
                .as_ref()
                .map(|r| r.players)
                .or(opts.players)
                .unwrap_or_else(|| G::new(seed).default_players());
            let frames = match (&source, *frames) {
                (Some(r), 0) => r.len() as u32,
                (_, f) => f,
            };
            let mut session: Session<G> = Session::local(seed, players, source);
            let mut random = RandomInputs::new(seed);
            let mut audio = [0i16; SAMPLES_PER_FRAME];
            for f in 0..frames {
                let inputs = if *random_inputs {
                    random.frame(f, players)
                } else {
                    [Input::NONE; MAX_PLAYERS]
                };
                let _ = session.tick(inputs);
                session.apu_mut().render_frame(&mut audio);
            }
            let mut gpu = Gpu::new();
            session.game().setup(&mut gpu);
            gpu.begin_frame();
            session.game().draw(&mut gpu);
            gpu.render();
            let mut report = HeadlessReport {
                frames,
                state_hash: session.state_hash(),
                frame_hash: Some(gpu.frame_hash()),
                dropped_sprite_lines: gpu.dropped_sprite_lines(),
                screenshot: None,
                recorded: None,
                mismatch: None,
                checkpoints: 0,
            };
            if let Some(path) = screenshot {
                std::fs::write(path, gpu.screenshot_png())?;
                report.screenshot = Some(path.clone());
            }
            if let Some(path) = &opts.record {
                std::fs::write(path, session.replay().encode())?;
                report.recorded = Some(path.clone());
            }
            Ok(report)
        }
        Mode::Window => unreachable!("window mode is not headless"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagami::{StateHasher, UpdateCtx};

    struct Dots {
        v: u32,
    }
    impl Game for Dots {
        fn new(seed: u32) -> Self {
            Dots { v: seed }
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            self.v = self
                .v
                .wrapping_mul(7)
                .wrapping_add(u32::from(ctx.input(0).bits()));
        }
        fn draw(&self, gpu: &mut Gpu) {
            gpu.clear(kagami::palette::AI);
            gpu.rect_fill((self.v % 200) as i32, 10, 8, 8, kagami::palette::KI);
        }
        fn hash(&self, h: &mut StateHasher) {
            h.write_u32(self.v);
        }
        fn default_players(&self) -> u8 {
            2
        }
    }

    fn tmp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("kagami-headless-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn headless_run_record_verify_roundtrip() {
        let rec = tmp("a.kgmr");
        let shot = tmp("a.png");
        let opts = Options {
            mode: Mode::Headless {
                frames: 120,
                screenshot: Some(shot.clone()),
                random_inputs: true,
            },
            record: Some(rec.clone()),
            seed: 3,
            ..Options::default()
        };
        let r = run_headless::<Dots>(&opts).unwrap();
        assert_eq!(r.frames, 120);
        assert_eq!(r.screenshot.as_deref(), Some(shot.as_str()));
        assert!(r.frame_hash.is_some());
        assert!(std::fs::metadata(&shot).unwrap().len() > 1000);
        let text = r.to_string();
        assert!(
            text.contains("state hash") && text.contains("screenshot:") && text.contains("replay:")
        );

        // Determinism: a second run matches.
        let r2 = run_headless::<Dots>(&opts).unwrap();
        assert_eq!(r.state_hash, r2.state_hash);
        assert_eq!(r.frame_hash, r2.frame_hash);

        // Verify the recorded replay.
        let v = run_headless::<Dots>(&Options {
            mode: Mode::Verify { path: rec.clone() },
            ..Options::default()
        })
        .unwrap();
        assert!(v.mismatch.is_none());
        assert_eq!(v.checkpoints, 2);
        assert_eq!(v.state_hash, r.state_hash);
        assert!(v.to_string().contains("verify: OK"));

        // Replay the file headlessly (frame count from the file).
        let p = run_headless::<Dots>(&Options {
            mode: Mode::Headless {
                frames: 0,
                screenshot: None,
                random_inputs: false,
            },
            replay: Some(rec.clone()),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(p.frames, 120);
        assert_eq!(p.state_hash, r.state_hash);

        // Corrupt a checkpoint and watch verify fail.
        let mut replay = Replay::decode(&std::fs::read(&rec).unwrap()).unwrap();
        replay.checkpoints[1].1 ^= 0xFF;
        let bad = tmp("bad.kgmr");
        std::fs::write(&bad, replay.encode()).unwrap();
        let v = run_headless::<Dots>(&Options {
            mode: Mode::Verify { path: bad },
            ..Options::default()
        })
        .unwrap();
        let m = v.mismatch.unwrap();
        assert_eq!(m.frame, 60);
        assert!(v.to_string().contains("verify: FAILED"));
    }

    #[test]
    fn errors_surface() {
        let missing = tmp("missing.kgmr");
        assert!(run_headless::<Dots>(&Options {
            mode: Mode::Verify { path: missing },
            ..Options::default()
        })
        .is_err());
        let junk = tmp("junk.kgmr");
        std::fs::write(&junk, b"junk").unwrap();
        assert!(run_headless::<Dots>(&Options {
            mode: Mode::Verify { path: junk },
            ..Options::default()
        })
        .is_err());
        let opts = Options {
            mode: Mode::Headless {
                frames: 1,
                screenshot: Some("/nonexistent-dir/x.png".into()),
                random_inputs: false,
            },
            ..Options::default()
        };
        assert!(run_headless::<Dots>(&opts).is_err());
    }

    #[test]
    fn random_inputs_hold_and_vary() {
        let mut r = RandomInputs::new(1);
        let first = r.frame(0, 2);
        let second = r.frame(1, 2);
        assert_eq!(first, second, "inputs are held across frames");
        let mut changed = false;
        for f in 2..200 {
            if r.frame(f, 2) != first {
                changed = true;
                break;
            }
        }
        assert!(changed);
        assert_eq!(r.frame(0, 1)[1], Input::NONE);
        assert!(r.frame(0, 1)[0].bits() & 0x80 == 0, "START is never random");
        assert!(r.frame(5, 1)[0].start(), "except once on frame 5");
    }

    #[test]
    fn default_players_comes_from_cartridge() {
        let opts = Options {
            mode: Mode::Headless {
                frames: 2,
                screenshot: None,
                random_inputs: false,
            },
            ..Options::default()
        };
        let r = run_headless::<Dots>(&opts).unwrap();
        assert_eq!(r.frames, 2);
    }
}
