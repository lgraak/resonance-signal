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

## Later milestones

- Evaluate platform capture requirements and libraries.
- Implement capture providers behind platform-neutral boundaries.
- Add optional FFT, spectrum, and frequency-band processing after practical requirements are defined.
- Define an appropriate client transport only when contract requirements justify it.

Consumer applications and visualization remain outside this roadmap and repository.
