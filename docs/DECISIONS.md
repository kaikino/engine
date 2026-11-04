# Decisions

Design decisions, in the order they were made, with the reasons. Each is
short on purpose. If you want to reopen one, open an issue that quotes it.

## D1. Fantasy console, not a general engine

A general 2D engine competes with Phaser, Bevy and LÖVE on their terms.
Fixed hardware limits give KAGAMI an identity and, more importantly, make
lockstep determinism practical: fixed resolution, tile coordinates, a small
tick rate and integer math remove the usual sources of divergence.

## D2. Lockstep over rollback for v0.1

Input-delay lockstep is fully correct with a `Game` trait of four methods.
Rollback needs state save/restore, prediction, and a frame-advantage
negotiation. The lockstep layer is complete and tested now; rollback is a
pure addition (see ROADMAP) and does not change the on-wire input format.

## D3. Rust

Determinism wants a language where the compiler can forbid floats in
simulation code (`clippy::float_arithmetic`), where integer overflow
behaviour is specified, and with no garbage collector to hiccup a 60 Hz
tick. Rust also builds for the web later from the same source.

## D4. `kagami` has zero dependencies

The console is the product. A console whose behaviour depends on a
third-party crate's version is harder to keep deterministic across years
and platforms. Everything the console does fits comfortably in standard
library Rust: integer math, `Vec<u8>` framebuffers, a hand-rolled wire
format, `UdpSocket`. The PNG writer is 80 lines because it only needs stored
deflate blocks.

## D5. Runtime dependencies: winit, softbuffer, cpal, and nothing else

The runtime talks to the operating system and must use well-maintained
crates for that. `softbuffer` presents a CPU framebuffer, which is exactly
what a 256 × 224 console wants and needs no GPU, so headless CI machines and
old laptops are fine. `cpal` is feature-gated (`audio`) so a build without
an audio stack still works. There is no argument-parser dependency; the CLI
is 150 lines of match arms with tests.

## D6. 16.16 fixed point

`i32` with 16 fractional bits gives ±32767 range and 1/65536 resolution,
plenty for a 256-pixel screen, and multiplication fits in `i64`. A 24.8
format would lack precision for slow velocities; 64-bit fixed point would
waste cache for nothing.

## D7. Sprites per scanline: 16

Real 16-bit hardware managed 32 per line; 8-bit hardware 8. Sixteen makes
the limit something a game can actually hit (a row of enemies) without
making it the first thing every game hits. Dropped sprites are reported,
not silently ignored, so a game can design around the limit.

## D8. Color 0 is the color key, and the overlay ignores it

Tiles and objects use palette index 0 as transparent by default (changeable
with `set_color_key`). The overlay buffer uses an out-of-band value for
"empty", so text and primitives can draw real black. This avoids the classic
"black text draws nothing" trap while keeping the hardware feel for tiles.

## D9. Tiles as ASCII art in source

No binary assets, no asset pipeline, no image decoder dependency. A tile is
eight lines of eight characters. Diffs are readable, and a cartridge is a
single `cargo run`. A 128-column block maps straight onto the tile sheet.

## D10. Inputs-only replays with periodic hash checkpoints

A replay that stores state would be large and could hide
nondeterminism. One byte per player per frame plus a hash every 60 frames is
tiny and self-verifying: `--verify` proves a build still agrees with a
recording.

## D11. FNV-1a for state hashes

It is not cryptographic and does not need to be. It is 10 lines, platform
independent, and sensitive to single-bit changes. The only requirement on a
desync detector is that two peers with different state almost never
collide, and 64-bit FNV-1a meets it.

## D12. The CPU opponent is part of the simulation

Single-player IPPON drives player 2 from the game's own `Rng`, so a
single-player match is as replayable and hashable as a two-player one.
No special-casing in the runtime.

## D13. Golden hash across three operating systems

One test pins the state hash of a scripted 600-frame match, and CI runs it
on Linux, macOS and Windows. A platform-dependent bug cannot pass CI. The
cost is that intentional simulation changes must update the constant, which
is a feature: it forces the changelog entry.

## D15. Save data is a record, not state

SRAM contents differ between consoles, so anything loaded from it must not
reach `update` or `hash`; otherwise two consoles with different saves would
desync on frame 0 and replays would not verify. The trait documentation
says so, the runtime refuses to load saves under netplay or replay
playback, and IPPON's tests assert its record leaves the hash untouched.

## D14. The browser runtime is a C ABI plus plain JavaScript

`winit`, `softbuffer` and `cpal` support `wasm32`, but routing the browser
build through them means `wasm-bindgen`, `web-sys` feature lists and a JS
bundler. The console only needs five things from a host: boot, step with
inputs, read the framebuffer, read the audio, read the hash. Those are
fourteen `extern "C"` functions and 150 lines of JavaScript, with no build
tooling beyond `cargo build --target wasm32-unknown-unknown`. The host is
also the smallest possible reference for porting KAGAMI anywhere else.

It is the one place the workspace allows the `unsafe_code` lint, for
`#[no_mangle]`; the crate contains no `unsafe` blocks.
