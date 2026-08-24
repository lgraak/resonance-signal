# ADR 0003: Stereo-first capture boundary

- Status: Accepted
- Date: 2026-08-23

## Context

Resonance Signal needs explicit platform-capture requirements before evaluating or implementing a Windows or Linux backend. The general `ChannelLayout` contract can represent positioned and discrete layouts wider than two channels, but product support does not require surround, spatial, Ambisonic, or object-based audio.

Treating every multichannel source as though its first two channels were a stereo program is incorrect. Layout order can place center dialogue, effects, low-frequency content, or other speakers outside those positions. A provider also cannot claim left/right semantics when the platform only exposes an ordered pair with unknown positions.

The capture boundary must therefore enforce product scope without narrowing valid provider-independent core types, coupling `resonance-core` to platform APIs, or inventing a downmix policy before there is backend evidence.

## Decision

### Supported capture products

Product capture supports exactly:

- mono: one ordered channel, represented as positioned `Mono` when that semantic meaning is known or as discrete one-channel audio when it is not; and
- stereo: two ordered channels, represented as positioned `FrontLeft`, `FrontRight` when those positions are known or as discrete two-channel audio when positions are unknown.

Known stereo is always interleaved left then right. A discrete pair preserves channel zero/channel one order and does not authorize consumers to infer speaker positions. An explicitly positioned two-channel layout other than front-left/front-right is not relabelled as stereo. `StreamDescriptor` and every emitted signal frame report the actual accepted layout.

Wider channel layouts, surround speaker mapping, spatial and object metadata, Ambisonics, and consumer-selectable object rendering are unsupported implementation scope. The existing core types remain general because narrowing them would be an unnecessary compatibility break and the types are useful independently of capture-product policy.

### Format negotiation and canonical output

A future backend first asks the platform for a valid mono or stereo representation when the platform can provide one. Platform-provided format conversion is acceptable if the resulting channel count and ordering can be represented truthfully. No claim is made here that a specific platform API or Rust library provides that capability.

The capture boundary reports the actual selected sample rate and channel layout. Backend-native integer or floating-point formats may remain internal; accepted provider output is bounded, complete, interleaved finite `f32` `AudioFrame` batches. Callback or polling batch boundaries may vary and do not define analysis windows.

The first batch in an uninterrupted stream starts at source frame index zero. Subsequent batches use contiguous indices and stream-relative monotonic timestamps. Sample rate and layout remain fixed until the stream ends. Interruption, restart, reconfiguration, timing discontinuity, or format change ends the stream; resumption creates a new opaque stream identity and timeline.

### Enforcement and failures

Mono/stereo acceptance is enforced at the platform-capture boundary owned by `resonance-agent`, before `StreamEvent::Started` and before frames reach `WindowScheduler`. `resonance-core` remains independent of platform backends, and `resonance-api` continues to expose the accepted format and portable lifecycle/failure semantics.

If a source exposes more than two channels, the backend may accept only a valid platform-provided mono or stereo representation. If it cannot obtain one, orchestration reports source-scoped `ErrorKind::UnsupportedFormat` with an appropriate recovery hint and does not start the stream.

Diagnostics retain enough evidence to evaluate the failure: selected source, native/source format, requested mono/stereo constraints, proposed or selected platform format, whether platform conversion was attempted, and stable backend error codes and text when available. The current `ProviderError::message` can carry human-readable context; a structured backend-diagnostics extension is deferred.

Resonance Signal never silently drops all but the first two channels, reorders known positions, duplicates a mono channel, invents speaker positions, or performs a custom downmix to make a source acceptable.

### Backend evaluation gate

A later selection milestone must evaluate candidates using authoritative evidence for:

- maintained Rust ecosystem support;
- Windows 11 playback-loopback and microphone capture;
- Linux PipeWire-compatible playback and microphone capture;
- valid mono/stereo negotiation and truthful layout reporting;
- actual sample-rate reporting and native-versus-converted format visibility;
- monotonic timestamp, source-position, discontinuity, and restart visibility;
- bounded callback or polling behavior and controllable batch ownership;
- observable platform errors and diagnostic codes;
- licensing compatibility with `GPL-3.0-only`; and
- test seams that allow provider orchestration and all core tests to run without audio hardware.

Dependency addition and capture implementation remain separate approval gates. This decision does not select WASAPI, PipeWire, a Rust library, an async runtime, or a threading model.

## Alternatives considered

### Limit `ChannelLayout` and `AudioFrame` to two channels

Rejected. It would turn product-capture policy into a breaking provider-independent data restriction without a technical need. Enforcement before stream creation is sufficient and preserves the current dependency direction.

### Keep the first two channels of every wider source

Rejected. Channel order does not guarantee a complete left/right program; the result can omit center dialogue or other essential content while appearing valid to consumers.

### Implement a provider-owned downmix now

Rejected. A correct policy requires explicit supported input layouts, mixing matrices, normalization and clipping/headroom rules, LFE treatment, metadata handling, provenance, and tests. Those choices require separate evidence and approval.

### Reject every discrete layout

Rejected. One- and two-channel sources can be valid even when a backend cannot assign portable speaker positions. Preserving discrete order communicates exactly what is known without inventing semantics.

### Select a capture dependency in this decision

Rejected. Requirements are now explicit, but no authoritative comparison has yet demonstrated that one dependency satisfies both platform, timing, lifecycle, licensing, and testability needs.

## Consequences

- Future capture implementations have one conservative acceptance rule before data enters the provider pipeline.
- Consumers can distinguish known mono/stereo positions from unknown ordered channels using the existing layout contract.
- General core layout types remain compatible, while wider capture formats remain unsupported products.
- `WindowScheduler` continues to receive fixed-format, bounded `AudioFrame` batches and uninterrupted-stream identities without platform dependencies.
- Some multichannel playback sources will be unavailable unless the platform can expose a valid mono/stereo representation.
- Custom downmixing, structured backend diagnostics, dependency selection, and capture implementation remain deferred decisions.
