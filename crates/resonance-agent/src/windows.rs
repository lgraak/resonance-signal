//! Production Windows playback-loopback capture using `wasapi` 0.24.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use resonance_api::contract::{
    ErrorKind, ErrorScope, ProviderError, RetryHint, SignalPacket, SignalPayload, SourceId,
    SourceKind, StreamDescriptor, StreamEndReason, StreamEvent, StreamId,
};
use wasapi::{
    deinitialize, initialize_mta, DeviceEventCallbacks, DeviceState, Direction, DisconnectReason,
    EventCallbacks, Role, SampleType, StreamMode, WaveFormat,
};

use crate::capture::{AudioFrameBuilder, CaptureError, CaptureFormat, CapturePacket, PacketFlags};

// Real-device validation showed one 10 ms packet at a time. Four slots keep
// callback work decoupled from ordinary processing without making buffering a
// public tuning surface. Exhaustion ends the stream instead of dropping data.
const HANDOFF_CAPACITY: usize = 4;
const EVENT_WAIT_TIMEOUT_MS: u32 = 100;
const END_NONE: u8 = 0;
const END_SOURCE_RECONFIGURED: u8 = 1;
const END_SOURCE_UNAVAILABLE: u8 = 2;
const END_INTERRUPTED: u8 = 3;
const AUDCLNT_E_DEVICE_INVALIDATED: i32 = 0x8889_0004_u32 as i32;

static STREAM_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// A cloneable request for a running capture operation to stop normally.
///
/// This is the production lifecycle control. The duration-based command-line
/// mode is retained only as a bounded diagnostic and validation harness.
#[derive(Clone, Debug, Default)]
pub struct CaptureStopToken(Arc<AtomicBool>);

impl CaptureStopToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_stop(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_stop_requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
enum RunLimit {
    UntilStopped,
    Duration(Duration),
}

/// Measurements gathered without adding work to the WASAPI event thread.
#[derive(Clone, Debug)]
pub struct CaptureReport {
    pub endpoint_name: String,
    pub native_sample_rate_hz: u32,
    pub native_channel_count: u16,
    pub output_sample_rate_hz: u32,
    pub output_channel_count: u16,
    pub maximum_packet_frames: u32,
    pub packet_count: u64,
    pub audio_frame_count: u64,
    pub source_frame_count: u64,
    pub minimum_packet_frames: Option<u32>,
    pub maximum_observed_packet_frames: Option<u32>,
    pub minimum_callback_interval: Option<Duration>,
    pub maximum_callback_interval: Option<Duration>,
    pub maximum_callback_duration: Option<Duration>,
    pub minimum_qpc_delta: Option<Duration>,
    pub maximum_qpc_delta: Option<Duration>,
    pub initial_discontinuity_observed: bool,
    pub end: CaptureEnd,
    /// Non-stable, human-readable detail for a terminal condition, when any.
    pub end_diagnostic: Option<String>,
}

impl fmt::Display for CaptureReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Windows WASAPI playback-loopback evidence")?;
        writeln!(formatter, "  endpoint: {}", self.endpoint_name)?;
        writeln!(
            formatter,
            "  native format: {} Hz, {} channel(s)",
            self.native_sample_rate_hz, self.native_channel_count
        )?;
        writeln!(
            formatter,
            "  output format: {} Hz, {} channel(s), interleaved f32",
            self.output_sample_rate_hz, self.output_channel_count
        )?;
        writeln!(
            formatter,
            "  packet frames: observed {:?}..{:?}, WASAPI maximum {}",
            self.minimum_packet_frames,
            self.maximum_observed_packet_frames,
            self.maximum_packet_frames
        )?;
        writeln!(
            formatter,
            "  packets / AudioFrames / source frames: {} / {} / {}",
            self.packet_count, self.audio_frame_count, self.source_frame_count
        )?;
        writeln!(
            formatter,
            "  callback interval: {:?}..{:?}",
            self.minimum_callback_interval, self.maximum_callback_interval
        )?;
        writeln!(
            formatter,
            "  maximum callback duration: {:?}",
            self.maximum_callback_duration
        )?;
        writeln!(
            formatter,
            "  WASAPI QPC delta: {:?}..{:?}",
            self.minimum_qpc_delta, self.maximum_qpc_delta
        )?;
        writeln!(
            formatter,
            "  initial discontinuity flag: {}",
            self.initial_discontinuity_observed
        )?;
        writeln!(
            formatter,
            "  observed latency: not measured (clock correlation deferred)"
        )?;
        write!(formatter, "  stream end: {:?}", self.end)?;
        if let Some(diagnostic) = &self.end_diagnostic {
            write!(formatter, " ({diagnostic})")?;
        }
        Ok(())
    }
}

/// Machine-actionable reason the capture owner stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureEnd {
    StopRequested,
    DurationElapsed,
    SourceReconfigured,
    SourceUnavailable,
    Interrupted,
    DataDiscontinuity,
    BoundedHandoffExhausted,
    Failed,
}

/// Captures the default playback endpoint until `stop` is requested or a
/// stream boundary occurs.
///
/// Provider events are emitted on an ordinary processing thread. The WASAPI
/// callback never invokes `on_event`, waits for processing, or allocates.
pub fn run_default_playback_loopback(
    stop: CaptureStopToken,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<CaptureReport, CaptureRunError> {
    run_capture(RunLimit::UntilStopped, stop, &mut on_event)
}

/// Runs the production capture boundary for a bounded diagnostic interval.
///
/// Duration is validation/CLI configuration, not a capture-backend tuning
/// parameter. Production owners should use [`run_default_playback_loopback`]
/// with a [`CaptureStopToken`].
pub fn run_default_playback_loopback_for(
    duration: Duration,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<CaptureReport, CaptureRunError> {
    if duration.is_zero() {
        return Err(CaptureRunError::InvalidDuration);
    }
    run_capture(
        RunLimit::Duration(duration),
        CaptureStopToken::new(),
        &mut on_event,
    )
}

fn run_capture(
    limit: RunLimit,
    stop: CaptureStopToken,
    on_event: &mut impl FnMut(StreamEvent),
) -> Result<CaptureReport, CaptureRunError> {
    const CAPACITY: usize = HANDOFF_CAPACITY;

    let (capture_tx, capture_rx) = mpsc::sync_channel(CAPACITY);
    let capture_stop = stop.clone();
    let handle = thread::Builder::new()
        .name("resonance-wasapi-loopback".to_string())
        .spawn(move || capture_thread(limit, capture_stop, capture_tx))
        .map_err(CaptureRunError::Spawn)?;

    let result = process_capture(capture_rx, stop, on_event);
    handle
        .join()
        .map_err(|_| CaptureRunError::CaptureThreadPanicked)?;
    result
}

fn process_capture(
    capture_rx: Receiver<CaptureMessage>,
    stop: CaptureStopToken,
    on_event: &mut impl FnMut(StreamEvent),
) -> Result<CaptureReport, CaptureRunError> {
    let started = match capture_rx
        .recv()
        .map_err(|_| CaptureRunError::CaptureChannelClosed)?
    {
        CaptureMessage::Started(started) => started,
        CaptureMessage::Failed(failure) => return Err(failure.into()),
        CaptureMessage::Packet(_) | CaptureMessage::Ended(_) => {
            return Err(CaptureRunError::Protocol("capture did not start first"))
        }
    };

    let stream_id = StreamId::new(format!(
        "wasapi-loopback-{}-{}",
        std::process::id(),
        STREAM_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
    .map_err(|error| CaptureRunError::Contract(error.to_string()))?;
    let source_id = SourceId::new("default-playback")
        .map_err(|error| CaptureRunError::Contract(error.to_string()))?;
    let descriptor = StreamDescriptor::new(
        stream_id.clone(),
        source_id.clone(),
        SourceKind::Playback,
        started.format.sample_rate(),
        started.format.channels().clone(),
    );
    on_event(StreamEvent::Started(descriptor));

    let mut evidence = CaptureReport {
        endpoint_name: started.endpoint_name,
        native_sample_rate_hz: started.native_sample_rate_hz,
        native_channel_count: started.native_channel_count,
        output_sample_rate_hz: started.format.sample_rate().hz(),
        output_channel_count: started.format.channel_count(),
        maximum_packet_frames: started.maximum_packet_frames,
        packet_count: 0,
        audio_frame_count: 0,
        source_frame_count: 0,
        minimum_packet_frames: None,
        maximum_observed_packet_frames: None,
        minimum_callback_interval: None,
        maximum_callback_interval: None,
        maximum_callback_duration: None,
        minimum_qpc_delta: None,
        maximum_qpc_delta: None,
        initial_discontinuity_observed: false,
        end: CaptureEnd::Failed,
        end_diagnostic: Some("capture channel closed without an end event".to_string()),
    };
    let mut builder = AudioFrameBuilder::new(started.format);

    loop {
        let message = capture_rx
            .recv()
            .map_err(|_| CaptureRunError::CaptureChannelClosed)?;
        match message {
            CaptureMessage::Packet(packet) => {
                update_packet_evidence(&mut evidence, &packet)?;
                let built = builder.push(&packet);
                let recycle_result = started.recycle_tx.try_send(packet.into_buffer());
                if recycle_result.is_err() {
                    stop.request_stop();
                    return Err(CaptureRunError::Protocol("capture buffer recycle failed"));
                }

                match built {
                    Ok(built) => {
                        evidence.initial_discontinuity_observed |= built.initial_discontinuity();
                        update_duration_range(
                            &mut evidence.minimum_qpc_delta,
                            &mut evidence.maximum_qpc_delta,
                            built.qpc_delta(),
                        );
                        evidence.audio_frame_count = evidence
                            .audio_frame_count
                            .checked_add(1)
                            .ok_or(CaptureRunError::EvidenceOverflow)?;
                        evidence.source_frame_count = evidence
                            .source_frame_count
                            .checked_add(u64::from(built.frame().window().frame_count()))
                            .ok_or(CaptureRunError::EvidenceOverflow)?;
                        on_event(StreamEvent::Data(SignalPacket::new(
                            stream_id.clone(),
                            SignalPayload::Waveform(built.into_frame()),
                        )));
                    }
                    Err(error) => {
                        stop.request_stop();
                        let message = error.to_string();
                        on_event(StreamEvent::Error(ProviderError::new(
                            ErrorKind::StreamInterrupted,
                            ErrorScope::Stream(stream_id.clone()),
                            RetryHint::RetryNow,
                            message.clone(),
                        )));
                        on_event(StreamEvent::Ended {
                            stream_id,
                            reason: StreamEndReason::Failed,
                        });
                        evidence.end = CaptureEnd::DataDiscontinuity;
                        evidence.end_diagnostic = Some(message);
                        drain_until_end(&capture_rx);
                        return Ok(evidence);
                    }
                }
            }
            CaptureMessage::Ended(termination) => {
                evidence.end = termination.end;
                evidence.end_diagnostic = termination.diagnostic;
                let (error, reason) = contract_end(
                    evidence.end,
                    evidence.end_diagnostic.as_deref(),
                    &source_id,
                    &stream_id,
                );
                if let Some(error) = error {
                    on_event(StreamEvent::Error(error));
                }
                on_event(StreamEvent::Ended { stream_id, reason });
                return Ok(evidence);
            }
            CaptureMessage::Failed(failure) => {
                let message = failure.message;
                on_event(StreamEvent::Error(ProviderError::new(
                    failure.kind,
                    ErrorScope::Stream(stream_id.clone()),
                    failure.retry_hint,
                    message.clone(),
                )));
                on_event(StreamEvent::Ended {
                    stream_id,
                    reason: StreamEndReason::Failed,
                });
                evidence.end = CaptureEnd::Failed;
                evidence.end_diagnostic = Some(message);
                return Ok(evidence);
            }
            CaptureMessage::Started(_) => {
                stop.request_stop();
                return Err(CaptureRunError::Protocol("capture started more than once"));
            }
        }
    }
}

fn drain_until_end(capture_rx: &Receiver<CaptureMessage>) {
    while let Ok(message) = capture_rx.recv() {
        if matches!(
            message,
            CaptureMessage::Ended(_) | CaptureMessage::Failed(_)
        ) {
            break;
        }
    }
}

fn contract_end(
    end: CaptureEnd,
    diagnostic: Option<&str>,
    source_id: &SourceId,
    stream_id: &StreamId,
) -> (Option<ProviderError>, StreamEndReason) {
    match end {
        CaptureEnd::StopRequested => (None, StreamEndReason::ProviderShutdown),
        CaptureEnd::DurationElapsed => (None, StreamEndReason::ConsumerCancelled),
        CaptureEnd::SourceReconfigured => (
            Some(ProviderError::new(
                ErrorKind::StreamInterrupted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryNow,
                diagnostic.unwrap_or("default playback device or stream format changed"),
            )),
            StreamEndReason::SourceReconfigured,
        ),
        CaptureEnd::SourceUnavailable => (
            Some(ProviderError::new(
                ErrorKind::SourceUnavailable,
                ErrorScope::Source(source_id.clone()),
                RetryHint::WaitForSource,
                diagnostic.unwrap_or("default playback endpoint became unavailable"),
            )),
            StreamEndReason::SourceEnded,
        ),
        CaptureEnd::Interrupted => (
            Some(ProviderError::new(
                ErrorKind::StreamInterrupted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryLater,
                diagnostic.unwrap_or("WASAPI audio session was interrupted"),
            )),
            StreamEndReason::Failed,
        ),
        CaptureEnd::BoundedHandoffExhausted => (
            Some(ProviderError::new(
                ErrorKind::ResourceExhausted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryLater,
                diagnostic.unwrap_or(
                    "bounded capture handoff exhausted; no audio packet was silently dropped",
                ),
            )),
            StreamEndReason::Failed,
        ),
        CaptureEnd::DataDiscontinuity => (
            Some(ProviderError::new(
                ErrorKind::StreamInterrupted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryNow,
                diagnostic.unwrap_or("capture continuity could not be proven"),
            )),
            StreamEndReason::Failed,
        ),
        CaptureEnd::Failed => (
            Some(ProviderError::new(
                ErrorKind::Internal,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryLater,
                diagnostic.unwrap_or("Windows capture failed"),
            )),
            StreamEndReason::Failed,
        ),
    }
}

fn update_packet_evidence(
    evidence: &mut CaptureReport,
    packet: &CapturePacket,
) -> Result<(), CaptureRunError> {
    evidence.packet_count = evidence
        .packet_count
        .checked_add(1)
        .ok_or(CaptureRunError::EvidenceOverflow)?;
    evidence.minimum_packet_frames = Some(
        evidence
            .minimum_packet_frames
            .map_or(packet.frame_count(), |current| {
                current.min(packet.frame_count())
            }),
    );
    evidence.maximum_observed_packet_frames = Some(
        evidence
            .maximum_observed_packet_frames
            .map_or(packet.frame_count(), |current| {
                current.max(packet.frame_count())
            }),
    );
    update_duration_range(
        &mut evidence.minimum_callback_interval,
        &mut evidence.maximum_callback_interval,
        packet.callback_interval(),
    );
    evidence.maximum_callback_duration = Some(
        evidence
            .maximum_callback_duration
            .map_or(packet.callback_duration(), |current| {
                current.max(packet.callback_duration())
            }),
    );
    Ok(())
}

fn update_duration_range(
    minimum: &mut Option<Duration>,
    maximum: &mut Option<Duration>,
    value: Option<Duration>,
) {
    if let Some(value) = value {
        *minimum = Some(minimum.map_or(value, |current| current.min(value)));
        *maximum = Some(maximum.map_or(value, |current| current.max(value)));
    }
}

fn capture_thread(limit: RunLimit, stop: CaptureStopToken, capture_tx: SyncSender<CaptureMessage>) {
    if let Err(error) = capture_thread_inner(limit, stop, &capture_tx) {
        let _ = capture_tx.send(CaptureMessage::Failed(error));
    }
}

fn capture_thread_inner(
    limit: RunLimit,
    stop: CaptureStopToken,
    capture_tx: &SyncSender<CaptureMessage>,
) -> Result<(), CaptureFailure> {
    initialize_mta().ok().map_err(|error| {
        CaptureFailure::internal(format!("failed to initialize COM MTA: {error}"))
    })?;
    let _com = ComGuard;

    let enumerator = wasapi::DeviceEnumerator::new().map_err(|error| {
        CaptureFailure::internal(format!(
            "failed to create WASAPI device enumerator: {error}"
        ))
    })?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .map_err(|error| {
            CaptureFailure::source_unavailable(format!(
                "default playback endpoint is unavailable: {error}"
            ))
        })?;
    let endpoint_id = device.get_id().map_err(|error| {
        CaptureFailure::source_unavailable(format!(
            "failed to read default playback endpoint identity: {error}"
        ))
    })?;
    let endpoint_name = device.get_friendlyname().map_err(|error| {
        CaptureFailure::source_unavailable(format!(
            "failed to read default playback endpoint name: {error}"
        ))
    })?;
    let mut audio_client = device.get_iaudioclient().map_err(|error| {
        CaptureFailure::source_unavailable(format!(
            "failed to open default playback endpoint: {error}"
        ))
    })?;
    let native_format = audio_client.get_mixformat().map_err(|error| {
        CaptureFailure::unsupported_format(format!(
            "failed to read default playback endpoint format: {error}"
        ))
    })?;
    let native_sample_rate_hz = native_format.get_samplespersec();
    let native_channel_count = native_format.get_nchannels();
    let output_channel_count = match native_channel_count {
        0 => {
            return Err(CaptureFailure::unsupported_format(
                "default playback endpoint reported zero channels",
            ))
        }
        1 => 1,
        _ => 2,
    };
    let format = CaptureFormat::mono_or_stereo(native_sample_rate_hz, output_channel_count)
        .map_err(|error| CaptureFailure::unsupported_format(error.to_string()))?;
    let desired_format = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        native_sample_rate_hz as usize,
        output_channel_count as usize,
        None,
    );

    let (default_period_hns, _) = audio_client.get_device_period().map_err(|error| {
        CaptureFailure::source_unavailable(format!(
            "failed to query default playback endpoint period: {error}"
        ))
    })?;
    audio_client
        .initialize_client(
            &desired_format,
            &Direction::Capture,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: default_period_hns,
            },
        )
        .map_err(|error| {
            CaptureFailure::unsupported_format(format!(
                "default playback format cannot be converted to supported {} Hz / {} channel f32: {error}",
                native_sample_rate_hz, output_channel_count
            ))
        })?;
    let event_handle = audio_client.set_get_eventhandle().map_err(|error| {
        CaptureFailure::internal(format!("failed to create WASAPI event handle: {error}"))
    })?;
    let maximum_packet_frames = audio_client.get_buffer_size().map_err(|error| {
        CaptureFailure::internal(format!("failed to query WASAPI buffer size: {error}"))
    })?;
    let capture_client = audio_client.get_audiocaptureclient().map_err(|error| {
        CaptureFailure::internal(format!("failed to create WASAPI capture client: {error}"))
    })?;
    let bytes_per_frame = format.bytes_per_frame();
    let buffer_bytes = usize::try_from(maximum_packet_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(bytes_per_frame))
        .ok_or_else(|| CaptureFailure::internal("WASAPI buffer size overflowed usize"))?;

    let end_signal = Arc::new(AtomicU8::new(END_NONE));
    let _device_events = register_device_events(&enumerator, &endpoint_id, end_signal.clone())
        .map_err(|error| {
            CaptureFailure::internal(format!(
                "failed to register playback endpoint notifications: {error}"
            ))
        })?;
    let session_control = audio_client.get_audiosessioncontrol().map_err(|error| {
        CaptureFailure::internal(format!("failed to open WASAPI session control: {error}"))
    })?;
    let _session_events =
        register_session_events(&session_control, end_signal.clone()).map_err(|error| {
            CaptureFailure::internal(format!(
                "failed to register WASAPI session notifications: {error}"
            ))
        })?;

    let (recycle_tx, recycle_rx) = mpsc::sync_channel(HANDOFF_CAPACITY);
    for _ in 0..HANDOFF_CAPACITY {
        recycle_tx.try_send(vec![0_u8; buffer_bytes]).map_err(|_| {
            CaptureFailure::internal("failed to initialize bounded capture buffer pool")
        })?;
    }
    audio_client.start_stream().map_err(|error| {
        CaptureFailure::source_unavailable(format!(
            "default playback endpoint could not start: {error}"
        ))
    })?;
    capture_tx
        .send(CaptureMessage::Started(CaptureStarted {
            endpoint_name,
            native_sample_rate_hz,
            native_channel_count,
            maximum_packet_frames,
            format,
            recycle_tx,
        }))
        .map_err(|_| CaptureFailure::internal("processing path closed before stream start"))?;
    let deadline = match limit {
        RunLimit::UntilStopped => None,
        RunLimit::Duration(duration) => {
            Some(Instant::now().checked_add(duration).ok_or_else(|| {
                CaptureFailure::internal("capture duration exceeds the monotonic clock range")
            })?)
        }
    };
    let mut previous_callback = None;
    let loop_end = capture_loop(
        &capture_client,
        &event_handle,
        bytes_per_frame,
        deadline,
        &stop,
        &end_signal,
        &recycle_rx,
        capture_tx,
        &mut previous_callback,
    );
    let stop_result = audio_client.stop_stream();

    let termination = match (loop_end, stop_result) {
        (Ok(end), Ok(())) => CaptureTermination::new(end),
        (Err(termination), _) => termination,
        (_, Err(error)) => wasapi_stream_termination("failed to stop WASAPI stream", error),
    };
    capture_tx
        .send(CaptureMessage::Ended(termination))
        .map_err(|_| CaptureFailure::internal("processing path closed before stream end"))
}

#[allow(clippy::too_many_arguments)]
fn capture_loop(
    capture_client: &wasapi::AudioCaptureClient,
    event_handle: &wasapi::Handle,
    bytes_per_frame: usize,
    deadline: Option<Instant>,
    stop: &CaptureStopToken,
    end_signal: &AtomicU8,
    recycle_rx: &Receiver<Vec<u8>>,
    capture_tx: &SyncSender<CaptureMessage>,
    previous_callback: &mut Option<Instant>,
) -> Result<CaptureEnd, CaptureTermination> {
    loop {
        if stop.is_stop_requested() {
            return Ok(CaptureEnd::StopRequested);
        }
        match end_signal.load(Ordering::Acquire) {
            END_NONE => {}
            END_SOURCE_RECONFIGURED => return Ok(CaptureEnd::SourceReconfigured),
            END_SOURCE_UNAVAILABLE => return Ok(CaptureEnd::SourceUnavailable),
            _ => return Ok(CaptureEnd::Interrupted),
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(CaptureEnd::DurationElapsed);
        }

        match event_handle.wait_for_event(EVENT_WAIT_TIMEOUT_MS) {
            Ok(()) => {}
            Err(wasapi::WasapiError::EventTimeout) => continue,
            Err(error) => return Err(wasapi_stream_termination("WASAPI event wait failed", error)),
        }

        let callback_started = Instant::now();
        let mut callback_interval =
            previous_callback.map(|previous| callback_started.duration_since(previous));
        *previous_callback = Some(callback_started);

        loop {
            let packet_frames = capture_client
                .get_next_packet_size()
                .map_err(|error| {
                    wasapi_stream_termination("failed to query the next WASAPI packet", error)
                })?
                .unwrap_or(0);
            if packet_frames == 0 {
                break;
            }

            let callback_work_started = Instant::now();
            let mut buffer = match recycle_rx.try_recv() {
                Ok(buffer) => buffer,
                Err(TryRecvError::Empty) => return Ok(CaptureEnd::BoundedHandoffExhausted),
                Err(TryRecvError::Disconnected) => {
                    return Err(CaptureTermination::failed(
                        "capture buffer recycle channel closed".to_string(),
                    ))
                }
            };
            let expected_bytes = usize::try_from(packet_frames)
                .ok()
                .and_then(|frames| frames.checked_mul(bytes_per_frame))
                .ok_or_else(|| {
                    CaptureTermination::failed("WASAPI packet size overflowed usize".to_string())
                })?;
            if expected_bytes > buffer.len() {
                return Err(CaptureTermination::failed(format!(
                    "WASAPI packet requires {expected_bytes} bytes but the bounded buffer holds {}",
                    buffer.len()
                )));
            }

            let (actual_frames, info) =
                capture_client
                    .read_from_device(&mut buffer)
                    .map_err(|error| {
                        wasapi_stream_termination("failed to read a WASAPI packet", error)
                    })?;
            let callback_duration = callback_work_started.elapsed();
            let byte_len = usize::try_from(actual_frames)
                .ok()
                .and_then(|frames| frames.checked_mul(bytes_per_frame))
                .ok_or_else(|| {
                    CaptureTermination::failed("captured packet size overflowed usize".to_string())
                })?;
            let packet = CapturePacket::new(
                buffer,
                byte_len,
                actual_frames,
                info.index,
                info.timestamp,
                PacketFlags {
                    data_discontinuity: info.flags.data_discontinuity,
                    silent: info.flags.silent,
                    timestamp_error: info.flags.timestamp_error,
                },
                callback_interval,
                callback_duration,
            )
            .map_err(|error| {
                CaptureTermination::with_diagnostic(
                    CaptureEnd::DataDiscontinuity,
                    error.to_string(),
                )
            })?;
            callback_interval = None;

            match capture_tx.try_send(CaptureMessage::Packet(packet)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => return Ok(CaptureEnd::BoundedHandoffExhausted),
                Err(TrySendError::Disconnected(_)) => {
                    return Err(CaptureTermination::failed(
                        "processing path closed during capture".to_string(),
                    ))
                }
            }
        }
    }
}

fn register_device_events(
    enumerator: &wasapi::DeviceEnumerator,
    endpoint_id: &str,
    end_signal: Arc<AtomicU8>,
) -> Result<wasapi::DeviceEventRegistration, wasapi::WasapiError> {
    let mut callbacks = DeviceEventCallbacks::new();
    let removed_id = endpoint_id.to_string();
    let removed_signal = end_signal.clone();
    callbacks.set_device_removed_callback(move |id| {
        if id == removed_id {
            set_end_once(&removed_signal, END_SOURCE_UNAVAILABLE);
        }
    });

    let state_id = endpoint_id.to_string();
    let state_signal = end_signal.clone();
    callbacks.set_device_state_callback(move |id, state| {
        if id == state_id && state != DeviceState::Active {
            set_end_once(&state_signal, END_SOURCE_UNAVAILABLE);
        }
    });

    let default_id = endpoint_id.to_string();
    callbacks.set_default_device_callback(move |direction, role, id| {
        if direction == Direction::Render
            && role == Role::Console
            && id.as_deref() != Some(default_id.as_str())
        {
            set_end_once(&end_signal, END_SOURCE_RECONFIGURED);
        }
    });
    enumerator.register_notification_callback(callbacks)
}

fn register_session_events(
    session_control: &wasapi::AudioSessionControl,
    end_signal: Arc<AtomicU8>,
) -> Result<wasapi::EventRegistration, wasapi::WasapiError> {
    let mut callbacks = EventCallbacks::new();
    callbacks.set_disconnected_callback(move |reason| {
        let end = match reason {
            DisconnectReason::DeviceRemoval => END_SOURCE_UNAVAILABLE,
            DisconnectReason::FormatChanged => END_SOURCE_RECONFIGURED,
            _ => END_INTERRUPTED,
        };
        set_end_once(&end_signal, end);
    });
    session_control.register_session_notification(callbacks)
}

fn set_end_once(signal: &AtomicU8, value: u8) {
    let _ = signal.compare_exchange(END_NONE, value, Ordering::AcqRel, Ordering::Acquire);
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        deinitialize();
    }
}

#[derive(Debug)]
struct CaptureStarted {
    endpoint_name: String,
    native_sample_rate_hz: u32,
    native_channel_count: u16,
    maximum_packet_frames: u32,
    format: CaptureFormat,
    recycle_tx: SyncSender<Vec<u8>>,
}

#[derive(Debug)]
enum CaptureMessage {
    Started(CaptureStarted),
    Packet(CapturePacket),
    Ended(CaptureTermination),
    Failed(CaptureFailure),
}

#[derive(Debug)]
struct CaptureFailure {
    kind: ErrorKind,
    retry_hint: RetryHint,
    message: String,
}

impl CaptureFailure {
    fn source_unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::SourceUnavailable,
            retry_hint: RetryHint::WaitForSource,
            message: message.into(),
        }
    }

    fn unsupported_format(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::UnsupportedFormat,
            retry_hint: RetryHint::ChangeFormat,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            retry_hint: RetryHint::RetryLater,
            message: message.into(),
        }
    }
}

impl From<CaptureFailure> for CaptureRunError {
    fn from(failure: CaptureFailure) -> Self {
        match failure.kind {
            ErrorKind::SourceUnavailable => Self::SourceUnavailable(failure.message),
            ErrorKind::UnsupportedFormat => Self::UnsupportedFormat(failure.message),
            _ => Self::Backend(failure.message),
        }
    }
}

#[derive(Debug)]
struct CaptureTermination {
    end: CaptureEnd,
    diagnostic: Option<String>,
}

impl CaptureTermination {
    fn new(end: CaptureEnd) -> Self {
        Self {
            end,
            diagnostic: None,
        }
    }

    fn failed(diagnostic: String) -> Self {
        Self::with_diagnostic(CaptureEnd::Failed, diagnostic)
    }

    fn with_diagnostic(end: CaptureEnd, diagnostic: String) -> Self {
        Self {
            end,
            diagnostic: Some(diagnostic),
        }
    }
}

fn wasapi_stream_termination(context: &str, error: wasapi::WasapiError) -> CaptureTermination {
    let end = match &error {
        wasapi::WasapiError::DeviceNotFound(_) | wasapi::WasapiError::IllegalDeviceState(_) => {
            CaptureEnd::SourceUnavailable
        }
        wasapi::WasapiError::Windows(error) if error.code().0 == AUDCLNT_E_DEVICE_INVALIDATED => {
            CaptureEnd::SourceUnavailable
        }
        wasapi::WasapiError::UnsupportedFormat | wasapi::WasapiError::UnsupportedSubformat(_) => {
            CaptureEnd::SourceReconfigured
        }
        _ => CaptureEnd::Interrupted,
    };
    CaptureTermination::with_diagnostic(end, format!("{context}: {error}"))
}

/// Failure to establish or operate the Windows capture boundary.
///
/// [`Self::kind`] and [`Self::retry_hint`] are stable, machine-actionable
/// categories. [`fmt::Display`] is human diagnostic text and is not an API.
#[non_exhaustive]
#[derive(Debug)]
pub enum CaptureRunError {
    InvalidDuration,
    SourceUnavailable(String),
    UnsupportedFormat(String),
    Spawn(std::io::Error),
    CaptureChannelClosed,
    CaptureThreadPanicked,
    Backend(String),
    Contract(String),
    Protocol(&'static str),
    EvidenceOverflow,
    Capture(CaptureError),
}

impl CaptureRunError {
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidDuration => ErrorKind::InvalidRequest,
            Self::SourceUnavailable(_) => ErrorKind::SourceUnavailable,
            Self::UnsupportedFormat(_) => ErrorKind::UnsupportedFormat,
            Self::Capture(CaptureError::UnsupportedChannelCount(_))
            | Self::Capture(CaptureError::InvalidFormat(_)) => ErrorKind::UnsupportedFormat,
            Self::Capture(_) => ErrorKind::StreamInterrupted,
            Self::Spawn(_)
            | Self::CaptureChannelClosed
            | Self::CaptureThreadPanicked
            | Self::Backend(_)
            | Self::Contract(_)
            | Self::Protocol(_)
            | Self::EvidenceOverflow => ErrorKind::Internal,
        }
    }

    pub const fn retry_hint(&self) -> RetryHint {
        match self {
            Self::InvalidDuration => RetryHint::DoNotRetry,
            Self::SourceUnavailable(_) => RetryHint::WaitForSource,
            Self::UnsupportedFormat(_) => RetryHint::ChangeFormat,
            Self::Capture(CaptureError::UnsupportedChannelCount(_))
            | Self::Capture(CaptureError::InvalidFormat(_)) => RetryHint::ChangeFormat,
            Self::Capture(_) => RetryHint::RetryNow,
            Self::Backend(_) => RetryHint::RetryLater,
            Self::Spawn(_)
            | Self::CaptureChannelClosed
            | Self::CaptureThreadPanicked
            | Self::Contract(_)
            | Self::Protocol(_)
            | Self::EvidenceOverflow => RetryHint::DoNotRetry,
        }
    }
}

impl fmt::Display for CaptureRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDuration => formatter.write_str("capture duration must be non-zero"),
            Self::SourceUnavailable(message)
            | Self::UnsupportedFormat(message)
            | Self::Backend(message) => formatter.write_str(message),
            Self::Spawn(error) => write!(formatter, "failed to spawn capture thread: {error}"),
            Self::CaptureChannelClosed => {
                formatter.write_str("capture channel closed without a terminal event")
            }
            Self::CaptureThreadPanicked => formatter.write_str("capture thread panicked"),
            Self::Contract(message) => write!(formatter, "provider contract error: {message}"),
            Self::Protocol(message) => write!(formatter, "capture protocol error: {message}"),
            Self::EvidenceOverflow => formatter.write_str("capture evidence counter overflowed"),
            Self::Capture(error) => error.fmt(formatter),
        }
    }
}

impl Error for CaptureRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Capture(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(frame_index: u64, qpc_timestamp_100ns: u64) -> CapturePacket {
        let samples = [0.25_f32, -0.25_f32];
        let mut bytes = Vec::with_capacity(samples.len() * std::mem::size_of::<f32>());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let byte_len = bytes.len();
        CapturePacket::new(
            bytes,
            byte_len,
            1,
            frame_index,
            qpc_timestamp_100ns,
            PacketFlags::default(),
            None,
            Duration::ZERO,
        )
        .unwrap()
    }

    fn run_fake_stream() -> (CaptureReport, Vec<StreamEvent>) {
        let (capture_tx, capture_rx) = mpsc::sync_channel(4);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel(2);
        capture_tx
            .send(CaptureMessage::Started(CaptureStarted {
                endpoint_name: "fake endpoint".to_string(),
                native_sample_rate_hz: 48_000,
                native_channel_count: 2,
                maximum_packet_frames: 480,
                format: CaptureFormat::mono_or_stereo(48_000, 2).unwrap(),
                recycle_tx,
            }))
            .unwrap();
        capture_tx
            .send(CaptureMessage::Packet(packet(100, 1_000)))
            .unwrap();
        capture_tx
            .send(CaptureMessage::Packet(packet(101, 1_208)))
            .unwrap();
        capture_tx
            .send(CaptureMessage::Ended(CaptureTermination::new(
                CaptureEnd::DurationElapsed,
            )))
            .unwrap();

        let mut events = Vec::new();
        let mut on_event = |event| events.push(event);
        let report = process_capture(capture_rx, CaptureStopToken::new(), &mut on_event).unwrap();
        (report, events)
    }

    #[test]
    fn bounded_handoff_delivers_every_packet_and_recycles_every_buffer() {
        let (report, events) = run_fake_stream();

        assert_eq!(report.packet_count, 2);
        assert_eq!(report.audio_frame_count, 2);
        assert_eq!(report.source_frame_count, 2);
        assert_eq!(report.end, CaptureEnd::DurationElapsed);
        assert_eq!(report.end_diagnostic, None);
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], StreamEvent::Started(_)));
        assert!(matches!(events[1], StreamEvent::Data(_)));
        assert!(matches!(events[2], StreamEvent::Data(_)));
        assert!(matches!(
            events[3],
            StreamEvent::Ended {
                reason: StreamEndReason::ConsumerCancelled,
                ..
            }
        ));
    }

    #[test]
    fn overload_is_explicit_and_machine_actionable() {
        let source_id = SourceId::new("default-playback").unwrap();
        let stream_id = StreamId::new("stream-test").unwrap();
        let (error, reason) = contract_end(
            CaptureEnd::BoundedHandoffExhausted,
            None,
            &source_id,
            &stream_id,
        );
        let error = error.unwrap();

        assert_eq!(error.kind(), ErrorKind::ResourceExhausted);
        assert_eq!(error.retry_hint(), RetryHint::RetryLater);
        assert_eq!(reason, StreamEndReason::Failed);
        assert!(error
            .message()
            .contains("no audio packet was silently dropped"));
    }

    #[test]
    fn normal_stop_and_platform_boundaries_map_to_contract_lifecycle() {
        let source_id = SourceId::new("default-playback").unwrap();
        let stream_id = StreamId::new("stream-test").unwrap();

        let (error, reason) = contract_end(CaptureEnd::StopRequested, None, &source_id, &stream_id);
        assert!(error.is_none());
        assert_eq!(reason, StreamEndReason::ProviderShutdown);

        let cases = [
            (
                CaptureEnd::SourceReconfigured,
                ErrorKind::StreamInterrupted,
                RetryHint::RetryNow,
                StreamEndReason::SourceReconfigured,
            ),
            (
                CaptureEnd::SourceUnavailable,
                ErrorKind::SourceUnavailable,
                RetryHint::WaitForSource,
                StreamEndReason::SourceEnded,
            ),
            (
                CaptureEnd::Interrupted,
                ErrorKind::StreamInterrupted,
                RetryHint::RetryLater,
                StreamEndReason::Failed,
            ),
            (
                CaptureEnd::DataDiscontinuity,
                ErrorKind::StreamInterrupted,
                RetryHint::RetryNow,
                StreamEndReason::Failed,
            ),
            (
                CaptureEnd::Failed,
                ErrorKind::Internal,
                RetryHint::RetryLater,
                StreamEndReason::Failed,
            ),
        ];
        for (end, kind, retry, expected_reason) in cases {
            let (error, reason) =
                contract_end(end, Some("diagnostic detail"), &source_id, &stream_id);
            let error = error.unwrap();
            assert_eq!(error.kind(), kind);
            assert_eq!(error.retry_hint(), retry);
            assert_eq!(reason, expected_reason);
            assert_eq!(error.message(), "diagnostic detail");
        }
    }

    #[test]
    fn separate_capture_runs_restart_stream_identity_and_timeline() {
        let (_, first_events) = run_fake_stream();
        let (_, second_events) = run_fake_stream();

        let first_stream = match &first_events[0] {
            StreamEvent::Started(descriptor) => descriptor.stream_id(),
            _ => panic!("first event was not stream start"),
        };
        let second_stream = match &second_events[0] {
            StreamEvent::Started(descriptor) => descriptor.stream_id(),
            _ => panic!("first event was not stream start"),
        };
        assert_ne!(first_stream, second_stream);

        for events in [&first_events, &second_events] {
            match &events[1] {
                StreamEvent::Data(packet) => match packet.payload() {
                    SignalPayload::Waveform(frame) => {
                        assert_eq!(frame.window().start().frame_index(), 0);
                        assert_eq!(frame.window().start().stream_time_ns(), 0);
                    }
                    _ => panic!("fake capture did not emit waveform data"),
                },
                _ => panic!("second event was not stream data"),
            }
        }
    }

    #[test]
    fn run_errors_expose_categories_separately_from_diagnostics() {
        let invalid_duration = CaptureRunError::InvalidDuration;
        assert_eq!(invalid_duration.kind(), ErrorKind::InvalidRequest);
        assert_eq!(invalid_duration.retry_hint(), RetryHint::DoNotRetry);

        let unsupported = CaptureRunError::Capture(CaptureError::UnsupportedChannelCount(6));
        assert_eq!(unsupported.kind(), ErrorKind::UnsupportedFormat);
        assert_eq!(unsupported.retry_hint(), RetryHint::ChangeFormat);
        assert!(unsupported.to_string().contains("one or two channels"));

        let unavailable = CaptureRunError::SourceUnavailable("endpoint missing".to_string());
        assert_eq!(unavailable.kind(), ErrorKind::SourceUnavailable);
        assert_eq!(unavailable.retry_hint(), RetryHint::WaitForSource);
    }

    #[test]
    fn runtime_backend_errors_retain_lifecycle_categories() {
        let unavailable = wasapi_stream_termination(
            "packet query failed",
            wasapi::WasapiError::IllegalDeviceState(0),
        );
        assert_eq!(unavailable.end, CaptureEnd::SourceUnavailable);
        assert!(unavailable
            .diagnostic
            .unwrap()
            .contains("packet query failed"));

        let format_changed =
            wasapi_stream_termination("packet read failed", wasapi::WasapiError::UnsupportedFormat);
        assert_eq!(format_changed.end, CaptureEnd::SourceReconfigured);
    }
}
