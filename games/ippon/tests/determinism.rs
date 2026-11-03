//! IPPON is KAGAMI's reference cartridge, so these tests are the engine's
//! end-to-end determinism and netplay guarantees, not just the game's.

use ippon::{Ippon, Phase};
use kagami::net::{Lockstep, LockstepConfig, LoopbackNetwork, StepResult};
use kagami::replay::Replay;
use kagami::{Apu, Game, Gpu, Input, StateHasher, UpdateCtx, MAX_PLAYERS};
use kagami_runtime::RandomInputs;

const SEED: u32 = 42;
const FRAMES: u32 = 600;

/// The state hash after 600 frames of seed 42 with seeded random inputs.
///
/// This value must be identical on every platform CI runs on. If it changes,
/// either the simulation was intentionally changed (update it, and note the
/// replay format is now incompatible with older recordings) or a
/// nondeterminism bug slipped in.
const GOLDEN_STATE_HASH: u64 = 0x5e6f_0a94_cf95_d5d6;

fn hash(g: &Ippon) -> u64 {
    let mut h = StateHasher::new();
    g.hash(&mut h);
    h.finish()
}

fn random_inputs() -> impl FnMut(u32) -> [Input; MAX_PLAYERS] {
    let mut r = RandomInputs::new(SEED);
    move |f| r.frame(f, 2)
}

fn simulate(frames: u32, mut inputs: impl FnMut(u32) -> [Input; MAX_PLAYERS]) -> (Ippon, u64) {
    let mut g = Ippon::new(SEED);
    let mut apu = Apu::new();
    for f in 0..frames {
        let mut ctx = UpdateCtx::new(inputs(f), f, 2, &mut apu);
        g.update(&mut ctx);
    }
    let h = hash(&g);
    (g, h)
}

#[test]
fn six_hundred_frames_twice_hash_identically() {
    let (a, ha) = simulate(FRAMES, random_inputs());
    let (b, hb) = simulate(FRAMES, random_inputs());
    assert_eq!(ha, hb);
    // The match actually happened: we are past the title and someone acted.
    assert_ne!(a.phase(), Phase::Title);
    let [f0, f1] = *a.fighters();
    assert_eq!(f0.x, b.fighters()[0].x);
    assert!(
        f0.x != kagami::Fx::from_int(80)
            || f1.x != kagami::Fx::from_int(176)
            || a.score() != [0, 0]
    );
}

#[test]
fn golden_hash_is_stable_across_platforms() {
    let (_, h) = simulate(FRAMES, random_inputs());
    if GOLDEN_STATE_HASH == 0 {
        eprintln!("golden hash not pinned yet: {h:016x}");
    } else {
        assert_eq!(
            h, GOLDEN_STATE_HASH,
            "simulation diverged from the pinned golden hash"
        );
    }
}

#[test]
fn replay_roundtrip_verifies() {
    let replay = Replay::record::<Ippon>(SEED, 2, FRAMES, 60, random_inputs());
    let bytes = replay.encode();
    assert!(
        bytes.len() < 2000,
        "600 frames of a 2P match is {} bytes",
        bytes.len()
    );
    let back = Replay::decode(&bytes).unwrap();
    let verified = back.verify::<Ippon>().unwrap();
    let (_, direct) = simulate(FRAMES, random_inputs());
    assert_eq!(verified, direct);
}

#[test]
fn lockstep_peers_on_a_lossy_network_mirror_each_other() {
    let net = LoopbackNetwork::new(2, 7, 3, 4, 200);
    let cfg = |p| LockstepConfig {
        local_player: p,
        players: 2,
        input_delay: 4,
        hash_interval: 20,
        ..LockstepConfig::default()
    };
    let mut games = [Ippon::new(SEED), Ippon::new(SEED)];
    let mut apus = [Apu::new(), Apu::new()];
    let mut sessions = [Lockstep::new(cfg(0)), Lockstep::new(cfg(1))];
    let mut endpoints = [net.endpoint(0), net.endpoint(1)];
    let mut inputs = random_inputs();
    let mut ticks = 0;
    while sessions.iter().any(|s| s.frame() < FRAMES) && ticks < FRAMES * 40 {
        let frame_inputs = inputs(ticks);
        for p in 0..2 {
            if sessions[p].frame() >= FRAMES {
                continue;
            }
            let r = sessions[p].step(
                &mut games[p],
                &mut apus[p],
                frame_inputs[p],
                &mut endpoints[p],
            );
            assert!(!matches!(r, StepResult::Desynced(_)), "{r:?}");
        }
        net.tick();
        ticks += 1;
    }
    assert_eq!(sessions[0].frame(), FRAMES);
    assert_eq!(sessions[1].frame(), FRAMES);
    assert_eq!(hash(&games[0]), hash(&games[1]));
    assert!(
        sessions[0].stats().stalls > 0,
        "20% loss should stall sometimes"
    );
    assert!(net.dropped() > 0);
    // Both peers rendered the same picture.
    let mut frames = Vec::new();
    for g in &games {
        let mut gpu = Gpu::new();
        g.setup(&mut gpu);
        gpu.begin_frame();
        g.draw(&mut gpu);
        gpu.render();
        frames.push(gpu.frame_hash());
    }
    assert_eq!(frames[0], frames[1]);
}

#[test]
fn a_corrupted_peer_is_caught_at_the_next_checkpoint() {
    let net = LoopbackNetwork::new(2, 1, 1, 0, 0);
    let cfg = |p| LockstepConfig {
        local_player: p,
        players: 2,
        input_delay: 2,
        hash_interval: 10,
        ..LockstepConfig::default()
    };
    let mut games = [Ippon::new(SEED), Ippon::new(SEED + 1)]; // different seeds: different petals
    let mut apus = [Apu::new(), Apu::new()];
    let mut sessions = [Lockstep::new(cfg(0)), Lockstep::new(cfg(1))];
    let mut endpoints = [net.endpoint(0), net.endpoint(1)];
    let mut desync = None;
    for _ in 0..200 {
        for p in 0..2 {
            if let StepResult::Desynced(d) =
                sessions[p].step(&mut games[p], &mut apus[p], Input::NONE, &mut endpoints[p])
            {
                desync.get_or_insert(d);
            }
        }
        net.tick();
        if desync.is_some() {
            break;
        }
    }
    let d = desync.expect("desync detected");
    assert_eq!(d.frame, 0, "the very first checkpoint already disagrees");
}

#[test]
fn one_player_mode_lets_the_cpu_win_or_lose() {
    // An idle human versus the CPU: the CPU should score within a minute.
    let mut g = Ippon::new(3);
    let mut apu = Apu::new();
    let mut start = [Input::NONE; MAX_PLAYERS];
    start[0] = Input::START;
    let mut ctx = UpdateCtx::new(start, 0, 1, &mut apu);
    g.update(&mut ctx);
    for f in 1..3600 {
        let mut ctx = UpdateCtx::new([Input::NONE; MAX_PLAYERS], f, 1, &mut apu);
        g.update(&mut ctx);
        if g.score()[1] > 0 {
            return;
        }
    }
    panic!("CPU never scored: {:?}", g.fighters());
}
