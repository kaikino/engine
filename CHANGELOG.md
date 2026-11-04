# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [Semantic Versioning](https://semver.org/).

Changes that alter the simulation (and therefore the golden hash and replay
compatibility) are marked **[sim]**.

## [Unreleased]

### Added

- **Rollback netcode.** `kagami::net::Rollback` predicts late remote inputs, snapshots the game each frame and re-simulates when a prediction was wrong; hashes are compared on confirmed frames only. The runtime exposes it as `--rollback` with `--prediction N`, for both UDP and loopback netplay, and the stats overlay shows rollbacks, re-simulated frames and the prediction lead.
- `Session::rollback_udp`, `Session::rollback_loopback`, `NetStats` (shared statistics for both netcodes).

- **Browser runtime.** `kagami-web` exposes a cartridge as fourteen C ABI exports; `web/kagami.js` is a dependency-free host (canvas, Web Audio, keyboard). `scripts/build-web.sh` produces an 86 KB `ippon.wasm`; `scripts/check-web.sh` runs it in headless Chromium and asserts the state hash equals the native build's. CI runs that check.

- **Katakana in the system font.** `kagami::gpu::kana` adds the 46 basic kana, small kana, ー, ・, 「」、。 and composes every voiced and semi-voiced form; `Gpu::text` draws them directly. `cargo run -p kagami --example font_sheet` renders the full set.

- **Raster effects.** Per-scanline horizontal scroll tables per background (`Background::set_line_scroll`), horizontal window masks (`Background::window`) and a mosaic register (`Gpu::set_mosaic`). IPPON uses a heat haze on its title and a mosaic ramp when a point is called.

- **Save data.** `Game::load_sram` / `Game::save_sram` with a 2 KB budget; the runtime persists `kagami-<title>.sram` for local play (`--sram`, `--no-sram`). IPPON keeps a lifetime win record on its title screen.
- **Gamepads.** The runtime reads gamepads through `gilrs` (feature `gamepad`, on by default), first pad to player 1, merged with the keyboard; `--no-gamepad` ignores them.

- **HANABI**, a second cartridge: a two-player co-op vertical shooter on a scrolling festival river with a per-scanline water reflection, bombs, oni bosses, a saved high score, kana text, and enough lanterns to overrun the 16-sprite scanline budget on purpose (its tests assert that it does). Builds for the browser too.

- **Browser netplay.** The lockstep and rollback sessions run inside the WebAssembly module (`kagami_net_*` exports, `QueueTransport`); `web/netplay.js` carries datagrams over an unordered, unreliable WebRTC data channel with serverless offer/answer-code signaling, and `web/index.html` has a netplay panel. `web/test/netplay.mjs` plays a match between two headless Chromium pages in both modes; CI runs it.

### Changed

- **[api]** `Game` now requires `Clone` (rollback needs to snapshot it). Deriving it is enough for plain-data games.
- `--delay` is now optional and defaults to 3 for lockstep and 1 for rollback.

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
