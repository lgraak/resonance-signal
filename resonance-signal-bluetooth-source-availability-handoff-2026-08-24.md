# Resonance Signal Task Handoff

## Objective

Collect bounded real-world evidence for one Bluetooth playback endpoint cohort (`Sony WH-1000XM5`) under `SourceUnavailable` scenarios for Milestone 6L-B-1.

## Execution Context

- host: `ARRAKIS`
- OS: `Microsoft Windows NT 10.0.26100.0`
- working directory: `D:\Aeons\Git\resonance-signal`
- repository: `https://github.com/lgraak/resonance-signal`
- branch: `main`
- Rust version: `rustc 1.98.0 (88d9e12ae 2026-08-18)`

## Repository State

- branch: `main`
- remote: `origin` → `https://github.com/lgraak/resonance-signal.git`
- branch: `main` at `d9ca301`
- HEAD commit: `d9ca301 (Correct handoff branch metadata to final commit)`
- working tree status at handoff: clean
- remote state:
  - `origin/main` at `d9ca301`

## Completed Work

- established new baseline capture on `Headphones (WH-1000XM5)` for stability (20-second run),
- injected one controlled availability fault by powering headphones off,
- collected terminal `SourceUnavailable` behavior and timing,
- executed manual restart captures to observe restoration, including one fallback/default-change path and one confirmed WH-1000XM5 restoration path.

## Evidence Summary

### Baseline

- endpoint: `Headphones (WH-1000XM5)`
- sample rate: `96000 Hz`
- channels: `2`
- sample cadence/timing:
  - packet size: `Some(960)..Some(960)` (`WASAPI max=2112`)
  - callback interval: `Some(3.8107ms)..Some(16.1916ms)`
  - maximum callback duration: `Some(168.5µs)`
  - QPC delta: `Some(9.9882ms)..Some(10.0126ms)`
- baseline duration/behavior: `ProviderShutdown` after configured run with normal stream identity and continuous packets (`2001 / 2001 / 1,920,960`).

### Failure Behavior

- trigger: headphones powered off while stream active,
- stream start: `2026-08-24T09:08:58.3374137-07:00`,
- `stream error`: `SourceUnavailable` with `0x88890004` at `2026-08-24T09:09:38.6509360-07:00`,
- end: `SourceEnded` at `2026-08-24T09:09:38.6509360-07:00`,
- terminal summary: `stream end: SourceUnavailable (failed to query the next WASAPI packet: Windows returned an error: 0x88890004)`.

### Restoration Behavior

- first manual restart after fault switched to `Realtek Digital Output (Realtek USB Audio)` and ended `SourceReconfigured`,
- second manual restart with confirmed active headphone endpoint restored to `Headphones (WH-1000XM5)` and completed normally (`StopRequested`),
- restart path showed immediate stream start and stable-cadence packet flow on confirmed WH-1000XM5 run.

### Measurements

- failure detection latency: ~`40.3 s` from failure run start to `SourceUnavailable` event,
- cleanup latency: immediate stream-end reporting on terminal failure,
- restart latency: command-to-start under `1 s` for restored WH-1000XM5 run,
- time to stable frames post-restoration: immediate once stream started,
- recurrence: one observed Bluetooth power-off event in this packet,
- resource behavior: no process crash or additional owner-runtime errors in observed runs.

## Decisions Made

- classification: `SourceUnavailable` remains the failure class; no recovery enabled.
- comparison with existing evidence:
  - same class and terminal behavior as prior evidence cohort,
  - different sample-rate/device-path characteristics (`96000 Hz` bluetooth path),
  - additional observed behavior: an intermediate default-device replacement (`SourceReconfigured`) can occur before stable restart to the original Bluetooth endpoint.

## Files Changed

Created:
- `docs/evidence/resonance-signal-bluetooth-source-availability-evidence-2026-08-24.md`
- `resonance-signal-bluetooth-source-availability-handoff-2026-08-24.md`

Modified:
- None

Removed:
- None

## Validation Completed

- `cargo fmt --all --check` — passed
- `cargo check --workspace --all-targets` — passed
- `cargo test --workspace` — passed (`75 + 2 + 4 + 25` tests and doctests)
- `cargo doc --workspace --no-deps` — passed
- capture runs executed:
  - baseline `--duration-seconds 20`,
  - failure-injection `--duration-seconds 180`,
  - manual restart checks `--duration-seconds 20` and `--duration-seconds 15`.

## Unresolved Issues and Assumptions

- Known limitations:
  - single-trial fault injection; no repeated cycle statistics,
  - one transient endpoint-selection fallback to Realtek observed during manual recovery attempt.
- Deferred work:
  - repeated WH-1000XM5 fault cycles and default-selection control checks,
  - resource-pressure and multi-instance impact tests (outside current evidence-only scope).
- Assumptions:
  - default endpoint selection was restored to the Bluetooth endpoint for the confirmed restoration run,
  - reported wall-clock timestamps are sufficient as monotonic anchors for this bounded packet.

## Safety, Rollback, and Access Considerations

- performed reversible hardware actions only (headphone power cycle),
- no code, driver, registry, service, or settings modifications,
- rollback approach for this task is evidence replay only: repeat run with same command sequence and a controlled sequence of headset on/off actions.

## Do Not Redo or Reopen

- Do not enable recovery from this packet.
- Do not add retry logic, timers, endpoint watchers, owner replacement, or reconnect behavior in this milestone.
- Do not generalize to other cohorts from this single Bluetooth trial.

## Next Recommended Action

- Collect additional WH-1000XM5 trials with explicit default-endpoint control to quantify:
  - repeat detection-time distribution,
  - restoration consistency to the same endpoint,
  - whether endpoint fallback to non-Bluetooth is stable and operator-observable.
