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

These functions are synchronous building blocks, not a buffering, scheduling, transport, or service API. Level calculation uses only temporary and output storage proportional to channel count; a `WaveformWindow` itself is zero-copy and no waveform-sized processing buffer is allocated. Inputs are expected to remain small real-time batches rather than unbounded recordings.

FFT and spectrum calculation are intentionally not implemented. `SpectrumFrame` remains a separate optional contract shape so a later processing implementation can add spectra or frequency-band products without changing `AudioFrame`, `LevelFrame`, or forcing those products on every subscriber.

## Subscriptions and sources

`SubscriptionRequest` contains non-empty, duplicate-free lists of `SourceSelector` and `SignalProduct` values. A single request can name multiple sources and products.

Sources are selected as either:

- `Default(DefaultSource::Playback)` or `Default(DefaultSource::Capture)`, resolved when the subscription starts; or
- `Id(SourceId)`, where `SourceId` is an opaque provider-assigned identifier.

Selected playback devices, microphones, and virtual devices are all addressed by opaque ID. `SourceKind` describes the resolved source as playback, microphone, virtual, or other. The contract makes no claim that IDs are portable across hosts or stable after device removal. Device discovery, friendly names, capabilities, and persistence rules are later API work.

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

Adding a transport requires a separate decision record. It must version its serialized schema independently and preserve the semantic contract above rather than treating Rust memory layout as a wire format.
