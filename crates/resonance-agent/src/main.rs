//! Executable entry point for the bounded Resonance Signal capture prototype.

#[cfg(windows)]
fn main() {
    use resonance_agent::windows::{run_default_playback_loopback, PrototypeConfig};

    let duration = match parse_duration(std::env::args().skip(1)) {
        Ok(Some(duration)) => duration,
        Ok(None) => return,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let config = PrototypeConfig::new(duration);
    match run_default_playback_loopback(config, print_lifecycle_event) {
        Ok(evidence) => println!("{evidence}"),
        Err(error) => {
            eprintln!("Windows playback-loopback prototype failed: {error}");
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
