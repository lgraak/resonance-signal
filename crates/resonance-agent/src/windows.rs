//! Bounded Windows playback-loopback prototype using `wasapi` 0.24.

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

const DEFAULT_HANDOFF_CAPACITY: usize = 4;
const EVENT_WAIT_TIMEOUT_MS: u32 = 100;
const END_NONE: u8 = 0;
const END_SOURCE_RECONFIGURED: u8 = 1;
const END_SOURCE_UNAVAILABLE: u8 = 2;
const END_INTERRUPTED: u8 = 3;

static STREAM_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Settings for one evidence-gathering capture run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrototypeConfig {
    duration: Duration,
    handoff_capacity: usize,
}

impl PrototypeConfig {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            handoff_capacity: DEFAULT_HANDOFF_CAPACITY,
        }
    }

    pub fn with_handoff_capacity(mut self, capacity: usize) -> Result<Self, PrototypeError> {
        if capacity == 0 {
            return Err(PrototypeError::InvalidHandoffCapacity);
        }
        self.handoff_capacity = capacity;
        Ok(self)
    }
}

/// Measurements gathered without adding work to the WASAPI event thread.
#[derive(Clone, Debug)]
pub struct PrototypeEvidence {
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
    pub end: PrototypeEnd,
}

impl fmt::Display for PrototypeEvidence {
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
        write!(formatter, "  stream end: {:?}", self.end)
    }
}

/// Why the capture owner stopped. Every non-duration reason is a stream boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrototypeEnd {
    DurationElapsed,
    SourceReconfigured,
    SourceUnavailable,
    Interrupted,
    DataDiscontinuity(String),
    BoundedHandoffExhausted,
    Failed(String),
}

/// Runs one bounded default-playback loopback stream and emits the existing
/// provider contract to `on_event` on the non-real-time processing thread.
pub fn run_default_playback_loopback(
    config: PrototypeConfig,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<PrototypeEvidence, PrototypeError> {
    if config.duration.is_zero() {
        return Err(PrototypeError::InvalidDuration);
    }
    if config.handoff_capacity == 0 {
        return Err(PrototypeError::InvalidHandoffCapacity);
    }

    let (capture_tx, capture_rx) = mpsc::sync_channel(config.handoff_capacity);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let capture_stop = stop_requested.clone();
    let handle = thread::Builder::new()
        .name("resonance-wasapi-loopback".to_string())
        .spawn(move || capture_thread(config, capture_stop, capture_tx))
        .map_err(PrototypeError::Spawn)?;

    let result = process_capture(capture_rx, stop_requested, &mut on_event);
    let join_result = handle
        .join()
        .map_err(|_| PrototypeError::CaptureThreadPanicked)?;
    if let Err(error) = join_result {
        return Err(PrototypeError::Wasapi(error));
    }
    result
}

fn process_capture(
    capture_rx: Receiver<CaptureMessage>,
    stop_requested: Arc<AtomicBool>,
    on_event: &mut impl FnMut(StreamEvent),
) -> Result<PrototypeEvidence, PrototypeError> {
    let started = match capture_rx
        .recv()
        .map_err(|_| PrototypeError::CaptureChannelClosed)?
    {
        CaptureMessage::Started(started) => started,
        CaptureMessage::Failed(message) => return Err(PrototypeError::Wasapi(message)),
        CaptureMessage::Packet(_) | CaptureMessage::Ended(_) => {
            return Err(PrototypeError::Protocol("capture did not start first"))
        }
    };

    let stream_id = StreamId::new(format!(
        "wasapi-loopback-{}-{}",
        std::process::id(),
        STREAM_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
    .map_err(|error| PrototypeError::Contract(error.to_string()))?;
    let source_id = SourceId::new("default-playback")
        .map_err(|error| PrototypeError::Contract(error.to_string()))?;
    let descriptor = StreamDescriptor::new(
        stream_id.clone(),
        source_id.clone(),
        SourceKind::Playback,
        started.format.sample_rate(),
        started.format.channels().clone(),
    );
    on_event(StreamEvent::Started(descriptor));

    let mut evidence = PrototypeEvidence {
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
        end: PrototypeEnd::Failed("capture channel closed without an end event".to_string()),
    };
    let mut builder = AudioFrameBuilder::new(started.format);

    loop {
        let message = capture_rx
            .recv()
            .map_err(|_| PrototypeError::CaptureChannelClosed)?;
        match message {
            CaptureMessage::Packet(packet) => {
                update_packet_evidence(&mut evidence, &packet)?;
                let built = builder.push(&packet);
                let recycle_result = started.recycle_tx.try_send(packet.into_buffer());
                if recycle_result.is_err() {
                    stop_requested.store(true, Ordering::Release);
                    return Err(PrototypeError::Protocol("capture buffer recycle failed"));
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
                            .ok_or(PrototypeError::EvidenceOverflow)?;
                        evidence.source_frame_count = evidence
                            .source_frame_count
                            .checked_add(u64::from(built.frame().window().frame_count()))
                            .ok_or(PrototypeError::EvidenceOverflow)?;
                        on_event(StreamEvent::Data(SignalPacket::new(
                            stream_id.clone(),
                            SignalPayload::Waveform(built.into_frame()),
                        )));
                    }
                    Err(error) => {
                        stop_requested.store(true, Ordering::Release);
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
                        evidence.end = PrototypeEnd::DataDiscontinuity(message);
                        drain_until_end(&capture_rx);
                        return Ok(evidence);
                    }
                }
            }
            CaptureMessage::Ended(end) => {
                evidence.end = end.clone();
                let (error, reason) = contract_end(&end, &source_id, &stream_id);
                if let Some(error) = error {
                    on_event(StreamEvent::Error(error));
                }
                on_event(StreamEvent::Ended { stream_id, reason });
                return Ok(evidence);
            }
            CaptureMessage::Failed(message) => {
                on_event(StreamEvent::Error(ProviderError::new(
                    ErrorKind::Internal,
                    ErrorScope::Stream(stream_id.clone()),
                    RetryHint::RetryLater,
                    message.clone(),
                )));
                on_event(StreamEvent::Ended {
                    stream_id,
                    reason: StreamEndReason::Failed,
                });
                evidence.end = PrototypeEnd::Failed(message);
                return Ok(evidence);
            }
            CaptureMessage::Started(_) => {
                stop_requested.store(true, Ordering::Release);
                return Err(PrototypeError::Protocol("capture started more than once"));
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
    end: &PrototypeEnd,
    source_id: &SourceId,
    stream_id: &StreamId,
) -> (Option<ProviderError>, StreamEndReason) {
    match end {
        PrototypeEnd::DurationElapsed => (None, StreamEndReason::ConsumerCancelled),
        PrototypeEnd::SourceReconfigured => (
            Some(ProviderError::new(
                ErrorKind::StreamInterrupted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryNow,
                "default playback device or stream format changed",
            )),
            StreamEndReason::SourceReconfigured,
        ),
        PrototypeEnd::SourceUnavailable => (
            Some(ProviderError::new(
                ErrorKind::SourceUnavailable,
                ErrorScope::Source(source_id.clone()),
                RetryHint::WaitForSource,
                "default playback endpoint became unavailable",
            )),
            StreamEndReason::SourceEnded,
        ),
        PrototypeEnd::Interrupted => (
            Some(ProviderError::new(
                ErrorKind::StreamInterrupted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryLater,
                "WASAPI audio session was interrupted",
            )),
            StreamEndReason::Failed,
        ),
        PrototypeEnd::BoundedHandoffExhausted => (
            Some(ProviderError::new(
                ErrorKind::ResourceExhausted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryLater,
                "bounded capture handoff exhausted; no audio packet was silently dropped",
            )),
            StreamEndReason::Failed,
        ),
        PrototypeEnd::DataDiscontinuity(message) | PrototypeEnd::Failed(message) => (
            Some(ProviderError::new(
                ErrorKind::StreamInterrupted,
                ErrorScope::Stream(stream_id.clone()),
                RetryHint::RetryNow,
                message.clone(),
            )),
            StreamEndReason::Failed,
        ),
    }
}

fn update_packet_evidence(
    evidence: &mut PrototypeEvidence,
    packet: &CapturePacket,
) -> Result<(), PrototypeError> {
    evidence.packet_count = evidence
        .packet_count
        .checked_add(1)
        .ok_or(PrototypeError::EvidenceOverflow)?;
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

fn capture_thread(
    config: PrototypeConfig,
    stop_requested: Arc<AtomicBool>,
    capture_tx: SyncSender<CaptureMessage>,
) -> Result<(), String> {
    if let Err(error) = capture_thread_inner(config, stop_requested, &capture_tx) {
        let _ = capture_tx.send(CaptureMessage::Failed(error.clone()));
        return Err(error);
    }
    Ok(())
}

fn capture_thread_inner(
    config: PrototypeConfig,
    stop_requested: Arc<AtomicBool>,
    capture_tx: &SyncSender<CaptureMessage>,
) -> Result<(), String> {
    initialize_mta()
        .ok()
        .map_err(|error| format!("failed to initialize COM MTA: {error}"))?;
    let _com = ComGuard;

    let enumerator = wasapi::DeviceEnumerator::new().map_err(display_error)?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .map_err(display_error)?;
    let endpoint_id = device.get_id().map_err(display_error)?;
    let endpoint_name = device.get_friendlyname().map_err(display_error)?;
    let mut audio_client = device.get_iaudioclient().map_err(display_error)?;
    let native_format = audio_client.get_mixformat().map_err(display_error)?;
    let native_sample_rate_hz = native_format.get_samplespersec();
    let native_channel_count = native_format.get_nchannels();
    let output_channel_count = match native_channel_count {
        0 => return Err("default playback endpoint reported zero channels".to_string()),
        1 => 1,
        _ => 2,
    };
    let format = CaptureFormat::mono_or_stereo(native_sample_rate_hz, output_channel_count)
        .map_err(|error| error.to_string())?;
    let desired_format = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        native_sample_rate_hz as usize,
        output_channel_count as usize,
        None,
    );

    let (default_period_hns, _) = audio_client.get_device_period().map_err(display_error)?;
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
            format!(
                "default playback format cannot be converted to supported {} Hz / {} channel f32: {error}",
                native_sample_rate_hz, output_channel_count
            )
        })?;
    let event_handle = audio_client.set_get_eventhandle().map_err(display_error)?;
    let maximum_packet_frames = audio_client.get_buffer_size().map_err(display_error)?;
    let capture_client = audio_client
        .get_audiocaptureclient()
        .map_err(display_error)?;
    let bytes_per_frame = format.bytes_per_frame();
    let buffer_bytes = usize::try_from(maximum_packet_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(bytes_per_frame))
        .ok_or_else(|| "WASAPI buffer size overflowed usize".to_string())?;

    let end_signal = Arc::new(AtomicU8::new(END_NONE));
    let _device_events = register_device_events(&enumerator, &endpoint_id, end_signal.clone())
        .map_err(display_error)?;
    let session_control = audio_client
        .get_audiosessioncontrol()
        .map_err(display_error)?;
    let _session_events =
        register_session_events(&session_control, end_signal.clone()).map_err(display_error)?;

    let (recycle_tx, recycle_rx) = mpsc::sync_channel(config.handoff_capacity);
    for _ in 0..config.handoff_capacity {
        recycle_tx
            .try_send(vec![0_u8; buffer_bytes])
            .map_err(|_| "failed to initialize bounded capture buffer pool".to_string())?;
    }
    capture_tx
        .send(CaptureMessage::Started(CaptureStarted {
            endpoint_name,
            native_sample_rate_hz,
            native_channel_count,
            maximum_packet_frames,
            format,
            recycle_tx,
        }))
        .map_err(|_| "processing path closed before stream start".to_string())?;

    audio_client.start_stream().map_err(display_error)?;
    let deadline = Instant::now()
        .checked_add(config.duration)
        .ok_or_else(|| "capture duration exceeds the monotonic clock range".to_string())?;
    let mut previous_callback = None;
    let loop_end = capture_loop(
        &capture_client,
        &event_handle,
        bytes_per_frame,
        deadline,
        &stop_requested,
        &end_signal,
        &recycle_rx,
        capture_tx,
        &mut previous_callback,
    );
    let stop_result = audio_client.stop_stream().map_err(display_error);

    let end = match (loop_end, stop_result) {
        (Ok(end), Ok(())) => end,
        (Err(error), _) | (_, Err(error)) => PrototypeEnd::Failed(error),
    };
    capture_tx
        .send(CaptureMessage::Ended(end))
        .map_err(|_| "processing path closed before stream end".to_string())
}

#[allow(clippy::too_many_arguments)]
fn capture_loop(
    capture_client: &wasapi::AudioCaptureClient,
    event_handle: &wasapi::Handle,
    bytes_per_frame: usize,
    deadline: Instant,
    stop_requested: &AtomicBool,
    end_signal: &AtomicU8,
    recycle_rx: &Receiver<Vec<u8>>,
    capture_tx: &SyncSender<CaptureMessage>,
    previous_callback: &mut Option<Instant>,
) -> Result<PrototypeEnd, String> {
    loop {
        if stop_requested.load(Ordering::Acquire) {
            return Ok(PrototypeEnd::Interrupted);
        }
        match end_signal.load(Ordering::Acquire) {
            END_NONE => {}
            END_SOURCE_RECONFIGURED => return Ok(PrototypeEnd::SourceReconfigured),
            END_SOURCE_UNAVAILABLE => return Ok(PrototypeEnd::SourceUnavailable),
            _ => return Ok(PrototypeEnd::Interrupted),
        }
        if Instant::now() >= deadline {
            return Ok(PrototypeEnd::DurationElapsed);
        }

        match event_handle.wait_for_event(EVENT_WAIT_TIMEOUT_MS) {
            Ok(()) => {}
            Err(wasapi::WasapiError::EventTimeout) => continue,
            Err(error) => return Err(error.to_string()),
        }

        let callback_started = Instant::now();
        let mut callback_interval =
            previous_callback.map(|previous| callback_started.duration_since(previous));
        *previous_callback = Some(callback_started);

        loop {
            let packet_frames = capture_client
                .get_next_packet_size()
                .map_err(display_error)?
                .unwrap_or(0);
            if packet_frames == 0 {
                break;
            }

            let callback_work_started = Instant::now();
            let mut buffer = match recycle_rx.try_recv() {
                Ok(buffer) => buffer,
                Err(TryRecvError::Empty) => return Ok(PrototypeEnd::BoundedHandoffExhausted),
                Err(TryRecvError::Disconnected) => {
                    return Err("capture buffer recycle channel closed".to_string())
                }
            };
            let expected_bytes = usize::try_from(packet_frames)
                .ok()
                .and_then(|frames| frames.checked_mul(bytes_per_frame))
                .ok_or_else(|| "WASAPI packet size overflowed usize".to_string())?;
            if expected_bytes > buffer.len() {
                return Err(format!(
                    "WASAPI packet requires {expected_bytes} bytes but the bounded buffer holds {}",
                    buffer.len()
                ));
            }

            let (actual_frames, info) = capture_client
                .read_from_device(&mut buffer)
                .map_err(display_error)?;
            let callback_duration = callback_work_started.elapsed();
            let byte_len = usize::try_from(actual_frames)
                .ok()
                .and_then(|frames| frames.checked_mul(bytes_per_frame))
                .ok_or_else(|| "captured packet size overflowed usize".to_string())?;
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
            .map_err(|error| error.to_string())?;
            callback_interval = None;

            match capture_tx.try_send(CaptureMessage::Packet(packet)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => return Ok(PrototypeEnd::BoundedHandoffExhausted),
                Err(TrySendError::Disconnected(_)) => {
                    return Err("processing path closed during capture".to_string())
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

fn display_error(error: impl fmt::Display) -> String {
    error.to_string()
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
    Ended(PrototypeEnd),
    Failed(String),
}

#[non_exhaustive]
#[derive(Debug)]
pub enum PrototypeError {
    InvalidDuration,
    InvalidHandoffCapacity,
    Spawn(std::io::Error),
    CaptureChannelClosed,
    CaptureThreadPanicked,
    Wasapi(String),
    Contract(String),
    Protocol(&'static str),
    EvidenceOverflow,
    Capture(CaptureError),
}

impl fmt::Display for PrototypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDuration => formatter.write_str("capture duration must be non-zero"),
            Self::InvalidHandoffCapacity => {
                formatter.write_str("bounded handoff capacity must be non-zero")
            }
            Self::Spawn(error) => write!(formatter, "failed to spawn capture thread: {error}"),
            Self::CaptureChannelClosed => {
                formatter.write_str("capture channel closed without a terminal event")
            }
            Self::CaptureThreadPanicked => formatter.write_str("capture thread panicked"),
            Self::Wasapi(message) => formatter.write_str(message),
            Self::Contract(message) => write!(formatter, "provider contract error: {message}"),
            Self::Protocol(message) => write!(formatter, "capture protocol error: {message}"),
            Self::EvidenceOverflow => formatter.write_str("capture evidence counter overflowed"),
            Self::Capture(error) => error.fmt(formatter),
        }
    }
}

impl Error for PrototypeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Capture(error) => Some(error),
            _ => None,
        }
    }
}
