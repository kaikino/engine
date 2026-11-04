#!/usr/bin/env bash
# Builds the IPPON cartridge for the browser into web/dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
J=${CARGO_BUILD_JOBS:+-j $CARGO_BUILD_JOBS}
cargo build $J -p ippon --lib --target wasm32-unknown-unknown --release
mkdir -p web/dist
cp target/wasm32-unknown-unknown/release/ippon.wasm web/dist/ippon.wasm
ls -la web/dist/ippon.wasm
