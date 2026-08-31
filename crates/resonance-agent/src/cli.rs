//! Console diagnostics retained separately from the native tray executable.

use resonance_api::contract::StreamEvent;

pub fn run(arguments: Vec<String>) -> Result<(), String> {
    let runtime_mode = runtime_mode(&arguments);
    crate::diagnostics::initialize(runtime_mode)?;
    match dispatch(arguments.into_iter()) {
        Ok(()) => {
            crate::diagnostics::orderly_exit(match runtime_mode {
                "serve_cli" => "console_or_cli_requested_stop",
                _ => "console_or_cli_completed",
            });
            Ok(())
        }
        Err(message) => {
            crate::diagnostics::unexpected_exit("runtime_error", &message);
            Err(message)
        }
    }
}

fn runtime_mode(arguments: &[String]) -> &'static str {
    match arguments.first().map(String::as_str) {
        Some("serve") => "serve_cli",
        Some("capture") | None => "capture_cli",
        Some("--version" | "-V") => "version_cli",
        Some("--help" | "-h") => "help_cli",
        Some(_) => "capture_cli",
    }
}

fn dispatch(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match args.next() {
        Some(command) if command == "serve" => match parse_serve_options(args)? {
            Some(config) => crate::transport::run(config),
            None => Ok(()),
        },
        Some(command) if command == "capture" => run_diagnostic(parse_options(args)?),
        Some(command) if command == "--version" || command == "-V" => {
            ensure_no_more_arguments(args, "version")?;
            print_version();
            Ok(())
        }
        Some(command) if command == "--help" || command == "-h" => {
            ensure_no_more_arguments(args, "help")?;
            print_help();
            Ok(())
        }
        Some(argument) => run_diagnostic(parse_options(std::iter::once(argument).chain(args))?),
        None => run_diagnostic(parse_options(args)?),
    }
}

fn ensure_no_more_arguments(
    mut args: impl Iterator<Item = String>,
    mode: &str,
) -> Result<(), String> {
    match args.next() {
        Some(argument) => Err(format!("unknown {mode} argument {argument:?}")),
        None => Ok(()),
    }
}

fn run_diagnostic(options: Option<DiagnosticOptions>) -> Result<(), String> {
    use crate::supervisor::{CaptureSupervisor, CaptureSupervisorStart};
    use crate::windows::CaptureOwnerCompletion;
    use std::time::Duration;

    let Some(options) = options else {
        return Ok(());
    };

    crate::diagnostics::set_lifecycle_state("capture_cli_starting");
    crate::diagnostics::info("capture_cli_startup_attempt");
    let mut supervisor =
        CaptureSupervisor::for_source(options.source_intent, print_lifecycle_event);
    match supervisor.start() {
        Ok(CaptureSupervisorStart::Started) => {}
        Ok(CaptureSupervisorStart::StoppedBeforeStart) => {
            return Err("capture supervisor stopped before startup".to_string());
        }
        Err(error) => {
            return Err(format!(
                "Windows playback-loopback supervisor failed to start: {error}"
            ));
        }
    }

    std::thread::sleep(options.duration);
    crate::diagnostics::info("capture_cli_stop_requested reason=duration_complete");
    if let Err(error) = supervisor.stop(Duration::from_secs(2)) {
        return Err(format!(
            "Windows playback-loopback supervisor shutdown failed: {error}"
        ));
    }
    match supervisor
        .owner_completion()
        .expect("a started supervisor retains its completed owner")
    {
        CaptureOwnerCompletion::Finished(report) => {
            crate::diagnostics::set_lifecycle_state("capture_cli_stopped");
            crate::diagnostics::info(&format!("capture_cli_completed end={:?}", report.end));
            println!("{report}");
        }
        CaptureOwnerCompletion::Failed(error) => {
            return Err(format!(
                "Windows playback-loopback capture failed: kind={:?}, retry={:?}, message={error}",
                error.kind(),
                error.retry_hint()
            ));
        }
        CaptureOwnerCompletion::StoppedBeforeStart => {
            return Err("capture owner stopped before startup".to_string());
        }
        CaptureOwnerCompletion::StartFailed(message) => {
            return Err(format!(
                "Windows playback-loopback owner failed to start: {message}"
            ));
        }
        CaptureOwnerCompletion::Panicked => {
            return Err("Windows playback-loopback owner panicked".to_string());
        }
    }
    Ok(())
}

fn parse_serve_options(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<crate::transport::AgentServiceConfig>, String> {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    let mut host = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut port = 48_480_u16;
    let mut host_seen = false;
    let mut port_seen = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--host" if !host_seen => {
                host_seen = true;
                let value = args
                    .next()
                    .ok_or_else(|| "--host requires 127.0.0.1 or ::1".to_string())?;
                host = value
                    .parse()
                    .map_err(|_| format!("invalid numeric listener address {value:?}"))?;
            }
            "--port" if !port_seen => {
                port_seen = true;
                let value = args
                    .next()
                    .ok_or_else(|| "--port requires a value".to_string())?;
                port = value
                    .parse::<u16>()
                    .map_err(|_| format!("invalid port {value:?}"))?;
                if port == 0 {
                    return Err("service port must be between 1 and 65535".to_string());
                }
            }
            "--help" | "-h" => {
                print_help();
                return Ok(None);
            }
            "--host" => return Err("--host was supplied more than once".to_string()),
            "--port" => return Err("--port was supplied more than once".to_string()),
            _ => return Err(format!("unknown serve argument {argument:?}")),
        }
    }
    crate::transport::AgentServiceConfig::new(SocketAddr::new(host, port)).map(Some)
}

fn print_version() {
    println!("resonance-agent {}", env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    println!(
        "Usage:\n  resonance-agent-cli.exe --help|--version\n  resonance-agent-cli.exe serve [--host 127.0.0.1|::1] [--port <1..=65535>]\n  resonance-agent-cli.exe capture [--duration-seconds <1..=3600>] [--source-id <opaque-id>]"
    );
}

fn print_lifecycle_event(event: StreamEvent) {
    match event {
        StreamEvent::Started(descriptor) => {
            crate::diagnostics::set_lifecycle_state("capture_cli_running");
            crate::diagnostics::info(&format!(
                "capture_session_started mode=cli sample_rate_hz={} channels={}",
                descriptor.sample_rate().hz(),
                descriptor.channels().channel_count().get()
            ));
            println!(
                "stream started: id={}, source={}, rate={} Hz, channels={}",
                descriptor.stream_id().as_str(),
                descriptor.source_id().as_str(),
                descriptor.sample_rate().hz(),
                descriptor.channels().channel_count().get()
            );
        }
        StreamEvent::Data(_) => {}
        StreamEvent::Error(error) => {
            crate::diagnostics::info(&format!(
                "capture_session_error mode=cli kind={:?} retry={:?}",
                error.kind(),
                error.retry_hint()
            ));
            eprintln!(
                "stream error: kind={:?}, retry={:?}, message={}",
                error.kind(),
                error.retry_hint(),
                error
            );
        }
        StreamEvent::Ended { stream_id, reason } => {
            crate::diagnostics::info(&format!(
                "capture_session_stopped mode=cli reason={reason:?}"
            ));
            println!("stream ended: id={}, reason={reason:?}", stream_id.as_str());
        }
        _ => {}
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiagnosticOptions {
    duration: std::time::Duration,
    source_intent: crate::windows::PlaybackCaptureIntent,
}

fn parse_options(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<DiagnosticOptions>, String> {
    const DEFAULT_SECONDS: u64 = 10;
    let mut seconds = DEFAULT_SECONDS;
    let mut source_intent = crate::windows::PlaybackCaptureIntent::DefaultPlayback;
    let mut duration_seen = false;
    let mut source_seen = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                println!(
                    "Usage: resonance-agent-cli.exe capture [--duration-seconds <1..=3600>] [--source-id <opaque-id>]"
                );
                return Ok(None);
            }
            "--duration-seconds" if !duration_seen => {
                duration_seen = true;
                let value = args
                    .next()
                    .ok_or_else(|| "--duration-seconds requires a value".to_string())?;
                seconds = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid duration {value:?}"))?;
                if !(1..=3600).contains(&seconds) {
                    return Err("duration must be between 1 and 3600 seconds".to_string());
                }
            }
            "--source-id" if !source_seen => {
                source_seen = true;
                let value = args
                    .next()
                    .ok_or_else(|| "--source-id requires an opaque SourceId".to_string())?;
                let source_id = resonance_api::contract::SourceId::new(value)
                    .map_err(|error| format!("invalid SourceId: {error}"))?;
                source_intent = crate::windows::PlaybackCaptureIntent::Explicit(source_id);
            }
            "--duration-seconds" => {
                return Err("--duration-seconds was supplied more than once".to_string());
            }
            "--source-id" => return Err("--source-id was supplied more than once".to_string()),
            _ => return Err(format!("unknown argument {argument:?}")),
        }
    }
    Ok(Some(DiagnosticOptions {
        duration: std::time::Duration::from_secs(seconds),
        source_intent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn args(values: &[&str]) -> impl Iterator<Item = String> {
        values
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn duration_defaults_to_ten_seconds() {
        assert_eq!(
            parse_options(args(&[])),
            Ok(Some(DiagnosticOptions {
                duration: Duration::from_secs(10),
                source_intent: crate::windows::PlaybackCaptureIntent::DefaultPlayback,
            }))
        );
    }

    #[test]
    fn duration_accepts_only_the_documented_range_and_shape() {
        assert!(parse_options(args(&["--duration-seconds", "1"])).is_ok());
        assert!(parse_options(args(&["--duration-seconds", "3600"])).is_ok());
        assert!(parse_options(args(&["--duration-seconds", "0"])).is_err());
        assert!(parse_options(args(&["--duration-seconds", "3601"])).is_err());
        assert!(parse_options(args(&["--unknown"])).is_err());
    }

    #[test]
    fn explicit_source_id_is_opaque_and_independent_of_duration_order() {
        let parsed = parse_options(args(&[
            "--source-id",
            "opaque-source-a",
            "--duration-seconds",
            "2",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(parsed.duration, Duration::from_secs(2));
        assert_eq!(
            parsed.source_intent,
            crate::windows::PlaybackCaptureIntent::Explicit(
                resonance_api::contract::SourceId::new("opaque-source-a").unwrap()
            )
        );
    }

    #[test]
    fn cli_modes_are_separate_from_the_tray_runtime() {
        assert_eq!(runtime_mode(&["serve".to_string()]), "serve_cli");
        assert_eq!(runtime_mode(&["capture".to_string()]), "capture_cli");
        assert_eq!(runtime_mode(&["--version".to_string()]), "version_cli");
    }
}
