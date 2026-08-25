# Resonance Signal

Resonance Signal is a provider-neutral audio signal service. It captures local
playback audio and exposes portable, normalized waveform data to independent
consumer applications.

Windows playback capture is implemented first. WASAPI and native endpoint IDs
remain private implementation details; consumers integrate through the
documented HTTP and WebSocket protocol. InfoPanel is expected to be the first
real consumer, but it is a separate project and Resonance Signal does not
depend on it.

> **The provider is the product. Consumers are clients.** Visualization and
> consumer-specific presentation do not belong in this repository.

## Current status

| Area | Status |
| --- | --- |
| Windows playback source discovery | Implemented |
| Default Playback selection | Implemented |
| Explicit opaque `SourceId` selection | Implemented |
| Stable provider-managed source identity | Implemented within one installation and host while continuity remains proven |
| Mono and stereo playback-loopback capture | Implemented |
| Local HTTP and WebSocket consumer service | Implemented, loopback only |
| Versioned binary waveform transport | Implemented |
| Bounded independent consumer sessions | Implemented, up to 16 simultaneous sessions |
| Consumer protocol | Documented as version 1 |
| InfoPanel consumer/plugin | Planned in a separate repository |
| Linux/PipeWire capture | Planned |

Today, the `resonance-agent` executable, playback discovery, capture adapter,
and local service are Windows-only. The core signal and consumer contracts are
provider-independent; the selected Linux direction is a future PipeWire
adapter behind the same public semantics.

The current product ceiling is mono and two-channel stereo. Wider, spatial,
and object-based sources are accepted only when the platform supplies a valid
mono or stereo representation. Resonance Signal never silently takes the first
two channels or invents a downmix.

Automatic recovery, endpoint watching, microphone capture, Linux capture,
Windows service installation, browser UI, and non-loopback networking are not
implemented. The current recovery policy remains deliberately disabled.

## Architecture at a glance

```text
Windows playback audio
        |
        v
Resonance Signal provider (resonance-agent)
        |
        +-- playback source discovery
        +-- private identity registry
        +-- capture lifecycle and format validation
        +-- bounded waveform scheduling
        |
        v
Local consumer service
HTTP + WebSocket on loopback
        |
        v
External consumer
```

The workspace keeps provider-independent contracts separate from platform
machinery:

| Crate | Responsibility |
| --- | --- |
| `resonance-core` | Signal types, processing primitives, and bounded window scheduling |
| `resonance-api` | Transport-independent consumer-facing semantic contracts |
| `resonance-agent` | Windows capture, private discovery and identity, lifecycle orchestration, local transport, and diagnostics |

Consumers never receive native WASAPI endpoint IDs, registry internals, or
backend diagnostics. See [Architecture](docs/architecture.md) for the internal
boundaries and [API](docs/api.md) for the semantic data contracts.

## Choosing a playback source

Resonance Signal keeps the user's selection intent separate from the physical
source and from a particular capture attempt.

### Default Playback

Default Playback means “use whichever source owns the Windows console default
playback role when a new capture attempt starts.”

```text
Default Playback intent
        |
        +-- headphones today
        |
        +-- speakers on a later attempt
```

The logical intent remains Default Playback even if the resolved `SourceId`
changes between attempts. An active stream never migrates to another source in
place.

### Explicit `SourceId`

Explicit selection pins an attempt to one opaque identity returned by source
discovery.

```text
Explicit Source A
        |
        +-- available   -> capture Source A
        |
        +-- unavailable -> fail; do not substitute
```

A same-named device, the current default, or another available source is never
used as a fallback. A proven source can retain its `SourceId` across process
restarts, metadata changes, and temporary absence within one provider
installation on one host. IDs are not portable across hosts, installations,
registry resets, or cases where continuity cannot be proven.

| Term | Meaning |
| --- | --- |
| Source intent | The durable choice between Default Playback and one explicit source |
| `SourceId` | The provider-managed identity of a playback source |
| `StreamId` | One uninterrupted capture attempt and timeline |

Every attempt receives a fresh `StreamId`, frame index zero, and stream-relative
timeline—even when it resolves the same `SourceId` again.

## Build on Windows

Prerequisites:

- stable Rust using the MSVC toolchain;
- Microsoft C++ Build Tools and a Windows SDK for native linking;
- Python 3 only if you want to run the example consumer (it uses the standard
  library and has no package dependencies).

From the repository root:

```powershell
cargo build
```

The debug executable is written to:

```text
target\debug\resonance-agent.exe
```

For an optimized build:

```powershell
cargo build --release
```

The release executable is written to:

```text
target\release\resonance-agent.exe
```

The executable can also run a bounded capture diagnostic. Run
`resonance-agent.exe --help` for the supported diagnostic options.

## Run the local consumer service

Start the debug build:

```powershell
.\target\debug\resonance-agent.exe serve
```

The default listener is:

```text
127.0.0.1:48480
```

Only numeric loopback addresses are accepted (`127.0.0.1` or `::1`). A wildcard
such as `0.0.0.0` and every non-loopback address are deliberately rejected.
Remote access is future work and requires a separate security design covering
authentication, authorization, transport protection, abuse controls, firewall
behavior, and deployment.

You can also run the service without locating the built executable:

```powershell
cargo run -p resonance-agent -- serve
```

## Quick sanity checks

With the service running, open these URLs in a browser or request them with an
HTTP client.

### Service status

<http://127.0.0.1:48480/v1/status>

A ready service returns a small status object similar to:

```json
{
  "protocol_version": 1,
  "status": "ready",
  "listener_scope": "loopback",
  "active_stream_sessions": 0
}
```

### Playback sources

<http://127.0.0.1:48480/v1/sources>

This returns a complete, replaceable discovery snapshot. For example:

```json
{
  "protocol_version": 1,
  "revision": "snapshot-...",
  "sources": [
    {
      "source_id": "id-...",
      "display_name": "Realtek Digital Output (Realtek USB Audio)",
      "kind": "playback",
      "availability": "available",
      "default_playback": true,
      "supported_products": ["waveform"]
    }
  ]
}
```

The revision and source IDs are opaque. Do not parse them or copy example IDs
as durable values. Display names are presentation only and can change or be
duplicated.

The base path <http://127.0.0.1:48480/v1/> is not itself a defined route and
returns HTTP 404 with `error: "not_found"`.

## Receive an actual waveform

`/v1/waveform` is a WebSocket endpoint, not an ordinary web page. Opening it in
a browser address bar will not display waveform samples; a client must perform
a WebSocket upgrade and supply exactly one source selection.

In a second terminal, run the included diagnostic consumer:

```powershell
python .\examples\consumer.py
```

It uses Default Playback unless `--source-id <opaque-id>` is supplied:

```powershell
python .\examples\consumer.py --source-id <opaque-id>
```

The consumer exercises the complete basic flow:

```text
discover sources
    -> open WebSocket
    -> receive stream_started
    -> receive and parse RSWF binary waveform frames
    -> send the exact stop request
    -> confirm stream_stopped
```

For exact query rules, JSON schemas, lifecycle messages, error handling, stop
control, and byte layout, use the [Consumer Protocol](docs/consumer-protocol.md).

## Consumer API overview

| Interface | Purpose |
| --- | --- |
| `GET /v1/status` | Report consumer-oriented service status |
| `GET /v1/sources` | Discover portable playback source descriptors |
| WebSocket `/v1/waveform?source=default-playback` | Start a Default Playback waveform session |
| WebSocket `/v1/waveform?source_id=<percent-encoded-id>` | Start an explicit-source waveform session |

Each WebSocket is an independent bounded capture session. One slow consumer is
terminated without blocking capture or accumulating unbounded latency for the
others.

## Waveform data

Waveform samples are:

- finite normalized linear PCM `f32` values encoded little-endian;
- mono or two-channel stereo;
- interleaved in sample-frame-major, channel-minor order;
- grouped into scheduled, non-overlapping waveform windows; and
- carried in versioned binary `RSWF` frames after a `stream_started` message.

`-1.0` and `1.0` are nominal full scale. Finite headroom outside that range is
preserved rather than clipped or dynamically normalized. Frame indices and
timestamps are relative to the active `StreamId`; they are not wall-clock time
and cannot be compared across streams.

See the [Consumer Protocol](docs/consumer-protocol.md) for the authoritative
40-byte header and payload layout.

## First consumer: InfoPanel

InfoPanel is intended to be the first real consumer used to validate the public
protocol end to end. Its plugin will live in a separate project/repository and
should integrate only through the documented HTTP and WebSocket contract.

Resonance Signal has no InfoPanel dependency and does not contain InfoPanel UI
or visualization code. A successful Windows waveform visualization will be an
external consumer-acceptance milestone before Linux capture work; it is not
implemented yet.

## Roadmap

### Completed

- [x] Windows playback source discovery
- [x] Default Playback capture
- [x] Explicit `SourceId` capture
- [x] Local loopback consumer service
- [x] Versioned external waveform protocol and diagnostic consumer

### Next validation

- [ ] First real consumer: InfoPanel plugin in its own repository
- [ ] Windows end-to-end consumer acceptance

### Later provider work

- [ ] Evidence-gated recovery design and implementation (currently disabled)
- [ ] Linux/PipeWire backend and Linux acceptance
- [ ] Microphone capture
- [ ] Secure non-loopback operation
- [ ] Production service installation
- [ ] Optional FFT, spectrum, and frequency-band products

The detailed milestone history and provider backlog live in the
[Roadmap](docs/roadmap.md).

## Documentation map

| Document | Purpose |
| --- | --- |
| [Consumer Protocol](docs/consumer-protocol.md) | Build an external consumer and implement the v1 wire format |
| [API](docs/api.md) | Understand signal, identity, lifecycle, and failure semantics |
| [Architecture](docs/architecture.md) | Understand crate ownership and internal provider design |
| [Roadmap](docs/roadmap.md) | Review completed milestones and deferred provider work |
| [Architecture decisions](docs/decisions/) | Read the accepted design records and tradeoffs |
| [Contributing](CONTRIBUTING.md) | Follow repository contribution expectations |

The source-selection, discovery, identity, and local transport decisions are
recorded in [ADR 0013](docs/decisions/0013-source-selection-model.md),
[ADR 0014](docs/decisions/0014-source-discovery-and-identity-model.md),
[ADR 0015](docs/decisions/0015-consumer-discovery-and-identity-registry.md),
and [ADR 0016](docs/decisions/0016-local-consumer-transport.md).

## Development validation

Use the repository's standard validation commands from the root:

```powershell
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo doc --workspace --no-deps
git diff --check
```

Clippy is not currently a required clean workspace gate.

## License

Resonance Signal is licensed under the GNU General Public License, version 3
only (`GPL-3.0-only`). See [LICENSE](LICENSE).
