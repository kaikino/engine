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
| **Relay / TURN for symmetric NATs** | Browser netplay is direct WebRTC with STUN; peers that cannot hole-punch need a relay. | separate project |
| **Cartridge loader** | Games are compiled in. Loading a cartridge at runtime means a sandbox (wasm modules are the obvious one). | `kagami-cart` crate |
| **Kanji in the system font** | Katakana is in. Kanji at 8 × 8 is a different project (and a different font size). | `kagami::gpu::font` |
| **Relay server / NAT traversal** | Out of the console's scope; a relay is a `Transport`. | separate project |

## Done since v0.1.0

- Rollback netcode (`kagami::net::Rollback`, `--rollback`).
- Browser runtime (`kagami-web`, `web/`), hash-checked against native in CI.
- Katakana in the system font.
- Raster effects: per-line scroll, window masks, mosaic.
- Save data (2 KB SRAM per cartridge) and gamepads.
- HANABI, a second cartridge exercising scrolling, raster effects and the sprite budget.
- Browser netplay over WebRTC with serverless signaling.

## v0.2 candidates

1. Gamepads in the browser host.
2. A tracker and tile editor that write Rust source.
3. A native-to-browser bridge transport.

## How to propose a change

Open an issue. Quote the decision in `docs/DECISIONS.md` you want to revisit
if there is one. Proposals that raise a hardware limit will be closed with
a link to the first section of this file.
