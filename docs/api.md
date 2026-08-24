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

Platform capture belongs behind orchestration in `resonance-agent`, not in `resonance-core` or `resonance-api`. A future backend boundary needs only to resolve a selected source, negotiate and validate its format, start and stop capture, and deliver bounded waveform batches or explicit lifecycle failures. A backend-specific generic framework or async runtime is not part of this contract.

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

Backend selection is also deferred. Candidates must later be evaluated for maintained Rust support; Windows 11 playback-loopback and microphone capture; Linux PipeWire-compatible playback and microphone capture; valid mono/stereo negotiation; sample-rate, timestamp, and discontinuity visibility; bounded callback or polling behavior; observable platform errors; GPL-3.0-only license compatibility; and test seams that do not require audio hardware in core tests. No current documentation claims that WASAPI, PipeWire, or a particular Rust library satisfies those criteria.

Adding a transport requires a separate decision record. It must version its serialized schema independently and preserve the semantic contract above rather than treating Rust memory layout as a wire format.
