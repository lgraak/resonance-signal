# Windows Source Identity Evidence Packet (Milestone 6L-C)

# Environment

- host: `ARRAKIS`
- OS: `Microsoft Windows NT 10.0.26100.0`
- repository: `https://github.com/lgraak/resonance-signal`
- repository commit: `f3f17ef19a227df07f184cf9fd66a1e5ce22c937`
- Rust toolchain: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- audio endpoints observed in this session:
  - `Headphones (WH-1000XM5)`
  - `Realtek Digital Output (Realtek USB Audio)` (from prior captured evidence)

## Baseline

### Baseline capture for this packet

- command: `cargo run -p resonance-agent -- --duration-seconds 3`
- start marker: `2026-08-24T09:27:58.3452518-07:00`
- selected source/endpoint: `Headphones (WH-1000XM5)` (via capture report)
- default playback device: observed as active playback endpoint in this run
- sample rate: `96000 Hz`
- channels: `2`
- packet behavior:
  - packet frames: `Some(960)..Some(960)`
  - WASAPI max packet frames: `2112`
  - packets/audio frames/source frames: `300 / 300 / 288000`
  - callback interval: `Some(8.9689ms)..Some(10.7655ms)`
  - max callback duration: `Some(49µs)`
  - WASAPI QPC delta: `Some(9.9886ms)..Some(10.0143ms)`
  - initial discontinuity: `true`
- stream end behavior: `StopRequested`
- end marker: `2026-08-24T09:28:01.4371357-07:00`

### Prior cohort baseline context (6L-B reference)

- command in prior run: `cargo run -p resonance-agent -- --duration-seconds 20`
- selected endpoint: `Headphones (WH-1000XM5)`
- sample rate/channels: `96000 Hz`, `2`
- packet cadence: `Some(960)..Some(960)` (`WASAPI max 2112`)
- stream end: `StopRequested` (`2001/2001/1,920,960`)

## Change Event

### Experiment A: Default Device Switching

- status: **not executed in this packet**
- exact action: could not be executed safely/reliably from this environment because no reliable scripted default-playback swap path was available and endpoint-control APIs returned access denied for direct COM/WMI-based inspection in this session.
- endpoint before: not changed in this packet
- endpoint after: no deterministic before/after pair captured
- timestamps: N/A

### Experiment B: Bluetooth Default Device On/Off

- exact action:
  - make `WH-1000XM5` default playback
  - start capture
  - power off headphones
  - restore power
  - start a new capture
- source evidence used:
  - captured failure at `2026-08-24T09:09:38.6509360-07:00` after power-off
- endpoint before failure: `Headphones (WH-1000XM5)`
- endpoint during first post-failure restart attempt: `Realtek Digital Output (Realtek USB Audio)` (`SourceReconfigured`)
- endpoint after confirmed restoration run: `Headphones (WH-1000XM5)`

## Existing Stream Behavior

- Does current stream change when default changes during a running capture?
  - **not observed in this packet** (no safe default-change execution during a running stream).
- For the Bluetooth power-off evidence:
  - stream event sequence:
    - `Started`
    - `Error(kind=SourceUnavailable, retry=WaitForSource, message=failed to query the next WASAPI packet: Windows returned an error: 0x88890004)`
    - `Ended(..., reason=SourceEnded)`
  - stream end token: `CaptureEnd::SourceUnavailable`
  - terminal event: same-window capture run ended with `SourceUnavailable` and no in-process restart
  - error classification: `SourceUnavailable` (device availability / source-lifecycle)

## New Capture Behavior

- Experiment A (default-switch) new capture: **not executed**; evidence currently missing.
- Experiment B:
  - first new capture after outage followed the active/reconfigured output path and selected `Realtek Digital Output (Realtek USB Audio)` with `SourceReconfigured`
  - second manual capture after explicit endpoint confirmation selected `Headphones (WH-1000XM5)` and completed with `StopRequested`
  - comparison:
    - does new capture match Windows default? **Yes when default/output context was aligned**
    - does new capture match previous endpoint? **No for first restart attempt, then Yes when endpoint was available as the intended default**

## Findings

- Does Windows capture follow default device?
  - New capture attempts appear to resolve to the current active default/output selection at startup.
- Does Windows preserve endpoint identity?
  - The same in-process stream does not preserve when endpoint disappears; it terminates.
  - Cross-process restart behavior does not guarantee previous endpoint preservation; it may follow a reconfigured default and can switch to alternate endpoint before returning to intended endpoint.
- What should Resonance Signal distinguish?
  - distinguish three concepts separately:
    - the user-intended **logical default selector** (`default-playback`)
    - the **resolved runtime endpoint** used for that capture
    - whether a restart is a **default-follow** or **fixed-previous-endpoint** behavior
  - this packet does not yet prove an explicit follow-default contract for an already-created owner; it only shows startup-time endpoint selection behavior and loss recovery behavior in manual restart scenarios.

## Open Items

- Experiment A requires a repeatable, auditable method to change default playback and capture the endpoint identity during both baseline and restarted runs.
- A small scripted step to switch defaults would allow definitive evidence that a live owner should not preserve previous endpoint by design.
