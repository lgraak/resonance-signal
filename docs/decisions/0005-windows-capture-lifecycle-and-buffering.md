# ADR 0005: Windows capture lifecycle and buffering

- Status: Accepted
- Date: 2026-08-23

## Context

The Milestone 5A WASAPI playback-loopback prototype proved the capture contract, and Milestone 5B real-device evidence established a 96 kHz stereo stream with 960-frame packets, approximately 10 ms packet/QPC cadence, sub-millisecond callback work, fresh stream identities on later runs, and an initial discontinuity flag on every observed run.

Productionization must keep the platform callback non-blocking, make lifecycle outcomes machine-actionable, and give a future owner explicit start/stop control without adding reconnect, transport, service, discovery, or consumer policy.

## Decision

The production Windows entry point captures the console-role default playback endpoint until a cloneable stop token is requested or the uninterrupted stream ends. Duration remains only in the diagnostic CLI/helper, is validated from 1 through 3600 seconds, and is not capture configuration.

The WASAPI owner retains a pool of four buffers and a synchronous handoff of the same depth. Each buffer is allocated once to the initialized client's maximum packet size. Pool depth, buffer size, event-wait cadence, format conversion, and packet sizing remain internal implementation details rather than public tuning knobs. The callback only waits for WASAPI, queries/copies a packet, records native evidence, and attempts non-blocking handoff. Pool or channel exhaustion ends the stream with `ResourceExhausted`; no packet is silently dropped.

Normal owner stop maps to `ProviderShutdown`; diagnostic duration expiry maps to `ConsumerCancelled`. Default endpoint replacement and observed format change map to source reconfiguration. Removal or disablement maps to source unavailable. Session interruption, continuity/timing failure, overload, and internal failure end the current stream explicitly. Initial packet discontinuity records pre-stream history; a later discontinuity ends the stream.

Capture does not reconnect automatically. A later owner invocation creates a new `StreamId`, frame index zero, and timestamp zero. The owner decides whether and when to retry from platform-neutral error kind, retry hint, and end reason.

`CaptureEnd`, `CaptureRunError::kind`, and `CaptureRunError::retry_hint` are machine-actionable. Diagnostic strings, capture reports, and console rendering are human evidence and must not be parsed for control behavior.

## Consequences

- Callback work and memory remain bounded and independent of consumer speed.
- Slow processing becomes a visible failed stream instead of hidden loss or unbounded latency.
- Future long-running owners have explicit normal-stop and retry policy boundaries.
- The four-slot depth is deliberately conservative and can change internally if later device evidence requires it without changing consumer contracts.
- Default-device selection, automatic reconnect, service lifetime, transport backpressure, microphone capture, and Linux capture remain separate decisions.

## Alternatives rejected

- Public buffer-depth and packet-size knobs: rejected because current evidence does not justify exposing backend tuning, and invalid combinations would weaken the bounded guarantee.
- Duration as production lifetime configuration: rejected because a reusable capture boundary must be owner-controlled rather than timer-owned.
- Silent overwrite or packet drop under load: rejected because it would violate uninterrupted-stream continuity.
- Automatic reconnect inside the adapter: deferred because retry ownership, backoff, shutdown coordination, and endpoint-following policy belong to a long-running owner that does not exist yet.
