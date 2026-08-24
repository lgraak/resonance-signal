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

Owns provider-independent signal values, validation, and lightweight processing. The `signal` module defines waveform, level, and spectrum frame structures without capture or transport dependencies. The `processing` module provides zero-copy waveform subwindows, per-channel RMS and peak calculation, and opt-in peak normalization.

### `resonance-api`

Owns the consumer-facing semantic contract: source selection, subscriptions, stream lifecycle, product payloads, contract versioning, and platform-neutral failures. It depends on and re-exports `resonance-core` signal types. No transport or network protocol is selected.

### `resonance-agent`

Provides the executable entry point. It will orchestrate capture and provider lifecycle work in later milestones.

Dependency direction is one way:

```text
resonance-agent  --->  resonance-api  --->  resonance-core
       |                                        ^
       +------------- future capture -----------+
```

`resonance-core` cannot depend on capture backends, transports, or consumers. `resonance-api` cannot depend on an operating-system capture implementation. Consumer concerns never flow back into these crates.

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

The first processing path is deliberately direct:

```text
AudioFrame
    |
    +-- borrowed WaveformWindow --> per-channel RMS + peak --> LevelFrame
    |
    +-- raw waveform remains independently available
```

`WaveformWindow` borrows a complete frame or a frame-aligned subwindow, avoiding waveform copies and exposing channel-specific iteration when needed. Level calculation is synchronous and stateless; capture or orchestration code decides window size, cadence, retention, and whether to publish the derived result. Peak normalization is an explicit slice helper and is never applied implicitly to provider output.

Future products remain separate branches from the waveform input. A later `SpectrumFrame`, frequency-band frame, or associated FFT metadata can be added as an independently requested product. Consumers that request only waveform or levels do not need to calculate or receive it, and the raw `AudioFrame` contract does not change.

## Current constraints

- No platform-specific capture library has been selected.
- No audio capture behavior is implemented.
- Processing is currently limited to waveform subwindows, RMS, sample peak, and explicit peak normalization.
- FFT, spectrum generation, frequency bands, smoothing, buffering policy, and processing scheduling are deferred.
- No device-discovery interface is defined.
- No serialization format, network service, IPC mechanism, or transport is defined.
- No consumer or visualization code belongs in this repository.

See [ADR 0001](decisions/0001-audio-data-contract.md) for the contract decision and rejected alternatives.
