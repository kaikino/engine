//! IPPON cartridge binary. See `cargo run -p ippon -- --help`.
fn main() -> std::process::ExitCode {
    kagami_runtime::main::<ippon::Ippon>()
}
