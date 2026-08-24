# Source Availability Evidence Packet (Cohort 1)

## Scenario

### Environment

- host: `ARRAKIS`
- OS: `Microsoft Windows NT 10.0.26100.0`
- repository commit: `bf9c89bc4dcfabdebfe2247225a99d42cf68bd20`
- device: `Realtek Digital Output (Realtek USB Audio)`
- device type: `playback output`
- connection type: `USB` (inferred from endpoint name and prior collection notes)

### Baseline

- endpoint: `Realtek Digital Output (Realtek USB Audio)` (`default-playback` selector)
- sample rate: `48000 Hz`
- channel layout: `2`
- packet size: `Some(480)..Some(480)` packets (`WASAPI max=1056`)
- callback timing: interval `Some(9.217ms)..Some(10.7657ms)`, max duration `Some(137.1µs)`
- QPC timing: `Some(9.9773ms)..Some(10.0238ms)`
- stable frame delivery: yes; baseline run ended in `StopRequested` after 20.0s with continuous frames.

### Failure Injection

- exact action performed: unplug/remove default playback output endpoint during active capture, then restore later while the process remained available for manual restart.
- approximate failure time: around `2026-08-24T08:46:08.375-07:00` observed stream end
- device transition: removed/disconnected then restored
- user-visible behavior: stream ended with source-unavailable error; no automatic in-process recovery occurred.

### Failure Observation

- StreamEvent sequence:
  - `Started`
  - `Error(kind=SourceUnavailable, retry=WaitForSource, default playback endpoint became unavailable)`
  - `Ended(id=..., reason=SourceEnded)` (reported in final summary as `SourceUnavailable`)
- terminal reason: `CaptureEnd::SourceUnavailable`
- error category: device availability / source lifecycle (`SourceUnavailable`)
- cleanup completion: owner completion and cleanup were reached during same run context after the terminal event.
- owner completion: observed via normal completion path (no replacement owner created).
- detection latency: ~`33.0 s` from baseline stream start to terminal source-unavailable signal.

### Restoration

- device availability restoration: endpoint returned by the time of manual restart run.
- whether original endpoint returned: yes, endpoint name remained `Realtek Digital Output (Realtek USB Audio)`.
- whether manual restart required: yes.
- restart latency: command-to-stream-start under `1 s` in local run.
- time to stable frames: immediate stream cadence after restart (`~10ms` packet cadence returned with normal packet sizing).

### Measurements

- failure detection time: `33.0 s`
- cleanup time: same-second transition from error signal to stream end
- restart time: `<1 s` (manual rerun)
- time to stable frames: immediate after restart `Started` event
- recurrence: one capture occurrence observed in this cohort
- resource behavior: no crash, no extra supervisor/runtime resource errors, no lock escalation observed.

### Assessment

- Do not enable recovery in this packet.
- Is this failure class consistent with prior evidence?  
  - Yes for class-level behavior: `SourceUnavailable` -> terminal `SourceEnded`/`CaptureEnd::SourceUnavailable`, no auto-recovery under disabled policy.
- What evidence is still missing?  
  - repeated multi-cycle recurrence at multiple timestamps; explicit disable/disable-enable and default-device change subtypes; resource-pressure and callback/backpressure deltas under load.
- Would automatic recovery be safe to consider later?  
  - Not yet. Evidence currently shows hard-termination behavior and manual restore viability, but not recovery efficacy/impact for diverse cohorts.

### Comparison vs prior SourceUnavailable evidence

- similarities:
  - same endpoint class (`default-playback`)
  - same terminal classification (`SourceUnavailable`/`SourceEnded`)
  - no in-process recovery under disabled policy
- differences:
  - this packet adds explicit restart latency and stable-frame resumption timing in the same packet
- unknowns:
  - no capture data for non-playback endpoints, no disable/restore notification timing from other hardware/transport paths, and no controlled multi-cohort recurrence matrix.

### Safety Requirements

- drivers not modified
- no permanent settings changed
- no software installed
- no registry edits
- no services created

## Compliance Note

Automatic recovery remains intentionally disabled for this evidence run.
