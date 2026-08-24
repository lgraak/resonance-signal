//! Diagnostic executable for the production Resonance Signal capture boundary.

#[cfg(windows)]
fn main() {
    use resonance_agent::windows::{CaptureOwner, CaptureOwnerCompletion, CaptureOwnerStart};
    use std::time::Duration;

    let duration = match parse_duration(std::env::args().skip(1)) {
        Ok(Some(duration)) => duration,
        Ok(None) => return,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let mut owner = CaptureOwner::new(print_lifecycle_event);
    match owner.start() {
        Ok(CaptureOwnerStart::Started) => {}
        Ok(CaptureOwnerStart::StopAlreadyRequested) => {
            eprintln!("capture owner stopped before startup");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("Windows playback-loopback owner failed to start: {error}");
            std::process::exit(1);
        }
    }

    std::thread::sleep(duration);
    match owner.shutdown(Duration::from_secs(2)) {
        Ok(CaptureOwnerCompletion::Finished(report)) => println!("{report}"),
        Ok(CaptureOwnerCompletion::Failed(error)) => {
            eprintln!(
                "Windows playback-loopback capture failed: kind={:?}, retry={:?}, message={error}",
                error.kind(),
                error.retry_hint()
            );
            std::process::exit(1);
        }
        Ok(CaptureOwnerCompletion::StoppedBeforeStart) => {
            eprintln!("capture owner stopped before startup");
            std::process::exit(1);
        }
        Ok(CaptureOwnerCompletion::StartFailed(message)) => {
            eprintln!("Windows playback-loopback owner failed to start: {message}");
            std::process::exit(1);
        }
        Ok(CaptureOwnerCompletion::Panicked) => {
            eprintln!("Windows playback-loopback owner panicked");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("Windows playback-loopback owner shutdown failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn print_lifecycle_event(event: resonance_api::contract::StreamEvent) {
    use resonance_api::contract::StreamEvent;

    match event {
        StreamEvent::Started(descriptor) => println!(
            "stream started: id={}, source={}, rate={} Hz, channels={}",
            descriptor.stream_id().as_str(),
            descriptor.source_id().as_str(),
            descriptor.sample_rate().hz(),
            descriptor.channels().channel_count().get()
        ),
        StreamEvent::Data(_) => {}
        StreamEvent::Error(error) => eprintln!(
            "stream error: kind={:?}, retry={:?}, message={}",
            error.kind(),
            error.retry_hint(),
            error
        ),
        StreamEvent::Ended { stream_id, reason } => {
            println!("stream ended: id={}, reason={reason:?}", stream_id.as_str())
        }
        _ => {}
    }
}

#[cfg(windows)]
fn parse_duration(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<std::time::Duration>, String> {
    const DEFAULT_SECONDS: u64 = 10;
    let Some(argument) = args.next() else {
        return Ok(Some(std::time::Duration::from_secs(DEFAULT_SECONDS)));
    };
    if argument == "--help" || argument == "-h" {
        println!("Usage: resonance-agent [--duration-seconds <1..=3600>]");
        return Ok(None);
    }
    if argument != "--duration-seconds" {
        return Err(format!("unknown argument {argument:?}"));
    }
    let value = args
        .next()
        .ok_or_else(|| "--duration-seconds requires a value".to_string())?;
    if args.next().is_some() {
        return Err("unexpected arguments after --duration-seconds".to_string());
    }
    let seconds = value
        .parse::<u64>()
        .map_err(|_| format!("invalid duration {value:?}"))?;
    if !(1..=3600).contains(&seconds) {
        return Err("duration must be between 1 and 3600 seconds".to_string());
    }
    Ok(Some(std::time::Duration::from_secs(seconds)))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("resonance-agent playback loopback is currently available only on Windows");
    std::process::exit(1);
}

#[cfg(all(test, windows))]
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
        assert_eq!(parse_duration(args(&[])), Ok(Some(Duration::from_secs(10))));
    }

    #[test]
    fn duration_accepts_only_the_documented_range_and_shape() {
        assert_eq!(
            parse_duration(args(&["--duration-seconds", "1"])),
            Ok(Some(Duration::from_secs(1)))
        );
        assert_eq!(
            parse_duration(args(&["--duration-seconds", "3600"])),
            Ok(Some(Duration::from_secs(3600)))
        );
        assert!(parse_duration(args(&["--duration-seconds", "0"]))
            .unwrap_err()
            .contains("between 1 and 3600"));
        assert!(parse_duration(args(&["--duration-seconds", "3601"]))
            .unwrap_err()
            .contains("between 1 and 3600"));
        assert!(parse_duration(args(&["--unknown"]))
            .unwrap_err()
            .contains("unknown argument"));
        assert!(parse_duration(args(&["--duration-seconds", "10", "extra"]))
            .unwrap_err()
            .contains("unexpected arguments"));
    }
}
