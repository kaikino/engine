//! Hostile-input robustness: nothing a peer can send may panic the console.

use kagami::net::{
    Lockstep, LockstepConfig, LoopbackNetwork, Message, Rollback, RollbackConfig, StepResult,
    Transport,
};
use kagami::replay::Replay;
use kagami::{Apu, Game, Gpu, Input, Rng, StateHasher, UpdateCtx};

#[derive(Clone)]
struct Tally(u32);
impl Game for Tally {
    fn new(seed: u32) -> Self {
        Tally(seed)
    }
    fn update(&mut self, ctx: &mut UpdateCtx) {
        self.0 = self
            .0
            .wrapping_mul(31)
            .wrapping_add(u32::from(ctx.inputs[0].bits()) + u32::from(ctx.inputs[1].bits()));
    }
    fn draw(&self, _: &mut Gpu) {}
    fn hash(&self, h: &mut StateHasher) {
        h.write_u32(self.0);
    }
}

fn random_bytes(rng: &mut Rng, max_len: usize) -> Vec<u8> {
    let len = rng.below(max_len as u32 + 1) as usize;
    (0..len).map(|_| rng.next_u32() as u8).collect()
}

#[test]
fn wire_decoder_never_panics_on_garbage() {
    let mut rng = Rng::new(0xC0FFEE);
    let mut decoded = 0;
    for _ in 0..20_000 {
        let bytes = random_bytes(&mut rng, 64);
        if let Some(m) = Message::decode(&bytes) {
            decoded += 1;
            // Whatever decodes must re-encode to something that decodes again.
            assert!(Message::decode(&m.encode()).is_some());
        }
    }
    assert!(
        decoded > 0,
        "some random bytes should parse (tags 1 and 2 are common)"
    );
}

#[test]
fn replay_decoder_never_panics_on_garbage() {
    let mut rng = Rng::new(7);
    let good = Replay::record::<Tally>(1, 2, 50, 10, |f| {
        [
            Input::from_bits(f as u8),
            Input::NONE,
            Input::NONE,
            Input::NONE,
        ]
    })
    .encode();
    for _ in 0..5_000 {
        let mut bytes = good.clone();
        // Flip a few bits, truncate, or append junk.
        for _ in 0..rng.below(4) {
            let i = rng.below(bytes.len() as u32) as usize;
            bytes[i] ^= 1 << rng.below(8);
        }
        if rng.chance(1, 3) {
            bytes.truncate(rng.below(bytes.len() as u32) as usize);
        }
        if rng.chance(1, 3) {
            bytes.extend(random_bytes(&mut rng, 8));
        }
        if let Ok(r) = Replay::decode(&bytes) {
            let _ = r.verify::<Tally>();
        }
    }
}

#[test]
fn lockstep_and_rollback_survive_a_garbage_storm() {
    let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
    let mut hostile = net.endpoint(1);
    let mut ep = net.endpoint(0);
    let mut rng = Rng::new(99);
    let mut ls = Lockstep::new(LockstepConfig {
        input_delay: 1,
        ..LockstepConfig::default()
    });
    let mut rb = Rollback::new(RollbackConfig::default(), Tally::new(1));
    let mut game = Tally::new(1);
    let mut apu = Apu::new();
    for i in 0..3_000u32 {
        // A mix of random bytes and well-formed but nonsensical messages.
        let data = if rng.chance(1, 2) {
            random_bytes(&mut rng, 40)
        } else {
            let m = if rng.chance(1, 2) {
                Message::Inputs {
                    from: rng.below(5) as u8,
                    last_frame: rng.next_u32(),
                    inputs: (0..rng.below(33))
                        .map(|_| Input::from_bits(rng.next_u32() as u8))
                        .collect(),
                }
            } else {
                Message::Hash {
                    from: rng.below(5) as u8,
                    frame: rng.next_u32(),
                    hash: u64::from(rng.next_u32()),
                }
            };
            m.encode()
        };
        hostile.send(0, &data);
        net.tick();
        let _ = ls.step(&mut game, &mut apu, Input::from_bits(i as u8), &mut ep);
        let _ = rb.step(Input::from_bits(i as u8), &mut ep);
    }
    // Either session may have stalled or flagged a desync from the nonsense,
    // but neither panicked and both still answer questions.
    let _ = ls.stats();
    let _ = rb.stats();
    assert!(ls.stats().rejected + ls.stats().received > 0);
    assert!(matches!(
        rb.step(Input::NONE, &mut ep),
        StepResult::Advanced(_) | StepResult::Waiting(_) | StepResult::Desynced(_)
    ));
}
