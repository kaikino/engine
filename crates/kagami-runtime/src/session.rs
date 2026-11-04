//! A running match: local, replayed, or networked.

use kagami::apu::Apu;
use kagami::net::{
    Desync, Lockstep, LockstepConfig, LoopbackEndpoint, LoopbackNetwork, Rollback, RollbackConfig,
    StepResult, UdpTransport,
};
use kagami::replay::Replay;
use kagami::{Game, Input, StateHasher, UpdateCtx, MAX_PLAYERS};

/// What happened during one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// The simulation advanced; this frame number was just simulated.
    Advanced(u32),
    /// The netcode is waiting for these remote players.
    Waiting(Vec<u8>),
    /// Peers disagree; the match is over.
    Desynced(Desync),
}

/// Network statistics common to both netcodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStats {
    /// Frames simulated.
    pub frames: u32,
    /// Ticks that waited on a remote peer.
    pub stalls: u32,
    /// Messages sent.
    pub sent: u64,
    /// Messages received.
    pub received: u64,
    /// Messages rejected.
    pub rejected: u64,
    /// Rollbacks performed (rollback netcode only).
    pub rollbacks: u32,
    /// Frames re-simulated (rollback netcode only).
    pub resimulated: u32,
    /// Frames confirmed (equals `frames` for lockstep).
    pub confirmed: u32,
}

impl From<kagami::net::LockstepStats> for NetStats {
    fn from(s: kagami::net::LockstepStats) -> Self {
        NetStats {
            frames: s.frames,
            stalls: s.stalls,
            sent: s.sent,
            received: s.received,
            rejected: s.rejected,
            rollbacks: 0,
            resimulated: 0,
            confirmed: s.frames,
        }
    }
}

impl From<kagami::net::RollbackStats> for NetStats {
    fn from(s: kagami::net::RollbackStats) -> Self {
        NetStats {
            frames: s.frames,
            stalls: s.stalls,
            sent: s.sent,
            received: s.received,
            rejected: s.rejected,
            rollbacks: s.rollbacks,
            resimulated: s.resimulated,
            confirmed: s.confirmed,
        }
    }
}

enum Kind<G: Game> {
    Local {
        game: G,
        frame: u32,
        source: Option<Replay>,
    },
    Udp {
        game: G,
        lockstep: Lockstep,
        transport: UdpTransport,
    },
    Loopback {
        games: Vec<G>,
        lockstep: Vec<Lockstep>,
        endpoints: Vec<LoopbackEndpoint>,
        net: LoopbackNetwork,
        apu2: Apu,
    },
    RollbackUdp {
        session: Rollback<G>,
        transport: UdpTransport,
    },
    RollbackLoopback {
        sessions: Vec<Rollback<G>>,
        endpoints: Vec<LoopbackEndpoint>,
        net: LoopbackNetwork,
    },
}

/// A match in progress. Owns the game(s), the sound processor and a replay
/// of everything that has happened.
pub struct Session<G: Game> {
    kind: Kind<G>,
    apu: Apu,
    players: u8,
    replay: Replay,
    desync: Option<Desync>,
}

impl<G: Game> Session<G> {
    /// A local match. With `source`, inputs come from that replay instead of
    /// the caller.
    pub fn local(seed: u32, players: u8, source: Option<Replay>) -> Session<G> {
        Session {
            kind: Kind::Local {
                game: G::new(seed),
                frame: 0,
                source,
            },
            apu: Apu::new(),
            players,
            replay: Replay::new(seed, players),
            desync: None,
        }
    }

    /// A lockstep match over UDP.
    pub fn udp(seed: u32, cfg: LockstepConfig, transport: UdpTransport) -> Session<G> {
        Session {
            kind: Kind::Udp {
                game: G::new(seed),
                lockstep: Lockstep::new(cfg),
                transport,
            },
            apu: Apu::new(),
            players: cfg.players,
            replay: Replay::new(seed, cfg.players),
            desync: None,
        }
    }

    /// A rollback match over UDP.
    pub fn rollback_udp(seed: u32, cfg: RollbackConfig, transport: UdpTransport) -> Session<G> {
        Session {
            kind: Kind::RollbackUdp {
                session: Rollback::new(cfg, G::new(seed)),
                transport,
            },
            apu: Apu::new(),
            players: cfg.players,
            replay: Replay::new(seed, cfg.players),
            desync: None,
        }
    }

    /// Two lockstep peers in one process on a simulated network. Player 0's
    /// view is the one displayed; both are hashed and compared.
    pub fn loopback(
        seed: u32,
        delay: u8,
        latency: u32,
        jitter: u32,
        loss_permille: u32,
    ) -> Session<G> {
        let net = LoopbackNetwork::new(2, seed, latency, jitter, loss_permille);
        let cfg = |p| LockstepConfig {
            local_player: p,
            players: 2,
            input_delay: delay,
            ..LockstepConfig::default()
        };
        Session {
            kind: Kind::Loopback {
                games: vec![G::new(seed), G::new(seed)],
                lockstep: vec![Lockstep::new(cfg(0)), Lockstep::new(cfg(1))],
                endpoints: vec![net.endpoint(0), net.endpoint(1)],
                net,
                apu2: Apu::new(),
            },
            apu: Apu::new(),
            players: 2,
            replay: Replay::new(seed, 2),
            desync: None,
        }
    }

    /// Two rollback peers in one process on a simulated network.
    pub fn rollback_loopback(
        seed: u32,
        delay: u8,
        prediction: u8,
        latency: u32,
        jitter: u32,
        loss_permille: u32,
    ) -> Session<G> {
        let net = LoopbackNetwork::new(2, seed, latency, jitter, loss_permille);
        let cfg = |p| RollbackConfig {
            local_player: p,
            players: 2,
            input_delay: delay,
            max_prediction: prediction,
            ..RollbackConfig::default()
        };
        Session {
            kind: Kind::RollbackLoopback {
                sessions: vec![
                    Rollback::new(cfg(0), G::new(seed)),
                    Rollback::new(cfg(1), G::new(seed)),
                ],
                endpoints: vec![net.endpoint(0), net.endpoint(1)],
                net,
            },
            apu: Apu::new(),
            players: 2,
            replay: Replay::new(seed, 2),
            desync: None,
        }
    }

    /// Number of players in the match.
    pub fn players(&self) -> u8 {
        self.players
    }

    /// The displayed game. Under rollback this is the predicted state.
    pub fn game(&self) -> &G {
        match &self.kind {
            Kind::Local { game, .. } | Kind::Udp { game, .. } => game,
            Kind::Loopback { games, .. } => &games[0],
            Kind::RollbackUdp { session, .. } => session.game(),
            Kind::RollbackLoopback { sessions, .. } => sessions[0].game(),
        }
    }

    /// The sound processor for the displayed game.
    pub fn apu_mut(&mut self) -> &mut Apu {
        match &mut self.kind {
            Kind::RollbackUdp { session, .. } => session.apu_mut(),
            Kind::RollbackLoopback { sessions, .. } => sessions[0].apu_mut(),
            Kind::Local { .. } | Kind::Udp { .. } | Kind::Loopback { .. } => &mut self.apu,
        }
    }

    /// Frames simulated so far (including predicted frames under rollback).
    pub fn frame(&self) -> u32 {
        match &self.kind {
            Kind::Local { frame, .. } => *frame,
            Kind::Udp { lockstep, .. } => lockstep.frame(),
            Kind::Loopback { lockstep, .. } => lockstep[0].frame(),
            Kind::RollbackUdp { session, .. } => session.frame(),
            Kind::RollbackLoopback { sessions, .. } => sessions[0].frame(),
        }
    }

    /// `true` when the session uses rollback netcode.
    pub fn is_rollback(&self) -> bool {
        matches!(
            self.kind,
            Kind::RollbackUdp { .. } | Kind::RollbackLoopback { .. }
        )
    }

    /// Hash of the displayed game's state.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.game().hash(&mut h);
        h.finish()
    }

    /// Network statistics, for networked sessions.
    pub fn stats(&self) -> Option<NetStats> {
        match &self.kind {
            Kind::Local { .. } => None,
            Kind::Udp { lockstep, .. } => Some(lockstep.stats().into()),
            Kind::Loopback { lockstep, .. } => Some(lockstep[0].stats().into()),
            Kind::RollbackUdp { session, .. } => Some(session.stats().into()),
            Kind::RollbackLoopback { sessions, .. } => Some(sessions[0].stats().into()),
        }
    }

    /// The replay recorded so far. Under rollback it holds confirmed frames
    /// only.
    pub fn replay(&self) -> &Replay {
        &self.replay
    }

    /// The detected desync, if any.
    pub fn desync(&self) -> Option<Desync> {
        self.desync
    }

    /// `true` for sessions replaying a file whose inputs are exhausted.
    pub fn replay_finished(&self) -> bool {
        match &self.kind {
            Kind::Local {
                frame,
                source: Some(r),
                ..
            } => *frame as usize >= r.len(),
            _ => false,
        }
    }

    /// Advances one tick with the local controller state.
    pub fn tick(&mut self, local: [Input; MAX_PLAYERS]) -> SessionEvent {
        if let Some(d) = self.desync {
            return SessionEvent::Desynced(d);
        }
        let players = self.players;
        match &mut self.kind {
            Kind::Local {
                game,
                frame,
                source,
            } => {
                let inputs = match source {
                    Some(r) => r.inputs(*frame),
                    None => local,
                };
                let mut ctx = UpdateCtx::new(inputs, *frame, players, &mut self.apu);
                game.update(&mut ctx);
                self.replay.push(inputs);
                let f = *frame;
                *frame += 1;
                if f % 60 == 0 {
                    let mut h = StateHasher::new();
                    game.hash(&mut h);
                    self.replay.checkpoint(f, h.finish());
                }
                SessionEvent::Advanced(f)
            }
            Kind::Udp {
                game,
                lockstep,
                transport,
            } => {
                let local_player = lockstep.config().local_player as usize;
                let r = lockstep.step(
                    game,
                    &mut self.apu,
                    local[local_player.min(MAX_PLAYERS - 1)],
                    transport,
                );
                Self::lockstep_event(r, lockstep, game, &mut self.replay, &mut self.desync)
            }
            Kind::Loopback {
                games,
                lockstep,
                endpoints,
                net,
                apu2,
            } => {
                let (g0, g1) = games.split_at_mut(1);
                let (l0, l1) = lockstep.split_at_mut(1);
                let (e0, e1) = endpoints.split_at_mut(1);
                let r0 = l0[0].step(&mut g0[0], &mut self.apu, local[0], &mut e0[0]);
                let r1 = l1[0].step(&mut g1[0], apu2, local[1], &mut e1[0]);
                net.tick();
                if let StepResult::Desynced(d) = r1 {
                    self.desync = Some(d);
                    return SessionEvent::Desynced(d);
                }
                Self::lockstep_event(r0, &l0[0], &g0[0], &mut self.replay, &mut self.desync)
            }
            Kind::RollbackUdp { session, transport } => {
                let local_player = session.config().local_player as usize;
                let r = session.step(local[local_player.min(MAX_PLAYERS - 1)], transport);
                Self::rollback_event(r, session, &mut self.replay, &mut self.desync)
            }
            Kind::RollbackLoopback {
                sessions,
                endpoints,
                net,
            } => {
                let (s0, s1) = sessions.split_at_mut(1);
                let (e0, e1) = endpoints.split_at_mut(1);
                let r0 = s0[0].step(local[0], &mut e0[0]);
                let r1 = s1[0].step(local[1], &mut e1[0]);
                net.tick();
                if let StepResult::Desynced(d) = r1 {
                    self.desync = Some(d);
                    return SessionEvent::Desynced(d);
                }
                Self::rollback_event(r0, &mut s0[0], &mut self.replay, &mut self.desync)
            }
        }
    }

    fn lockstep_event(
        r: StepResult,
        lockstep: &Lockstep,
        game: &G,
        replay: &mut Replay,
        desync: &mut Option<Desync>,
    ) -> SessionEvent {
        match r {
            StepResult::Advanced(f) => {
                replay.push(lockstep.last_inputs());
                if f % 60 == 0 {
                    let mut h = StateHasher::new();
                    game.hash(&mut h);
                    replay.checkpoint(f, h.finish());
                }
                SessionEvent::Advanced(f)
            }
            StepResult::Waiting(w) => SessionEvent::Waiting(w),
            StepResult::Desynced(d) => {
                *desync = Some(d);
                SessionEvent::Desynced(d)
            }
        }
    }

    fn rollback_event(
        r: StepResult,
        session: &mut Rollback<G>,
        replay: &mut Replay,
        desync: &mut Option<Desync>,
    ) -> SessionEvent {
        for (f, inputs, hash) in session.drain_confirmed() {
            replay.push(inputs);
            if f % 60 == 0 {
                replay.checkpoint(f, hash);
            }
        }
        match r {
            StepResult::Advanced(f) => SessionEvent::Advanced(f),
            StepResult::Waiting(w) => SessionEvent::Waiting(w),
            StepResult::Desynced(d) => {
                *desync = Some(d);
                SessionEvent::Desynced(d)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagami::Gpu;

    #[derive(Clone)]
    struct Count(u32);
    impl Game for Count {
        fn new(seed: u32) -> Self {
            Count(seed)
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            self.0 = self
                .0
                .wrapping_mul(3)
                .wrapping_add(u32::from(ctx.input(0).bits()) + u32::from(ctx.input(1).bits()));
        }
        fn draw(&self, _gpu: &mut Gpu) {}
        fn hash(&self, h: &mut StateHasher) {
            h.write_u32(self.0);
        }
    }

    fn inputs(f: u32) -> [Input; MAX_PLAYERS] {
        let mut i = [Input::NONE; MAX_PLAYERS];
        i[0] = Input::from_bits((f % 5) as u8);
        i[1] = Input::from_bits((f % 3) as u8 * 16);
        i
    }

    #[test]
    fn local_records_replay_that_verifies() {
        let mut s: Session<Count> = Session::local(5, 2, None);
        for f in 0..130 {
            assert_eq!(s.tick(inputs(f)), SessionEvent::Advanced(f));
        }
        assert_eq!(s.frame(), 130);
        assert_eq!(s.players(), 2);
        assert!(s.stats().is_none());
        assert!(s.desync().is_none());
        assert!(!s.replay_finished());
        assert!(!s.is_rollback());
        let replay = s.replay().clone();
        assert_eq!(replay.len(), 130);
        assert_eq!(replay.checkpoints.len(), 3);
        assert_eq!(replay.verify::<Count>().unwrap(), s.state_hash());
        // Playing the replay back reproduces the state.
        let mut p: Session<Count> = Session::local(5, 2, Some(replay));
        for _ in 0..130 {
            p.tick([Input::A; MAX_PLAYERS]); // ignored: inputs come from the replay
        }
        assert!(p.replay_finished());
        assert_eq!(p.state_hash(), s.state_hash());
        assert_eq!(p.game().0, s.game().0);
        p.apu_mut().set_master_volume(3);
    }

    #[test]
    fn loopback_mirrors_and_matches_local() {
        let mut net: Session<Count> = Session::loopback(9, 2, 2, 1, 50);
        let mut advanced = 0;
        let mut ticks = 0;
        while advanced < 200 && ticks < 5000 {
            match net.tick(inputs(ticks)) {
                SessionEvent::Advanced(_) => advanced += 1,
                SessionEvent::Waiting(w) => assert_eq!(w, vec![1]),
                SessionEvent::Desynced(d) => panic!("{d}"),
            }
            ticks += 1;
        }
        assert_eq!(advanced, 200);
        assert_eq!(net.frame(), 200);
        let st = net.stats().unwrap();
        assert!(st.frames >= 200);
        assert_eq!(st.confirmed, st.frames);
        assert_eq!(st.rollbacks, 0);
        // The replay recorded from player 0's view verifies against a fresh sim.
        let replay = net.replay().clone();
        assert_eq!(replay.verify::<Count>().unwrap(), net.state_hash());
    }

    #[test]
    fn rollback_loopback_records_a_verifiable_replay() {
        let mut net: Session<Count> = Session::rollback_loopback(9, 1, 8, 5, 2, 100);
        assert!(net.is_rollback());
        let mut ticks = 0;
        while net.replay().len() < 240 && ticks < 5000 {
            match net.tick(inputs(ticks)) {
                SessionEvent::Desynced(d) => panic!("{d}"),
                SessionEvent::Advanced(_) | SessionEvent::Waiting(_) => {}
            }
            ticks += 1;
        }
        let st = net.stats().unwrap();
        assert!(st.rollbacks > 0, "{st:?}");
        assert!(st.confirmed >= 240);
        let replay = net.replay().clone();
        assert!(replay.checkpoints.len() >= 4);
        assert!(replay.verify::<Count>().is_ok());
        net.apu_mut().set_master_volume(2);
        assert!(net.frame() >= 240);
    }

    #[test]
    fn rollback_udp_single_peer_runs() {
        let t = UdpTransport::bind("127.0.0.1:0", &[None]).unwrap();
        let cfg = RollbackConfig {
            players: 1,
            input_delay: 0,
            ..RollbackConfig::default()
        };
        let mut s: Session<Count> = Session::rollback_udp(1, cfg, t);
        for f in 0..10 {
            assert_eq!(s.tick(inputs(f)), SessionEvent::Advanced(f));
        }
        assert_eq!(s.stats().unwrap().confirmed, 10);
        assert_eq!(s.replay().len(), 10);
        assert_eq!(s.frame(), 10);
        assert_eq!(
            s.game().0,
            s.replay().verify::<Count>().map(|_| s.game().0).unwrap()
        );
    }

    #[test]
    fn udp_session_single_peer_runs() {
        let t = UdpTransport::bind("127.0.0.1:0", &[None]).unwrap();
        let cfg = LockstepConfig {
            players: 1,
            input_delay: 1,
            ..LockstepConfig::default()
        };
        let mut s: Session<Count> = Session::udp(1, cfg, t);
        for f in 0..10 {
            assert_eq!(s.tick(inputs(f)), SessionEvent::Advanced(f));
        }
        assert_eq!(s.frame(), 10);
        assert_eq!(s.stats().unwrap().frames, 10);
        assert_eq!(s.replay().len(), 10);
    }
}
