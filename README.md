# Resonance Signal

Resonance Signal is a standalone, cross-platform audio signal provider. It is intended to capture audio sources, process audio data, and expose a reusable interface that independent applications can consume.

The provider is the product. Consumers are clients, and visualization is outside this repository's scope.

## Status

The foundation, audio-data-contract, basic signal-processing, bounded-window-scheduling, and stereo-first capture-requirements milestones are complete. The workspace defines provider-independent waveform and derived signal frames, a transport-neutral multi-source contract, bounded analysis cadence with explicit discontinuity handling, zero-copy waveform subwindows, RMS and peak levels, explicit peak normalization, and the requirements future platform capture backends must satisfy. Audio capture, FFT processing, device discovery, serialization, and service transports are not implemented yet.

Supported capture products are mono and two-channel stereo. Surround, spatial, and object-based audio are outside the product scope. A future capture backend may accept a multichannel source only when the platform can provide a valid mono or stereo representation; it must never silently keep the first two channels or invent a downmix.

## Architecture direction

```text
Audio Capture Layer
        |
        v
AudioFrame
        |
        v
Bounded Window Scheduling
        |
        v
Signal Processing
        |
        v
API / Client Interface
        |
        v
External Consumers
```

The workspace is divided into three crates:

- `resonance-core`: core data structures, shared types, and provider-independent logic.
- `resonance-api`: consumer-facing semantic contracts without a selected transport or serialization format.
- `resonance-agent`: executable entry point and future capture orchestration.

See [Architecture](docs/architecture.md), [API](docs/api.md), and [Roadmap](docs/roadmap.md) for the current project boundaries.

## Development

Use stable Rust. From the repository root:

```text
cargo fmt --all --check
cargo check --workspace --all-targets
cargo build --workspace
```

## License

Resonance Signal is licensed under the GNU General Public License, version 3 only (`GPL-3.0-only`). See [LICENSE](LICENSE).
