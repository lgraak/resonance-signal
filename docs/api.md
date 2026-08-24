# API

`resonance-api` exposes the transport-independent consumer contract. It depends on and re-exports the provider-independent signal types from `resonance-core`; it does not define a socket, service endpoint, serialization encoding, capture API, or callback runtime.

The current audio contract version is `0.1` (`AUDIO_CONTRACT_VERSION`). It is suitable for implementation work, but the crate and contract are pre-1.0 and therefore do not yet promise semver-stable Rust or wire compatibility.

## Signal model

### Waveform frames

`AudioFrame` is the baseline data product:

- Samples are finite `f32` linear PCM values.
- `-1.0` and `1.0` are nominal full scale. Values outside that range are retained rather than clipped.
- Samples are interleaved in sample-frame-major, channel-minor order.
- Each frame carries its `SampleRate`, `ChannelLayout`, `SignalWindow`, and owned sample buffer.
- A frame contains one or more complete sample frames. The provider may vary batch sizes; consumers must not assume a fixed batch duration.
- Positioned layouts preserve explicit channel order. A discrete layout preserves channel count and order when semantic positions are unknown.

Product capture support is narrower than the general signal type:

- Mono is one ordered channel. It is reported as positioned `Mono` when that meaning is known, or as discrete one-channel audio when the backend cannot establish a portable semantic position.
- Stereo is two ordered channels. A known stereo pair is reported as `FrontLeft`, then `FrontRight`; the interleaved sample order follows that layout. An unknown two-channel source may be accepted as a discrete two-channel layout, preserving channel zero/channel one order without inventing left/right positions.
- An explicitly positioned two-channel layout that is not a left/right stereo pair is not re-labelled as stereo. It requires a valid platform-provided mono/stereo representation or is unsupported.

`StreamDescriptor` reports the actual accepted layout at stream start, and every signal frame retains it. Consumers must inspect that value instead of assuming that every two-channel stream has known speaker positions. The core `ChannelLayout` remains capable of representing wider layouts for compatibility and provider-independent signal work; that generality does not make wider capture formats supported products.

`FrameTimestamp` has two stream-relative values:

- `frame_index` is the zero-based sample-frame position in one uninterrupted stream and is authoritative for detecting continuity.
- `stream_time_ns` is time in that stream's monotonic clock domain. It is not wall-clock time and is not comparable across provider restarts or distinct streams.

The first frame in a stream starts at index zero. Within a stream, waveform windows are contiguous and the format is fixed. A source interruption or format change ends that stream; a resumed source receives a new `StreamId` and timeline.

Frames are bounded, independently owned batches. The contract does not require an unbounded queue or prescribe transport backpressure. A future transport must bound buffering and surface overflow rather than silently accumulating data.

### Derived frames

Derived products are opt-in. A subscription can request waveform data, derived data, or both:

- `LevelFrame` contains per-channel RMS and sample peak over an explicit `SignalWindow`. RMS is `sqrt(mean(sample^2))`; peak is `max(abs(sample))`.
- `SpectrumFrame` contains channel-major, single-sided linear magnitude spectra. It reports the sample rate, source window, FFT size, window function, and channel layout needed to interpret each bin.
- Spectrum bin `n` is centered at `n * sample_rate / fft_size` hertz. Source windows shorter than the FFT size are zero-padded. Magnitudes are divided by the sum of window coefficients; all bins except DC and an even-sized FFT's Nyquist bin are doubled. A bin-centered sinusoid therefore reports its peak amplitude.
- `Rectangular` uses `w[n] = 1`. `Hann` uses the periodic form `w[n] = 0.5 - 0.5 * cos(2 * pi * n / N)` for `0 <= n < N`.

Raw waveform is the flexibility baseline and must remain available. Provider-computed levels and spectra avoid duplicate expensive work when several consumers need the same analysis. They do not replace raw data and are not forced on consumers. In contract `0.1`, the provider selects derived window length, hop, FFT size, and supported window function and reports the actual values in every derived frame. Capability discovery and analysis-parameter negotiation are deferred until capture and processing constraints are known.

## Processing primitives

`resonance-core::processing` provides small, provider-independent operations over the signal model:

- `WaveformWindow` is a borrowed, frame-aligned view over all or part of an `AudioFrame`. Creating a subwindow validates its bounds and advances its source frame index and stream-relative time. Reading interleaved samples or one channel does not copy or deinterleave the frame.
- `calculate_levels` consumes a `WaveformWindow` and returns a `LevelFrame` with per-channel RMS and maximum absolute sample magnitude. It walks interleaved samples once, retains channel layout and source-window alignment, accumulates squares as `f64`, and does not clip finite values outside nominal full scale.
- `rms` and `peak` calculate scalar levels for a non-empty finite sample slice. Empty or non-finite input returns `ProcessingError`.
- `peak_normalization_gain` and `normalize_peak_in_place` provide explicit opt-in peak normalization. Silence uses unity gain, invalid targets are rejected, and targets above `1.0` remain permitted. In-place normalization validates the complete input before changing it.

These functions are synchronous building blocks rather than a transport or service API. Level calculation uses only temporary and output storage proportional to channel count; a `WaveformWindow` itself is zero-copy and no waveform-sized processing buffer is allocated. Inputs are expected to remain small real-time batches rather than unbounded recordings.

FFT and spectrum calculation are intentionally not implemented. `SpectrumFrame` remains a separate optional contract shape so a later processing implementation can add spectra or frequency-band products without changing `AudioFrame`, `LevelFrame`, or forcing those products on every subscriber.

## Window scheduling

`resonance-core::scheduling::WindowScheduler` converts contiguous `AudioFrame` batches into complete, owned analysis windows. It is generic over the orchestration layer's stream identity type, so `resonance-core` remains independent of `resonance-api::StreamId`. Each `ScheduledWindow` retains that identity and contains an `AudioFrame`; existing processing functions consume it through `WaveformWindow::entire` without duplicated level or waveform logic.

The scheduler uses non-overlapping tumbling windows:

- `WindowSchedulerConfig::default()` targets `33,333,333 ns`, approximately one output per 30 FPS visualization update, and accepts at most eight windows of input per push.
- A `16,666,667 ns` target produces approximately 60 updates per second. At 48 kHz the 30 FPS and 60 FPS settings become exactly 1,600 and 800 sample frames.
- Target duration is converted to the nearest non-zero whole sample-frame count for each stream. Output cadence is sample-clock driven, not wall-clock driven.
- A window is emitted immediately when enough contiguous samples exist. Windows do not overlap, and the next window begins at the preceding window's end.
- Slower input remains as a partial window until more frames arrive. Empty input is a no-op; the scheduler never pads, repeats, synthesizes, or emits incomplete data.

Memory and per-call work are bounded. Retained input is always less than one complete window. The configured `max_windows_per_push` limits accepted input and the number of outputs a single call can create; oversized calls return `SchedulingError::OversizedInput` without changing scheduler state. The default therefore accepts at most about 267 ms of 30 FPS input per call. Output queuing and consumer backpressure remain transport responsibilities and must be bounded separately.

Continuity is checked before samples are combined. Source frame indices must be exact. Stream timestamps must match the preceding batch within one nanosecond, allowing only integer timestamp quantization. A frame-index gap, timestamp gap, overlap, or fixed-format change drops the partial window, returns an explicit error, and invalidates that stream identity. Processing resumes only with a new identity. A normal identity change also drops any old partial window, accepts the new stream independently, and reports a `StreamBoundary` containing the discarded frame count. Samples from distinct or discontinuous streams are never combined.

Scheduling is synchronous and arrival-driven. It does not create a timer, async task, capture callback, queue, service, transport, or timeout-based flush. Capture evidence may later justify overlapping hops or a different default, but those are configuration and orchestration decisions rather than changes to `AudioFrame`.

## Subscriptions and sources

`SubscriptionRequest` contains non-empty, duplicate-free lists of `SourceSelector` and `SignalProduct` values. A single request can name multiple sources and products.

Sources are selected as either:

- `Default(DefaultSource::Playback)` or `Default(DefaultSource::Capture)`, resolved when the subscription starts; or
- `Id(SourceId)`, where `SourceId` is an opaque provider-assigned identifier.

Selected playback devices, microphones, and virtual devices are all addressed by opaque ID. `SourceKind` describes the resolved source as playback, microphone, virtual, or other. The contract makes no claim that IDs are portable across hosts or stable after device removal. Device discovery, friendly names, capabilities, and persistence rules are later API work.

## Capture-provider boundary

Platform capture belongs behind orchestration in `resonance-agent`, not in `resonance-core` or `resonance-api`. The production Windows playback boundary uses `wasapi` 0.24.0; the selected future Linux approach is the official PipeWire Rust binding. Neither dependency is part of this API. A backend boundary resolves a selected source, negotiates and validates its format, starts and stops capture, and delivers bounded waveform batches or explicit lifecycle failures. A backend-specific generic framework or async runtime is not part of this contract.

Before a stream starts, capture orchestration must:

1. request a valid mono or stereo representation from the platform when one is available;
2. reject a selected format with more than two channels, or an incompatible explicitly positioned two-channel layout, unless the platform itself supplies a valid mono/stereo conversion;
3. normalize accepted native integer or floating-point samples into finite, interleaved `f32` samples while preserving ordering;
4. create opaque source and uninterrupted-stream identities; and
5. publish a `StreamDescriptor` containing the actual sample rate and accepted channel layout.

The descriptor and all frames in one stream have fixed sample rate and layout. Capture callback or polling boundaries become variable-sized, complete, bounded `AudioFrame` batches; they do not define analysis windows or stream boundaries. The first batch starts with source frame index zero. Later batches use contiguous source frame indices and stream-relative monotonic timestamps. Platform timing evidence must be converted conservatively into those existing semantics rather than being described as wall-clock time.

An interruption, restart, device reconfiguration, timing discontinuity, or format change ends the current stream. Resumption gets a new `StreamId`, frame index zero, and a new monotonic timeline. The orchestration layer passes accepted frames and their stream identity to `WindowScheduler`; the scheduler remains capture-backend independent.

If the platform cannot supply a valid mono or stereo representation of a wider source, orchestration reports `ErrorKind::UnsupportedFormat` for that source with an appropriate change-format or do-not-retry hint and does not start the stream. Diagnostics should retain the requested source, native/source format, requested mono/stereo constraints, selected or proposed platform format, whether platform conversion was attempted, and the backend's stable error code and message when available. The current contract may carry human-readable diagnostic context in `ProviderError::message`; a structured backend-diagnostics extension is deferred.

Blindly selecting the first two channels is invalid because channel order in a wider layout need not place the complete stereo program first and may omit content such as center-channel dialogue. Resonance Signal does not reorder, discard, duplicate, synthesize, or mix channels to force acceptance. A custom downmix requires a separate evidence-backed decision defining input layouts, matrices, normalization/headroom, LFE treatment, metadata handling, testing, and consumer-visible provenance.

One request may resolve to several `StreamDescriptor` values. Each descriptor identifies one uninterrupted source stream and fixes its source, sample rate, and channel layout. `StreamEvent` then carries lifecycle events, `SignalPacket` data, errors, and an explicit end reason. A format change is a stream boundary, not an in-place mutation.

## Failure model

`ProviderError` separates machine-actionable fields from diagnostic text:

- `ErrorKind`: source unavailable, permission denied, stream interrupted, unsupported format, invalid request, resource exhausted, or internal failure.
- `ErrorScope`: the subscription, a source, or a stream.
- `RetryHint`: retry now/later, wait for the source, request permission, change format, or do not retry.
- `message`: human-readable diagnostics only; consumers must not parse it for behavior.

These categories are platform-neutral. Backend error codes can be included in future structured diagnostics, but Windows or Linux error types must not leak into the shared contract.

## Stability

The intended stable 1.0 surface is:

- finite normalized `f32` waveform samples;
- interleaved frame and channel-order semantics;
- stream-relative frame index and monotonic timestamp meanings;
- opaque source and stream identities;
- multi-source, multi-product subscription semantics;
- stream-boundary rules and platform-neutral error categories.

The following remain experimental in 0.1:

- the Rust constructors and enum inventory;
- derived analysis configuration and exact product set;
- device discovery and capability negotiation;
- serialization field names and numeric encoding;
- transport, framing, delivery, backpressure, and authentication;
- wall-clock correlation between streams or hosts.

Backend selection is recorded in [ADR 0004](decisions/0004-capture-backend-selection.md), the production Windows lifecycle/buffering policy is recorded in [ADR 0005](decisions/0005-windows-capture-lifecycle-and-buffering.md), the explicit capture-owner state machine is recorded in [ADR 0006](decisions/0006-capture-owner-lifecycle.md), the supervisor boundary is recorded in [ADR 0007](decisions/0007-capture-supervisor-boundary.md), and the recovery decision model is recorded in [ADR 0008](decisions/0008-recovery-policy-boundary.md). These decisions do not change the consumer contract: platform types and dependencies remain private to `resonance-agent`.

## Windows playback-loopback capture

The production Windows boundary captures the console-role default rendering endpoint in WASAPI shared loopback mode. It requests finite interleaved 32-bit float PCM at the endpoint mix sample rate. A native mono mix remains mono. Every other non-zero native channel count is explicitly offered to the Windows audio engine as front-left/front-right stereo with automatic conversion; initialization fails as unsupported rather than selecting channels or performing a project-owned downmix. The active output descriptor never contains more than two channels.

`CaptureOwner` is the one-run resource owner beneath the production `CaptureSupervisor` lifecycle entry point. `CaptureOwner::new` creates an inert, single-use owner without allocating a thread or WASAPI resource. `start` creates one ordinary owner worker; `CaptureOwnerStart::Started` means that worker was created, while the existing `StreamEvent::Started` remains the authoritative notification that format validation succeeded and the stream became active. A stop requested while the owner is inert prevents initialization and completes as `StoppedBeforeStart`. A second `start` is rejected.

`request_stop` is idempotent. `wait_for_completion(timeout)` observes natural completion without requesting stop, joins the owner worker after completion, and retains the result. `shutdown(timeout)` requests stop and then uses the same wait path. A `CaptureOwnerCompletion` describes a normal capture report, capture failure, start failure, pre-start stop, or panic. If a wait expires, it returns `CaptureOwnerShutdownTimeout` and retains the worker, callback, and completion channel so the same owner can be waited again. Dropping a started owner is a final cleanup path that requests stop and joins. Event callbacks run on the ordinary owner worker, must return promptly, and have ended before a successful completion wait returns; they never run on the WASAPI thread.

`run_default_playback_loopback` and its `CaptureStopToken` remain the blocking adapter seam owned by `CaptureOwner`. `run_default_playback_loopback_for` remains available as a bounded diagnostic helper, but the command-line executable now validates the supervisor and owner lifecycle together: it starts an indefinite run, waits for `--duration-seconds`, requests supervisor stop, and allows two seconds for completion. The duration accepts 1 through 3600 seconds and defaults to 10; it is diagnostic configuration, not a backend tuning parameter. Source selection remains the default console playback endpoint until discovery and explicit selection are designed.

The event-driven WASAPI owner thread copies packets into a four-buffer preallocated pool. A non-blocking synchronous channel with the same capacity hands packets to the ordinary processing thread, where bytes are decoded, finite values are checked, and `AudioFrame` values are constructed. Four buffers are an internal implementation decision based on the validated 10 ms packet cadence and sub-millisecond callback work. Each buffer is allocated once to the maximum packet size reported by the initialized WASAPI client. Buffer depth, buffer byte size, and the 100 ms interruptible event-wait interval are deliberately not configurable. Buffer-pool or handoff exhaustion ends the stream with `ErrorKind::ResourceExhausted`; no packet is silently dropped and the capture thread does not block behind processing.

WASAPI `BufferInfo.index` proves native packet continuity. The first accepted native device position is normalized to provider frame index zero, and later positions must exactly match the prior packet end. `BufferInfo.timestamp` is the first-frame QPC timestamp in 100-nanosecond units; timestamp-error flags and backward QPC movement end the stream, and QPC deltas are included in the capture report. `AudioFrame.stream_time_ns` is calculated from the normalized contiguous source-frame index and negotiated sample rate. This preserves a sample-clock timeline compatible with the existing scheduler instead of exposing absolute QPC values or callback scheduling jitter. The conversion rounds down only when an integer number of source frames cannot be expressed as whole nanoseconds.

A data-discontinuity flag on the first accepted packet describes history before the new stream and is recorded in the report; real-device validation observed this startup flag consistently. A later discontinuity flag, device-position gap, invalid timestamp, non-finite sample, or packet-shape error emits `StreamInterrupted` and ends the stream as failed. Default-device replacement or an observed format change emits `StreamInterrupted` and `SourceReconfigured`. Endpoint removal or disablement emits `SourceUnavailable` and `SourceEnded`. Other session interruptions and internal failures end the stream as failed. A requested normal stop emits `ProviderShutdown`; diagnostic duration expiry emits `ConsumerCancelled`.

The capture owner does not reconnect automatically and cannot be restarted. The recovery-disabled `CaptureSupervisor` in `resonance-agent` owns desired-running intent, creates one owner through `CaptureOwnerFactory`, starts and stops it, forwards its events, and records joined completion. Its states are `Idle`, `Running`, `Stopping`, and `Completed`; it cannot be started twice. A stop before start creates no owner. Explicit stop clears desired-running intent before requesting owner stop, so replacement eligibility is suppressed even if a terminal event and completion follow.

`replacement_eligible` only reports that desired-running intent remains enabled, an `Ended` event was delivered to the consumer, owner completion was received, and resources were released. It does not create a replacement or constitute recovery policy. Startup failure or panic without an `Ended` event is not mechanically eligible.

The documented future policy evaluates typed intent, terminal outcome, completion, resource release, source-selection mode, and bounded retry state. Explicit stop invalidates pending recovery before owner shutdown; late events remain visible evidence but cannot restore intent. Device loss, reconfiguration, interruption, and resource exhaustion are conditional cases. Unsupported format under unchanged conditions, a coarsely classified startup failure, and worker panic remain stopped. Retry hints constrain a decision but are never commands. Automatic reconnect, retry/backoff mechanisms, endpoint watching, default-device following, and replacement creation remain unimplemented.

These agent-only policy decisions do not change `resonance-core` or `resonance-api`. Consumers observe ordered lifecycle events, platform-neutral error categories and scopes, retry hints, end reasons, and independent stream identities. They do not parse policy, attempt counts, logs, console output, `Display` text, or `ProviderError::message`. Every later capture run still requires a new `StreamId`, frame index zero, and timestamp zero; recovery cannot be hidden as continuation.

On Windows, a ten-second evidence run is:

```text
cargo run -p resonance-agent -- --duration-seconds 10
```

The executable prints lifecycle events and a final measurement summary while data events flow through the same provider-event callback without dumping sample buffers: native and accepted format, buffer and packet frame sizes, packet and source-frame counts, callback intervals, packet-read duration, QPC deltas, initial-discontinuity observation, and terminal lifecycle reason. This output is a human diagnostic client of the capture boundary, not the permanent API. End-to-end latency remains unmeasured because the implementation does not correlate the QPC clock to the consumer observation clock. Real-device results are observational and are not part of the hardware-independent test suite.

Adding a transport requires a separate decision record. It must version its serialized schema independently and preserve the semantic contract above rather than treating Rust memory layout as a wire format.
