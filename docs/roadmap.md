# Roadmap

## Milestone 1: Foundation

- Buildable Rust workspace with `resonance-core`, `resonance-api`, and `resonance-agent`.
- Initial crate and module boundaries.
- Architecture, API, and roadmap documentation skeletons.
- Stable-Rust CI for Windows, Linux, formatting, checking, and builds.

## Milestone 2: Core contracts

- Defined the provider-independent waveform, level, and spectrum data model.
- Defined the first multi-source, multi-product client contract.
- Established pre-1.0 compatibility and versioning expectations.
- Added focused validation and contract tests.

## Milestone 3A: Practical signal processing primitives

- Added borrowed, frame-aligned waveform windows without copying sample data.
- Added scalar and per-channel RMS and maximum absolute peak calculations.
- Added explicit peak-normalization helpers that preserve out-of-range headroom.
- Added synthetic tests for silence, constant and known waveforms, multiple channels, subwindows, invalid input, and normalization.

## Milestone 3B: Bounded window scheduling and processing cadence

- Added configurable, non-overlapping analysis windows with 30 FPS and 60 FPS visualization cadences.
- Bounded retained partial samples and per-push work without introducing an output queue or async runtime.
- Added explicit frame-index, timestamp, format, and uninterrupted-stream boundary handling.
- Added synthetic tests for completion, accumulation, multiple outputs, oversized input, discontinuities, stream changes, and empty input.

## Milestone 4A: Stereo-first capture requirements and provider boundary

- Limited supported capture products to mono and two-channel stereo while preserving the wider provider-independent core layout model.
- Established front-left/front-right ordering for known stereo and conservative discrete layouts when one- or two-channel positions are unknown.
- Placed format enforcement at the future capture boundary in `resonance-agent`; unsupported wider sources fail before an active stream starts.
- Prohibited silent first-two-channel extraction and custom downmixing without a separate evidence-backed decision.
- Defined format, batch, timing, identity, lifecycle, diagnostic, and backend-evaluation requirements without selecting a dependency or implementing capture.

## Milestone 4B: Capture backend selection

- Evaluated direct WASAPI and PipeWire access, focused Rust bindings, CPAL, and GStreamer against the capture contract using current upstream documentation and source.
- Selected `wasapi-rs` 0.24.0 as the initial Windows direction and official `pipewire-rs` 0.10.1 bindings as the initial Linux direction.
- Rejected a third-party cross-platform capture layer for the first implementation because required native timestamp-validity, source-position, and provenance evidence would be lost.
- Defined one bounded Windows playback-loopback prototype as the next implementation milestone without adding dependencies or capture code.

## Milestone 5A: Windows WASAPI playback-loopback prototype

- Added the Windows-only `wasapi` 0.24.0 dependency to `resonance-agent`; `resonance-core` and `resonance-api` remain platform-independent.
- Opened the console-role default playback endpoint in event-driven shared loopback mode and requested mono or explicit front-left/front-right interleaved float output at the endpoint mix sample rate.
- Added a preallocated four-buffer pool and matching bounded, non-blocking handoff from the COM-owned WASAPI thread to ordinary processing.
- Converted native packets into validated `AudioFrame` and existing `StreamEvent` output with contiguous normalized frame indexes and sample-derived stream timestamps.
- Retained and validated WASAPI device positions, QPC timestamps, discontinuity/silence/timestamp flags, endpoint identity, endpoint/default-device notifications, and session-disconnect reasons.
- Made interruption, format change, device invalidation, timing discontinuity, and handoff exhaustion explicit stream boundaries; automatic reconnect remains deferred.
- Added hardware-independent conversion, frame-generation, unsupported-format, non-finite-value, and discontinuity tests plus runtime evidence reporting.

## Milestone 5B: Windows real-device validation

- Validated default-playback loopback against the WH-1000XM5 endpoint at 96 kHz, two-channel interleaved `f32`.
- Observed 960-frame packets representing approximately 10 ms of audio, approximately 10 ms QPC deltas, and sub-millisecond callback work.
- Confirmed that repeated capture runs create new stream identities and restart the stream-relative frame index and timestamp at zero.
- Classified the consistently observed first-packet discontinuity flag as startup history; later discontinuities remain stream-ending failures.

## Milestone 5C: Production Windows capture boundary

- Replaced duration-owned prototype orchestration with an explicit production stop token; retained duration only in the bounded diagnostic runner.
- Kept a four-slot, maximum-packet-sized preallocated pool and matching non-blocking handoff as internal implementation details rather than public tuning knobs.
- Formalized normal stop, source replacement, format change, endpoint loss, interruption, discontinuity, bounded overload, and internal-failure mappings to existing provider events.
- Separated machine-actionable terminal categories and retry hints from human diagnostics and console evidence output.
- Added hardware-independent tests for CLI validation, lifecycle/error mapping, bounded handoff delivery, explicit overload, and stream restart identity/timeline behavior.
- Preserved explicit owner-controlled restart; automatic reconnect remains deferred.

## Later milestones

- Define and validate the first long-running capture owner, including reconnect policy and controlled endpoint-replacement acceptance, without selecting transport prematurely.
- Add microphone capture and the Linux PipeWire adapter only in separately scoped milestones.
- Add optional FFT, spectrum, and frequency-band processing after practical requirements are defined.
- Define an appropriate client transport only when contract requirements justify it.

Consumer applications and visualization remain outside this roadmap and repository.
