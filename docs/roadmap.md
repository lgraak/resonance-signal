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

## Later milestones

- Implement and validate one bounded Windows playback-loopback adapter behind the existing platform-neutral boundary.
- Add microphone capture and the Linux PipeWire adapter only after the first prototype evidence is reviewed.
- Add optional FFT, spectrum, and frequency-band processing after practical requirements are defined.
- Define an appropriate client transport only when contract requirements justify it.

Consumer applications and visualization remain outside this roadmap and repository.
