//! HANABI's determinism and netplay guarantees, mirroring IPPON's.

use hanabi::{Hanabi, Phase};
use kagami::net::{LoopbackNetwork, Rollback, RollbackConfig, StepResult};
use kagami::replay::Replay;
use kagami::{Apu, Game, Input, StateHasher, UpdateCtx, MAX_PLAYERS};
use kagami_runtime::RandomInputs;

const SEED: u32 = 42;
const FRAMES: u32 = 600;

/// State hash after 600 frames of seed 42 with seeded random inputs.
/// Must match on every platform; see games/ippon/tests/determinism.rs.
const GOLDEN_STATE_HASH: u64 = 0x53df_5d5e_2d32_0078;

fn hash(g: &Hanabi) -> u64 {
    let mut h = StateHasher::new();
    g.hash(&mut h);
    h.finish()
}

fn random_inputs() -> impl FnMut(u32) -> [Input; MAX_PLAYERS] {
    let mut r = RandomInputs::new(SEED);
    move |f| r.frame(f, 2)
}

fn simulate(frames: u32, mut inputs: impl FnMut(u32) -> [Input; MAX_PLAYERS]) -> (Hanabi, u64) {
    let mut g = Hanabi::new(SEED);
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
    let (_, hb) = simulate(FRAMES, random_inputs());
    assert_eq!(ha, hb);
    assert_ne!(a.phase(), Phase::Title);
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
    let replay = Replay::record::<Hanabi>(SEED, 2, FRAMES, 60, random_inputs());
    let back = Replay::decode(&replay.encode()).unwrap();
    let (_, direct) = simulate(FRAMES, random_inputs());
    assert_eq!(back.verify::<Hanabi>().unwrap(), direct);
}

#[test]
fn rollback_peers_confirm_identical_matches() {
    let net = LoopbackNetwork::new(2, 3, 4, 2, 100);
    let cfg = |p| RollbackConfig {
        local_player: p,
        players: 2,
        input_delay: 1,
        max_prediction: 8,
        hash_interval: 20,
        ..RollbackConfig::default()
    };
    let mut sessions = [
        Rollback::new(cfg(0), Hanabi::new(SEED)),
        Rollback::new(cfg(1), Hanabi::new(SEED)),
    ];
    let mut endpoints = [net.endpoint(0), net.endpoint(1)];
    let mut inputs = random_inputs();
    let mut confirmed: [Vec<(u32, [Input; MAX_PLAYERS], u64)>; 2] = [Vec::new(), Vec::new()];
    let mut ticks = 0;
    while confirmed.iter().any(|c| c.len() < FRAMES as usize) && ticks < FRAMES * 40 {
        let frame_inputs = inputs(ticks);
        for p in 0..2 {
            let r = sessions[p].step(frame_inputs[p], &mut endpoints[p]);
            assert!(!matches!(r, StepResult::Desynced(_)), "{r:?}");
            confirmed[p].extend(sessions[p].drain_confirmed());
        }
        net.tick();
        ticks += 1;
    }
    let n = FRAMES as usize;
    assert_eq!(&confirmed[0][..n], &confirmed[1][..n]);
    assert!(sessions[0].stats().rollbacks > 0);
}
