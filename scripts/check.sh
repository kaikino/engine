#!/usr/bin/env bash
# The full quality gate. CI runs exactly this.
set -euo pipefail
cd "$(dirname "$0")/.."
JOBS="${CARGO_BUILD_JOBS:-}"
J=${JOBS:+-j $JOBS}

echo "== format"
cargo fmt --all -- --check
echo "== clippy (warnings are errors)"
cargo clippy $J --workspace --all-targets --all-features -- -D warnings
echo "== docs (warnings are errors)"
RUSTDOCFLAGS="-D warnings" cargo doc $J --workspace --no-deps --all-features
echo "== tests"
cargo test $J --workspace --all-features
echo "== placeholders"
scripts/check-stubs.sh
echo "== release build"
cargo build $J --workspace --release
echo "== headless smoke: IPPON 600 frames, replay record + verify"
tmp=$(mktemp -d)
target/release/ippon --headless --frames 600 --random-inputs --seed 42 \
  --record "$tmp/smoke.kgmr" --screenshot "$tmp/smoke.png" | tee "$tmp/report.txt"
target/release/ippon --verify "$tmp/smoke.kgmr" | tee -a "$tmp/report.txt"
grep -q "verify: OK" "$tmp/report.txt"
echo "== golden replay from docs/replays verifies on this build"
target/release/ippon --verify docs/replays/demo.kgmr | grep "verify: OK"
echo "== all checks passed"
