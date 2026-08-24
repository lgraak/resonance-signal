# ADR 0004: Capture backend selection

- Status: Accepted
- Date: 2026-08-23
- Evidence reviewed: 2026-08-23

## Context

Resonance Signal is a standalone audio signal provider. Platform capture is owned by `resonance-agent`; accepted audio enters the existing provider-independent `resonance-api` and `resonance-core` contracts as bounded, finite, interleaved `f32` `AudioFrame` batches. Platform API types, identifiers, clocks, and errors must not enter the core or consumer API.

The capture choice determines whether the provider can report an uninterrupted stream truthfully. A library that delivers samples but hides the accepted format, source position, timestamp validity, discontinuities, device replacement, or renegotiation cannot satisfy the contract. Convenience and one cross-platform API are therefore secondary to preserving platform evidence.

This decision selects implementation directions only. It adds no dependency and implements no capture path.

## Requirements

Every backend must:

- capture microphone input and playback output on its platform;
- accept and report an actual mono or two-channel stereo representation, or reject the source before stream start;
- produce complete, bounded, variable-sized sample batches suitable for conversion to interleaved finite `f32`;
- provide a monotonic time basis and enough source-position evidence to detect gaps, overlaps, or invalid timestamps;
- expose interruptions, device loss, restart, default-device change, format renegotiation, and other continuity-breaking events;
- preserve a stable backend source identity long enough for `resonance-agent` to assign its own opaque `SourceId`;
- allow the real-time or event callback to do fixed, bounded work and hand data to non-real-time orchestration without an unbounded queue;
- retain native error codes, flags, selected formats, and source properties for diagnostics;
- permit hardware-independent tests above a narrow capture-adapter seam; and
- be license-compatible with a `GPL-3.0-only` executable.

Windows additionally requires Windows 11 WASAPI rendering-endpoint loopback and capture-endpoint microphone input. Linux requires native PipeWire-compatible sink-monitor and source capture, including graph lifecycle and discontinuity evidence where PipeWire supplies it.

The comparative result is:

| Candidate | Ecosystem and API | Timing, format, and lifecycle evidence | Latency and testability | Operational surface | License | Result |
| --- | --- | --- | --- | --- | --- | --- |
| Direct WASAPI via `windows` | Authoritative, current bindings; application owns COM policy | Complete native evidence | Event-driven; project seam still needed | Medium implementation and debugging burden | MIT/Apache-2.0 | Compatible escape hatch |
| `wasapi` 0.24.0 | Current focused safe wrapper | Preserves device position, QPC timestamp, flags, format, endpoint, and notifications | Event-driven or polled; fixed-slice path; project seam needed | Narrow Windows-only dependency | MIT | Selected for Windows |
| Direct `libpipewire` / `pipewire-sys` | Authoritative native API; unsafe FFI surface | Complete native graph and buffer evidence | Real-time process callback; project seam still needed | Medium implementation and safety burden | Primarily MIT | Compatible escape hatch |
| `pipewire` 0.10.1 | Current official safe bindings | Preserves negotiation, graph time, registry, state, and raw metadata access | Real-time process callback; preallocated handoff required | System PipeWire runtime/development integration | MIT | Selected for Linux |
| CPAL 0.18.2 | Largest Rust audio I/O community; clean callback API | Loses WASAPI timestamp-validity/source position and timing provenance | Strong callback model and custom-host seam | Small Rust surface plus platform libraries | Apache-2.0 | Rejected initially |
| GStreamer / `gstreamer-rs` | Very mature media framework and Rust bindings | Rich caps, clocks, buffers, bus, and state; native flag mapping is indirect | Mature streaming model and test sources | Largest runtime/plugin and packaging surface | MIT/Apache-2.0 plus LGPL-2.1-or-later runtime | Rejected initially |

## Candidates evaluated

### Windows: direct WASAPI through `windows`

The `windows` crate is the maintained Microsoft Rust projection for Win32 and COM APIs. Direct use provides complete access to `IAudioClient`, `IAudioCaptureClient`, `IMMDeviceEnumerator`, `IMMNotificationClient`, and audio-session events.

Strengths:

- It exposes the complete WASAPI contract without a wrapper-defined information boundary.
- `IAudioCaptureClient::GetBuffer` returns packet frame count, device position, QPC timestamp, `DATA_DISCONTINUITY`, and `TIMESTAMP_ERROR`. These map directly to Resonance Signal frame indices, stream-relative time, and stream-boundary decisions.
- Rendering endpoints can be opened in shared loopback mode; capture endpoints provide microphone input. Event-driven buffering is supported on Windows 11.
- Endpoint IDs and endpoint/default-device notifications provide source identity and lifecycle evidence.
- `windows` is actively maintained, broadly used, and dual-licensed MIT or Apache-2.0.

Weaknesses:

- Resonance Signal would own COM initialization, interface lifetimes, format structures, event handles, HRESULT classification, buffer release discipline, notification implementations, and more unsafe-adjacent code.
- The larger implementation and review surface duplicates functionality already provided by a focused safe wrapper.
- Hardware-independent tests still require a project-owned adapter seam because COM interfaces are not simple test doubles.

Contract compatibility: complete, but at unnecessary initial implementation cost. Keep direct `windows` use as the escape hatch if the selected wrapper later hides required evidence or lags a required Windows API.

Maintenance: strongest upstream ownership and API coverage among the Windows options. The cost is application-owned audio policy rather than binding maturity.

### Windows: `wasapi` 0.24.0 (`wasapi-rs`)

`wasapi-rs` is a focused safe Rust wrapper that follows the WASAPI object model rather than replacing it with a generic audio callback API.

Strengths:

- It supports rendering-endpoint loopback, capture, shared/exclusive modes, event-driven and polled operation, format queries, format-support checks, endpoint IDs, and endpoint notifications.
- `AudioCaptureClient::read_from_device` accepts a caller-owned slice and returns `BufferInfo` containing the first frame's WASAPI device index, 100-nanosecond QPC timestamp, and decoded discontinuity, silence, and timestamp-error flags. No required packet evidence is discarded.
- Device callbacks cover add/remove, state, property, and default-device changes. Audio-session disconnection callbacks and HRESULT results provide complementary stream-loss evidence.
- The slice API permits fixed-capacity capture work. The deque helper is not suitable for the real-time path because it can grow; it is not required.
- The wrapper is MIT-licensed, uses current `windows` bindings, has runnable loopback and record examples, and released 0.24.0 from an active repository in August 2026.

Weaknesses:

- It is Windows-only and has a smaller maintainer/community base than CPAL or `windows`.
- Its types intentionally stay close to WASAPI, so `resonance-agent` must still own format policy, stream identities, reconnection, bounded transfer, sample conversion, and diagnostic mapping.
- `AudioCaptureClient` is neither `Send` nor `Sync`, so the adapter needs explicit COM-thread ownership and cannot move a live client between ordinary Rust worker threads.
- Automated tests for wrapper calls remain limited without Windows audio endpoints; orchestration must be tested behind a fake adapter.

Contract compatibility: best initial Windows fit. It exposes every required packet-level truth signal while removing repetitive binding and COM plumbing.

Maintenance: narrower than `windows`, but current and purpose-built. Pin an evaluated release during implementation and re-audit its public evidence surface before upgrades.

### Linux: direct `libpipewire` / `pipewire-sys`

Direct C FFI or `pipewire-sys` exposes `pw_stream`, registry, SPA parameters, buffer metadata, and graph timing without a safe wrapper.

Strengths:

- It provides complete access to stream states, format parameters, registry globals, buffer metadata, graph timing, xrun/discontinuity indicators, and target properties.
- It is the lowest-level practical route when a safe binding has a coverage gap.
- PipeWire's native diagnostics (`PIPEWIRE_DEBUG`, `pw-dump`, `pw-mon`, and `pw-top`) are directly applicable.

Weaknesses:

- Resonance Signal would own raw pointer lifetimes, listener hooks, SPA POD construction/parsing, buffer return discipline, and C ABI safety.
- This duplicates the maintained safe coverage already present in `pipewire-rs` and expands the capture code and review surface.
- Direct FFI does not improve hardware-independent testing; a project seam is still required.

Contract compatibility: complete, but not justified while the safe official bindings expose the required APIs. Use the raw escape hatch only for a demonstrated binding gap.

Maintenance: the native PipeWire API is actively maintained, but direct use transfers more compatibility and safety work to Resonance Signal.

### Linux: `pipewire` 0.10.1 (`pipewire-rs`)

`pipewire-rs` is the Rust binding maintained in the PipeWire project. It provides safe wrappers over `libpipewire` and re-exports the lower-level system and SPA bindings when needed.

Strengths:

- `Stream` exposes process, state-change, parameter-change, IO-change, and buffer callbacks; `Stream::time` exposes graph `now`, `ticks`, rate, and delay.
- Native PipeWire sink playback capture is requested with `stream.capture.sink=true`; microphone capture uses source nodes. A selected target can use `object.serial` or `node.name` rather than an ephemeral global ID.
- Negotiated SPA audio format parameters report the actual sample format, sample rate, channel count, and channel positions. A change can end the current Resonance Signal stream before accepting a new descriptor.
- PipeWire buffer headers can expose sequence, PTS, gap, corruption, and discontinuity metadata. `pw_time.ticks` is monotonic and documented for timeline and xrun detection; `pw_buffer.time` provides capture-cycle time on current PipeWire.
- Registry `global` and `global_remove` events expose hotplug/reconfiguration. Stream states distinguish connecting, paused, streaming, unconnected, and error.
- The process callback can dequeue and return PipeWire-owned mapped buffers with minimal copying. The implementation can copy only into a fixed-capacity handoff and perform conversion and publication off the real-time callback.
- The crate is MIT-licensed, is published and documented by PipeWire, and released 0.10.1 on 2026-08-19.

Weaknesses:

- It requires the system PipeWire development/runtime libraries and distribution integration.
- PipeWire objects and local listeners require deliberate loop/thread ownership; the library documentation notes that most high-level objects are not generally `Send` or `Sync`.
- PipeWire policy and target movement are normally controlled by the session manager. The adapter must distinguish an intentional default-target move from continuity inside one stream.
- Some newest native fields require version-gated APIs or the re-exported raw bindings. Minimum supported PipeWire runtime and enabled crate features must be recorded by the implementation milestone.

Contract compatibility: best initial Linux fit. It preserves negotiated format, graph timing, buffer metadata, target identity, and lifecycle events without requiring application-owned raw FFI.

Maintenance: official, current bindings with a direct path to native APIs. Documentation is lower-level than CPAL, but the PipeWire C reference and diagnostic tools are strong.

### Cross-platform: CPAL 0.18.2

CPAL is the mature RustAudio cross-platform audio I/O library. Current CPAL supports WASAPI and optional native PipeWire backends, playback-endpoint loopback, microphone capture, timestamps, device changes, and xrun errors.

Strengths:

- It has the largest Rust audio I/O community of the candidates, active releases, stable callback patterns, broad examples, and Apache-2.0 licensing.
- Its public callback reports capture and callback instants. Current WASAPI code reads QPC timestamps and emits an xrun error for `DATA_DISCONTINUITY`; current PipeWire code reports stream/device changes and xruns.
- A single API would reduce platform-specific orchestration and make basic synthetic/custom-host tests easier.
- It exposes negotiated stream configuration and supports explicit fixed buffer-size requests where the backend permits them.

Weaknesses:

- On WASAPI, CPAL 0.18.2 reads the device position and buffer flags but exposes neither device position nor timestamp validity to the data callback. Its source handles `DATA_DISCONTINUITY` as a generic xrun but does not test `AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR` before publishing the QPC-derived capture instant. Resonance Signal therefore cannot prove whether a delivered timestamp was valid.
- Generic errors and device-change events do not preserve all native codes, endpoint/session detail, or a single ordered stream of packet and lifecycle evidence.
- The PipeWire backend can fall back from graph timing to a locally estimated timestamp. That is reasonable for general audio I/O but does not expose provenance to a provider whose contract distinguishes known timing from inferred timing.
- Platform-specific interop would reintroduce separate code while retaining the generic layer, producing two authorities for lifecycle and diagnostics.

Contract compatibility: close, but insufficient for the initial truth-preserving contract. Reconsider if CPAL adds explicit timestamp-validity, source-position/provenance, and ordered lifecycle metadata, or if measured prototypes show that the extra evidence is unnecessary.

Maintenance: strongest community adoption and a very active 0.18 line. Rejection is based on information loss, not maturity.

### Cross-platform: GStreamer with `gstreamer-rs`

GStreamer has maintained Rust bindings and platform source elements, including `wasapi2src` loopback on Windows and `pipewiresrc` on Linux. It provides caps negotiation, timestamped buffers, device monitoring, bus errors, state transitions, and extensive graph diagnostics.

Strengths:

- It is mature, widely deployed, well instrumented, and capable of both required platforms.
- Caps, buffer timestamps, bus messages, and pipeline state provide a rich media framework.
- `wasapi2src` supports endpoint selection and loopback; PipeWire integration is established.
- `gstreamer-rs` is MIT/Apache-2.0 and GStreamer is LGPL-2.1-or-later, compatible with this GPLv3-only application.

Weaknesses:

- It introduces a large native runtime and plugin deployment surface for a provider that only needs PCM capture.
- Correct operation depends on specific platform plugins being installed and version-aligned, increasing packaging and support complexity on both Windows and Linux.
- Pipeline timestamps and state are framework-level abstractions; proving the exact mapping from WASAPI timestamp-error/device-position flags or PipeWire discontinuity metadata would require plugin-specific investigation and possibly custom elements.
- The graph, GLib main-loop, plugin discovery, and conversion policy would become major operational architecture rather than a narrow capture adapter.

Contract compatibility: technically possible, but disproportionate and less transparent than the selected native wrappers.

Maintenance: excellent upstream maturity and debugging capability, offset by the broadest dependency and deployment surface.

## Decision

### Windows

Use `wasapi` 0.24.0 as the initial Windows capture approach, added later as a Windows-target-only dependency of `resonance-agent`.

The adapter will use shared event-driven WASAPI. Playback capture opens the selected rendering endpoint in loopback mode; microphone capture opens a capture endpoint. It will use the fixed-slice read API, preserve every `BufferInfo` field before conversion, and treat a data discontinuity, timestamp error, device-position mismatch, device invalidation, session disconnection, relevant endpoint change, or accepted-format change as an end to the current uninterrupted stream.

The selected initialized `WaveFormat`, endpoint ID, requested versus accepted conversion path, HRESULTs, and notification reason remain backend diagnostics. Only the mapped source identity, accepted mono/stereo format, stream-relative timestamp, bounded samples, and portable lifecycle/error events cross the `resonance-agent` boundary.

Use direct `windows` bindings only when a concrete requirement cannot be obtained through the pinned `wasapi` release.

### Linux

Use `pipewire` 0.10.1 as the initial Linux capture approach, added later as a Linux-target-only dependency of `resonance-agent`.

The adapter will create a native input `pw_stream`, select a source by stable PipeWire properties, set `stream.capture.sink=true` for playback/sink capture, and request finite `F32` mono or front-left/front-right stereo. It will parse and record the actual negotiated format. Stream states, registry changes, format parameters, SPA header flags, buffer time, and `pw_time` will drive lifecycle and continuity decisions.

The real-time process callback may inspect metadata and copy a bounded amount into preallocated storage; it must not allocate, log, wait, convert an unbounded batch, invoke consumer code, or grow a queue. Direct `pipewire-sys`/SPA access is permitted inside the Linux adapter only for metadata or versioned functions not surfaced safely by `pipewire-rs`.

### Shared abstraction

Do not use a third-party cross-platform capture abstraction for the first implementation.

The existing `AudioFrame`, `StreamDescriptor`, `StreamEvent`, `ProviderError`, and scheduler contracts are the shared boundary. A small crate-private adapter seam may be introduced when implementation begins so orchestration can be tested without hardware, but it must carry the evidence Resonance Signal needs rather than mimic CPAL, WASAPI, or PipeWire. Do not generalize a public backend trait before both platform adapters provide implementation evidence.

## Rejected alternatives

- Direct WASAPI through `windows`: rejected as the first path because `wasapi-rs` already exposes the required packet, format, event, and notification evidence with less unsafe-adjacent code. Retained as an escape hatch.
- Direct PipeWire FFI: rejected as the first path because official safe bindings expose the required stream, timing, registry, and parameter APIs. Retained as a narrow escape hatch.
- CPAL as the shared backend: rejected because its public contract currently loses Windows timestamp-validity and source-position evidence and can substitute inferred PipeWire timing without reporting provenance.
- GStreamer: rejected because its runtime/plugin and graph-management surface is disproportionate, while exact native discontinuity mapping is less direct.
- ALSA or PulseAudio as the Linux authority: rejected because the product requirement is PipeWire-compatible playback and microphone capture with PipeWire graph lifecycle. Compatibility bridges hide the native registry, target, parameter, and graph-timing evidence.
- Rodio and other playback-oriented libraries: rejected because they do not provide the required capture, loopback, lifecycle, and timing contract and would still depend on a lower-level capture backend.
- One platform-neutral implementation completed for both operating systems at once: rejected because it would multiply hardware and lifecycle variables before one adapter proves the contract.

## Consequences

Benefits:

- Each platform adapter retains the native truth needed to establish or end an uninterrupted `StreamId`.
- Platform-specific dependencies remain confined to `resonance-agent`; `resonance-core` and `resonance-api` stay unchanged.
- The selected safe wrappers reduce binding boilerplate while preserving raw escape hatches.
- Fixed-capacity handoff and hardware-independent orchestration tests can be designed explicitly instead of inheriting an unbounded callback model.
- Native diagnostics remain available: HRESULTs and endpoint/session notifications on Windows; PipeWire error strings, properties, and graph tools on Linux.

Limitations:

- Resonance Signal will maintain two platform adapters and two lifecycle mappings.
- The initial Windows and Linux dependency versions are evidence snapshots, not permanent pins; each upgrade requires checking the fields and events on which continuity depends.
- Real-device acceptance is still required. Documentation and source inspection do not prove driver-specific timestamp quality, conversion behavior, callback cadence, or restart recovery.
- Some multichannel playback devices will remain unsupported if the platform cannot produce a truthful mono or stereo stream.

Future migration considerations:

- Keep adapter-to-orchestration messages owned by Resonance Signal and free of public dependency types so either wrapper can be replaced without changing core/API contracts.
- Preserve native evidence in diagnostics and tests; do not reduce it to text before lifecycle mapping.
- Reconsider CPAL only against an explicit evidence checklist and recorded device matrix, not merely to reduce dependency count.
- If a selected wrapper becomes unmaintained, move the same adapter semantics to its underlying `windows` or `pipewire-sys` APIs before considering a contract change.

## Prototype plan

The next milestone is one Windows playback-loopback prototype only.

1. Add `wasapi` as a Windows-target-only `resonance-agent` dependency and introduce one private WASAPI playback adapter plus a hardware-free fake adapter.
2. Open one selected or default rendering endpoint in shared event-driven loopback mode and request a valid mono or front-left/front-right stereo format.
3. Copy each packet through fixed-capacity storage, convert the accepted native representation to finite interleaved `f32`, and produce bounded `AudioFrame` batches.
4. Derive the stream-relative timeline from the first valid WASAPI device position and QPC timestamp. End the stream on discontinuity, timestamp error, invalidation, relevant endpoint change, or format change.
5. Validate a normal loopback stream, silence, device stop/restart, default-device change, format reconfiguration, injected fake discontinuity/timestamp-error paths, and bounded behavior under a deliberately slow downstream test consumer.
6. Record actual selected format, packet-size distribution, timestamp/device-position deltas, callback duration, and lifecycle events. Do not add microphone capture, Linux capture, transport, service behavior, or automatic reconnection in this milestone.

Only after that evidence is reviewed should microphone support or the Linux PipeWire adapter begin.

## Evidence

Primary sources reviewed on 2026-08-23:

- [Microsoft: WASAPI loopback recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [Microsoft: `IAudioCaptureClient::GetBuffer` packet metadata and flags](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer)
- [Microsoft: WASAPI invalid-device recovery](https://learn.microsoft.com/en-us/windows/win32/coreaudio/recovering-from-an-invalid-device-error)
- [Microsoft: endpoint device events](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-events)
- [`windows-rs` repository and licensing](https://github.com/microsoft/windows-rs)
- [`wasapi` 0.24.0 `BufferInfo`](https://docs.rs/wasapi/0.24.0/wasapi/struct.BufferInfo.html)
- [`wasapi` 0.24.0 capture client](https://docs.rs/wasapi/0.24.0/wasapi/struct.AudioCaptureClient.html)
- [`wasapi-rs` supported functionality and examples](https://github.com/HEnquist/wasapi-rs/tree/v0.24.0)
- [PipeWire stream model and timing](https://pipewire.pages.freedesktop.org/pipewire/page_streams.html)
- [PipeWire `pw_time` timeline and xrun evidence](https://pipewire.pages.freedesktop.org/pipewire/structpw__time.html)
- [PipeWire stream events](https://pipewire.pages.freedesktop.org/pipewire/structpw__stream__events.html)
- [PipeWire buffer discontinuity metadata](https://pipewire.pages.freedesktop.org/pipewire/group__spa__buffer.html)
- [PipeWire sink-capture property](https://pipewire.pages.freedesktop.org/pipewire/devel/group__pw__keys.html)
- [`pipewire` 0.10.1 Rust bindings](https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/)
- [`pipewire` 0.10.1 stream API](https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/stream/struct.Stream.html)
- [`pipewire` 0.10.1 registry lifecycle](https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/registry/index.html)
- [CPAL 0.18.2 WASAPI capture source](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/wasapi/stream.rs#L789-L858)
- [CPAL 0.18.2 PipeWire timing source](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/pipewire/stream.rs#L299-L350)
- [CPAL repository, platform support, and licensing](https://github.com/RustAudio/cpal/tree/v0.18.2)
- [GStreamer `wasapi2src`](https://gstreamer.freedesktop.org/documentation/wasapi2/wasapi2src.html)
- [`gstreamer-rs` installation and licensing](https://gstreamer.freedesktop.org/documentation/rust/stable/latest/docs/gstreamer/)
- [GNU license compatibility guidance for GPLv3, Apache-2.0, and permissive licenses](https://www.gnu.org/licenses/license-compatibility.en.html)
