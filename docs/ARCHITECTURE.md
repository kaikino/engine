# Architecture

KAGAMI is three layers, each a crate, each depending only on the one below.

```
┌──────────────────────────────────────────────────────────────┐
│  games/ippon, games/hello            cartridges (impl Game)  │
├──────────────────────────────────────────────────────────────┤
│  kagami-runtime     window · keyboard · audio device · CLI   │
│                     Session (local / replay / UDP / loopback)│
│                     headless runner                          │
├──────────────────────────────────────────────────────────────┤
│  kagami  (no deps)  fx · rng · input · hash · Game trait     │
│                     gpu (tiles, bg, oam, overlay, font, png) │
│                     apu (4-voice synth, songs, sfx)          │
│                     net (lockstep, wire, loopback, udp)      │
│                     replay                                   │
└──────────────────────────────────────────────────────────────┘
```

## The frame

One tick of the runtime, 60 times per second:

```
keyboard ──► Controllers ──► Session::tick(local inputs)
                                 │
                 ┌───────────────┴────────────────┐
                 │ Local            │ Lockstep     │
                 │ inputs as given  │ advance()    │  ◄── Transport (UDP / loopback)
                 │                  │ ok? else WAIT│
                 └───────────────┬──┴──────────────┘
                                 ▼
                     Game::update(UpdateCtx)         ← the only place state changes
                                 │
                        Game::hash ──► replay checkpoint, lockstep report_hash
                                 │
                       Apu::render_frame ──► 735 samples ──► audio queue ──► device
                                 │
                      Gpu::begin_frame, Game::draw, Gpu::render ──► RGBA ──► window
```

`update` and `draw` are separate on purpose. `update` is pure with respect
to its inputs and is the only thing hashed, replayed or synchronized.
`draw` is free to be skipped (headless), repeated (stalls) or decorated
(the runtime draws the WAITING and DESYNC banners on top).

## Data flow of a networked match

```
 Console A                                  Console B
 ─────────                                  ─────────
 frame f: input a_f  ──Inputs{f+d, window}──►  stored
 frame f: needs b_f  ◄──Inputs{f+d, window}──  input b_f
 both have a_f, b_f  →  update(f)  →  hash_f   both have a_f, b_f  →  update(f)  →  hash_f
 every 30 frames:  ──Hash{f, hash_f}──►  compare  ──►  equal, or Desync{f}
```

Each input message carries the last 8 inputs, so any single lost packet is
healed by the next one. A peer that stalls for three ticks re-sends its
window. State never crosses the wire.

## The GPU scan-out

`Gpu::render` walks 224 scanlines. Per line: fill backdrop, draw BG0 then
BG1 (non-priority tiles), select up to 16 objects whose vertical span
covers the line in OAM order, draw them back-to-front, draw priority BG
tiles, then the overlay. The result is a 256 × 224 array of palette
indices, which the remap table and palette turn into RGBA. Everything is a
plain `Vec<u8>` and the whole pass is well under a millisecond in release
builds.

## Why fixed point

Floating-point results can differ between compilers, CPUs and optimization
levels (fused multiply-add, x87 vs SSE, library `sin`). Fixed-point integer
arithmetic cannot. `Fx` is an `i32` with 16 fractional bits; multiplication
widens to `i64` and shifts back. Trigonometry reads a table generated at
compile time with integer-only Taylor series. The crate denies
`clippy::float_arithmetic` so a float cannot slip into the simulation by
accident; cartridges are asked to do the same.

## Determinism tests as the engine's tests

`games/ippon/tests/determinism.rs` runs the reference cartridge 600 frames
twice and compares hashes, verifies an encoded replay reproduces the same
hash, runs two lockstep peers across a simulated network with latency,
jitter and 20% loss and asserts they end identical, and plants a corrupted
peer to assert the desync is caught at the first checkpoint. The same file
pins a golden hash that CI checks on Linux, macOS and Windows. These are
the engine's real acceptance tests; the unit tests under each module are the
scaffolding.

## Where things live

| Concern | File |
|---|---|
| Fixed point, vectors, rects, trig table | `crates/kagami/src/fx.rs` |
| xoshiro128** RNG | `crates/kagami/src/rng.rs` |
| Controller bits, edge detection | `crates/kagami/src/input.rs` |
| FNV-1a state hasher | `crates/kagami/src/hash.rs` |
| `Game` trait, `UpdateCtx`, `simulate` | `crates/kagami/src/game.rs` |
| Compositor, OAM, backgrounds, overlay | `crates/kagami/src/gpu/mod.rs` |
| Palette, font, ASCII-art parser, PNG writer | `crates/kagami/src/gpu/{palette,font,art,png}.rs` |
| Synth, songs, effects | `crates/kagami/src/apu/mod.rs` |
| Lockstep state machine | `crates/kagami/src/net/lockstep.rs` |
| Wire format | `crates/kagami/src/net/wire.rs` |
| Transports | `crates/kagami/src/net/{loopback,udp}.rs` |
| Replay format and verification | `crates/kagami/src/replay.rs` |
| Window, timing, presentation | `crates/kagami-runtime/src/window.rs` |
| Match sessions (local/replay/udp/loopback) | `crates/kagami-runtime/src/session.rs` |
| Headless runner, random input driver | `crates/kagami-runtime/src/headless.rs` |
| Audio device, resampler | `crates/kagami-runtime/src/audio.rs` |
| Command line | `crates/kagami-runtime/src/cli.rs` |
