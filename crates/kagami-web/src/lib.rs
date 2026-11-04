//! Browser runtime for KAGAMI cartridges.
//!
//! The console has no platform code, so a browser build needs only a thin
//! C ABI that JavaScript can drive: create the game, step one frame with
//! packed inputs, and read the RGBA framebuffer and the frame's audio
//! samples straight out of WebAssembly memory. No `wasm-bindgen`, no JS
//! bundler, no dependencies: `web/kagami.js` is a few hundred lines of plain
//! JavaScript.
//!
//! Netplay works the same way: the lockstep or rollback session runs inside
//! the module, and the host moves datagrams between it and whatever carries
//! them (an `RTCDataChannel` in `web/netplay.js`).
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
//! [`Console`] and the [`cartridge!`] macro.
#![warn(missing_docs)]
// `#[no_mangle]` trips the `unsafe_code` lint although it contains no unsafe
// operations; the exports below are plain safe Rust. Allowed only here.
#![allow(unsafe_code)]

use kagami::apu::SAMPLES_PER_FRAME;
use kagami::net::{Lockstep, LockstepConfig, Rollback, RollbackConfig, StepResult, Transport};
use kagami::replay::Replay;
use kagami::{Apu, Game, Gpu, Input, StateHasher, UpdateCtx, MAX_PLAYERS, SCREEN_H, SCREEN_W};
use std::collections::{BTreeMap, VecDeque};

/// Largest datagram the host may hand to the module.
pub const MAX_DATAGRAM: usize = 512;

/// A transport whose both ends are queues the host pumps.
#[derive(Default)]
pub struct QueueTransport {
    inbox: VecDeque<Vec<u8>>,
    outbox: VecDeque<(u8, Vec<u8>)>,
}

impl QueueTransport {
    /// Queues a datagram that arrived from the network.
    pub fn deliver(&mut self, data: &[u8]) {
        self.inbox.push_back(data.to_vec());
    }

    /// Takes the next datagram the session wants sent, with its destination.
    pub fn take_outgoing(&mut self) -> Option<(u8, Vec<u8>)> {
        self.outbox.pop_front()
    }
}

impl Transport for QueueTransport {
    fn send(&mut self, to: u8, data: &[u8]) {
        self.outbox.push_back((to, data.to_vec()));
    }
    fn recv(&mut self) -> Option<Vec<u8>> {
        self.inbox.pop_front()
    }
}

enum Engine<G: Game> {
    Local {
        game: G,
        apu: Apu,
    },
    Playback {
        game: G,
        apu: Apu,
        source: Replay,
    },
    Lockstep {
        game: G,
        apu: Apu,
        session: Lockstep,
    },
    Rollback {
        session: Rollback<G>,
    },
}

/// Netplay status after the last step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetStatus {
    /// Not in netplay, or the last step advanced.
    Ok,
    /// Waiting on remote input; the bitmask names the players.
    Waiting(u8),
    /// The peers disagree, starting at this frame.
    Desynced(u32),
}

/// A cartridge plus its picture and sound processors, driven one frame at a
/// time by the host.
pub struct Console<G: Game> {
    engine: Engine<G>,
    gpu: Gpu,
    audio: [i16; SAMPLES_PER_FRAME],
    frame: u32,
    players: u8,
    seed: u32,
    transport: QueueTransport,
    status: NetStatus,
    confirmed_hashes: BTreeMap<u32, u64>,
    inbox_buf: Vec<u8>,
    outbox_buf: Vec<u8>,
    outbox_to: u8,
    /// Everything simulated so far (confirmed frames under rollback).
    recording: Replay,
    /// Encoded form of `recording`, refreshed by [`Console::export_replay`].
    export_buf: Vec<u8>,
    /// Staging area for a replay the host wants loaded.
    import_buf: Vec<u8>,
}

impl<G: Game> Console<G> {
    /// Boots a cartridge with a match seed and player count (`1..=4`).
    pub fn new(seed: u32, players: u8) -> Console<G> {
        let game = G::new(seed);
        let mut gpu = Gpu::new();
        game.setup(&mut gpu);
        Console {
            engine: Engine::Local {
                game,
                apu: Apu::new(),
            },
            gpu,
            audio: [0; SAMPLES_PER_FRAME],
            frame: 0,
            players: players.clamp(1, MAX_PLAYERS as u8),
            seed,
            transport: QueueTransport::default(),
            status: NetStatus::Ok,
            confirmed_hashes: BTreeMap::new(),
            inbox_buf: vec![0; MAX_DATAGRAM],
            outbox_buf: vec![0; MAX_DATAGRAM],
            outbox_to: 0,
            recording: Replay::new(seed, players.clamp(1, MAX_PLAYERS as u8)),
            export_buf: Vec::new(),
            import_buf: Vec::new(),
        }
    }

    // ----- replays -------------------------------------------------------

    /// Prepares a staging buffer of `len` bytes for a replay file and
    /// returns it for the host to fill.
    pub fn import_buf(&mut self, len: usize) -> &mut [u8] {
        self.import_buf.clear();
        self.import_buf.resize(len.min(1 << 24), 0);
        &mut self.import_buf
    }

    /// Decodes the staged replay and restarts the cartridge playing it
    /// back; inputs passed to `step` are ignored until it ends. Returns the
    /// number of frames, or `None` if the data is not a replay.
    pub fn load_replay(&mut self) -> Option<u32> {
        let source = Replay::decode(&self.import_buf).ok()?;
        self.seed = source.seed;
        self.players = source.players;
        self.frame = 0;
        self.transport = QueueTransport::default();
        self.status = NetStatus::Ok;
        self.confirmed_hashes.clear();
        self.recording = Replay::new(source.seed, source.players);
        let game = G::new(source.seed);
        let frames = source.len() as u32;
        self.engine = Engine::Playback {
            game,
            apu: Apu::new(),
            source,
        };
        Some(frames)
    }

    /// `true` while a loaded replay is driving the inputs.
    pub fn is_playback(&self) -> bool {
        matches!(self.engine, Engine::Playback { .. })
    }

    /// Frames in the loaded replay (0 when not playing one back).
    pub fn playback_len(&self) -> u32 {
        match &self.engine {
            Engine::Playback { source, .. } => source.len() as u32,
            _ => 0,
        }
    }

    /// Re-simulates the staged replay from scratch and checks its
    /// checkpoints: `Some(0)` on success, `Some(frame + 1)` for the first
    /// mismatching checkpoint, `None` if the data is not a replay.
    pub fn verify_replay(&self) -> Option<u32> {
        let r = Replay::decode(&self.import_buf).ok()?;
        Some(match r.verify::<G>() {
            Ok(_) => 0,
            Err(m) => m.frame + 1,
        })
    }

    /// Encodes the recording so far into the export buffer; returns its
    /// length.
    pub fn export_replay(&mut self) -> usize {
        self.export_buf = self.recording.encode();
        self.export_buf.len()
    }

    /// The encoded replay produced by [`Console::export_replay`].
    pub fn export_buf(&self) -> &[u8] {
        &self.export_buf
    }

    /// Frames recorded so far.
    pub fn recorded_frames(&self) -> u32 {
        self.recording.len() as u32
    }

    /// Restarts the cartridge as a netplay peer. `prediction == 0` selects
    /// lockstep; anything else selects rollback with that window.
    pub fn start_net(&mut self, local_player: u8, players: u8, delay: u8, prediction: u8) {
        self.players = players.clamp(1, MAX_PLAYERS as u8);
        let local_player = local_player.min(self.players - 1);
        self.frame = 0;
        self.transport = QueueTransport::default();
        self.status = NetStatus::Ok;
        self.confirmed_hashes.clear();
        self.recording = Replay::new(self.seed, self.players);
        let game = G::new(self.seed);
        self.engine = if prediction == 0 {
            let cfg = LockstepConfig {
                local_player,
                players: self.players,
                input_delay: delay.min(32),
                ..LockstepConfig::default()
            };
            Engine::Lockstep {
                game,
                apu: Apu::new(),
                session: Lockstep::new(cfg),
            }
        } else {
            let cfg = RollbackConfig {
                local_player,
                players: self.players,
                input_delay: delay.min(32),
                max_prediction: prediction.min(32),
                ..RollbackConfig::default()
            };
            Engine::Rollback {
                session: Rollback::new(cfg, game),
            }
        };
    }

    /// Leaves netplay and restarts as a local game.
    pub fn stop_net(&mut self) {
        let game = G::new(self.seed);
        self.engine = Engine::Local {
            game,
            apu: Apu::new(),
        };
        self.frame = 0;
        self.transport = QueueTransport::default();
        self.status = NetStatus::Ok;
        self.confirmed_hashes.clear();
        self.recording = Replay::new(self.seed, self.players);
    }

    /// `true` while a netplay session is active.
    pub fn is_net(&self) -> bool {
        matches!(
            self.engine,
            Engine::Lockstep { .. } | Engine::Rollback { .. }
        )
    }

    /// Hands a datagram from the network to the session.
    pub fn deliver(&mut self, data: &[u8]) {
        self.transport.deliver(data);
    }

    /// Takes the next outgoing datagram, if any.
    pub fn take_outgoing(&mut self) -> Option<(u8, Vec<u8>)> {
        self.transport.take_outgoing()
    }

    /// Status after the last step.
    pub fn status(&self) -> NetStatus {
        self.status
    }

    /// Hash of a confirmed frame under rollback (or any simulated frame
    /// under lockstep), if still remembered.
    pub fn confirmed_hash(&self, frame: u32) -> Option<u64> {
        self.confirmed_hashes.get(&frame).copied()
    }

    fn remember(&mut self, frame: u32, hash: u64) {
        self.confirmed_hashes.insert(frame, hash);
        if self.confirmed_hashes.len() > 256 {
            let drop_below = frame.saturating_sub(256);
            self.confirmed_hashes = self.confirmed_hashes.split_off(&drop_below);
        }
    }

    /// Simulates one frame. `packed` holds one input byte per player,
    /// player 0 in the low byte. Under netplay only the local player's byte
    /// is read. Renders the picture and the frame's audio.
    /// Returns the frame number that was simulated (or would have been,
    /// when waiting).
    pub fn step(&mut self, packed: u32) -> u32 {
        let frame = self.frame;
        let mut remembered: Vec<(u32, u64)> = Vec::new();
        match &mut self.engine {
            Engine::Local { game, apu } => {
                let mut inputs = [Input::NONE; MAX_PLAYERS];
                for (p, slot) in inputs.iter_mut().enumerate().take(self.players as usize) {
                    *slot = Input::from_bits((packed >> (p * 8)) as u8);
                }
                let mut ctx = UpdateCtx::new(inputs, frame, self.players, apu);
                game.update(&mut ctx);
                self.frame += 1;
                let h = kagami::hash::hash_with(|h| game.hash(h));
                remembered.push((frame, h));
                self.recording.push(inputs);
                if frame % 60 == 0 {
                    self.recording.checkpoint(frame, h);
                }
                apu.render_frame(&mut self.audio);
            }
            Engine::Playback { game, apu, source } => {
                if (frame as usize) < source.len() {
                    let inputs = source.inputs(frame);
                    let mut ctx = UpdateCtx::new(inputs, frame, self.players, apu);
                    game.update(&mut ctx);
                    self.frame += 1;
                    let h = kagami::hash::hash_with(|h| game.hash(h));
                    remembered.push((frame, h));
                    self.recording.push(inputs);
                    if frame % 60 == 0 {
                        self.recording.checkpoint(frame, h);
                    }
                }
                apu.render_frame(&mut self.audio);
            }
            Engine::Lockstep { game, apu, session } => {
                let local = session.config().local_player as usize;
                let input = Input::from_bits((packed >> (local * 8)) as u8);
                let r = session.step(game, apu, input, &mut self.transport);
                self.status = match r {
                    StepResult::Advanced(f) => {
                        self.frame = f + 1;
                        let h = kagami::hash::hash_with(|h| game.hash(h));
                        remembered.push((f, h));
                        self.recording.push(session.last_inputs());
                        if f % 60 == 0 {
                            self.recording.checkpoint(f, h);
                        }
                        NetStatus::Ok
                    }
                    StepResult::Waiting(w) => {
                        NetStatus::Waiting(w.iter().fold(0u8, |m, p| m | (1 << p)))
                    }
                    StepResult::Desynced(d) => NetStatus::Desynced(d.frame),
                };
                apu.render_frame(&mut self.audio);
            }
            Engine::Rollback { session } => {
                let local = session.config().local_player as usize;
                let input = Input::from_bits((packed >> (local * 8)) as u8);
                let r = session.step(input, &mut self.transport);
                for (f, inputs, h) in session.drain_confirmed() {
                    remembered.push((f, h));
                    self.recording.push(inputs);
                    if f % 60 == 0 {
                        self.recording.checkpoint(f, h);
                    }
                }
                self.status = match r {
                    StepResult::Advanced(f) => {
                        self.frame = f + 1;
                        NetStatus::Ok
                    }
                    StepResult::Waiting(w) => {
                        NetStatus::Waiting(w.iter().fold(0u8, |m, p| m | (1 << p)))
                    }
                    StepResult::Desynced(d) => NetStatus::Desynced(d.frame),
                };
                session.apu_mut().render_frame(&mut self.audio);
            }
        }
        for (f, h) in remembered {
            self.remember(f, h);
        }
        let gpu = &mut self.gpu;
        gpu.begin_frame();
        match &self.engine {
            Engine::Local { game, .. }
            | Engine::Playback { game, .. }
            | Engine::Lockstep { game, .. } => game.draw(gpu),
            Engine::Rollback { session } => session.game().draw(gpu),
        }
        gpu.render();
        frame
    }

    /// The game as it should be displayed.
    pub fn game(&self) -> &G {
        match &self.engine {
            Engine::Local { game, .. }
            | Engine::Playback { game, .. }
            | Engine::Lockstep { game, .. } => game,
            Engine::Rollback { session } => session.game(),
        }
    }

    fn apu_mut(&mut self) -> &mut Apu {
        match &mut self.engine {
            Engine::Local { apu, .. }
            | Engine::Playback { apu, .. }
            | Engine::Lockstep { apu, .. } => apu,
            Engine::Rollback { session } => session.apu_mut(),
        }
    }

    /// Network statistics: `(frames, stalls, sent, received, rollbacks,
    /// resimulated, confirmed)`. Zeros when not in netplay.
    pub fn net_stats(&self) -> [u32; 7] {
        match &self.engine {
            Engine::Local { .. } | Engine::Playback { .. } => [0; 7],
            Engine::Lockstep { session, .. } => {
                let s = session.stats();
                [
                    s.frames,
                    s.stalls,
                    s.sent as u32,
                    s.received as u32,
                    0,
                    0,
                    s.frames,
                ]
            }
            Engine::Rollback { session } => {
                let s = session.stats();
                [
                    s.frames,
                    s.stalls,
                    s.sent as u32,
                    s.received as u32,
                    s.rollbacks,
                    s.resimulated,
                    s.confirmed,
                ]
            }
        }
    }

    /// The last rendered frame as RGBA8, `SCREEN_W * SCREEN_H * 4` bytes.
    pub fn framebuffer(&self) -> &[u8] {
        self.gpu.rgba()
    }

    /// The last frame's 735 mono 16-bit samples at 44.1 kHz.
    pub fn audio(&self) -> &[i16; SAMPLES_PER_FRAME] {
        &self.audio
    }

    /// Hash of the displayed game state.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.game().hash(&mut h);
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
        self.apu_mut().set_master_volume(v);
    }

    /// The cartridge title.
    pub fn title(&self) -> &'static str {
        self.game().title()
    }

    /// Staging buffer the host writes an incoming datagram into.
    pub fn inbox_buf(&mut self) -> &mut [u8] {
        &mut self.inbox_buf
    }

    /// Moves `len` bytes from the inbox staging buffer into the session.
    pub fn recv_staged(&mut self, len: usize) {
        let len = len.min(MAX_DATAGRAM);
        let data = self.inbox_buf[..len].to_vec();
        self.deliver(&data);
    }

    /// Stages the next outgoing datagram; returns its length (0 = none).
    pub fn stage_outgoing(&mut self) -> usize {
        match self.take_outgoing() {
            Some((to, data)) => {
                let n = data.len().min(MAX_DATAGRAM);
                self.outbox_buf[..n].copy_from_slice(&data[..n]);
                self.outbox_to = to;
                n
            }
            None => 0,
        }
    }

    /// The staged outgoing datagram's bytes.
    pub fn outbox_buf(&self) -> &[u8] {
        &self.outbox_buf
    }

    /// Destination player of the staged outgoing datagram.
    pub fn outbox_to(&self) -> u8 {
        self.outbox_to
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
/// | `kagami_hash_lo() -> u32`, `kagami_hash_hi() -> u32` | state hash halves (no `BigInt` needed) |
/// | `kagami_frame() -> u32` | frames simulated |
/// | `kagami_set_volume(v: u32)` | master volume 0..=15 |
/// | `kagami_title_ptr() -> *const u8`, `kagami_title_len() -> u32` | UTF-8 title |
/// | `kagami_net_start(local, players, delay, prediction)` | restart as a netplay peer (prediction 0 = lockstep) |
/// | `kagami_net_stop()` | back to local play |
/// | `kagami_inbox_ptr() -> *mut u8`, `kagami_net_recv(len: u32)` | hand a received datagram to the session |
/// | `kagami_net_outgoing() -> u32` | stage the next datagram to send; returns its length or 0 |
/// | `kagami_outbox_ptr() -> *const u8`, `kagami_net_outgoing_to() -> u32` | the staged datagram and its destination player |
/// | `kagami_net_status() -> u32` | 0 ok, 1 waiting, 2 desynced |
/// | `kagami_net_waiting() -> u32` | bitmask of players being waited on |
/// | `kagami_net_desync_frame() -> u32` | first disagreeing frame |
/// | `kagami_net_stat(i: u32) -> u32` | frames, stalls, sent, received, rollbacks, resimulated, confirmed |
/// | `kagami_confirmed_hash_lo(f)`, `kagami_confirmed_hash_hi(f)` | hash of a confirmed frame, 0 if unknown |
/// | `kagami_replay_export() -> u32`, `kagami_replay_export_ptr()` | encode the recording so far; its length and bytes |
/// | `kagami_replay_frames() -> u32` | frames recorded |
/// | `kagami_replay_import_ptr(len: u32) -> *mut u8` | staging buffer for a replay file |
/// | `kagami_replay_load() -> u32` | play back the staged replay; its frame count, or `0xFFFF_FFFF` if invalid |
/// | `kagami_replay_verify() -> u32` | 0 = all checkpoints match, n = first mismatch at frame n-1, `0xFFFF_FFFF` = invalid |
/// | `kagami_replay_playing() -> u32`, `kagami_replay_len() -> u32` | playback state |
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

            #[no_mangle]
            pub extern "C" fn kagami_net_start(local: u32, players: u32, delay: u32, prediction: u32) {
                with(|c| c.start_net(local as u8, players as u8, delay as u8, prediction as u8));
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_stop() {
                with(|c| c.stop_net());
            }

            #[no_mangle]
            pub extern "C" fn kagami_inbox_ptr() -> *mut u8 {
                with(|c| c.inbox_buf().as_mut_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_recv(len: u32) {
                with(|c| c.recv_staged(len as usize));
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_outgoing() -> u32 {
                with(|c| c.stage_outgoing() as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_outbox_ptr() -> *const u8 {
                with(|c| c.outbox_buf().as_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_outgoing_to() -> u32 {
                with(|c| u32::from(c.outbox_to()))
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_status() -> u32 {
                with(|c| match c.status() {
                    $crate::NetStatus::Ok => 0,
                    $crate::NetStatus::Waiting(_) => 1,
                    $crate::NetStatus::Desynced(_) => 2,
                })
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_waiting() -> u32 {
                with(|c| match c.status() {
                    $crate::NetStatus::Waiting(m) => u32::from(m),
                    _ => 0,
                })
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_desync_frame() -> u32 {
                with(|c| match c.status() {
                    $crate::NetStatus::Desynced(f) => f,
                    _ => 0,
                })
            }

            #[no_mangle]
            pub extern "C" fn kagami_net_stat(i: u32) -> u32 {
                with(|c| c.net_stats().get(i as usize).copied().unwrap_or(0))
            }

            #[no_mangle]
            pub extern "C" fn kagami_confirmed_hash_lo(frame: u32) -> u32 {
                with(|c| c.confirmed_hash(frame).unwrap_or(0) as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_confirmed_hash_hi(frame: u32) -> u32 {
                with(|c| (c.confirmed_hash(frame).unwrap_or(0) >> 32) as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_export() -> u32 {
                with(|c| c.export_replay() as u32)
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_export_ptr() -> *const u8 {
                with(|c| c.export_buf().as_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_frames() -> u32 {
                with(|c| c.recorded_frames())
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_import_ptr(len: u32) -> *mut u8 {
                with(|c| c.import_buf(len as usize).as_mut_ptr())
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_load() -> u32 {
                with(|c| c.load_replay().unwrap_or(u32::MAX))
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_verify() -> u32 {
                with(|c| c.verify_replay().unwrap_or(u32::MAX))
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_playing() -> u32 {
                with(|c| u32::from(c.is_playback()))
            }

            #[no_mangle]
            pub extern "C" fn kagami_replay_len() -> u32 {
                with(|c| c.playback_len())
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
            self.n = self
                .n
                .wrapping_mul(7)
                .wrapping_add(1 + u32::from(ctx.input(0).bits()));
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
        assert!(!c.is_net());
        assert_eq!(c.step(0), 0);
        assert_eq!(c.frame(), 1);
        assert_eq!(c.framebuffer().len(), (WIDTH * HEIGHT * 4) as usize);
        assert_eq!(
            &c.framebuffer()[..3],
            &kagami::palette::rgb(kagami::palette::KI)
        );
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
        assert_eq!(c.confirmed_hash(1), Some(h));
        c.set_volume(0);
        c.step(0);
        assert!(c.audio().iter().all(|&s| s == 0));
        assert_eq!(Console::<Blink>::new(1, 9).players(), 4);
        let mut one: Console<Blink> = Console::new(1, 1);
        let mut two: Console<Blink> = Console::new(1, 2);
        one.step(u32::from(Input::B.bits()) << 8);
        two.step(u32::from(Input::B.bits()) << 8);
        assert_ne!(one.state_hash(), two.state_hash());
        assert_eq!(c.net_stats(), [0; 7]);
        assert_eq!(c.status(), NetStatus::Ok);
    }

    /// Two consoles wired back to back through their queue transports.
    fn pump(a: &mut Console<Blink>, b: &mut Console<Blink>) {
        while let Some((_, d)) = a.take_outgoing() {
            b.deliver(&d);
        }
        while let Some((_, d)) = b.take_outgoing() {
            a.deliver(&d);
        }
    }

    #[test]
    fn two_consoles_lockstep_through_queues() {
        let mut a: Console<Blink> = Console::new(7, 2);
        let mut b: Console<Blink> = Console::new(7, 2);
        a.start_net(0, 2, 2, 0);
        b.start_net(1, 2, 2, 0);
        assert!(a.is_net() && b.is_net());
        for i in 0..200u32 {
            let ia = if i % 10 < 5 { Input::A } else { Input::NONE };
            let ib = if i % 7 < 3 { Input::B } else { Input::NONE };
            a.step(u32::from(ia.bits()));
            b.step(u32::from(ib.bits()) << 8);
            pump(&mut a, &mut b);
        }
        // Lockstep peers may be a frame apart; compare a frame both confirmed.
        let f = a.frame().min(b.frame()) - 1;
        assert!(f > 150);
        assert_eq!(a.confirmed_hash(f), b.confirmed_hash(f));
        assert!(a.confirmed_hash(f).is_some());
        assert_ne!(a.status(), NetStatus::Desynced(0));
        let s = a.net_stats();
        assert!(s[2] > 0 && s[3] > 0);
        a.stop_net();
        assert!(!a.is_net());
        assert_eq!(a.frame(), 0);
    }

    #[test]
    fn two_consoles_rollback_through_queues_with_delayed_delivery() {
        let mut a: Console<Blink> = Console::new(3, 2);
        let mut b: Console<Blink> = Console::new(3, 2);
        a.start_net(0, 2, 1, 8);
        b.start_net(1, 2, 1, 8);
        let mut held: Vec<(bool, Vec<u8>)> = Vec::new(); // (to_a, data)
        for i in 0..300u32 {
            a.step(u32::from(Input::from_bits((i / 4 % 3) as u8).bits()));
            b.step(u32::from(Input::from_bits((i / 5 % 2 * 32) as u8).bits()) << 8);
            // Deliver with three frames of latency.
            while let Some((_, d)) = a.take_outgoing() {
                held.push((false, d));
            }
            while let Some((_, d)) = b.take_outgoing() {
                held.push((true, d));
            }
            if i >= 3 {
                let due = held.len().saturating_sub(4);
                for (to_a, d) in held.drain(..due) {
                    if to_a {
                        a.deliver(&d);
                    } else {
                        b.deliver(&d);
                    }
                }
            }
        }
        let sa = a.net_stats();
        assert!(sa[4] > 0, "rollbacks happened: {sa:?}");
        let f = sa[6].min(b.net_stats()[6]).saturating_sub(1);
        assert!(f > 200);
        assert_eq!(a.confirmed_hash(f), b.confirmed_hash(f));
        assert!(a.confirmed_hash(f).is_some());
        assert!(!matches!(a.status(), NetStatus::Desynced(_)));
    }

    #[test]
    fn replays_record_export_load_and_verify() {
        let mut c: Console<Blink> = Console::new(5, 2);
        for i in 0..70u32 {
            c.step(u32::from(Input::from_bits((i % 3) as u8).bits()));
        }
        assert_eq!(c.recorded_frames(), 70);
        let n = c.export_replay();
        assert!(n > 0);
        let bytes = c.export_buf().to_vec();
        let decoded = Replay::decode(&bytes).unwrap();
        assert_eq!(decoded.len(), 70);
        assert_eq!(decoded.checkpoints.len(), 2);
        assert_eq!(decoded.verify::<Blink>().unwrap(), c.state_hash());

        // Load it into a fresh console and play it back; inputs are ignored.
        let mut d: Console<Blink> = Console::new(1, 1);
        d.import_buf(bytes.len()).copy_from_slice(&bytes);
        assert_eq!(d.verify_replay(), Some(0));
        assert_eq!(d.load_replay(), Some(70));
        assert!(d.is_playback());
        assert_eq!(d.playback_len(), 70);
        assert_eq!(d.players(), 2);
        for _ in 0..80 {
            d.step(0xFF);
        }
        assert_eq!(d.frame(), 70, "playback stops at the end");
        assert_eq!(d.state_hash(), c.state_hash());
        // The playback itself was recorded again, identically.
        d.export_replay();
        assert_eq!(d.export_buf(), &bytes[..]);

        // Garbage is rejected; a corrupted checkpoint is located.
        let mut e: Console<Blink> = Console::new(1, 1);
        e.import_buf(4).copy_from_slice(b"junk");
        assert_eq!(e.load_replay(), None);
        assert_eq!(e.verify_replay(), None);
        assert!(!e.is_playback());
        let mut bad = decoded.clone();
        bad.checkpoints[1].1 ^= 1;
        let bad_bytes = bad.encode();
        e.import_buf(bad_bytes.len()).copy_from_slice(&bad_bytes);
        assert_eq!(e.verify_replay(), Some(61));
        // Netplay and stop reset the recording.
        c.start_net(0, 2, 1, 0);
        assert_eq!(c.recorded_frames(), 0);
        c.stop_net();
        assert_eq!(c.recorded_frames(), 0);
    }

    #[test]
    fn staging_buffers_round_trip_datagrams() {
        let mut c: Console<Blink> = Console::new(1, 2);
        c.start_net(0, 2, 1, 0);
        c.step(0);
        let n = c.stage_outgoing();
        assert!(n > 0 && n <= MAX_DATAGRAM);
        assert_eq!(c.outbox_to(), 1);
        let msg = c.outbox_buf()[..n].to_vec();
        // Feed it to a second console as if it came off the wire.
        let mut d: Console<Blink> = Console::new(1, 2);
        d.start_net(1, 2, 1, 0);
        d.inbox_buf()[..n].copy_from_slice(&msg);
        d.recv_staged(n);
        d.step(0);
        assert_eq!(d.net_stats()[3], 1, "one datagram received");
        // Frame 0 is a hash checkpoint, so a 14-byte hash message follows.
        assert_eq!(c.stage_outgoing(), 14);
        assert_eq!(c.stage_outgoing(), 0);
        // Oversized stages are clamped, never panics.
        d.recv_staged(9999);
    }

    #[test]
    fn exports_work_through_the_macro() {
        use __kagami_web_exports as x;
        x::kagami_init(3, 1);
        assert_eq!(x::kagami_width(), 256);
        assert_eq!(x::kagami_height(), 224);
        assert_eq!(x::kagami_fb_len(), 256 * 224 * 4);
        assert_eq!(x::kagami_audio_len(), 735);
        assert_eq!(x::kagami_step(0), 0);
        assert_eq!(x::kagami_frame(), 1);
        assert!(!x::kagami_fb_ptr().is_null());
        assert!(!x::kagami_audio_ptr().is_null());
        let lo = x::kagami_hash_lo();
        let hi = x::kagami_hash_hi();
        let direct: Console<Blink> = {
            let mut c = Console::new(3, 1);
            c.step(0);
            c
        };
        assert_eq!((u64::from(hi) << 32) | u64::from(lo), direct.state_hash());
        assert_eq!(x::kagami_confirmed_hash_lo(0), lo);
        assert_eq!(x::kagami_confirmed_hash_hi(0), hi);
        assert_eq!(x::kagami_confirmed_hash_lo(99), 0);
        x::kagami_set_volume(3);
        assert_eq!(x::kagami_title_len(), 5);
        assert!(!x::kagami_title_ptr().is_null());
        // Netplay exports.
        x::kagami_net_start(0, 2, 1, 0);
        assert_eq!(x::kagami_frame(), 0);
        x::kagami_step(0);
        assert_eq!(x::kagami_net_status(), 0);
        let n = x::kagami_net_outgoing();
        assert!(n > 0);
        assert_eq!(x::kagami_net_outgoing_to(), 1);
        assert!(!x::kagami_outbox_ptr().is_null());
        assert!(!x::kagami_inbox_ptr().is_null());
        x::kagami_net_recv(0);
        x::kagami_step(0);
        assert_eq!(x::kagami_net_status(), 1, "waiting on player 2");
        assert_eq!(x::kagami_net_waiting(), 0b10);
        assert_eq!(x::kagami_net_desync_frame(), 0);
        assert!(x::kagami_net_stat(2) > 0);
        assert_eq!(x::kagami_net_stat(99), 0);
        x::kagami_net_stop();
        assert_eq!(x::kagami_net_status(), 0);
        x::kagami_init(3, 1);
        assert_eq!(x::kagami_frame(), 0);
        // Replay exports.
        x::kagami_step(0);
        x::kagami_step(16);
        assert_eq!(x::kagami_replay_frames(), 2);
        let n = x::kagami_replay_export();
        assert!(n > 0 && !x::kagami_replay_export_ptr().is_null());
        assert!(!x::kagami_replay_import_ptr(4).is_null());
        assert_eq!(x::kagami_replay_load(), u32::MAX);
        assert_eq!(x::kagami_replay_verify(), u32::MAX);
        assert_eq!(x::kagami_replay_playing(), 0);
        assert_eq!(x::kagami_replay_len(), 0);
    }
}
