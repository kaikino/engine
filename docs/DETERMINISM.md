# Determinism

The whole console rests on one promise: given the same seed and the same
inputs, `Game::update` produces the same state on every machine, every time.
This document is the contract and the checklist.

## What the console guarantees

- `Fx` arithmetic is integer arithmetic. Same bits everywhere.
- `Rng` is xoshiro128** seeded by splitmix32 from a `u32`. Same sequence everywhere.
- `Fx::sin_turns`, `cos_turns` and `sqrt` read compile-time tables or use integer Newton iteration.
- `Apu` sequencing is frame-driven and integer-only. The sample stream is identical everywhere, though it is not hashed, because audio never feeds back into state.
- `Gpu` is deterministic too, and `Gpu::frame_hash` lets you golden-test pictures, but the GPU is presentation and is never hashed for netplay.
- The lockstep layer delivers the same `[Input; 4]` for frame *f* to every peer before any peer simulates *f*.
- Hashes are 64-bit FNV-1a over little-endian bytes. `write_usize` always writes 8 bytes so 32-bit and 64-bit builds agree.

## What the cartridge must do

1. Derive all randomness from a `kagami::Rng` stored in the game and seeded from the match seed. Hash it with `rng.hash_state(h)`.
2. Use `Fx` or integers. Never `f32`/`f64`. Add `#![deny(clippy::float_arithmetic)]` to the crate root; the `kagami` crate itself is built with that lint.
3. Never read a clock, a file, an environment variable, or thread-local state in `update`.
4. Never iterate a `HashMap` or `HashSet` in `update`; their order is randomized per process. Use `Vec`, arrays or `BTreeMap`.
5. Hash every field that `update` reads. If in doubt, hash it; hashing is cheap.
6. Keep `draw` free of side effects on game state. It takes `&self`, which makes mutation a compile error, but interior mutability (`Cell`, `RefCell`) would defeat that; do not use it.
7. Treat `ctx.players` as part of the inputs: it is the same on all peers.
8. If you depend on `MAX_PLAYERS` slot ordering, remember unused slots are `Input::NONE`, identically everywhere.

## What happens on a desync

Every `hash_interval` frames (30 by default) each peer sends its state hash
to the others. When a peer holds both its own and a remote hash for the
same frame and they differ, it reports a `Desync { frame, local_hash,
remote_hash, remote_player }`. The runtime stops the match and prints it.
The frame number is the first checkpoint after the divergence, so the bug
happened within the preceding interval.

To find it: save the replay from one peer (F4 or `--record`), run `--verify`
on the other peer's build. The verifier reports the first checkpoint that
disagrees, which narrows it to 60 frames, and `Replay::verify` is a
one-liner to bisect further from a test.

## How CI enforces it

- `games/ippon/tests/determinism.rs` pins `GOLDEN_STATE_HASH`, the state of a 600-frame two-player match driven by seeded inputs. CI runs it on Linux, macOS and Windows. If it differs anywhere, the build fails.
- `docs/replays/demo.kgmr` is an 1800-frame recorded match with 30 checkpoints. `scripts/check.sh` verifies it with the release binary.
- Two lockstep peers are run in-process across a simulated network with latency, jitter and 20% loss, and must finish with identical hashes.

## Changing the simulation

Any change to `update` logic, `Fx`, `Rng`, the lockstep input schedule, or
the hash function changes the golden hash and invalidates old replays. That
is expected; it is also a compatibility break. Update the hash and the demo
replay in the same commit and say so in `CHANGELOG.md`.
