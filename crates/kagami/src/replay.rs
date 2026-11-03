//! Inputs-only replays.
//!
//! Because the simulation is deterministic, a replay is just the seed, the
//! player count and one byte per player per frame. A ten-minute two-player
//! match is about 72 KB; with checkpoints it also verifies itself.
//!
//! ```
//! use kagami::replay::Replay;
//! use kagami::{Input, MAX_PLAYERS};
//! let mut r = Replay::new(42, 2);
//! let mut f = [Input::NONE; MAX_PLAYERS];
//! f[0] = Input::A;
//! r.push(f);
//! r.checkpoint(0, 0xABCD);
//! let bytes = r.encode();
//! assert_eq!(Replay::decode(&bytes).unwrap(), r);
//! ```

use crate::apu::Apu;
use crate::game::{Game, UpdateCtx, MAX_PLAYERS};
use crate::hash::StateHasher;
use crate::input::Input;

const MAGIC: &[u8; 4] = b"KGMR";
const VERSION: u8 = 1;

/// A recorded match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// Match seed passed to [`Game::new`].
    pub seed: u32,
    /// Number of players (`1..=MAX_PLAYERS`).
    pub players: u8,
    /// Inputs for every frame, in order.
    pub frames: Vec<[Input; MAX_PLAYERS]>,
    /// `(frame, state hash after that frame)` pairs, ascending by frame.
    pub checkpoints: Vec<(u32, u64)>,
}

/// Errors from [`Replay::decode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// Not a KAGAMI replay.
    BadMagic,
    /// A format version this build does not understand.
    BadVersion(u8),
    /// The data ended early or has an impossible field.
    Truncated,
    /// Player count out of range.
    BadPlayers(u8),
}

impl core::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReplayError::BadMagic => write!(f, "not a KAGAMI replay"),
            ReplayError::BadVersion(v) => write!(f, "unsupported replay version {v}"),
            ReplayError::Truncated => write!(f, "replay data is truncated"),
            ReplayError::BadPlayers(p) => write!(f, "invalid player count {p}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// A checkpoint whose hash did not match during [`Replay::verify`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayMismatch {
    /// The frame of the failing checkpoint.
    pub frame: u32,
    /// Hash stored in the replay.
    pub expected: u64,
    /// Hash the current build produced.
    pub actual: u64,
}

impl core::fmt::Display for ReplayMismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "replay diverged at frame {}: expected {:016x}, got {:016x}",
            self.frame, self.expected, self.actual
        )
    }
}

impl std::error::Error for ReplayMismatch {}

impl Replay {
    /// Starts an empty replay.
    pub fn new(seed: u32, players: u8) -> Replay {
        Replay {
            seed,
            players,
            frames: Vec::new(),
            checkpoints: Vec::new(),
        }
    }

    /// Appends one frame of inputs.
    pub fn push(&mut self, inputs: [Input; MAX_PLAYERS]) {
        self.frames.push(inputs);
    }

    /// Records a state hash after `frame`. Frames must be pushed in order.
    pub fn checkpoint(&mut self, frame: u32, hash: u64) {
        self.checkpoints.push((frame, hash));
    }

    /// Number of recorded frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// `true` if no frames are recorded.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Inputs for a frame, or all-none past the end.
    pub fn inputs(&self, frame: u32) -> [Input; MAX_PLAYERS] {
        self.frames
            .get(frame as usize)
            .copied()
            .unwrap_or([Input::NONE; MAX_PLAYERS])
    }

    /// Serializes to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let players = (self.players as usize).clamp(1, MAX_PLAYERS);
        let mut v =
            Vec::with_capacity(20 + self.frames.len() * players + self.checkpoints.len() * 12);
        v.extend_from_slice(MAGIC);
        v.push(VERSION);
        v.extend_from_slice(&self.seed.to_le_bytes());
        v.push(players as u8);
        v.extend_from_slice(&(self.frames.len() as u32).to_le_bytes());
        for f in &self.frames {
            for i in &f[..players] {
                v.push(i.bits());
            }
        }
        v.extend_from_slice(&(self.checkpoints.len() as u32).to_le_bytes());
        for (frame, hash) in &self.checkpoints {
            v.extend_from_slice(&frame.to_le_bytes());
            v.extend_from_slice(&hash.to_le_bytes());
        }
        v
    }

    /// Parses bytes produced by [`Replay::encode`].
    pub fn decode(data: &[u8]) -> Result<Replay, ReplayError> {
        let mut pos = 0usize;
        let take = |pos: &mut usize, n: usize| -> Result<&[u8], ReplayError> {
            let s = data.get(*pos..*pos + n).ok_or(ReplayError::Truncated)?;
            *pos += n;
            Ok(s)
        };
        if take(&mut pos, 4)? != MAGIC {
            return Err(ReplayError::BadMagic);
        }
        let version = take(&mut pos, 1)?[0];
        if version != VERSION {
            return Err(ReplayError::BadVersion(version));
        }
        let seed = u32::from_le_bytes(
            take(&mut pos, 4)?
                .try_into()
                .map_err(|_| ReplayError::Truncated)?,
        );
        let players = take(&mut pos, 1)?[0];
        if players == 0 || players as usize > MAX_PLAYERS {
            return Err(ReplayError::BadPlayers(players));
        }
        let n = u32::from_le_bytes(
            take(&mut pos, 4)?
                .try_into()
                .map_err(|_| ReplayError::Truncated)?,
        );
        let mut frames = Vec::with_capacity((n as usize).min(1 << 20));
        for _ in 0..n {
            let raw = take(&mut pos, players as usize)?;
            let mut f = [Input::NONE; MAX_PLAYERS];
            for (slot, b) in f.iter_mut().zip(raw) {
                *slot = Input::from_bits(*b);
            }
            frames.push(f);
        }
        let c = u32::from_le_bytes(
            take(&mut pos, 4)?
                .try_into()
                .map_err(|_| ReplayError::Truncated)?,
        );
        let mut checkpoints = Vec::with_capacity((c as usize).min(1 << 16));
        for _ in 0..c {
            let frame = u32::from_le_bytes(
                take(&mut pos, 4)?
                    .try_into()
                    .map_err(|_| ReplayError::Truncated)?,
            );
            let hash = u64::from_le_bytes(
                take(&mut pos, 8)?
                    .try_into()
                    .map_err(|_| ReplayError::Truncated)?,
            );
            checkpoints.push((frame, hash));
        }
        if pos != data.len() {
            return Err(ReplayError::Truncated);
        }
        Ok(Replay {
            seed,
            players,
            frames,
            checkpoints,
        })
    }

    /// Re-simulates the replay from scratch and verifies every checkpoint.
    /// Returns the final state hash on success.
    pub fn verify<G: Game>(&self) -> Result<u64, ReplayMismatch> {
        let mut game = G::new(self.seed);
        let mut apu = Apu::new();
        let mut next_cp = 0usize;
        let mut last_hash = {
            let mut h = StateHasher::new();
            game.hash(&mut h);
            h.finish()
        };
        for (frame, inputs) in self.frames.iter().enumerate() {
            let frame = frame as u32;
            let mut ctx = UpdateCtx::new(*inputs, frame, self.players, &mut apu);
            game.update(&mut ctx);
            let mut h = StateHasher::new();
            game.hash(&mut h);
            last_hash = h.finish();
            while let Some(&(cp_frame, expected)) = self.checkpoints.get(next_cp) {
                if cp_frame > frame {
                    break;
                }
                next_cp += 1;
                if cp_frame == frame && expected != last_hash {
                    return Err(ReplayMismatch {
                        frame,
                        expected,
                        actual: last_hash,
                    });
                }
            }
        }
        Ok(last_hash)
    }

    /// Records a game played with the given input source for `frames` frames,
    /// taking a checkpoint every `checkpoint_interval` frames (0 = none).
    pub fn record<G: Game>(
        seed: u32,
        players: u8,
        frames: u32,
        checkpoint_interval: u32,
        mut inputs: impl FnMut(u32) -> [Input; MAX_PLAYERS],
    ) -> Replay {
        let mut replay = Replay::new(seed, players);
        let mut game = G::new(seed);
        let mut apu = Apu::new();
        for frame in 0..frames {
            let i = inputs(frame);
            replay.push(i);
            let mut ctx = UpdateCtx::new(i, frame, players, &mut apu);
            game.update(&mut ctx);
            if checkpoint_interval > 0 && frame % checkpoint_interval == 0 {
                let mut h = StateHasher::new();
                game.hash(&mut h);
                replay.checkpoint(frame, h.finish());
            }
        }
        replay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::Gpu;

    struct Acc {
        v: u32,
    }
    impl Game for Acc {
        fn new(seed: u32) -> Self {
            Acc { v: seed }
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            self.v = self
                .v
                .wrapping_mul(31)
                .wrapping_add(u32::from(ctx.input(0).bits()));
        }
        fn draw(&self, _gpu: &mut Gpu) {}
        fn hash(&self, h: &mut StateHasher) {
            h.write_u32(self.v);
        }
    }

    fn inputs(f: u32) -> [Input; MAX_PLAYERS] {
        let mut i = [Input::NONE; MAX_PLAYERS];
        i[0] = Input::from_bits((f % 13) as u8);
        i
    }

    #[test]
    fn record_verify_roundtrip() {
        let r = Replay::record::<Acc>(7, 1, 100, 10, inputs);
        assert_eq!(r.len(), 100);
        assert!(!r.is_empty());
        assert_eq!(r.checkpoints.len(), 10);
        let bytes = r.encode();
        assert_eq!(bytes.len(), 4 + 1 + 4 + 1 + 4 + 100 + 4 + 120);
        let back = Replay::decode(&bytes).unwrap();
        assert_eq!(back, r);
        let h = back.verify::<Acc>().unwrap();
        assert_eq!(
            h,
            crate::hash::hash_with(|hh| hh.write_u32(replay_final(7)))
        );
        assert_eq!(r.inputs(5)[0], Input::from_bits(5));
        assert_eq!(r.inputs(999), [Input::NONE; MAX_PLAYERS]);
    }

    fn replay_final(seed: u32) -> u32 {
        let mut g = Acc::new(seed);
        let mut apu = Apu::new();
        for f in 0..100 {
            let mut ctx = UpdateCtx::new(inputs(f), f, 1, &mut apu);
            g.update(&mut ctx);
        }
        g.v
    }

    #[test]
    fn verify_detects_divergence() {
        let mut r = Replay::record::<Acc>(7, 1, 50, 10, inputs);
        r.checkpoints[3].1 ^= 1; // corrupt the checkpoint at frame 30
        let err = r.verify::<Acc>().unwrap_err();
        assert_eq!(err.frame, 30);
        assert!(err.to_string().contains("frame 30"));
        // A checkpoint past the end is simply never reached.
        let mut r = Replay::record::<Acc>(7, 1, 5, 0, inputs);
        r.checkpoint(99, 1);
        assert!(r.verify::<Acc>().is_ok());
        assert!(Replay::new(1, 1).is_empty());
    }

    #[test]
    fn decode_errors() {
        let r = Replay::record::<Acc>(1, 2, 3, 1, inputs);
        let bytes = r.encode();
        assert_eq!(Replay::decode(b"NOPE").unwrap_err(), ReplayError::BadMagic);
        let mut v = bytes.clone();
        v[4] = 9;
        assert_eq!(Replay::decode(&v).unwrap_err(), ReplayError::BadVersion(9));
        let mut v = bytes.clone();
        v[9] = 0;
        assert_eq!(Replay::decode(&v).unwrap_err(), ReplayError::BadPlayers(0));
        v[9] = 5;
        assert_eq!(Replay::decode(&v).unwrap_err(), ReplayError::BadPlayers(5));
        assert_eq!(
            Replay::decode(&bytes[..bytes.len() - 1]).unwrap_err(),
            ReplayError::Truncated
        );
        let mut v = bytes.clone();
        v.push(0);
        assert_eq!(Replay::decode(&v).unwrap_err(), ReplayError::Truncated);
        assert_eq!(
            Replay::decode(&bytes[..6]).unwrap_err(),
            ReplayError::Truncated
        );
        for e in [
            ReplayError::BadMagic,
            ReplayError::BadVersion(1),
            ReplayError::Truncated,
            ReplayError::BadPlayers(0),
        ] {
            assert_ne!(e.to_string(), "");
        }
        // Player count clamps on encode.
        let weird = Replay {
            seed: 0,
            players: 0,
            frames: vec![[Input::A; MAX_PLAYERS]],
            checkpoints: vec![],
        };
        assert_eq!(Replay::decode(&weird.encode()).unwrap().players, 1);
    }
}
