# Resonance Signal

Resonance Signal is a standalone, cross-platform audio signal provider. It is intended to capture audio sources, process audio data, and expose a reusable interface that independent applications can consume.

The provider is the product. Consumers are clients, and visualization is outside this repository's scope.

## Status

The foundation, audio-data-contract, basic signal-processing, bounded-window-scheduling, stereo-first capture-requirements, capture-backend-selection, Windows playback-loopback prototype, real-device validation, Windows capture-boundary productionization, capture-owner lifecycle, capture-supervisor-boundary design, recovery-disabled capture-supervisor state, recovery-decision-policy design, recovery-policy-representation, retry-state-policy-design, retry-state-transition, supervisor-retry-state-integration, and recovery-configuration-model milestones are complete. The workspace defines provider-independent waveform and derived signal frames, a transport-neutral multi-source contract, bounded analysis cadence with explicit discontinuity handling, zero-copy waveform subwindows, RMS and peak levels, explicit peak normalization, and a lifecycle-managed Windows playback-capture component in `resonance-agent`. The Windows adapter uses `wasapi` 0.24.0 to turn the default playback endpoint into bounded, validated `AudioFrame` and `StreamEvent` output with explicit owner and supervisor state, startup, bounded shutdown/completion waiting, completion reporting, and overload behavior. `CaptureSupervisor` owns capture intent, the current attempt identity, retry state, state revision, an immutable validated recovery-configuration snapshot, advisory recovery evaluation, and at most one single-use `CaptureOwner`. Recovery configuration represents the automatic-attempt budget and exhaustion behavior, cooldown, backoff and jitter requirements, stable agent-level failure classification, stable-run reset evidence, and reset behavior without running any of them. Every accepted definition has an explicit nonzero version and a deterministic content fingerprint; changed content or version invalidates earlier recovery assumptions. The runtime remains bound to an explicit fail-closed recovery-disabled definition until operational values and configuration loading are separately approved. After completion the supervisor creates an immutable agent-internal evaluation snapshot, invokes the pure recovery policy, and records the decision without executing it. Automatic reconnect, retry scheduling, backoff execution, endpoint watching, and replacement capture remain unimplemented. Recovery configuration, counters, state, cooldown, snapshots, and decisions stay private to `resonance-agent`; `resonance-core` and `resonance-api` are unchanged. Microphone and Linux capture, FFT processing, device discovery, serialization, service installation, and service transports are also not implemented.

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
- `resonance-agent`: platform capture ownership, the private recovery configuration, policy, and retry-state model, the production Windows playback-loopback adapter, provider-event orchestration, and the diagnostic executable.

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

The diagnostic creates a `CaptureSupervisor`, which commits one attempt, creates and starts one `CaptureOwner`, waits for the requested duration, then disables running intent, requests owner stop, and waits up to two seconds for joined completion. A supervisor stop before `start` creates no owner, repeated stop requests are harmless, and a completion timeout retains ownership so the caller can wait again. `CaptureOwner::wait_for_completion` also permits natural completion to be observed without implicitly requesting stop. Buffer-pool depth, maximum packet allocation, and WASAPI event-wait cadence remain internal implementation details. The supervisor owns and mutates retry state, retains an accepted immutable recovery-configuration snapshot, builds immutable lifecycle/configuration evaluation snapshots after joined completion, and records advisory policy decisions. Configuration loading, environment input, runtime reload, clocks, timers, delay calculation, entropy, and recovery execution are absent. No decision creates another owner, starts a retry loop, sleeps, schedules a timer, watches an endpoint, or follows a default device. A future replacement would require separately approved execution work and would produce a new stream identity and timeline.

## License

Resonance Signal is licensed under the GNU General Public License, version 3 only (`GPL-3.0-only`). See [LICENSE](LICENSE).
