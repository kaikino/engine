//! The lockstep session state machine.

use super::wire::{Message, MAX_WINDOW};
use super::Transport;
use crate::apu::Apu;
use crate::game::{Game, UpdateCtx, MAX_PLAYERS};
use crate::hash::StateHasher;
use crate::input::Input;
use std::collections::BTreeMap;

/// Session parameters. Every peer must use identical `players`,
/// `input_delay` and `hash_interval`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockstepConfig {
    /// This peer's player index.
    pub local_player: u8,
    /// Total players in the session (`1..=MAX_PLAYERS`).
    pub players: u8,
    /// Frames between pressing a button and it taking effect (`0..=32`).
    /// 2–4 is comfortable on a LAN; 5–8 across the internet.
    pub input_delay: u8,
    /// Frames between state-hash exchanges. `0` disables desync detection.
    pub hash_interval: u32,
    /// Recent inputs repeated in every input message so lost packets heal
    /// (`1..=32`).
    pub redundancy: u8,
    /// Resend the last input window after this many consecutive stalled
    /// ticks (`1..`). Keeps sessions alive through burst loss.
    pub resend_after_stalls: u32,
}

impl Default for LockstepConfig {
    fn default() -> Self {
        LockstepConfig {
            local_player: 0,
            players: 2,
            input_delay: 3,
            hash_interval: 30,
            redundancy: 8,
            resend_after_stalls: 3,
        }
    }
}

/// A detected divergence between peers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desync {
    /// First frame whose hash disagreed.
    pub frame: u32,
    /// Our hash after that frame.
    pub local_hash: u64,
    /// The remote peer's hash after that frame.
    pub remote_hash: u64,
    /// The peer we disagreed with.
    pub remote_player: u8,
}

impl core::fmt::Display for Desync {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "desync at frame {} with player {}: local {:016x} != remote {:016x}",
            self.frame, self.remote_player, self.local_hash, self.remote_hash
        )
    }
}

/// Counters for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockstepStats {
    /// Frames simulated.
    pub frames: u32,
    /// Ticks that could not advance because a remote input was missing.
    pub stalls: u32,
    /// Messages sent.
    pub sent: u64,
    /// Messages received and understood.
    pub received: u64,
    /// Messages ignored as malformed or from an unknown player.
    pub rejected: u64,
}

/// Outcome of one [`Lockstep::step`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepResult {
    /// The game advanced one frame; this was its number.
    Advanced(u32),
    /// Waiting for input from these players.
    Waiting(Vec<u8>),
    /// A desync has been detected; the session should end.
    Desynced(Desync),
}

/// A lockstep session for one peer.
pub struct Lockstep {
    cfg: LockstepConfig,
    frame: u32,
    inputs: Vec<BTreeMap<u32, Input>>,
    local_hashes: BTreeMap<u32, u64>,
    remote_hashes: Vec<BTreeMap<u32, u64>>,
    desync: Option<Desync>,
    consecutive_stalls: u32,
    stats: LockstepStats,
    last_inputs: [Input; MAX_PLAYERS],
}

impl Lockstep {
    /// Starts a session at frame zero. The first `input_delay` frames of every
    /// player are pre-filled with no input, by convention.
    ///
    /// # Panics
    /// Panics if the configuration is out of range.
    pub fn new(cfg: LockstepConfig) -> Lockstep {
        assert!(
            cfg.players >= 1 && (cfg.players as usize) <= MAX_PLAYERS,
            "players must be 1..=MAX_PLAYERS"
        );
        assert!(cfg.local_player < cfg.players, "local_player out of range");
        assert!(
            (cfg.input_delay as usize) <= MAX_WINDOW,
            "input_delay too large"
        );
        assert!(
            cfg.redundancy >= 1 && (cfg.redundancy as usize) <= MAX_WINDOW,
            "redundancy"
        );
        assert!(
            cfg.resend_after_stalls >= 1,
            "resend_after_stalls must be >= 1"
        );
        let mut inputs: Vec<BTreeMap<u32, Input>> =
            (0..cfg.players).map(|_| BTreeMap::new()).collect();
        for map in &mut inputs {
            for f in 0..u32::from(cfg.input_delay) {
                map.insert(f, Input::NONE);
            }
        }
        Lockstep {
            cfg,
            frame: 0,
            inputs,
            local_hashes: BTreeMap::new(),
            remote_hashes: (0..cfg.players).map(|_| BTreeMap::new()).collect(),
            desync: None,
            consecutive_stalls: 0,
            stats: LockstepStats::default(),
            last_inputs: [Input::NONE; MAX_PLAYERS],
        }
    }

    /// Inputs used for the most recently advanced frame.
    pub fn last_inputs(&self) -> [Input; MAX_PLAYERS] {
        self.last_inputs
    }

    /// The configuration.
    pub fn config(&self) -> &LockstepConfig {
        &self.cfg
    }

    /// Next frame to simulate.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Diagnostics.
    pub fn stats(&self) -> LockstepStats {
        self.stats
    }

    /// The detected desync, if any.
    pub fn desync(&self) -> Option<Desync> {
        self.desync
    }

    /// Remote players whose input for the current frame has not arrived.
    pub fn waiting_on(&self) -> Vec<u8> {
        (0..self.cfg.players)
            .filter(|&p| p != self.cfg.local_player)
            .filter(|&p| !self.inputs[p as usize].contains_key(&self.frame))
            .collect()
    }

    /// Drains the transport, applying every message.
    pub fn poll(&mut self, t: &mut impl Transport) {
        while let Some(data) = t.recv() {
            match Message::decode(&data) {
                Some(m) => self.handle(m),
                None => self.stats.rejected += 1,
            }
        }
    }

    /// Applies one decoded message (exposed for custom transports and tests).
    pub fn handle(&mut self, m: Message) {
        match m {
            Message::Inputs {
                from,
                last_frame,
                inputs,
            } => {
                if from >= self.cfg.players || from == self.cfg.local_player {
                    self.stats.rejected += 1;
                    return;
                }
                self.stats.received += 1;
                let n = inputs.len() as u32;
                let first = last_frame.saturating_sub(n - 1);
                for (i, inp) in inputs.iter().enumerate() {
                    let f = first + i as u32;
                    if f >= self.frame {
                        self.inputs[from as usize].entry(f).or_insert(*inp);
                    }
                }
            }
            Message::Hash { from, frame, hash } => {
                if from >= self.cfg.players || from == self.cfg.local_player {
                    self.stats.rejected += 1;
                    return;
                }
                self.stats.received += 1;
                self.remote_hashes[from as usize].insert(frame, hash);
                self.check_hash(from, frame);
            }
        }
    }

    fn check_hash(&mut self, player: u8, frame: u32) {
        if self.desync.is_some() {
            return;
        }
        let (Some(&local), Some(&remote)) = (
            self.local_hashes.get(&frame),
            self.remote_hashes[player as usize].get(&frame),
        ) else {
            return;
        };
        if local != remote {
            self.desync = Some(Desync {
                frame,
                local_hash: local,
                remote_hash: remote,
                remote_player: player,
            });
        }
    }

    fn send_input_window(&mut self, t: &mut impl Transport) {
        let local = &self.inputs[self.cfg.local_player as usize];
        let Some((&last_frame, _)) = local.iter().next_back() else {
            return;
        };
        let n = usize::from(self.cfg.redundancy);
        let first = last_frame.saturating_sub(n as u32 - 1);
        let inputs: Vec<Input> = (first..=last_frame)
            .map(|f| local.get(&f).copied().unwrap_or(Input::NONE))
            .collect();
        let msg = Message::Inputs {
            from: self.cfg.local_player,
            last_frame,
            inputs,
        }
        .encode();
        self.broadcast(t, &msg);
    }

    fn broadcast(&mut self, t: &mut impl Transport, data: &[u8]) {
        for p in 0..self.cfg.players {
            if p != self.cfg.local_player {
                t.send(p, data);
                self.stats.sent += 1;
            }
        }
    }

    /// Tries to advance one frame.
    ///
    /// Polls the transport, and if every player's input for the current frame
    /// is known: schedules `local_input` for `frame + input_delay`, sends it,
    /// and returns the inputs to simulate this frame. Otherwise returns
    /// `None` and, after a few stalls, resends the last input window.
    pub fn advance(
        &mut self,
        local_input: Input,
        t: &mut impl Transport,
    ) -> Option<[Input; MAX_PLAYERS]> {
        self.poll(t);
        if self.desync.is_some() {
            return None;
        }
        let local = self.cfg.local_player as usize;
        let ready = (0..self.cfg.players as usize)
            .filter(|&p| p != local)
            .all(|p| self.inputs[p].contains_key(&self.frame));
        if !ready {
            self.stats.stalls += 1;
            self.consecutive_stalls += 1;
            if self.consecutive_stalls % self.cfg.resend_after_stalls == 0 {
                self.send_input_window(t);
            }
            return None;
        }
        self.consecutive_stalls = 0;
        // Schedule and send our input for the future frame.
        let target = self.frame + u32::from(self.cfg.input_delay);
        self.inputs[self.cfg.local_player as usize]
            .entry(target)
            .or_insert(local_input);
        self.send_input_window(t);
        // Gather this frame's inputs.
        let mut out = [Input::NONE; MAX_PLAYERS];
        for (p, slot) in out.iter_mut().enumerate().take(self.cfg.players as usize) {
            *slot = self.inputs[p]
                .get(&self.frame)
                .copied()
                .unwrap_or(Input::NONE);
        }
        // Prune history older than the redundancy window.
        let keep_from = self
            .frame
            .saturating_sub(u32::from(self.cfg.redundancy) + 1);
        for map in &mut self.inputs {
            *map = map.split_off(&keep_from);
        }
        self.frame += 1;
        self.stats.frames += 1;
        self.last_inputs = out;
        Some(out)
    }

    /// Records the state hash after simulating `frame`, shares it with peers
    /// on hash-interval frames, and checks it against any hashes already
    /// received.
    pub fn report_hash(&mut self, frame: u32, hash: u64, t: &mut impl Transport) {
        if self.cfg.hash_interval == 0 || frame % self.cfg.hash_interval != 0 {
            return;
        }
        self.local_hashes.insert(frame, hash);
        let msg = Message::Hash {
            from: self.cfg.local_player,
            frame,
            hash,
        }
        .encode();
        self.broadcast(t, &msg);
        for p in 0..self.cfg.players {
            if p != self.cfg.local_player {
                self.check_hash(p, frame);
            }
        }
        // Keep a bounded history.
        let keep_from = frame.saturating_sub(self.cfg.hash_interval * 64);
        self.local_hashes = self.local_hashes.split_off(&keep_from);
        for m in &mut self.remote_hashes {
            *m = m.split_off(&keep_from);
        }
    }

    /// One full tick: advance if possible, simulate the game, hash, report.
    ///
    /// This is what runtimes call once per 60 Hz tick.
    pub fn step<G: Game>(
        &mut self,
        game: &mut G,
        apu: &mut Apu,
        local_input: Input,
        t: &mut impl Transport,
    ) -> StepResult {
        if let Some(d) = self.desync {
            return StepResult::Desynced(d);
        }
        let Some(inputs) = self.advance(local_input, t) else {
            if let Some(d) = self.desync {
                return StepResult::Desynced(d);
            }
            return StepResult::Waiting(self.waiting_on());
        };
        let frame = self.frame - 1;
        let mut ctx = UpdateCtx::new(inputs, frame, self.cfg.players, apu);
        game.update(&mut ctx);
        let mut h = StateHasher::new();
        game.hash(&mut h);
        self.report_hash(frame, h.finish(), t);
        match self.desync {
            Some(d) => StepResult::Desynced(d),
            None => StepResult::Advanced(frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::LoopbackNetwork;

    /// A game whose state is the sum of all inputs ever seen, per player.
    #[derive(Clone)]
    struct Sum {
        totals: [u32; MAX_PLAYERS],
        frames: u32,
        corrupt: bool,
    }
    impl Game for Sum {
        fn new(_seed: u32) -> Self {
            Sum {
                totals: [0; MAX_PLAYERS],
                frames: 0,
                corrupt: false,
            }
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            for (t, i) in self.totals.iter_mut().zip(ctx.inputs.iter()) {
                *t += u32::from(i.bits());
            }
            self.frames += 1;
            if self.corrupt && ctx.frame == 100 {
                self.totals[0] += 1;
            }
        }
        fn draw(&self, _gpu: &mut crate::gpu::Gpu) {}
        fn hash(&self, h: &mut StateHasher) {
            for t in &self.totals {
                h.write_u32(*t);
            }
            h.write_u32(self.frames);
        }
    }

    fn cfg(local: u8, players: u8) -> LockstepConfig {
        LockstepConfig {
            local_player: local,
            players,
            ..Default::default()
        }
    }

    fn run_session(
        players: u8,
        frames: u32,
        latency: u32,
        jitter: u32,
        loss: u32,
        corrupt_player: Option<u8>,
    ) -> (Vec<Sum>, Vec<Lockstep>, Vec<Option<Desync>>) {
        let net = LoopbackNetwork::new(players, 99, latency, jitter, loss);
        let mut games: Vec<Sum> = (0..players).map(|_| Sum::new(1)).collect();
        if let Some(p) = corrupt_player {
            games[p as usize].corrupt = true;
        }
        let mut apus: Vec<Apu> = (0..players).map(|_| Apu::new()).collect();
        let mut sessions: Vec<Lockstep> = (0..players)
            .map(|p| Lockstep::new(cfg(p, players)))
            .collect();
        let mut endpoints: Vec<_> = (0..players).map(|p| net.endpoint(p)).collect();
        let mut desyncs = vec![None; players as usize];
        let mut ticks = 0;
        while sessions.iter().any(|s| s.frame() < frames) && ticks < frames * 50 {
            for p in 0..players as usize {
                if sessions[p].frame() >= frames {
                    continue;
                }
                let input = Input::from_bits((p as u8 + 1) * (sessions[p].frame() as u8 % 7));
                match sessions[p].step(&mut games[p], &mut apus[p], input, &mut endpoints[p]) {
                    StepResult::Desynced(d) => desyncs[p] = Some(d),
                    StepResult::Advanced(_) | StepResult::Waiting(_) => {}
                }
            }
            net.tick();
            ticks += 1;
        }
        (games, sessions, desyncs)
    }

    #[test]
    fn two_players_stay_in_sync_on_perfect_network() {
        let (games, sessions, desyncs) = run_session(2, 300, 1, 0, 0, None);
        assert_eq!(games[0].totals, games[1].totals);
        assert_eq!(games[0].frames, 300);
        assert!(desyncs.iter().all(Option::is_none));
        assert_eq!(sessions[0].stats().frames, 300);
        assert!(sessions[0].desync().is_none());
        // Inputs of both players were actually mixed in.
        assert!(games[0].totals[0] > 0 && games[0].totals[1] > 0);
    }

    #[test]
    fn four_players_with_jitter_and_loss_converge() {
        let (games, sessions, desyncs) = run_session(4, 240, 2, 3, 150, None);
        for g in &games[1..] {
            assert_eq!(g.totals, games[0].totals);
            assert_eq!(g.frames, 240);
        }
        assert!(desyncs.iter().all(Option::is_none));
        assert!(
            sessions[0].stats().stalls > 0,
            "loss should cause some stalls"
        );
        assert!(sessions[0].stats().sent > 0 && sessions[0].stats().received > 0);
    }

    #[test]
    fn desync_is_detected_at_the_right_frame() {
        let (_games, _sessions, desyncs) = run_session(2, 200, 1, 0, 0, Some(1));
        // Corruption happens at frame 100; hashes exchange every 30 frames,
        // so the first disagreeing checkpoint is frame 120.
        for d in &desyncs {
            let d = d.expect("both peers detect the desync");
            assert_eq!(d.frame, 120);
            assert_ne!(d.local_hash, d.remote_hash);
            assert!(d.to_string().contains("frame 120"));
        }
        assert_eq!(desyncs[0].unwrap().remote_player, 1);
        assert_eq!(desyncs[1].unwrap().remote_player, 0);
    }

    #[test]
    fn single_player_never_waits() {
        let net = LoopbackNetwork::new(1, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Lockstep::new(LockstepConfig {
            players: 1,
            input_delay: 0,
            ..cfg(0, 1)
        });
        let mut g = Sum::new(0);
        let mut apu = Apu::new();
        for f in 0..10 {
            assert_eq!(
                s.step(&mut g, &mut apu, Input::A, &mut ep),
                StepResult::Advanced(f)
            );
        }
        assert_eq!(g.totals[0], 16 * 10);
        assert_eq!(s.stats().sent, 0);
        assert_eq!(s.waiting_on(), Vec::<u8>::new());
    }

    #[test]
    fn input_delay_is_honored() {
        let net = LoopbackNetwork::new(1, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Lockstep::new(LockstepConfig {
            players: 1,
            input_delay: 3,
            ..cfg(0, 1)
        });
        let mut seen = Vec::new();
        for _ in 0..6 {
            let inputs = s.advance(Input::B, &mut ep).unwrap();
            seen.push(inputs[0]);
        }
        assert_eq!(
            seen,
            vec![
                Input::NONE,
                Input::NONE,
                Input::NONE,
                Input::B,
                Input::B,
                Input::B
            ]
        );
        assert_eq!(s.last_inputs()[0], Input::B);
    }

    #[test]
    fn waiting_reports_missing_players_and_resends() {
        let net = LoopbackNetwork::new(2, 1, 0, 0, 1000); // everything is lost
        let mut ep = net.endpoint(0);
        let mut s = Lockstep::new(LockstepConfig {
            input_delay: 1,
            ..cfg(0, 2)
        });
        let mut g = Sum::new(0);
        let mut apu = Apu::new();
        assert_eq!(
            s.step(&mut g, &mut apu, Input::A, &mut ep),
            StepResult::Advanced(0)
        );
        for _ in 0..6 {
            assert_eq!(
                s.step(&mut g, &mut apu, Input::A, &mut ep),
                StepResult::Waiting(vec![1])
            );
        }
        assert_eq!(s.stats().stalls, 6);
        // 1 input window + 1 hash for frame 0 + 2 resends (after stalls 3 and 6).
        assert_eq!(s.stats().sent, 4);
        assert_eq!(s.waiting_on(), vec![1]);
    }

    #[test]
    fn rejects_bad_senders_and_garbage() {
        let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Lockstep::new(cfg(0, 2));
        s.handle(Message::Inputs {
            from: 0,
            last_frame: 5,
            inputs: vec![Input::A],
        }); // from self
        s.handle(Message::Inputs {
            from: 7,
            last_frame: 5,
            inputs: vec![Input::A],
        }); // unknown
        s.handle(Message::Hash {
            from: 0,
            frame: 0,
            hash: 1,
        });
        s.handle(Message::Hash {
            from: 9,
            frame: 0,
            hash: 1,
        });
        assert_eq!(s.stats().rejected, 4);
        let mut other = net.endpoint(1);
        other.send(0, &[0xFE, 1, 2]);
        net.tick();
        s.poll(&mut ep);
        assert_eq!(s.stats().rejected, 5);
        assert_eq!(s.config().players, 2);
    }

    #[test]
    fn late_duplicate_inputs_do_not_overwrite() {
        let mut s = Lockstep::new(LockstepConfig {
            input_delay: 0,
            ..cfg(0, 2)
        });
        s.handle(Message::Inputs {
            from: 1,
            last_frame: 0,
            inputs: vec![Input::A],
        });
        s.handle(Message::Inputs {
            from: 1,
            last_frame: 0,
            inputs: vec![Input::B],
        });
        let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let inputs = s.advance(Input::NONE, &mut ep).unwrap();
        assert_eq!(inputs[1], Input::A);
    }

    #[test]
    fn hash_interval_zero_disables_checks() {
        let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Lockstep::new(LockstepConfig {
            hash_interval: 0,
            ..cfg(0, 2)
        });
        s.report_hash(0, 123, &mut ep);
        s.handle(Message::Hash {
            from: 1,
            frame: 0,
            hash: 456,
        });
        assert!(s.desync().is_none());
        assert_eq!(s.stats().sent, 0);
    }

    #[test]
    fn step_after_desync_keeps_reporting_it() {
        let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Lockstep::new(LockstepConfig {
            input_delay: 0,
            hash_interval: 1,
            ..cfg(0, 2)
        });
        let mut g = Sum::new(0);
        let mut apu = Apu::new();
        s.handle(Message::Inputs {
            from: 1,
            last_frame: 0,
            inputs: vec![Input::NONE],
        });
        s.handle(Message::Hash {
            from: 1,
            frame: 0,
            hash: 0,
        });
        let r = s.step(&mut g, &mut apu, Input::NONE, &mut ep);
        assert!(matches!(r, StepResult::Desynced(d) if d.frame == 0));
        assert!(matches!(
            s.step(&mut g, &mut apu, Input::NONE, &mut ep),
            StepResult::Desynced(_)
        ));
        assert!(s.advance(Input::NONE, &mut ep).is_none());
    }

    #[test]
    #[should_panic(expected = "local_player out of range")]
    fn config_validation() {
        let _ = Lockstep::new(cfg(2, 2));
    }
}
