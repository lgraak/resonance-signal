# Source Availability Evidence Packet (Bluetooth Cohort)

## Environment

- host: `ARRAKIS`
- OS: `Microsoft Windows NT 10.0.26100.0`
- repository commit: `7769bc04d51ebe64368a70558502355123f137f5c0`
- device: `Headphones (WH-1000XM5)`
- connection type: `Bluetooth`

## Baseline

- endpoint name: `Headphones (WH-1000XM5)`
- sample rate: `96000 Hz`
- channels: `2`
- packet size: `Some(960)..Some(960)` packets (`WASAPI max=2112`)
- callback timing: `Some(3.8107ms)..Some(16.1916ms)` with maximum callback duration `Some(168.5µs)`
- QPC timing: `Some(9.9882ms)..Some(10.0126ms)`
- stable frame delivery: yes; 20-second baseline completed with `2001 / 2001 / 1,920,960` packets / audio frames / source frames and `StopRequested` end.

Output captured:

- `START_BLUETOOTH_BASELINE2 2026-08-24T09:08:33.9022203-07:00`
- `stream started: id=wasapi-loopback-48164-1, source=default-playback, rate=96000 Hz, channels=2`
- `stream end: id=wasapi-loopback-48164-1, reason=ProviderShutdown`
- evidence summary `stream end: StopRequested`

## Failure Injection

- exact action performed: powered off `Headphones (WH-1000XM5)` during active capture.
- approximate failure time:
  - `stream started` at `2026-08-24T09:08:58.3374137-07:00`
  - `stream error: kind=SourceUnavailable, retry=WaitForSource, message=failed to query the next WASAPI packet: Windows returned an error: 0x88890004` at `2026-08-24T09:09:38.6509360-07:00`
  - `stream ended: id=wasapi-loopback-20152-1, reason=SourceEnded` at `2026-08-24T09:09:38.6509360-07:00`
- observed Windows behavior: endpoint became unavailable with `0x88890004`, stream terminated without automatic recovery.

## Failure Observation

- StreamEvent sequence:
  - `Started`
  - `Error(kind=SourceUnavailable, retry=WaitForSource, message=failed to query the next WASAPI packet: Windows returned an error: 0x88890004)`
  - `Ended(id=wasapi-loopback-20152-1, reason=SourceEnded)`
- terminal reason: `CaptureEnd::SourceUnavailable`
- error category: device availability / source lifecycle (`SourceUnavailable`)
- cleanup behavior: terminal error transitioned quickly to stream end; no duplicate owner/restart occurred in same run.
- owner completion behavior: process completed run lifecycle to final evidence with `stream end: SourceUnavailable`.
- detection time: ~`40.3 s` from run start to source-unavailable event.

## Restoration

- headphones powered back on after fault window (manual operator action).
- endpoint return behavior:
  - first manual restart immediately after fault returned to fallback default endpoint behavior and ended with `SourceReconfigured` on `Realtek Digital Output (Realtek USB Audio)`.
  - a follow-up manual restart run with headphones confirmed as active endpoint restored to `Headphones (WH-1000XM5)` and completed normally.
- whether manual restart was required: yes.
- restart timing: command-to-stream start ~`0.1 s` (`09:12:26.2049363` to `09:12:26.3031418`)
- time to stable frames: immediate on post-recovery run (`stream started` then normal cadence and `1500` packets in 15 seconds).

## Measurements

- failure detection time: `40.3 s`
- cleanup time: same-second transition from error to stream end
- restart time (manual): immediate/sub-second
- time to stable frames: immediate after restart confirmation run
- recurrence observed: one Bluetooth availability occurrence in this packet
- resource behavior: no crash; no additional runtime resource errors during the run

## Assessment

- Bluetooth appears to be the same failure class (`SourceUnavailable`) as prior evidence.
- What evidence is still missing:
  - controlled compare with repeated power-cycle attempts,
  - precise operator action-to-detection lag bound under multiple outages,
  - whether the fallback endpoint behavior is deterministic across repeated on/off transitions.
- Would automatic recovery be reasonable to investigate later:
  - Same candidate class behavior is present, but this is still single-trial evidence and should remain manual-only for now.

## Comparison with prior SourceUnavailable evidence

- similarities:
  - terminal class and mapping (`SourceUnavailable` + `SourceEnded`),
  - immediate stream termination and no in-process automatic restart,
  - cleanup reporting and owner completion remain as implemented.
- differences:
  - Bluetooth baseline sample rate was `96000 Hz` vs previous USB cohort `48000 Hz`,
  - failure path included direct Windows error `0x88890004` with short immediate unavailability period,
  - restart path exhibited a `SourceReconfigured` fallback run (`Realtek`) before successful WH-1000XM5 endpoint restart.
- unknowns:
  - whether endpoint-selection persistence influences post-fault restore behavior,
  - whether repeated Bluetooth power cycles produce comparable outage and timing behavior.

## Safety Requirements

- no drivers, registry edits, services, or installed software.
- only reversible headphone power actions were used.

## Compliance Note

Automatic recovery was not enabled. Evidence is collection-only for Milestone 6L-B-1.

