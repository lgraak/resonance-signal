# Resonance Signal Consumer Protocol v1

This document is the complete integration contract for external Resonance Signal consumers. A client does not need to link Rust code or inspect provider implementation source.

> **Milestone 6U supports local-machine loopback clients only.** The service is not designed or secured for untrusted networks.

## Connection

Normal beta launch starts the tray-managed provider automatically. For a
console diagnostic session from the repository root, use:

```text
cargo run -p resonance-agent --bin resonance-agent-cli -- serve
```

The default listener is `127.0.0.1:48480`. `--host ::1` selects IPv6 loopback and `--port <1..=65535>` selects another port. The agent rejects wildcard and non-loopback addresses. The HTTP base path is `/v1`; the WebSocket path is `/v1/waveform`.

Only one long-running agent process should own the installation's private
identity registry and loopback port. The Windows beta tray process owns that
user-session lifecycle; Windows Service installation remains out of scope.

## Protocol versioning

The current wire protocol version is integer `1`. Every JSON response or event contains `protocol_version: 1`; every binary waveform frame contains version `1` in its header. A client must reject unsupported versions before interpreting other fields.

Version 1 JSON objects may gain fields. Clients must ignore unknown response and event fields. Field removal, field meaning changes, enum meaning changes, endpoint changes, or binary layout changes require a new protocol namespace or binary version. Client-to-server control objects are different: the server accepts only the exact documented shape and rejects unknown fields.

## HTTP operations

### `GET /v1/status`

Returns minimal consumer-oriented process status:

```json
{
  "protocol_version": 1,
  "status": "ready",
  "listener_scope": "loopback",
  "active_stream_sessions": 0
}
```

`status` is `ready` or `stopping`. This operation is diagnostic, not an administrative control plane.

### `GET /v1/sources`

Returns a complete replaceable discovery snapshot:

```json
{
  "protocol_version": 1,
  "revision": "snapshot-1234-1",
  "sources": [
    {
      "source_id": "opaque-provider-id",
      "display_name": "Speakers",
      "kind": "playback",
      "availability": "available",
      "default_playback": true,
      "supported_products": ["waveform"]
    }
  ]
}
```

The `revision` is an opaque equality-only freshness token for this service response. Do not parse, order, or use it as source identity. Each `source_id` is opaque and is meaningful only within one provider installation on one host. Persist it only as Explicit Source intent and tolerate later failure after registry reset or loss of proven continuity.

`display_name` is a nullable presentation label and may be duplicated or changed. `kind` is currently `playback`. `availability` is `available`, `unavailable`, or `unknown` and is advisory until capture start. `default_playback` is point-in-time membership in the Windows Default Playback role. `supported_products` currently contains only `waveform`.

Default Playback is a logical selection option, never a synthetic `SourceId`. Selecting an ID whose descriptor currently has `default_playback: true` is still Explicit Source intent.

Discovery failure returns HTTP `503` with `error: "source_discovery_unavailable"`. No response contains a native endpoint ID, continuity token, registry namespace, tombstone, schema, or storage path.

## Starting a waveform stream

Open one WebSocket connection per independent consumer stream/session.

Default Playback:

```text
ws://127.0.0.1:48480/v1/waveform?source=default-playback
```

Explicit Source:

```text
ws://127.0.0.1:48480/v1/waveform?source_id=<percent-encoded-opaque-SourceId>
```

Supply exactly one form. `source_id` is limited to 256 UTF-8 bytes. Missing, duplicate-form, empty, and oversized selection requests return HTTP `400` before upgrade. An unknown, unavailable, retired, or changed explicit ID emits `stream_error` and never falls back to Default Playback, a same-named source, or another endpoint.

Default Playback resolves the current role for this new attempt. Explicit Source resolves only the exact current mapping for the supplied ID. Active streams never migrate. Every attempt receives a fresh `StreamId` and zero-based timeline, including repeated attempts for the same `SourceId`.

## Stream-start metadata

The first meaningful stream message is a UTF-8 JSON text event:

```json
{
  "type": "stream_started",
  "protocol_version": 1,
  "stream_id": "stream-1234-1",
  "source_id": "opaque-provider-id",
  "source_kind": "playback",
  "sample_rate_hz": 48000,
  "channels": 2,
  "channel_order": ["front_left", "front_right"],
  "sample_format": "f32-le",
  "window_duration_ns": 33333333
}
```

- `stream_id` identifies one uninterrupted attempt and must not be reused as source identity.
- `source_id` is the exact resolved opaque source.
- `sample_rate_hz` is sample frames per second.
- `channels` is `1` or `2`. Mono order is `mono`; canonical stereo order is `front_left`, `front_right`. A future discrete channel order is represented as `discrete` entries without invented speaker meaning.
- `sample_format` is finite normalized linear PCM `f32` encoded little-endian. `-1.0` and `1.0` are nominal full scale; finite headroom outside that range is preserved rather than clipped or dynamically peak-normalized.
- `window_duration_ns` is the scheduler target. Integer sample-frame rounding determines actual window duration.

Do not interpret binary frames until this event has been accepted.

## Binary waveform frame

Each complete scheduled window is one WebSocket binary message. The maximum complete message size is 1,048,576 bytes. Multi-byte values are unsigned little-endian except samples, which are IEEE-754 binary32 little-endian.

| Offset | Size | Type | Meaning |
| ---: | ---: | --- | --- |
| 0 | 4 | ASCII | Magic `RSWF` |
| 4 | 1 | `u8` | Binary format version, currently `1` |
| 5 | 1 | `u8` | Header length, currently `40` |
| 6 | 2 | `u16` | Flags, currently zero; reject unknown nonzero bits |
| 8 | 8 | `u64` | Session-local packet sequence, zero-based and increasing by one |
| 16 | 8 | `u64` | Zero-based source sample-frame index for the first frame in this window |
| 24 | 8 | `u64` | Stream-relative monotonic time in nanoseconds for the first frame |
| 32 | 4 | `u32` | Number of sample frames in this window |
| 36 | 2 | `u16` | Channel count; must match `stream_started` |
| 38 | 2 | `u16` | Reserved, currently zero |
| 40 | variable | `f32[]` | Interleaved waveform samples |

Message length must equal `40 + frame_count * channels * 4`. Samples are sample-frame-major and channel-minor. Stereo payload order is `L0, R0, L1, R1, ...`. `frame_count` counts complete interleaved sample frames, not individual `f32` values.

Sequence numbers describe delivered scheduled windows. Frame indices and stream times describe source continuity. A consumer must reject malformed length, unsupported version, nonzero reserved/flag fields, invalid channel count, non-finite samples, sequence regression, or continuity inconsistent with the active stream.

## Lifecycle and error messages

All lifecycle and error messages are WebSocket text messages containing UTF-8 JSON.

### `stream_error`

```json
{
  "type": "stream_error",
  "protocol_version": 1,
  "kind": "source_unavailable",
  "scope": { "type": "subscription" },
  "retry": "wait_for_source"
}
```

`kind` is one of `source_unavailable`, `permission_denied`, `stream_interrupted`, `unsupported_format`, `invalid_request`, `resource_exhausted`, `internal`, or the transport-specific `consumer_too_slow`. `scope.type` is `subscription`, `source`, or `stream`; source and stream scopes include the corresponding opaque ID. `retry` is `retry_now`, `retry_later`, `wait_for_source`, `request_permission`, `change_format`, or `do_not_retry`.

An error is followed by `stream_stopped` when a stream had started. A startup failure may emit only `stream_error` and then close because no `StreamId` exists. Retry guidance is advisory: this milestone performs no automatic recovery or stream replacement.

### `stream_stopped`

```json
{
  "type": "stream_stopped",
  "protocol_version": 1,
  "stream_id": "stream-1234-1",
  "reason": "consumer_cancelled"
}
```

`reason` is `consumer_cancelled`, `source_ended`, `source_reconfigured`, `provider_shutdown`, or `failed`. The current connection is terminal after this event. To continue after source, format, or provider boundaries, create a new WebSocket and accept its new `StreamId` and metadata. There is no in-place migration.

## Stopping and client input

For a confirmed clean stop, send this exact JSON text message:

```json
{"type":"stop"}
```

The server stops that session, emits `stream_stopped` with `reason: "consumer_cancelled"`, and closes normally. Closing or disconnecting the WebSocket also releases the session, but the disconnected client cannot observe confirmation. Client binary messages, other text objects, extra stop fields, fragmented oversized messages, and messages above 1,024 bytes are rejected.

## Backpressure and multiple consumers

Each WebSocket owns an independent capture supervisor, window scheduler, and bounded 16-item event queue. The service accepts at most 16 simultaneous sessions and rejects further upgrades with `resource_exhausted`. Capture-to-transport delivery uses non-blocking queue insertion on the ordinary capture-processing thread; network I/O never runs on the platform capture thread. Socket writes have a two-second deadline.

If the per-client queue fills or a socket write stalls, the provider terminates only that unhealthy session, emits `consumer_too_slow` when the socket remains writable, and releases its capture resources. It does not accumulate arbitrary latency. Other clients continue independently. Registry-backed discovery and capture startup validation are briefly serialized; active captures are not globally serialized or mixed.

## Complete example session

```text
client -> GET /v1/sources
server -> 200 snapshot with source_id S and default_playback=true

client -> WebSocket /v1/waveform?source=default-playback
server -> text stream_started(stream_id=A, source_id=S, ...)
server -> binary RSWF sequence=0 frame_index=0 ...
server -> binary RSWF sequence=1 frame_index=<next> ...
client -> text {"type":"stop"}
server -> text stream_stopped(stream_id=A, reason=consumer_cancelled)
server -> normal WebSocket close

client -> WebSocket /v1/waveform?source_id=S
server -> text stream_started(stream_id=B, source_id=S, ...)
          B differs from A; the source matches S exactly
```

Run the dependency-free diagnostic consumer from the repository root after starting the service:

```text
python examples/consumer.py
python examples/consumer.py --source-id <opaque-source-id>
```

It performs HTTP discovery, WebSocket upgrade validation, stream-start parsing, byte-for-byte binary waveform parsing, an exact stop request, and clean stream-end validation.

## Security and deployment boundary

Milestone 6U supports local-machine loopback clients only. Loopback does not make the protocol appropriate for mutually untrusted local users, browser-origin exposure, containers with surprising network topology, or hostile inputs beyond the documented bounds.

Non-loopback listening is future work and requires a separate security design covering authentication, authorization, transport protection, origin and abuse controls, resource quotas, firewall behavior, deployment, and operational auditability. Version 1 does not provide remote authentication or TLS, and `0.0.0.0` is not supported or documented as an operational option.
