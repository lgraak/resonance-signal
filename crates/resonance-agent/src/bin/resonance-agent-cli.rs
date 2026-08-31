//! Console-subsystem entry point for Resonance Signal diagnostics.

#[cfg(windows)]
fn main() {
    if let Err(message) = resonance_agent::cli::run(std::env::args().skip(1).collect()) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("resonance-agent diagnostics are currently available only on Windows");
    std::process::exit(1);
}
