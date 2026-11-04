//! Rollback netcode: predict, simulate ahead, correct.

use super::wire::{Message, MAX_WINDOW};
use super::{Desync, StepResult, Transport};
use crate::apu::Apu;
use crate::game::{Game, UpdateCtx, MAX_PLAYERS};
use crate::hash::StateHasher;
use crate::input::Input;
use std::collections::{BTreeMap, VecDeque};

/// Rollback session parameters. All peers must agree on `players`,
/// `input_delay`, `max_prediction` and `hash_interval`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackConfig {
    /// This peer's player index.
    pub local_player: u8,
    /// Total players (`1..=MAX_PLAYERS`).
    pub players: u8,
    /// Frames of local input delay (`0..=32`). 1 is a good default.
    pub input_delay: u8,
    /// How many frames the simulation may run ahead of confirmed inputs
    /// before stalling (`1..=32`). 8 covers ~130 ms of one-way latency.
    pub max_prediction: u8,
    /// Frames between state-hash exchanges on confirmed frames. `0` disables.
    pub hash_interval: u32,
    /// Recent inputs repeated in each input message (`1..=32`).
    pub redundancy: u8,
}

impl Default for RollbackConfig {
    fn default() -> Self {
        RollbackConfig {
            local_player: 0,
            players: 2,
            input_delay: 1,
            max_prediction: 8,
            hash_interval: 30,
            redundancy: 12,
        }
    }
}

/// Counters for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RollbackStats {
    /// Frames simulated for the first time (excluding re-simulation).
    pub frames: u32,
    /// Frames confirmed (all inputs known).
    pub confirmed: u32,
    /// Times a snapshot was restored because a prediction was wrong.
    pub rollbacks: u32,
    /// Frames re-simulated during rollbacks.
    pub resimulated: u32,
    /// Ticks that could not advance because prediction was maxed out.
    pub stalls: u32,
    /// Messages sent.
    pub sent: u64,
    /// Messages received and understood.
    pub received: u64,
    /// Messages ignored as malformed or mis-addressed.
    pub rejected: u64,
}

/// A rollback session for one peer.
///
/// Unlike [`super::Lockstep`], the session owns the game and the sound
/// processor, because it must be able to replace them with snapshots.
pub struct Rollback<G: Game> {
    cfg: RollbackConfig,
    game: G,
    apu: Apu,
    /// Next frame to simulate (one past the last simulated frame).
    frame: u32,
    /// All inputs are known for every frame below this one.
    confirmed: u32,
    /// Known inputs per player.
    inputs: Vec<BTreeMap<u32, Input>>,
    /// Inputs actually used to simulate each not-yet-confirmed frame.
    used: BTreeMap<u32, [Input; MAX_PLAYERS]>,
    /// Game state *before* each not-yet-confirmed frame.
    snapshots: VecDeque<(u32, G)>,
    /// Hash after each simulated frame (rewritten on re-simulation).
    frame_hashes: BTreeMap<u32, u64>,
    local_hashes: BTreeMap<u32, u64>,
    remote_hashes: Vec<BTreeMap<u32, u64>>,
    desync: Option<Desync>,
    stats: RollbackStats,
    /// Confirmed frames not yet drained by the caller (for replays):
    /// `(frame, inputs, state hash after the frame)`.
    confirmed_queue: VecDeque<(u32, [Input; MAX_PLAYERS], u64)>,
}

impl<G: Game> Rollback<G> {
    /// Starts a session owning `game`.
    ///
    /// # Panics
    /// Panics if the configuration is out of range.
    pub fn new(cfg: RollbackConfig, game: G) -> Rollback<G> {
        assert!(
            cfg.players >= 1 && (cfg.players as usize) <= MAX_PLAYERS,
            "players"
        );
        assert!(cfg.local_player < cfg.players, "local_player out of range");
        assert!(
            (cfg.input_delay as usize) <= MAX_WINDOW,
            "input_delay too large"
        );
        assert!(
            cfg.max_prediction >= 1 && (cfg.max_prediction as usize) <= MAX_WINDOW,
            "max_prediction must be 1..=32"
        );
        assert!(
            cfg.redundancy >= 1 && (cfg.redundancy as usize) <= MAX_WINDOW,
            "redundancy"
        );
        let mut inputs: Vec<BTreeMap<u32, Input>> =
            (0..cfg.players).map(|_| BTreeMap::new()).collect();
        for map in &mut inputs {
            for f in 0..u32::from(cfg.input_delay) {
                map.insert(f, Input::NONE);
            }
        }
        Rollback {
            cfg,
            game,
            apu: Apu::new(),
            frame: 0,
            confirmed: 0,
            inputs,
            used: BTreeMap::new(),
            snapshots: VecDeque::new(),
            frame_hashes: BTreeMap::new(),
            local_hashes: BTreeMap::new(),
            remote_hashes: (0..cfg.players).map(|_| BTreeMap::new()).collect(),
            desync: None,
            stats: RollbackStats::default(),
            confirmed_queue: VecDeque::new(),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &RollbackConfig {
        &self.cfg
    }

    /// The game in its current (possibly predicted) state. Draw this.
    pub fn game(&self) -> &G {
        &self.game
    }

    /// The sound processor.
    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// Next frame to simulate.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Frames whose inputs are all known.
    pub fn confirmed_frame(&self) -> u32 {
        self.confirmed
    }

    /// Diagnostics.
    pub fn stats(&self) -> RollbackStats {
        self.stats
    }

    /// The detected desync, if any.
    pub fn desync(&self) -> Option<Desync> {
        self.desync
    }

    /// Remote players whose input for the oldest unconfirmed frame is missing.
    pub fn waiting_on(&self) -> Vec<u8> {
        (0..self.cfg.players)
            .filter(|&p| p != self.cfg.local_player)
            .filter(|&p| !self.inputs[p as usize].contains_key(&self.confirmed))
            .collect()
    }

    /// Takes the confirmed frames recorded since the last call, in order,
    /// each with the state hash after that frame.
    pub fn drain_confirmed(&mut self) -> Vec<(u32, [Input; MAX_PLAYERS], u64)> {
        self.confirmed_queue.drain(..).collect()
    }

    /// Drains the transport.
    pub fn poll(&mut self, t: &mut impl Transport) {
        while let Some(data) = t.recv() {
            match Message::decode(&data) {
                Some(m) => self.handle(m),
                None => self.stats.rejected += 1,
            }
        }
    }

    /// Applies one decoded message.
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
                    if f >= self.confirmed {
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

    fn broadcast(&mut self, t: &mut impl Transport, data: &[u8]) {
        for p in 0..self.cfg.players {
            if p != self.cfg.local_player {
                t.send(p, data);
                self.stats.sent += 1;
            }
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

    /// Best-known input for a player on a frame: the real one if it has
    /// arrived, otherwise the most recent one we have (or none).
    fn best_input(&self, player: usize, frame: u32) -> (Input, bool) {
        let map = &self.inputs[player];
        if let Some(&i) = map.get(&frame) {
            return (i, true);
        }
        let predicted = map
            .range(..frame)
            .next_back()
            .map_or(Input::NONE, |(_, &i)| i);
        (predicted, false)
    }

    fn inputs_for(&self, frame: u32) -> ([Input; MAX_PLAYERS], bool) {
        let mut out = [Input::NONE; MAX_PLAYERS];
        let mut all_known = true;
        for (p, slot) in out.iter_mut().enumerate().take(self.cfg.players as usize) {
            let (i, known) = self.best_input(p, frame);
            *slot = i;
            all_known &= known;
        }
        (out, all_known)
    }

    fn simulate(&mut self, frame: u32, inputs: [Input; MAX_PLAYERS], live_audio: bool) {
        self.snapshots.push_back((frame, self.game.clone()));
        if live_audio {
            let mut ctx = UpdateCtx::new(inputs, frame, self.cfg.players, &mut self.apu);
            self.game.update(&mut ctx);
        } else {
            let mut scratch = self.apu.clone();
            let mut ctx = UpdateCtx::new(inputs, frame, self.cfg.players, &mut scratch);
            self.game.update(&mut ctx);
        }
        let mut h = StateHasher::new();
        self.game.hash(&mut h);
        self.frame_hashes.insert(frame, h.finish());
        self.used.insert(frame, inputs);
    }

    /// Re-simulates from the first mispredicted frame, if any.
    fn reconcile(&mut self) {
        let mut first_wrong: Option<u32> = None;
        for (&f, used) in &self.used {
            let (actual, _) = self.inputs_for(f);
            if actual != *used {
                first_wrong = Some(f);
                break;
            }
        }
        let Some(start) = first_wrong else { return };
        // Restore the snapshot taken before `start`.
        while let Some((f, _)) = self.snapshots.back() {
            if *f > start {
                self.snapshots.pop_back();
            } else {
                break;
            }
        }
        let Some((f, state)) = self.snapshots.pop_back() else {
            return;
        };
        debug_assert_eq!(f, start);
        self.game = state;
        self.stats.rollbacks += 1;
        let end = self.frame;
        for frame in start..end {
            let (inputs, _) = self.inputs_for(frame);
            self.simulate(frame, inputs, false);
            self.stats.resimulated += 1;
        }
    }

    /// Advances `confirmed` as far as inputs allow and reports hashes.
    fn confirm(&mut self, t: &mut impl Transport) {
        while self.confirmed < self.frame {
            let f = self.confirmed;
            let (inputs, all_known) = self.inputs_for(f);
            if !all_known {
                break;
            }
            self.confirmed += 1;
            self.stats.confirmed += 1;
            let hash = self.frame_hashes.get(&f).copied().unwrap_or(0);
            self.confirmed_queue.push_back((f, inputs, hash));
            self.used.remove(&f);
            while matches!(self.snapshots.front(), Some((sf, _)) if *sf <= f) {
                self.snapshots.pop_front();
            }
            if self.cfg.hash_interval > 0 && f % self.cfg.hash_interval == 0 {
                if let Some(&hash) = self.frame_hashes.get(&f) {
                    self.local_hashes.insert(f, hash);
                    let msg = Message::Hash {
                        from: self.cfg.local_player,
                        frame: f,
                        hash,
                    }
                    .encode();
                    self.broadcast(t, &msg);
                    for p in 0..self.cfg.players {
                        if p != self.cfg.local_player {
                            self.check_hash(p, f);
                        }
                    }
                }
            }
            self.frame_hashes.remove(&f);
        }
        // Bounded histories.
        let keep_from = self
            .confirmed
            .saturating_sub(u32::from(self.cfg.redundancy) + 1);
        for map in &mut self.inputs {
            *map = map.split_off(&keep_from);
        }
        let hash_keep = self
            .confirmed
            .saturating_sub(self.cfg.hash_interval.max(1) * 64);
        self.local_hashes = self.local_hashes.split_off(&hash_keep);
        for m in &mut self.remote_hashes {
            *m = m.split_off(&hash_keep);
        }
    }

    /// One tick: receive, reconcile, confirm, then simulate one more frame
    /// with the best-known inputs, or stall if too far ahead.
    pub fn step(&mut self, local_input: Input, t: &mut impl Transport) -> StepResult {
        if let Some(d) = self.desync {
            return StepResult::Desynced(d);
        }
        self.poll(t);
        self.reconcile();
        self.confirm(t);
        if let Some(d) = self.desync {
            return StepResult::Desynced(d);
        }
        if self.frame - self.confirmed >= u32::from(self.cfg.max_prediction) {
            self.stats.stalls += 1;
            // Keep peers fed while we wait.
            self.send_input_window(t);
            return StepResult::Waiting(self.waiting_on());
        }
        let target = self.frame + u32::from(self.cfg.input_delay);
        self.inputs[self.cfg.local_player as usize]
            .entry(target)
            .or_insert(local_input);
        self.send_input_window(t);
        let frame = self.frame;
        let (inputs, _) = self.inputs_for(frame);
        self.simulate(frame, inputs, true);
        self.frame += 1;
        self.stats.frames += 1;
        // Inputs may already be complete (single player, or everything arrived).
        self.confirm(t);
        match self.desync {
            Some(d) => StepResult::Desynced(d),
            None => StepResult::Advanced(frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Lockstep, LockstepConfig, LoopbackNetwork};

    /// Inputs matter and history matters: a tiny LCG mixed with inputs.
    #[derive(Clone)]
    struct Mix {
        v: u32,
        frames: u32,
        corrupt: bool,
    }
    impl Game for Mix {
        fn new(seed: u32) -> Self {
            Mix {
                v: seed,
                frames: 0,
                corrupt: false,
            }
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            let mut x = self.v.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            for (p, i) in ctx.inputs.iter().enumerate() {
                x = x.wrapping_add(u32::from(i.bits()) << (p * 8));
            }
            self.v = x;
            self.frames += 1;
            if self.corrupt && ctx.frame == 50 {
                self.v ^= 1;
            }
        }
        fn draw(&self, _gpu: &mut crate::gpu::Gpu) {}
        fn hash(&self, h: &mut StateHasher) {
            h.write_u32(self.v);
            h.write_u32(self.frames);
        }
    }

    fn input_for(player: usize, frame: u32) -> Input {
        // Changes every few frames so predictions are sometimes wrong.
        Input::from_bits(((frame / 3 + player as u32 * 7) % 17) as u8)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_rollback(
        players: u8,
        frames: u32,
        latency: u32,
        jitter: u32,
        loss: u32,
        corrupt: Option<u8>,
        delay: u8,
        max_prediction: u8,
    ) -> (Vec<Rollback<Mix>>, Vec<Option<Desync>>, u32) {
        let net = LoopbackNetwork::new(players, 5, latency, jitter, loss);
        let mut sessions: Vec<Rollback<Mix>> = (0..players)
            .map(|p| {
                let mut g = Mix::new(3);
                g.corrupt = corrupt == Some(p);
                Rollback::new(
                    RollbackConfig {
                        local_player: p,
                        players,
                        input_delay: delay,
                        max_prediction,
                        hash_interval: 10,
                        ..RollbackConfig::default()
                    },
                    g,
                )
            })
            .collect();
        let mut endpoints: Vec<_> = (0..players).map(|p| net.endpoint(p)).collect();
        let mut desyncs = vec![None; players as usize];
        let mut ticks = 0;
        while sessions.iter().any(|s| s.confirmed_frame() < frames) && ticks < frames * 50 {
            for p in 0..players as usize {
                if sessions[p].desync().is_some() {
                    continue;
                }
                let r = sessions[p].step(input_for(p, ticks), &mut endpoints[p]);
                if let StepResult::Desynced(d) = r {
                    desyncs[p] = Some(d);
                }
            }
            net.tick();
            ticks += 1;
            if desyncs.iter().any(Option::is_some) {
                break;
            }
        }
        (sessions, desyncs, ticks)
    }

    /// Reference: simulate with the exact inputs each player would have had
    /// confirmed, to compare against the rollback result.
    fn reference(frames: u32, confirmed: &[(u32, [Input; MAX_PLAYERS], u64)]) -> u64 {
        let mut g = Mix::new(3);
        let mut apu = Apu::new();
        for &(f, inputs, _) in confirmed.iter().take(frames as usize) {
            let mut ctx = UpdateCtx::new(inputs, f, 2, &mut apu);
            g.update(&mut ctx);
        }
        let mut h = StateHasher::new();
        g.hash(&mut h);
        h.finish()
    }

    #[test]
    fn peers_converge_with_latency_and_predictions_are_corrected() {
        let (mut s, desyncs, _) = run_rollback(2, 300, 4, 2, 100, None, 1, 8);
        assert!(desyncs.iter().all(Option::is_none), "{desyncs:?}");
        let st = s[0].stats();
        assert!(
            st.rollbacks > 0,
            "latency must have caused mispredictions: {st:?}"
        );
        assert!(st.resimulated >= st.rollbacks);
        assert!(st.confirmed >= 300);
        // Both peers agree on every confirmed frame, and on a reference sim.
        let c0 = s[0].drain_confirmed();
        let c1 = s[1].drain_confirmed();
        let n = c0.len().min(c1.len());
        assert!(n >= 300);
        assert_eq!(&c0[..n], &c1[..n]);
        let h0 = reference(300, &c0);
        let h1 = reference(300, &c1);
        assert_eq!(h0, h1);
        // The hash reported with the 300th confirmed frame is that reference.
        assert_eq!(c0[299].2, h0);
        assert!(
            s[0].stats().stalls < s[0].stats().frames / 4,
            "{:?}",
            s[0].stats()
        );
    }

    #[test]
    fn rollback_stalls_far_less_than_lockstep_at_the_same_latency() {
        let latency = 6;
        let (s, _, _) = run_rollback(2, 240, latency, 0, 0, None, 1, 10);
        let rollback_stalls = s[0].stats().stalls;

        // Lockstep with the same input delay on the same network.
        let net = LoopbackNetwork::new(2, 5, latency, 0, 0);
        let mut games = [Mix::new(3), Mix::new(3)];
        let mut apus = [Apu::new(), Apu::new()];
        let mut ls = [
            Lockstep::new(LockstepConfig {
                local_player: 0,
                input_delay: 1,
                ..LockstepConfig::default()
            }),
            Lockstep::new(LockstepConfig {
                local_player: 1,
                input_delay: 1,
                ..LockstepConfig::default()
            }),
        ];
        let mut eps = [net.endpoint(0), net.endpoint(1)];
        let mut ticks = 0;
        while ls.iter().any(|l| l.frame() < 240) && ticks < 240 * 50 {
            for p in 0..2 {
                if ls[p].frame() < 240 {
                    let _ = ls[p].step(
                        &mut games[p],
                        &mut apus[p],
                        input_for(p, ticks),
                        &mut eps[p],
                    );
                }
            }
            net.tick();
            ticks += 1;
        }
        let lockstep_stalls = ls[0].stats().stalls;
        assert!(lockstep_stalls > 0);
        assert!(
            rollback_stalls * 4 < lockstep_stalls,
            "rollback {rollback_stalls} vs lockstep {lockstep_stalls}"
        );
    }

    #[test]
    fn four_players_with_loss() {
        let (mut s, desyncs, _) = run_rollback(4, 150, 3, 3, 150, None, 1, 8);
        assert!(desyncs.iter().all(Option::is_none));
        let c0 = s[0].drain_confirmed();
        for other in s.iter_mut().skip(1) {
            let c = other.drain_confirmed();
            let n = c.len().min(c0.len()).min(150);
            assert_eq!(&c[..n], &c0[..n]);
        }
    }

    #[test]
    fn desync_is_detected_on_confirmed_frames() {
        let (_, desyncs, _) = run_rollback(2, 200, 2, 0, 0, Some(1), 1, 8);
        for d in &desyncs {
            let d = d.expect("both peers detect the desync");
            // Corruption at frame 50, hashes every 10 frames: first bad checkpoint is 50.
            assert_eq!(d.frame, 50);
        }
    }

    #[test]
    fn max_prediction_bounds_how_far_ahead_we_run() {
        // Everything lost: we predict up to the limit, then wait.
        let net = LoopbackNetwork::new(2, 1, 0, 0, 1000);
        let mut ep = net.endpoint(0);
        let mut s = Rollback::new(
            RollbackConfig {
                max_prediction: 5,
                input_delay: 0,
                ..RollbackConfig::default()
            },
            Mix::new(1),
        );
        for f in 0..5 {
            assert_eq!(s.step(Input::A, &mut ep), StepResult::Advanced(f));
        }
        assert_eq!(s.step(Input::A, &mut ep), StepResult::Waiting(vec![1]));
        assert_eq!(s.frame(), 5);
        assert_eq!(s.confirmed_frame(), 0);
        assert_eq!(s.stats().stalls, 1);
        assert_eq!(s.waiting_on(), vec![1]);
        assert_eq!(s.drain_confirmed().len(), 0);
        // Now the remote's inputs for 0..5 arrive and differ from the prediction (NONE).
        s.handle(Message::Inputs {
            from: 1,
            last_frame: 4,
            inputs: vec![Input::B; 5],
        });
        let r = s.step(Input::A, &mut ep);
        assert_eq!(r, StepResult::Advanced(5));
        assert_eq!(s.stats().rollbacks, 1);
        assert_eq!(s.stats().resimulated, 5);
        assert_eq!(s.confirmed_frame(), 5);
        let confirmed = s.drain_confirmed();
        assert_eq!(confirmed.len(), 5);
        assert_eq!(confirmed[0].1[1], Input::B);
        assert_eq!(confirmed[0].1[0], Input::A);
        assert_eq!(s.config().max_prediction, 5);
    }

    #[test]
    fn single_player_confirms_immediately_and_never_rolls_back() {
        let net = LoopbackNetwork::new(1, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Rollback::new(
            RollbackConfig {
                players: 1,
                input_delay: 0,
                ..RollbackConfig::default()
            },
            Mix::new(9),
        );
        for f in 0..20 {
            assert_eq!(
                s.step(Input::from_bits(f as u8), &mut ep),
                StepResult::Advanced(f)
            );
        }
        assert_eq!(s.confirmed_frame(), 20);
        assert_eq!(s.stats().rollbacks, 0);
        assert_eq!(s.stats().sent, 0);
        assert_eq!(s.game().frames, 20);
        s.apu_mut().set_master_volume(1);
        assert!(s.desync().is_none());
    }

    #[test]
    fn rejects_bad_senders_and_keeps_reporting_desync() {
        let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
        let mut ep = net.endpoint(0);
        let mut s = Rollback::new(
            RollbackConfig {
                input_delay: 0,
                hash_interval: 1,
                ..RollbackConfig::default()
            },
            Mix::new(1),
        );
        s.handle(Message::Inputs {
            from: 0,
            last_frame: 0,
            inputs: vec![Input::A],
        });
        s.handle(Message::Inputs {
            from: 5,
            last_frame: 0,
            inputs: vec![Input::A],
        });
        s.handle(Message::Hash {
            from: 0,
            frame: 0,
            hash: 1,
        });
        s.handle(Message::Hash {
            from: 7,
            frame: 0,
            hash: 1,
        });
        assert_eq!(s.stats().rejected, 4);
        let mut other = net.endpoint(1);
        other.send(0, &[0xEE]);
        net.tick();
        s.poll(&mut ep);
        assert_eq!(s.stats().rejected, 5);
        // A wrong remote hash for frame 0 shows up once frame 0 confirms.
        s.handle(Message::Inputs {
            from: 1,
            last_frame: 0,
            inputs: vec![Input::NONE],
        });
        s.handle(Message::Hash {
            from: 1,
            frame: 0,
            hash: 0xBAD,
        });
        assert!(matches!(s.step(Input::NONE, &mut ep), StepResult::Desynced(d) if d.frame == 0));
        assert!(matches!(
            s.step(Input::NONE, &mut ep),
            StepResult::Desynced(_)
        ));
    }

    #[test]
    #[should_panic(expected = "max_prediction")]
    fn config_validation() {
        let _ = Rollback::new(
            RollbackConfig {
                max_prediction: 0,
                ..RollbackConfig::default()
            },
            Mix::new(1),
        );
    }
}
