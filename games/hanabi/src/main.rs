//! HANABI cartridge binary. See `cargo run -p hanabi -- --help`.
fn main() -> std::process::ExitCode {
    kagami_runtime::main::<hanabi::Hanabi>()
}
