# Resonance Signal

Resonance Signal is a standalone, cross-platform audio signal provider. It is intended to capture audio sources, process audio data, and expose a reusable interface that independent applications can consume.

The provider is the product. Consumers are clients, and visualization is outside this repository's scope.

## Status

The foundation, audio-data-contract, basic signal-processing, bounded-window-scheduling, stereo-first capture-requirements, capture-backend-selection, Windows playback-loopback prototype, real-device validation, Windows capture-boundary productionization, capture-owner lifecycle, and capture-supervisor-boundary milestones are complete. The workspace defines provider-independent waveform and derived signal frames, a transport-neutral multi-source contract, bounded analysis cadence with explicit discontinuity handling, zero-copy waveform subwindows, RMS and peak levels, explicit peak normalization, and a lifecycle-managed Windows playback-capture component in `resonance-agent`. The Windows adapter uses `wasapi` 0.24.0 to turn the default playback endpoint into bounded, validated `AudioFrame` and `StreamEvent` output with explicit ownership, startup, bounded shutdown waiting, completion reporting, and overload behavior. Recovery policy belongs to a future `CaptureSupervisor` above the single-use `CaptureOwner`; the boundary is defined, but the supervisor and automatic reconnect are not implemented. Microphone and Linux capture, FFT processing, device discovery, serialization, service installation, and service transports are also not implemented.

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
- `resonance-agent`: platform capture ownership, the production Windows playback-loopback adapter, provider-event orchestration, and the diagnostic executable.

See [Architecture](docs/architecture.md), [API](docs/api.md), and [Roadmap](docs/roadmap.md) for the current project boundaries.

## Development

Use stable Rust. From the repository root:

```text
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo doc --workspace --no-deps
```

On Windows, the diagnostic executable performs a bounded ten-second run by default:

```text
cargo run -p resonance-agent -- --duration-seconds 10
```

The diagnostic creates a `CaptureOwner`, starts one capture run, waits for the requested duration, then requests stop and waits up to two seconds for joined completion. Production code uses the same single-use owner with its own shutdown deadline. A stop requested before `start` skips initialization, repeated stop requests are harmless, and a shutdown timeout retains ownership so the caller can wait again. Buffer-pool depth, maximum packet allocation, and WASAPI event-wait cadence remain internal implementation details. The future `CaptureSupervisor` will own recovery decisions around completed owners. Automatic reconnect remains deferred: a later run requires a new owner and produces a new stream identity and timeline.

## License

Resonance Signal is licensed under the GNU General Public License, version 3 only (`GPL-3.0-only`). See [LICENSE](LICENSE).
