# ADR 0016: Local Consumer Transport

Status: Accepted and implemented by Milestone 6U

## Context

The provider already owns portable source discovery, Default Playback and Explicit `SourceId` selection, bounded waveform scheduling, and typed stream lifecycle. External clients need a language-neutral local process boundary without linking Rust or learning private Windows endpoint and registry structures.

Continuous waveform samples are too frequent and large for repeated JSON arrays. A slow consumer must not block capture or create unbounded memory growth. The milestone does not define hostile-network security, remote administration, service installation, or consumer-specific presentation.

## Decision

`resonance-agent serve` exposes versioned HTTP JSON discovery/status and one WebSocket per independent waveform session under `/v1`. Version 1 uses JSON text for metadata, lifecycle, errors, and the exact stop control, plus a fixed 40-byte little-endian header and interleaved `f32` payload for waveform windows.

The listener accepts only numeric loopback addresses. Configuration represents a socket address but validates loopback before binding, preserving a future configuration seam without enabling non-loopback operation.

Each WebSocket creates a separate capture supervisor and scheduler. A bounded per-client queue, a 16-session process limit, and timed socket writes isolate slow clients and bound aggregate work. Queue overflow terminates that client rather than blocking capture or accumulating latency. Discovery and registry-backed capture startup are briefly serialized so concurrent refreshes cannot stale another session's startup binding; active capture ownership remains independent.

Provider error categories and source/stream identities are mapped to portable protocol values. Native endpoint IDs, registry namespaces, continuity evidence, storage paths, and backend diagnostics do not cross the boundary. Default Playback remains role intent and Explicit Source remains exact opaque identity with no substitution.

## Alternatives Considered

- Direct Rust linking, DLL/FFI, shared memory, and gRPC were rejected because they increase runtime/language coupling or deployment complexity for the first local consumer boundary.
- JSON waveform arrays were rejected because they increase bandwidth, allocation, and parsing cost.
- One global capture session was rejected because the existing owner model safely permits independent captures and future consumers may select different sources.
- Unbounded queues and blocking socket writes were rejected because they violate the capture boundedness contract.
- Wildcard or LAN binding was rejected because version 1 has no remote authentication, authorization, or transport protection.

## Consequences

External clients can discover sources and consume waveform streams using ordinary HTTP and WebSocket libraries. Wire compatibility is explicit from the first release, waveform framing is deterministic, and one slow client cannot accumulate unbounded provider memory.

The agent now carries private mature HTTP/WebSocket, async runtime, and JSON dependencies. Thread-per-capture ownership remains inside the existing supervisor while network tasks run on the async runtime. Concurrent startup is serialized around private registry work, which may add brief connection latency but does not serialize active streams.

Version 1 is local-only and does not claim isolation between mutually untrusted local users. Remote access, authentication, TLS, browser-origin policy, abuse controls, service manager installation, persistent consumer configuration, and automatic recovery remain separate decisions.

## Implementation Status

Milestone 6U implements `/v1/status`, `/v1/sources`, `/v1/waveform`, the binary `RSWF` frame, exact stop control, bounded queue/backpressure behavior, the external diagnostic consumer, focused protocol tests, and the consumer specification in `docs/consumer-protocol.md`.
