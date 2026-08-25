# Milestone 6T Explicit SourceId Playback Capture Handoff

Date: 2026-08-24 America/Los_Angeles
Status: completed locally; commit and publication pending final scope validation
Model: GPT-5 (Codex)
Effort: unknown
Repository: Resonance Signal at `D:\Aeons\Git\resonance-signal`
Branch: `main`
HEAD: `4087a3e76c120c0845521216867e4d7caba33013` (verified starting revision; completion commit pending at handoff creation)
Authoritative remote: `origin` -> `https://github.com/lgraak/resonance-signal.git`

> This handoff is a continuation checkpoint, not authoritative truth. Current
> repository, remote, runtime, and test evidence wins if it conflicts with this
> document.

## Objective

Implement Milestone 6T Windows playback capture for an explicit opaque `SourceId`, preserving exact-ID/no-substitution semantics, independent Default Playback behavior, new stream identity per attempt, private native endpoint identity, recovery-disabled operation, and all stated platform/scope exclusions. The bounded implementation and validation work is complete locally.

## Authoritative Sources

- `AGENTS.md` instructions supplied with the work packet: scope, repository verification, validation, Git, and handoff requirements. No repository-owned `AGENTS.md` exists in this checkout.
- `docs/standards/ai-project-prompt-standard-v1.md` and `docs/standards/ai-project-handoff-standard-v1.md`: preflight, validation, publication, and checkpoint rules.
- `docs/decisions/0013-source-selection-model.md`, `docs/decisions/0014-source-discovery-and-identity-model.md`, and `docs/decisions/0015-consumer-discovery-and-identity-registry.md`: settled source intent, identity, registry, revision, and no-substitution semantics.
- `docs/api.md`, `docs/architecture.md`, and `docs/roadmap.md`: current contract, ownership boundaries, and milestone state.
- `crates/resonance-agent/src/discovery.rs`, `identity.rs`, `windows_discovery.rs`, `windows.rs`, and `supervisor.rs`: authoritative implementation contracts.
- Configured GitHub `origin`: authoritative remote. An approved `git fetch --prune origin` on 2026-08-24 verified `origin/main` at the starting revision with no ahead/behind difference before modification; remote state remains time-sensitive until post-push readback.
- Fresh Arrakis Windows discovery and capture runs described under Validation Completed: time-sensitive real-device evidence for the current endpoint and registry state.

## Execution Context

- Windows host Arrakis; PowerShell; repository root `D:\Aeons\Git\resonance-signal`.
- Stable Rust workspace with the existing `wasapi` 0.24.0 dependency; no dependency was added or changed.
- The managed sandbox could compile and test but could not write the production registry under `%LOCALAPPDATA%\Resonance Signal\provider-state`; approved real-device runs were therefore executed with the required existing local access.
- A normal initial remote query failed with Windows Schannel `SEC_E_NO_CREDENTIALS`; the approved Git context subsequently fetched and verified `origin/main` successfully without changing remotes.

## Current Repository State

- Starting branch and HEAD: `main` at `4087a3e76c120c0845521216867e4d7caba33013`.
- Starting working tree: clean; no unrelated user changes were present or discarded.
- Upstream and synchronization before modification: `main...origin/main`, ahead/behind `0/0`, with `origin/main` at the starting revision after fresh fetch.
- Working tree at handoff creation: only the intended Milestone 6T source, test, documentation, and this checkpoint changes are present.
- Commit and authoritative remote readback: pending final scope validation at handoff creation; verify current Git state rather than inferring later publication from this checkpoint.
- Preserved unrelated changes: None; none were present.

## Current Known-Good State

- The complete changed workspace passed the required formatting, all-target check, test, documentation, and diff checks on 2026-08-24.
- Hardware-independent results: 139 tests passed across the workspace; the single ignored real-endpoint discovery test was not counted as passing in the general suite.
- Real Windows results: the ignored discovery test was run separately and passed; one Default Playback capture and two Explicit Source captures completed normally on `Realtek Digital Output (Realtek USB Audio)` at 48 kHz stereo; an unknown explicit ID was rejected before `Started`.
- Starting remote known-good revision: `4087a3e76c120c0845521216867e4d7caba33013`. The completion revision and remote readback must be taken from current Git evidence after publication.

## Completed Work

- Added agent-level `PlaybackCaptureIntent::{DefaultPlayback, Explicit(SourceId)}` and carried it through production supervisor, factory, owner, and WASAPI startup construction.
- Extended private playback discovery snapshots with exact live source-to-endpoint capture bindings while keeping backend-native endpoint identity private.
- Added fresh explicit-ID resolution and revision-bound startup revalidation. Unknown, unavailable, retired, stale, missing, and endpoint-mismatched mappings fail closed before stream publication.
- Opened the exact privately mapped WASAPI endpoint for explicit capture; no later default endpoint is independently resolved or substituted.
- Kept Default Playback role-resolved at each attempt. Explicit capture does not treat default-role movement as a boundary, but endpoint removal/state/session changes still end its pinned stream.
- Emitted the exact resolved opaque requested `SourceId` in `StreamDescriptor`. Every attempt still obtains a new `StreamId` and restarts its timeline.
- Added `--source-id <opaque-id>` as a narrow diagnostic option without adding transport, configuration persistence, or a stable consumer service API.
- Added hardware-independent coverage for exact binding, duplicate names, independent default resolution, no substitution, unknown/retired/unavailable IDs, stale binding rejection, descriptor identity, endpoint mismatch, and repeated-stream identity behavior.
- Reconciled README, API, architecture, accepted ADR implementation status, and roadmap milestone state.

## Decisions Made

- Reused the accepted portable `SourceId` and existing private registry rather than broadening `resonance-api` or introducing a Windows-native selector type.
- Represented all active private endpoint bindings in one discovery snapshot so explicit and default capture share identical revision and registry evidence without sharing selection semantics.
- Revalidated the complete attempt binding after notification registration. Any discovery revision change during startup rejects publication even when the requested source ID still exists.
- Registered default-role-change termination only for Default Playback. Treating a role change as an explicit-stream boundary would collapse identity-pinned intent back into role-following behavior.
- Mapped registry resolution failures for unknown, absent, retired, and stale identities to the existing portable `SourceUnavailable`/`WaitForSource` startup outcome; registry durability failures remain `Internal`.
- Kept endpoint watching, automatic retry/recovery, owner replacement, migration, microphone capture, PipeWire, transport, UI, and service behavior deferred.

## Files Changed

- `crates/resonance-agent/src/discovery.rs`: stores and resolves exact live capture bindings, revalidates explicit bindings, and adds no-substitution/failure tests.
- `crates/resonance-agent/src/windows.rs`: adds playback intent, exact explicit startup selection, intent-specific revalidation/notifications, and failure classification.
- `crates/resonance-agent/src/supervisor.rs`: constructs production owners for Default Playback or Explicit Source intent.
- `crates/resonance-agent/src/main.rs`: adds the narrow opaque `--source-id` diagnostic option and parser tests.
- `README.md`: records implemented explicit capture semantics, remaining gaps, and diagnostic usage.
- `docs/api.md`: reconciles the capture entry points, failure behavior, descriptor identity, and CLI seam.
- `docs/architecture.md`: documents the two private intent paths and intent-specific notification behavior.
- `docs/decisions/0013-source-selection-model.md`: reconciles Windows Explicit Source implementation status.
- `docs/decisions/0014-source-discovery-and-identity-model.md`: reconciles exact explicit binding implementation status.
- `docs/decisions/0015-consumer-discovery-and-identity-registry.md`: records Milestone 6T implementation coverage.
- `docs/roadmap.md`: records Milestone 6T completion and removes explicit capture from later work.
- `docs/handoffs/resonance-signal-explicit-source-capture-handoff-2026-08-24.md`: this continuation checkpoint.
- Generated artifacts: ordinary Cargo outputs under ignored `target/`; not staged.
- Unrelated existing changes: None.

## Validation Completed

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets`: passed without warnings.
- Focused explicit-selection tests: four explicit capture tests passed; unavailable/no-substitution tests passed; descriptor, stream identity, endpoint mismatch, and Default Playback role-movement regression tests passed.
- `cargo test --workspace`: passed, 139 tests passed and 1 hardware-dependent discovery test ignored by its declared default; no failures.
- `cargo doc --workspace --no-deps`: passed.
- `git diff --check`: passed; Git emitted only the checkout's expected LF-to-CRLF notices.
- `cargo test -p resonance-agent windows_discovery::tests::real_windows_playback_discovery_refresh_and_reopen -- --ignored --nocapture`: passed and found one available playback descriptor, `Realtek Digital Output (Realtek USB Audio)`, with a stable opaque `SourceId` and Default Playback role.
- Default Playback runtime: `cargo run -p resonance-agent -- --duration-seconds 2` started and stopped normally on the Realtek endpoint, reporting opaque source `id-ns-17540-1787625582378565400-1`, stream `wasapi-loopback-29912-1`, 48 kHz, and two channels.
- Explicit runtime, run 1: the same opaque source captured the Realtek endpoint and stopped normally with stream `wasapi-loopback-38280-1`.
- Explicit runtime, run 2: the same opaque source captured the Realtek endpoint and stopped normally with distinct stream `wasapi-loopback-40236-1`.
- Unknown-ID runtime: `unknown-explicit-source` failed as `SourceUnavailable` with `WaitForSource` and emitted no `Started` event.
- Native identity exposure review: runtime diagnostics showed friendly endpoint name plus opaque source/stream IDs and did not print a native WASAPI endpoint ID.
- Not run: multi-endpoint role-versus-explicit live comparison, because only one active playback endpoint was available and device state was not modified for coverage. Retired/unavailable/stale/mismatch paths were validated through deterministic hardware-independent tests rather than device mutation.

## Production State Versus Repository State

- Implemented: exact identity-pinned Windows Explicit Source capture, independent Default Playback capture, fail-closed startup, truthful descriptor identity, and diagnostic selection.
- Committed: pending at handoff creation.
- Pushed: pending at handoff creation.
- Deployed or activated: no service or production deployment exists in this milestone; only explicit local diagnostic executions occurred.
- Runtime-validated: current Arrakis Realtek endpoint for one Default Playback run, two Explicit Source runs, and unknown-ID rejection.
- Documented or planned only: Windows vertical-slice acceptance remains the next milestone; endpoint watching, recovery execution, microphone capture, PipeWire, transport, UI, and service behavior remain deferred.
- Unverified: live multi-endpoint explicit selection while another endpoint owns Default Playback; runtime device disappearance/retirement/stale-start races for explicit capture.

## Unresolved Issues and Unverified Assumptions

- Only one active playback endpoint was available, so real-device proof could not compare explicit source A against a distinct current default B without changing device state.
- The real unavailable, retired, stale-start, and opened-endpoint-mismatch paths were not induced on hardware; deterministic tests cover their private resolution and pre-`Started` rejection behavior.
- The diagnostic `SourceId` is valid only within the current installation/host registry namespace and may become stale after reset, corruption fallback, incompatible migration, host change, or retirement.
- Publication state in this file is the pre-commit checkpoint and must be refreshed from Git/remote evidence.

## Safety, Rollback, and Access Considerations

- No device state, operating-system default role, dependency, recovery behavior, service, transport, or external consumer was modified.
- Real validation created normal read/reconcile activity in the existing provider identity registry and opened/stopped the selected loopback endpoint; it did not expose the private endpoint ID.
- The initial sandboxed runtime failures were access-boundary evidence (`Access is denied` for the LocalAppData registry), not capture failures. Approved host-context runs succeeded.
- If rollback is required after publication, prefer a normal revert of the scoped milestone commit after review. Do not reset shared history, force-push, delete registry state, or rewrite the mapping namespace.
- Remote publication uses the configured HTTPS GitHub `origin`; no credentials or secret values are stored in this checkpoint.

## Do Not Redo or Reopen

- Do not merge Default Playback and Explicit Source into one resolution rule. Default is role-resolved per attempt; explicit is exact-ID only.
- Do not use friendly names, default status, formats, or native endpoint IDs as portable source identity.
- Do not substitute another source for unknown, unavailable, retired, stale, ambiguous, or mismatched explicit identity.
- Do not treat an active explicit stream as following later default-role movement.
- Do not reuse `StreamId` or continue an old timeline across attempts, including repeated capture of the same `SourceId`.
- Do not repeat the sandboxed real-runtime command expecting LocalAppData access; use the approved host context when real registry/device validation is explicitly in scope.
- Do not add endpoint watching, retry/recovery execution, migration, microphone/PipeWire capture, transport, UI, or service work under Milestone 6T.

## Next Recommended Action

Run a Windows vertical-slice acceptance milestone covering discovery, Default Playback capture, Explicit `SourceId` capture, identity persistence, failure boundaries, and consumer-contract consistency before beginning Linux/PipeWire implementation.
