# Playback Device Removal Evidence Packet

## Scenario

### Environment

- Host: ARRAKIS
- OS: Microsoft Windows NT 10.0.26100.0 (Win32NT)
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Repository: https://github.com/lgraak/resonance-signal
- Commit: `bf9c89bc4dcfabdebfe2247225a99d42cd68bd20`
- Capture device: `Realtek Digital Output (Realtek USB Audio)`
- Device type: playback output endpoint
- Connection type: USB (inferred from endpoint name)

### Baseline

- Capture command: `cargo run -p resonance-agent -- --duration-seconds 20`
- Timestamped start: `2026-08-24T08:48:02.334-07:00`
- Baseline stream start: `stream started: id=wasapi-loopback-46420-1, source=default-playback, rate=48000 Hz, channels=2`
- Packet behavior: packets `2000`, audio frames `2000`, source frames `960000`, observed packet size range `Some(480)..Some(480)`, WASAPI max packet `1056`.
- Callback timing: interval `Some(9.217ms)..Some(10.7657ms)`, maximum duration `Some(137.1µs)`.
- QPC timing: delta `Some(9.9773ms)..Some(10.0238ms)`.
- Frame continuity: `initial_discontinuity_observed: true`.
- Baseline completion: `stream end: StopRequested` after 20.0 seconds.

### Failure Injection

- Command: `cargo run -p resonance-agent -- --duration-seconds 120` with live line timestamping.
- Timestamped start: `2026-08-24T08:45:35.337-07:00`
- Action:
  - Playback output device was removed/disconnected during a running stream, then later restored.
- Observed timeline:
  - `2026-08-24T08:46:08.375-07:00` `stream ended: id=wasapi-loopback-47256-1, reason=SourceEnded`
  - `2026-08-24T08:46:08.376-07:00` `stream error: kind=SourceUnavailable, retry=WaitForSource, message=default playback endpoint became unavailable`
- OS/device reaction: default playback endpoint reported unavailable (Windows error `0x88890004` on non-timestamped companion run).
- User-visible impact:
  - Diagnostic stream terminated.
  - No automatic capture restart occurred within the same process lifetime.

### Failure Observation

- Stream-event sequence (observed):
  - `Started`
  - `Error(kind=SourceUnavailable, retry=WaitForSource, default playback endpoint became unavailable)`
  - `Ended(id=wasapi-loopback-47256-1, reason=SourceEnded)` (reported as `SourceUnavailable` in final summary)
- Terminal reason:
  - `CaptureEnd::SourceUnavailable` (stream end reason in report)
- Completion result:
  - `stream end: SourceUnavailable (failed to query the next WASAPI packet: Windows returned an error: 0x88890004)` in report
  - cleanup occurred in the same run context after event emission
- Error category:
  - Source lifecycle / device availability
- Diagnostics captured:
  - endpoint identity remained `Realtek Digital Output (Realtek USB Audio)`
  - report `stream end: SourceUnavailable`

### Restoration Observation

- During the failing capture window, the running owner did not recover automatically.
- Manual restart comparison:
  - `START_RESTART 2026-08-24T08:47:37.3648877-07:00`
  - `08:47:37.450-07:00 stream started: id=wasapi-loopback-38832-1, source=default-playback, rate=48000 Hz, channels=2`
  - `08:47:57.440-07:00 stream end: StopRequested`
- Manual restart required to observe new stream identity and restore delivery.
- Device availability at new start:
  - endpoint name unchanged (`Realtek Digital Output (Realtek USB Audio)`)

### Measurements

- Detection time from baseline stream start to source-unavailable signal: ~`33.0 s`
- Cleanup to stream end signal: immediate to same-second from injected failure event
- Restoration availability:
  - endpoint usable for new run after restore action (exact restore action timing inferred from successful restart run)
- Restart time:
  - observed command-to-start latency ~`~0.1 s` on hot cache
- Time to stable frames after restart:
  - stream started immediately with normal packet cadence within the restart run
- Recurrence:
  - 1 captured occurrence for this scenario
- Resource behavior:
  - no crash, no additional resource errors observed in supervisor path

### Recovery Assessment

- Would automatic recovery be beneficial?
  - Yes, a single source-unavailable failure produced hard stream termination and the owner did not self-recover.
- What evidence would still be required?
  - A bounded restart experiment proving recovery time and reliability after automatic relaunch.
  - Evidence that recovery does not violate sample continuity guarantees.
  - Multi-episode recurrence and resource pressure measurements over repeated removal/restore cycles.
  - Explicit user-impact metrics for forced restart on different playback-class devices.
- What risks remain?
  - No natural in-process recovery path; restart could mask transient device blips and alter stream identity/timestamps.
  - Recovery behavior is still disabled by design and would require separate milestone approval.
