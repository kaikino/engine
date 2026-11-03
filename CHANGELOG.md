# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [Semantic Versioning](https://semver.org/).

Changes that alter the simulation (and therefore the golden hash and replay
compatibility) are marked **[sim]**.

## [Unreleased]

## [0.1.0] - 2026-11-03

First release.

### Added

- `kagami`: the console. Zero dependencies.
  - 16.16 fixed-point `Fx`, `Vec2`, `Rect` with table-driven trig and exact integer square roots. Float arithmetic is a compile error inside the crate.
  - `Rng` (xoshiro128\*\*), `Input` with edge detection, `StateHasher` (FNV-1a 64).
  - `Gpu`: 256 × 224, fixed 32-color palette of traditional Japanese pigments, 512 tiles, two scrolling background layers with flip and priority, 128 objects with a 16-per-scanline limit, overlay primitives and text, built-in 8 × 8 font, palette remap, ASCII-art tile parser, PNG screenshots.
  - `Apu`: four-voice integer chiptune synthesizer with tracker songs and sound effects.
  - `net`: input-delay lockstep with redundant input windows, periodic state-hash exchange and desync reporting; `Transport` trait with UDP and simulated loopback implementations.
  - `replay`: inputs-only replay format with self-verifying checkpoints.
  - `Game` trait and `simulate` helper.
- `kagami-runtime`: window (winit + softbuffer), keyboard for two local players, audio output (cpal, feature-gated), headless mode with screenshots, replay record/playback/verify, loopback and UDP netplay, stats overlay, command line.
- `games/ippon`: a two-player kendo duel with CPU opponent, the reference cartridge; its tests pin a cross-platform golden hash.
- `games/hello`: the minimal cartridge.
- Documentation: manual, architecture, determinism contract, netcode, decisions, roadmap.
- CI on Linux, macOS and Windows; coverage gate; release workflow producing binaries for all three.

[Unreleased]: https://github.com/kaikino/engine/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kaikino/engine/releases/tag/v0.1.0
