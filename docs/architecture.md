# Architecture

Resonance Signal is a standalone audio signal provider. It owns capture, signal processing, and the client-facing provider interface. External consumers remain outside the provider and consume exposed audio data rather than embedding capture implementations.

## Layer direction

```text
Audio Capture Layer
        |
        v
Audio Processing Layer
        |
        v
API / Client Interface
        |
        v
External Consumers
```

Data and dependencies should flow toward the client interface. Presentation and visualization concerns belong to consumers, not to Resonance Signal.

## Workspace boundaries

### `resonance-core`

Owns provider-independent signal values, validation, lightweight processing, and bounded analysis-window scheduling. The `signal` module defines waveform, level, and spectrum frame structures without capture or transport dependencies. The `scheduling` module accumulates contiguous waveform batches into complete windows with bounded retention and explicit discontinuity handling. The `processing` module provides zero-copy waveform subwindows, per-channel RMS and peak calculation, and opt-in peak normalization.

### `resonance-api`

Owns the consumer-facing semantic contract: source selection, subscriptions, stream lifecycle, product payloads, contract versioning, and platform-neutral failures. It depends on and re-exports `resonance-core` signal types. No transport or network protocol is selected.

### `resonance-agent`

Provides the executable entry point. It owns future platform-capture adapters, capture-format enforcement, and provider lifecycle orchestration. No capture adapter is implemented or selected yet.

Dependency direction is one way:

```text
resonance-agent  --->  resonance-api  --->  resonance-core
       |                                        ^
       +------------- future capture -----------+
```

`resonance-core` cannot depend on capture backends, transports, or consumers. `resonance-api` cannot depend on an operating-system capture implementation. Consumer concerns never flow back into these crates.

## Stereo-first capture boundary

Resonance Signal supports capture products with one or two channels. Mono is one ordered channel. Known stereo is ordered front-left then front-right; an unknown two-channel layout may remain discrete without guessed speaker positions. Surround layouts, spatial/object metadata, and explicitly positioned non-stereo pairs are not supported capture products.

The enforcement point is the capture boundary owned by `resonance-agent`. Keeping the restriction there preserves the provider-independent `ChannelLayout` type and avoids a breaking core/API contraction while ensuring unsupported platform formats never enter an active product stream.

```text
platform source
      |
      v
backend negotiation and mono/stereo validation  -- unsupported --> ProviderError
      |
      v
bounded interleaved f32 AudioFrame batches
      |
      v
WindowScheduler --> signal processing --> resonance-api events --> consumers
```

A future backend may use a native platform format internally, but its accepted output must have an actual non-zero sample rate, one accepted channel layout, finite interleaved `f32` samples, contiguous source frame indices, and stream-relative monotonic timestamps. Capture batch sizes may vary and must remain bounded. The fixed format, source ID, and uninterrupted-stream ID are established before publishing `StreamEvent::Started`.

Platform-provided conversion is acceptable only when it yields a valid mono or stereo representation whose ordering can be reported truthfully. A source with more than two channels is otherwise rejected as `UnsupportedFormat`. Selecting the first two channels, relabelling known non-left/right positions, or performing a custom downmix is forbidden. Unknown one- and two-channel layouts may remain discrete because that preserves order without inventing semantics.

Interruption, restart, reconfiguration, timestamp discontinuity, or format change ends the stream. A resumed source receives a new stream ID, frame index zero, and a new monotonic timeline. This preserves the continuity rules already enforced by `WindowScheduler`; no backend-specific timing or identity type enters `resonance-core`.

The eventual backend selection must demonstrate maintained Rust support, Windows 11 playback-loopback and microphone capture, Linux PipeWire-compatible playback and microphone capture, valid mono/stereo negotiation, visible timestamps and discontinuities, bounded callbacks or polling, observable errors, GPL-3.0-only compatibility, and hardware-independent test seams. Backend claims, dependency selection, and implementation are deferred to a separate evidence-gathering milestone.

## Contract flow

1. A consumer submits a `SubscriptionRequest` naming one or more source selectors and signal products.
2. The provider resolves each selector and emits a `StreamDescriptor` for every uninterrupted source stream.
3. The provider emits bounded `SignalPacket` values containing requested waveform, level, or spectrum payloads.
4. Errors carry a platform-neutral category, scope, and recovery hint.
5. Interruption, source reconfiguration, or format change ends the stream. Resumption creates a new stream identity and timeline.

The semantic contract is intentionally independent of delivery mechanics. A future in-process, local IPC, or network transport can carry the same event model. That transport must define bounded buffering, backpressure, serialization, ordering, and authentication separately.

## Raw and processed responsibility

Resonance Signal always treats raw waveform data as the canonical flexibility boundary. Consumers may request only raw data and perform arbitrary analysis themselves.

The provider contract also permits opt-in levels and magnitude spectra. Computing these once in the provider prevents every consumer from repeating the same expensive work and ensures derived frames share explicit source windows. Derived products remain additive: they cannot make waveform access conditional, and visualization-specific aggregation does not belong in the provider.

The first scheduled processing path is deliberately direct:

```text
AudioFrame
    |
    v
bounded WindowScheduler --> complete AudioFrame window
    |                               |
    |                               +-- borrowed WaveformWindow --> RMS + peak --> LevelFrame
    |
    +-- raw waveform remains independently available
```

`WindowScheduler` uses configurable, non-overlapping tumbling windows. Its default is approximately 30 outputs per second; a 60 FPS target is a configuration change. Duration is rounded to the nearest whole sample frame for the active sample rate, so cadence follows the source sample clock. Complete owned output frames allow one window to span several capture batches while remaining directly consumable by the existing zero-copy `WaveformWindow` processing boundary.

The scheduler retains less than one window between calls and bounds accepted work by a configured maximum number of windows per push. It emits immediately on completion, retains partial slow input without padding or timeout flushing, and rejects oversized calls without mutating state. Queuing completed output for slower consumers belongs to a future transport and must have its own bounded backpressure policy.

Frame index, timestamp, fixed format, and caller-supplied uninterrupted-stream identity establish continuity. Gaps, overlaps, timestamp discontinuities, or same-stream format changes return errors, discard partial data, and require a new stream identity. A normal identity change reports the number of discarded partial frames. No invalid or cross-stream samples silently enter an analysis window.

`WaveformWindow` borrows a complete frame or a frame-aligned subwindow, avoiding waveform copies and exposing channel-specific iteration when needed. Level calculation remains synchronous and stateless; orchestration decides whether to schedule and publish the derived result. Peak normalization is an explicit slice helper and is never applied implicitly to provider output.

Future products remain separate branches from the waveform input. A later `SpectrumFrame`, frequency-band frame, or associated FFT metadata can be added as an independently requested product. Consumers that request only waveform or levels do not need to calculate or receive it, and the raw `AudioFrame` contract does not change.

## Current constraints

- No platform-specific capture library has been selected.
- No audio capture behavior is implemented.
- Supported future capture output is limited to mono and two-channel stereo; wider, spatial, and object-based formats are rejected unless the platform supplies a valid mono/stereo representation.
- Custom downmixing and silent first-two-channel extraction are prohibited.
- Processing is currently limited to bounded tumbling-window scheduling, waveform subwindows, RMS, sample peak, and explicit peak normalization.
- FFT, spectrum generation, frequency bands, smoothing, overlapping hops, transport queues, and output backpressure are deferred.
- No device-discovery interface is defined.
- No serialization format, network service, IPC mechanism, or transport is defined.
- No consumer or visualization code belongs in this repository.

See [ADR 0001](decisions/0001-audio-data-contract.md) for the audio contract, [ADR 0002](decisions/0002-bounded-window-scheduling.md) for scheduling and buffering decisions, and [ADR 0003](decisions/0003-stereo-first-capture-boundary.md) for capture scope and enforcement.
