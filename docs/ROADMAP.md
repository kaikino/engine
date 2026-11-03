# Roadmap

## Will never change

These are the console. Changing them would make a different console.

- 256 × 224 at 60 Hz
- the 32-color palette
- 512 tiles of 8 × 8, two background layers, 128 objects, 16 per scanline
- four sound voices
- eight-button controllers, four players
- fixed-point-only simulation
- inputs-only replays

## Out of scope for v0.1.0 (deliberately)

| Feature | Why not yet | Where it would go |
|---|---|---|
| **Rollback netcode** | Needs `Game::save/restore` plus prediction; lockstep is correct today and the wire format already carries what rollback needs. | `kagami::net::rollback`, opt-in alongside `Lockstep` |
| **WebAssembly build** | All three runtime dependencies support `wasm32`; the browser build simply was not verified end to end. | `kagami-runtime` with a `web` feature; WebRTC `Transport` |
| **Cartridge loader** | Games are compiled in. Loading a cartridge at runtime means a sandbox (wasm modules are the obvious one). | `kagami-cart` crate |
| **Gamepads** | Keyboard only. `gilrs` is the likely dependency. | `kagami-runtime::keys` |
| **Katakana / kanji in the system font** | The 8 × 8 font covers printable ASCII; a kana set is 46+ glyphs of pixel art that deserve care. | `kagami::gpu::font` |
| **Mosaic, windowing, per-line scroll (HDMA-style effects)** | Fun, retro, cheap to add once a game needs them. | `Gpu` |
| **Relay server / NAT traversal** | Out of the console's scope; a relay is a `Transport`. | separate project |
| **Save data** | Cartridges have no persistent storage. A 2 KB battery-backed "SRAM" API is the natural shape. | `kagami::sram` |

## v0.2 candidates

1. Rollback with `Game::save`/`restore` (default impls via a `Clone` bound).
2. `wasm32` runtime and a WebRTC transport, so IPPON runs in a browser tab.
3. Gamepad support.
4. Kana glyphs.

## How to propose a change

Open an issue. Quote the decision in `docs/DECISIONS.md` you want to revisit
if there is one. Proposals that raise a hardware limit will be closed with
a link to the first section of this file.
