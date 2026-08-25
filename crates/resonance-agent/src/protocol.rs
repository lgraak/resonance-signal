//! Consumer-safe JSON and binary waveform protocol encoding.

use resonance_api::contract::{
    AudioFrame, ChannelLayout, ChannelPosition, DefaultSource, DiscoverySnapshot, ErrorKind,
    ErrorScope, ProviderError, RetryHint, SignalPayload, SourceAvailability, SourceKind,
    StreamDescriptor, StreamEndReason, StreamEvent, StreamId,
};
use resonance_core::scheduling::WindowScheduler;
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: u8 = 1;
pub const BINARY_HEADER_LEN: usize = 40;
pub const BINARY_MAGIC: [u8; 4] = *b"RSWF";
pub const MAX_BINARY_FRAME_BYTES: usize = 1_048_576;

#[derive(Debug)]
pub enum EncodedEvent {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Debug, Default)]
pub struct SessionEncoder {
    scheduler: WindowScheduler<StreamId>,
    sequence: u64,
}

impl SessionEncoder {
    pub fn encode(&mut self, event: StreamEvent) -> Result<Vec<EncodedEvent>, String> {
        match event {
            StreamEvent::Started(descriptor) => Ok(vec![EncodedEvent::Text(
                stream_started_json(&descriptor).to_string(),
            )]),
            StreamEvent::Data(packet) => match packet.payload() {
                SignalPayload::Waveform(_) => {
                    let stream_id = packet.stream_id().clone();
                    let SignalPayload::Waveform(frame) = packet.payload().clone() else {
                        unreachable!();
                    };
                    let scheduled = self
                        .scheduler
                        .push(stream_id, frame)
                        .map_err(|error| error.to_string())?;
                    let mut encoded = Vec::with_capacity(scheduled.windows().len());
                    for window in scheduled.into_windows() {
                        let sequence = self.sequence;
                        self.sequence = self
                            .sequence
                            .checked_add(1)
                            .ok_or_else(|| "waveform sequence overflowed".to_string())?;
                        encoded.push(EncodedEvent::Binary(encode_waveform_frame(
                            sequence,
                            window.frame(),
                        )?));
                    }
                    Ok(encoded)
                }
                _ => Ok(Vec::new()),
            },
            StreamEvent::Error(error) => Ok(vec![EncodedEvent::Text(
                provider_error_json(&error).to_string(),
            )]),
            StreamEvent::Ended { stream_id, reason } => Ok(vec![EncodedEvent::Text(
                stream_stopped_json(stream_id.as_str(), reason).to_string(),
            )]),
            _ => Ok(Vec::new()),
        }
    }
}

pub fn discovery_json(snapshot: &DiscoverySnapshot, revision: &str) -> Value {
    let sources = snapshot
        .sources()
        .iter()
        .map(|source| {
            json!({
                "source_id": source.source_id().as_str(),
                "display_name": source.display_name(),
                "kind": source_kind_name(source.kind()),
                "availability": availability_name(source.availability()),
                "default_playback": source.default_roles().contains(&DefaultSource::Playback),
                "supported_products": source.supported_products().iter().map(|_| "waveform").collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "revision": revision,
        "sources": sources,
    })
}

pub fn stream_started_json(descriptor: &StreamDescriptor) -> Value {
    json!({
        "type": "stream_started",
        "protocol_version": PROTOCOL_VERSION,
        "stream_id": descriptor.stream_id().as_str(),
        "source_id": descriptor.source_id().as_str(),
        "source_kind": source_kind_name(descriptor.source_kind()),
        "sample_rate_hz": descriptor.sample_rate().hz(),
        "channels": descriptor.channels().channel_count().get(),
        "channel_order": channel_order(descriptor.channels()),
        "sample_format": "f32-le",
        "window_duration_ns": resonance_core::scheduling::DEFAULT_WINDOW_DURATION.as_nanos() as u64,
    })
}

pub fn provider_error_json(error: &ProviderError) -> Value {
    json!({
        "type": "stream_error",
        "protocol_version": PROTOCOL_VERSION,
        "kind": error_kind_name(error.kind()),
        "scope": error_scope_json(error.scope()),
        "retry": retry_hint_name(error.retry_hint()),
    })
}

pub fn startup_error_json(kind: ErrorKind, retry: RetryHint) -> Value {
    json!({
        "type": "stream_error",
        "protocol_version": PROTOCOL_VERSION,
        "kind": error_kind_name(kind),
        "scope": { "type": "subscription" },
        "retry": retry_hint_name(retry),
    })
}

pub fn service_error_json(kind: &str) -> Value {
    json!({
        "type": "stream_error",
        "protocol_version": PROTOCOL_VERSION,
        "kind": kind,
        "scope": { "type": "subscription" },
        "retry": "do_not_retry",
    })
}

pub fn stream_stopped_json(stream_id: &str, reason: StreamEndReason) -> Value {
    json!({
        "type": "stream_stopped",
        "protocol_version": PROTOCOL_VERSION,
        "stream_id": stream_id,
        "reason": end_reason_name(reason),
    })
}

pub fn encode_waveform_frame(sequence: u64, frame: &AudioFrame) -> Result<Vec<u8>, String> {
    let channel_count = frame.channels().channel_count().get();
    if channel_count > 2 {
        return Err("consumer waveform transport supports mono or stereo only".to_string());
    }
    let frame_count = frame.window().frame_count();
    let sample_count = usize::try_from(frame_count)
        .ok()
        .and_then(|count| count.checked_mul(usize::from(channel_count)))
        .ok_or_else(|| "waveform sample count overflowed".to_string())?;
    if sample_count != frame.samples().len() {
        return Err("waveform sample count did not match its format".to_string());
    }
    let payload_len = sample_count
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| "waveform payload length overflowed".to_string())?;
    if BINARY_HEADER_LEN.saturating_add(payload_len) > MAX_BINARY_FRAME_BYTES {
        return Err("waveform frame exceeds the protocol size bound".to_string());
    }
    let mut output = Vec::with_capacity(
        BINARY_HEADER_LEN
            .checked_add(payload_len)
            .ok_or_else(|| "waveform frame length overflowed".to_string())?,
    );
    output.extend_from_slice(&BINARY_MAGIC);
    output.push(PROTOCOL_VERSION);
    output.push(BINARY_HEADER_LEN as u8);
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&sequence.to_le_bytes());
    output.extend_from_slice(&frame.window().start().frame_index().to_le_bytes());
    output.extend_from_slice(&frame.window().start().stream_time_ns().to_le_bytes());
    output.extend_from_slice(&frame_count.to_le_bytes());
    output.extend_from_slice(&channel_count.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    for sample in frame.samples() {
        output.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(output)
}

fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Playback => "playback",
        SourceKind::Microphone => "microphone",
        SourceKind::Virtual => "virtual",
        SourceKind::Other => "other",
        _ => "other",
    }
}

fn availability_name(availability: SourceAvailability) -> &'static str {
    match availability {
        SourceAvailability::Available => "available",
        SourceAvailability::Unavailable => "unavailable",
        SourceAvailability::Unknown => "unknown",
        _ => "unknown",
    }
}

fn channel_order(layout: &ChannelLayout) -> Vec<&'static str> {
    match layout.positions() {
        Some(positions) => positions
            .iter()
            .map(|position| channel_name(*position))
            .collect(),
        None => (0..layout.channel_count().get())
            .map(|_| "discrete")
            .collect(),
    }
}

fn channel_name(position: ChannelPosition) -> &'static str {
    match position {
        ChannelPosition::Mono => "mono",
        ChannelPosition::FrontLeft => "front_left",
        ChannelPosition::FrontRight => "front_right",
        _ => "other",
    }
}

fn error_kind_name(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::SourceUnavailable => "source_unavailable",
        ErrorKind::PermissionDenied => "permission_denied",
        ErrorKind::StreamInterrupted => "stream_interrupted",
        ErrorKind::UnsupportedFormat => "unsupported_format",
        ErrorKind::InvalidRequest => "invalid_request",
        ErrorKind::ResourceExhausted => "resource_exhausted",
        ErrorKind::Internal => "internal",
        _ => "internal",
    }
}

fn retry_hint_name(retry: RetryHint) -> &'static str {
    match retry {
        RetryHint::RetryNow => "retry_now",
        RetryHint::RetryLater => "retry_later",
        RetryHint::WaitForSource => "wait_for_source",
        RetryHint::RequestPermission => "request_permission",
        RetryHint::ChangeFormat => "change_format",
        RetryHint::DoNotRetry => "do_not_retry",
        _ => "do_not_retry",
    }
}

fn error_scope_json(scope: &ErrorScope) -> Value {
    match scope {
        ErrorScope::Subscription => json!({ "type": "subscription" }),
        ErrorScope::Source(source_id) => {
            json!({ "type": "source", "source_id": source_id.as_str() })
        }
        ErrorScope::Stream(stream_id) => {
            json!({ "type": "stream", "stream_id": stream_id.as_str() })
        }
        _ => json!({ "type": "subscription" }),
    }
}

fn end_reason_name(reason: StreamEndReason) -> &'static str {
    match reason {
        StreamEndReason::ConsumerCancelled => "consumer_cancelled",
        StreamEndReason::SourceEnded => "source_ended",
        StreamEndReason::SourceReconfigured => "source_reconfigured",
        StreamEndReason::ProviderShutdown => "provider_shutdown",
        StreamEndReason::Failed => "failed",
        _ => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_api::contract::{
        ChannelLayout, ChannelPosition, FrameTimestamp, SampleRate, SourceId,
    };

    #[test]
    fn binary_waveform_layout_is_little_endian_and_self_consistent() {
        let frame = AudioFrame::new(
            FrameTimestamp::new(9, 187_500),
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::FrontLeft, ChannelPosition::FrontRight])
                .unwrap(),
            vec![0.25, -0.5, 1.0, -1.0],
        )
        .unwrap();
        let encoded = encode_waveform_frame(7, &frame).unwrap();
        assert_eq!(&encoded[0..4], b"RSWF");
        assert_eq!(encoded[4], 1);
        assert_eq!(encoded[5], 40);
        assert_eq!(u64::from_le_bytes(encoded[8..16].try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(encoded[16..24].try_into().unwrap()), 9);
        assert_eq!(
            u64::from_le_bytes(encoded[24..32].try_into().unwrap()),
            187_500
        );
        assert_eq!(u32::from_le_bytes(encoded[32..36].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(encoded[36..38].try_into().unwrap()), 2);
        let samples = encoded[40..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect::<Vec<_>>();
        assert_eq!(samples, frame.samples());
    }

    #[test]
    fn stream_metadata_contains_only_portable_identity() {
        let descriptor = StreamDescriptor::new(
            StreamId::new("stream-1").unwrap(),
            SourceId::new("source-1").unwrap(),
            SourceKind::Playback,
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
        );
        let text = stream_started_json(&descriptor).to_string();
        assert!(text.contains("source-1"));
        assert!(!text.to_ascii_lowercase().contains("wasapi"));
        assert!(!text.contains("endpoint"));
    }

    #[test]
    fn binary_waveform_frame_rejects_the_protocol_size_limit() {
        let frame = AudioFrame::new(
            FrameTimestamp::new(0, 0),
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
            vec![0.0; (MAX_BINARY_FRAME_BYTES - BINARY_HEADER_LEN) / 4 + 1],
        )
        .unwrap();
        assert!(encode_waveform_frame(0, &frame)
            .unwrap_err()
            .contains("size bound"));
    }
}
