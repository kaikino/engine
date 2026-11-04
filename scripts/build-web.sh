#!/usr/bin/env bash
# Builds the IPPON and HANABI cartridges for the browser into web/dist/.
# Open web/index.html?cart=hanabi for the second one.
set -euo pipefail
cd "$(dirname "$0")/.."
J=${CARGO_BUILD_JOBS:+-j $CARGO_BUILD_JOBS}
cargo build $J -p ippon -p hanabi --lib --target wasm32-unknown-unknown --release
mkdir -p web/dist
cp target/wasm32-unknown-unknown/release/ippon.wasm web/dist/ippon.wasm
cp target/wasm32-unknown-unknown/release/hanabi.wasm web/dist/hanabi.wasm
ls -la web/dist/*.wasm
