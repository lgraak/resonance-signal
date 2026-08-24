# Resonance Signal

Resonance Signal is a standalone, cross-platform audio signal provider. It is intended to capture audio sources, process audio data, and expose a reusable interface that independent applications can consume.

The provider is the product. Consumers are clients, and visualization is outside this repository's scope.

## Status

The foundation, audio-data-contract, basic signal-processing, and bounded-window-scheduling milestones are complete. The workspace defines provider-independent waveform and derived signal frames, a transport-neutral multi-source contract, bounded analysis cadence with explicit discontinuity handling, zero-copy waveform subwindows, RMS and peak levels, and explicit peak normalization. Audio capture, FFT processing, device discovery, serialization, and service transports are not implemented yet.

## Architecture direction

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

The workspace is divided into three crates:

- `resonance-core`: core data structures, shared types, and provider-independent logic.
- `resonance-api`: client-facing contracts, serialization types, and API definitions.
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
