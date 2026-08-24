# ADR 0001: Audio data contract

- Status: Accepted
- Date: 2026-08-23
- Contract version: 0.1

## Context

Resonance Signal needs a provider/consumer boundary before platform capture implementations exist. The boundary must support playback, microphone, and virtual sources; several simultaneous sources; consumers other than the first planned visualization; and both raw and reusable derived data. It must not bind the core model to WASAPI, PipeWire, a transport, or a wire encoding.

The contract also needs deterministic frame boundaries, channel ordering, time semantics, interruption behavior, and portable failures. Leaving those properties to individual capture backends would make consumers platform-aware and make a future transport difficult to version.

## Decision

### Waveform representation

The canonical waveform product is `AudioFrame`, a bounded owned batch of finite, normalized `f32` linear PCM samples.

- Samples are interleaved, sample-frame major and channel minor.
- Nominal full scale is `[-1.0, 1.0]`; finite values outside that range are retained for headroom.
- Every frame carries sample rate, ordered channel layout, a source window, and samples.
- Only complete, non-empty sample frames are valid.
- Positioned channel layouts use portable semantic positions. A discrete layout preserves count and order without inventing positions.

Interleaving matches common capture and transport layouts and lets consumers process complete time slices. `f32` avoids exposing backend integer widths and gives processing stages a single representation. Owned variable-sized batches avoid imposing a backend period on consumers while keeping lifetime and buffering explicit.

### Time and continuity

`FrameTimestamp` contains a zero-based source frame index and nanoseconds in the uninterrupted stream's monotonic clock domain. Frame index is authoritative for continuity; monotonic time supports scheduling and correlation inside that stream. Neither value is wall-clock time.

A `StreamId` names exactly one uninterrupted, fixed-format stream. Interruption, device reconfiguration, or format change ends it. Resumption creates a new stream ID and resets the timeline. This makes discontinuities explicit instead of hiding gaps or changing format inside a stream.

### Raw and derived products

Waveform data is the baseline product and remains available independently. Consumers can therefore implement arbitrary processing without depending on provider-specific visualization choices.

Consumers may also request provider-computed levels and single-sided magnitude spectra. These products are aligned to explicit source windows, can be requested without waveform delivery, and avoid repeating common or expensive processing for every consumer.

Levels define per-channel RMS and sample peak in normalized full-scale units. Spectra report channel layout, sample rate, source window, FFT size, window function, and coherent-gain-corrected single-sided linear magnitudes. Shorter source windows are zero-padded to the FFT size. Spectrum magnitudes are divided by the sum of window coefficients, with all bins except DC and an even-sized FFT's Nyquist bin doubled. `Rectangular` uses unit coefficients; `Hann` is the periodic `0.5 - 0.5 * cos(2 * pi * n / N)` form. These shapes are shared signal data, not visualization data.

Derived analysis configuration is experimental in contract 0.1. The provider chooses supported window/hop/FFT parameters and reports the actual values. Capability discovery and parameter negotiation will be designed with processing and backend evidence.

### Sources and subscriptions

`SubscriptionRequest` contains non-empty, duplicate-free source and product lists. Source selectors support default playback, default capture, or an opaque provider-assigned source ID. Opaque IDs avoid embedding platform device paths in the contract. One subscription can resolve to multiple stream descriptors.

Device discovery, names, capabilities, and identifier persistence are deliberately separate future contracts. Virtual devices are ordinary resolved sources and do not require a visualization-specific concept.

### Events, buffering, and errors

The provider contract is an ordered stream of lifecycle, data, error, and end events. Data is carried as bounded, independently owned packets. Queue size, delivery threads, serialization, backpressure, and authentication belong to a future transport decision. Implementations must not treat this contract as authorization for unbounded buffering.

Failures use portable `ErrorKind`, `ErrorScope`, and `RetryHint` fields plus human-readable diagnostic text. Consumers act on the structured values, never backend error codes or message parsing.

### Stability

The contract version begins at 0.1. Waveform representation, channel ordering, stream-relative timing, opaque identities, multi-source subscriptions, stream boundaries, and portable failure categories are intended to form the stable 1.0 semantic surface.

Derived configuration, exact enum inventories, Rust constructors, discovery, serialization, transport, and cross-stream wall-clock correlation remain experimental. A future serialized protocol receives its own version and does not use Rust memory layout as a wire format.

## Alternatives considered

### Backend-native integer samples

Rejected as the shared representation. Preserving native `i16`, `i24`, `i32`, or float formats would push conversion and format branching into every consumer. Backends may convert into the canonical `f32` representation; a future diagnostic or lossless-native extension can be additive if evidence requires it.

### Planar samples

Rejected for the baseline. Planar data benefits some per-channel processing but is less natural for complete sample-frame delivery and many capture APIs. Consumers can deinterleave bounded frames when needed. Fixing one layout also removes per-frame ambiguity.

### Wall-clock timestamps

Rejected as the primary clock. Wall clocks can jump and cannot reliably express sample continuity. Stream-relative frame index plus monotonic time is deterministic. Cross-host or wall-clock correlation can later add an explicit clock-correlation structure without changing frame semantics.

### Format only in a stream header

Rejected as the sole representation. A stream descriptor defines negotiated format, but self-describing frames are safer for in-process use, recorded batches, tests, and future message boundaries. Implementations must verify that frame metadata matches the descriptor.

### Raw waveform only

Rejected. It maximizes theoretical flexibility but forces every consumer to repeat level and FFT work. Opt-in derived products centralize reusable work without withholding raw data.

### Derived data only

Rejected. It would lock consumers to provider-selected algorithms and make new uses impossible without provider changes.

### One implicit default output

Rejected. It cannot model microphones, selected devices, virtual devices, or concurrent sources. Explicit selectors and per-source stream identities scale without making device discovery part of this milestone.

### Platform-specific errors

Rejected. HRESULT, Win32, PipeWire, and portal errors are implementation diagnostics, not portable consumer behavior. Stable categories and recovery hints preserve useful action across platforms.

### Select a transport and serialization now

Rejected. Capture and consumer evidence is not yet sufficient to choose in-process callbacks, local IPC, or network delivery. Freezing a wire schema now would couple semantic decisions to guessed transport constraints.

## Consequences

- Capture backends must normalize samples, map channel layouts conservatively, and create new streams at discontinuities or format changes.
- Consumers receive a platform-neutral model and may subscribe to several sources and products.
- Raw frames can be larger than use-case-specific metrics, so future transports need bounded buffering and explicit backpressure.
- Derived processing can be shared, but its scheduling and configuration require a later decision backed by real processing and capture constraints.
- Serialization, discovery, and transport remain deliberately unavailable until separate milestones.
