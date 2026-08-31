//! Native Windows tray entry point for Resonance Signal.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(message) = resonance_agent::diagnostics::initialize("tray") {
        resonance_agent::tray::show_fatal_error(&format!(
            "Failed to initialize durable diagnostics: {message}"
        ));
        std::process::exit(1);
    }
    let result = match arguments.as_slice() {
        [] => resonance_agent::tray::run(),
        [argument] if argument == "--tray" || argument == "tray" => {
            resonance_agent::tray::run()
        }
        _ => Err(
            "resonance-agent.exe is the tray runtime; use resonance-agent-cli.exe for --help, --version, serve, and capture"
                .to_string(),
        ),
    };
    match result {
        Ok(()) => resonance_agent::diagnostics::orderly_exit("tray_exit"),
        Err(message) => {
            resonance_agent::diagnostics::unexpected_exit("tray_runtime_error", &message);
            resonance_agent::tray::show_fatal_error(&message);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("resonance-agent tray runtime is currently available only on Windows");
    std::process::exit(1);
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn tray_binary_is_windows_only() {
        assert!(cfg!(windows));
    }
}
