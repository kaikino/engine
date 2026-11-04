# Contributing

Thanks for helping build a console.

## Setup

```bash
rustup toolchain install stable --component rustfmt clippy
# Linux only: system libraries for the window and audio
sudo apt-get install libasound2-dev libudev-dev libx11-xcb-dev libxkbcommon-dev libwayland-dev
git clone https://github.com/kaikino/engine kagami && cd kagami
scripts/check.sh
```

`scripts/check.sh` is the whole gate CI runs. If it passes locally it will
pass in CI, except for the cross-platform golden hash, which only CI can
check on all three operating systems.

Individual steps:

```bash
cargo test --workspace                       # unit + integration tests
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps              # RUSTDOCFLAGS=-D warnings in CI
cargo run -p ippon -- --headless --frames 600 --random-inputs --screenshot out.png
```

Coverage, if you have `cargo-llvm-cov`:

```bash
cargo llvm-cov --workspace --all-features --ignore-filename-regex 'window\.rs|main\.rs'
```

## Rules of the road

- **Determinism first.** Read `docs/DETERMINISM.md`. Any change to simulation code must keep the golden hash test passing or update it with a **[sim]** changelog entry and a regenerated `docs/replays/demo.kgmr`.
- **No placeholders.** Placeholder macros and unfinished-work markers fail the build (`scripts/check-stubs.sh` lists the patterns). Finish the feature or leave it out and note it in the roadmap.
- **Every public item has a doc comment.** `cargo doc` runs with warnings as errors.
- **Tests go with the code.** Unit tests live in the module; end-to-end tests live in `games/ippon/tests`.
- **Clippy pedantic, warnings are errors.** The workspace allows a short list of style lints in `Cargo.toml`; do not add `#[allow]` in code without a comment saying why.
- **The hardware limits are not up for discussion** in a pull request. See `docs/ROADMAP.md`.

## Commits and pull requests

- Conventional Commits: `feat(gpu): …`, `fix(net): …`, `docs: …`, `test: …`, `ci: …`.
- One logical change per commit. Keep the suite green at every commit.
- Fill in the pull request template, especially the determinism section.
- Add a line under *Unreleased* in `CHANGELOG.md`.

## Making a release

1. Move *Unreleased* entries under a new version heading with today's date.
2. Bump `version` in the workspace `Cargo.toml`.
3. `scripts/check.sh`.
4. Commit, tag `vX.Y.Z`, push the tag. The release workflow builds and attaches binaries.
