# Resonance Signal

Resonance Signal is a standalone, cross-platform audio signal provider. It is intended to capture audio sources, process audio data, and expose a reusable interface that independent applications can consume.

The provider is the product. Consumers are clients, and visualization is outside this repository's scope.

## Status

The provider-independent audio contract, signal processing, bounded scheduling, Windows playback-loopback capture, capture ownership and supervision, private recovery policy/state model, source discovery and identity registry, portable discovery contract, mapped Default Playback and explicit `SourceId` capture, and local consumer transport milestones are complete.

The accepted hybrid source model keeps Default Playback and Explicit Source as distinct intents. Each Default Playback attempt resolves the current Windows console-role endpoint. Each Explicit Source attempt resolves only its requested live opaque `SourceId`; it never substitutes the current default, a same-named endpoint, or another available endpoint. Both paths open the exact private endpoint mapping, revalidate the revision-bound binding before publishing `Started`, and report the resolved registry-backed `SourceId`. Repeated attempts for the same source retain the `SourceId` but always create a new `StreamId` and timeline. Native endpoint IDs remain private to `resonance-agent`.

`resonance-api` provides an owned `DiscoverySnapshot`, opaque equality-only `DiscoveryRevision`, portable `SourceDescriptor`, three-state `SourceAvailability`, supported products, and point-in-time default roles. Discovery never creates capture ownership or turns a descriptor marked default into Default Playback intent. A private installation-and-host-bound registry retains proven identity across restart and compatible upgrade, atomically persists live mappings and permanent retirement tombstones, detects stale snapshot use, and loses continuity safely rather than guessing after corruption, incompatible migration, host change, or reset. Friendly names and backend IDs are not identity, retired IDs are never reassigned, and ambiguous return requires rediscovery.

The Windows capture adapter uses `wasapi` 0.24.0 to produce bounded, validated `AudioFrame` and `StreamEvent` output with explicit owner and supervisor lifecycle, startup, bounded shutdown/completion waiting, completion reporting, and overload behavior. `resonance-agent serve` exposes portable discovery/status over loopback HTTP and one independent bounded waveform session per WebSocket. JSON carries lifecycle metadata and a versioned little-endian binary frame carries scheduled waveform windows. See [Consumer Protocol](docs/consumer-protocol.md). `CaptureSupervisor` remains recovery-disabled; automatic reconnect, retry scheduling, endpoint watching, replacement capture, microphone support, Linux/PipeWire, FFT, Windows service installation, LAN access, UI, and visualization are not implemented.

Supported capture products are mono and two-channel stereo. Surround, spatial, and object-based audio are outside the product scope. A capture backend may accept a multichannel source only when the platform can provide a valid mono or stereo representation; it must never silently keep the first two channels or invent a downmix.

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
- `resonance-api`: transport-independent consumer-facing semantic contracts.
- `resonance-agent`: platform capture ownership, private source discovery and identity mapping, private recovery configuration/policy/state, the production Windows playback-loopback adapter, provider-event orchestration, loopback consumer transport, and diagnostics.

See [Architecture](docs/architecture.md), [API](docs/api.md), and [Roadmap](docs/roadmap.md) for the current project boundaries.

## Development

Use stable Rust. From the repository root:

```text
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo doc --workspace --no-deps
```

On Windows, the diagnostic executable performs a bounded ten-second Default Playback run by default:

```text
cargo run -p resonance-agent -- --duration-seconds 10
```

An opaque `SourceId` returned by discovery can be selected explicitly:

```text
cargo run -p resonance-agent -- --source-id <opaque-source-id> --duration-seconds 10
```

Start the local consumer service and exercise it from a second process:

```text
cargo run -p resonance-agent -- serve
python examples/consumer.py
```

The default listener is `127.0.0.1:48480`; non-loopback addresses are rejected. The external contract is documented in [Consumer Protocol](docs/consumer-protocol.md).

The diagnostic creates a `CaptureSupervisor`, commits one attempt, creates and starts one `CaptureOwner`, resolves the requested intent through the private identity registry under `%LOCALAPPDATA%\Resonance Signal\provider-state`, waits for the requested duration, then disables running intent, requests owner stop, and waits up to two seconds for joined completion. A stop before `start` creates no owner, repeated stop requests are harmless, and a completion timeout retains ownership so the caller can wait again. Buffer-pool depth, maximum packet allocation, and WASAPI event-wait cadence remain internal implementation details. Recovery decisions remain advisory: none creates another owner, starts a retry loop, sleeps, schedules a timer, watches an endpoint, or migrates an active stream.

## License

Resonance Signal is licensed under the GNU General Public License, version 3 only (`GPL-3.0-only`). See [LICENSE](LICENSE).
