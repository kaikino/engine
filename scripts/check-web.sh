#!/usr/bin/env bash
# Builds the browser runtime and proves it matches the native build.
# Needs: the wasm32-unknown-unknown target, Node 18+, and Playwright with
# Chromium (`npx playwright install chromium`).
set -euo pipefail
cd "$(dirname "$0")/.."
J=${CARGO_BUILD_JOBS:+-j $CARGO_BUILD_JOBS}
echo "== web: build wasm"
scripts/build-web.sh
echo "== web: native reference hash (120 frames, 2 players, seed 1, no input)"
cargo build $J -p ippon --release
native=$(target/release/ippon --headless --frames 120 --players 2 --seed 1 | sed -n 's/^state hash: //p')
echo "native state hash: $native"
echo "== web: browser e2e"
node web/test/e2e.mjs "$native"
echo "== web: a replay recorded in Chromium verifies with the native binary"
target/release/ippon --verify web/dist/browser.kgmr | grep "verify: OK"
echo "== web: netplay e2e (two pages over WebRTC, lockstep and rollback)"
node web/test/netplay.mjs
echo "== web: tile editor exports art the console parses"
node web/test/tiles.mjs
cargo run $J -q -p kagami --example art_check -- web/dist/tiles-export.rs
echo "== web: tracker export parses natively and the wasm synth matches the native synth sample for sample"
node web/test/tracker.mjs
cargo run $J -q -p kagami --example song_check -- web/dist/song-export.rs web/dist/song-samples.bin
