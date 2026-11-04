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
| **Browser netplay** | The wasm build runs locally; netplay in a browser needs a WebRTC data-channel `Transport` and a signaling server. | `kagami-web` + a small signaling service |
| **Cartridge loader** | Games are compiled in. Loading a cartridge at runtime means a sandbox (wasm modules are the obvious one). | `kagami-cart` crate |
| **Gamepads** | Keyboard only. `gilrs` is the likely dependency. | `kagami-runtime::keys` |
| **Kanji in the system font** | Katakana is in. Kanji at 8 × 8 is a different project (and a different font size). | `kagami::gpu::font` |
| **Mosaic, windowing, per-line scroll (HDMA-style effects)** | Fun, retro, cheap to add once a game needs them. | `Gpu` |
| **Relay server / NAT traversal** | Out of the console's scope; a relay is a `Transport`. | separate project |
| **Save data** | Cartridges have no persistent storage. A 2 KB battery-backed "SRAM" API is the natural shape. | `kagami::sram` |

## Done since v0.1.0

- Rollback netcode (`kagami::net::Rollback`, `--rollback`).
- Browser runtime (`kagami-web`, `web/`), hash-checked against native in CI.
- Katakana in the system font.

## v0.2 candidates

1. WebRTC transport for browser netplay.
2. Gamepad support.
3. Raster effects (per-line scroll, mosaic, windows).
4. Save data (SRAM).

## How to propose a change

Open an issue. Quote the decision in `docs/DECISIONS.md` you want to revisit
if there is one. Proposals that raise a hardware limit will be closed with
a link to the first section of this file.
