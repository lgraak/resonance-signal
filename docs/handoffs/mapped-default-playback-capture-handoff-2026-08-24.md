# Mapped Default Playback Capture Handoff

Date: 2026-08-24 (America/Los_Angeles)
Status: completed locally; commit and publication pending final validation
Model: Codex (GPT-5)
Effort: unknown (not exposed to this session)
Repository: `resonance-signal` at `D:\Aeons\Git\resonance-signal`
Branch: `main`
HEAD: `ebb9b24ef2f7478ea2d6249823b5dbc5420521ef` (verified starting revision; completion commit pending at handoff creation)
Authoritative remote: `origin` -> `https://github.com/lgraak/resonance-signal.git`

> This handoff is a continuation checkpoint, not authoritative truth. Current
> repository, remote, runtime, and test evidence wins if it conflicts with this
> document.

## Objective

Complete Milestone 6S by replacing the Windows Default Playback capture path's logical `default-playback` source placeholder with the opaque registry-backed `SourceId` of the exact endpoint resolved for that capture attempt. The bounded objective was achieved locally. Default Playback remains logical role intent; explicit-source capture, endpoint watching, active-stream migration, recovery execution, transport, UI, microphone capture, and Linux/PipeWire remain excluded.

## Authoritative Sources

- `docs/decisions/0013-source-selection-model.md`: governs the separation of Default Playback intent, resolved `SourceId`, and uninterrupted `StreamId`.
- `docs/decisions/0014-source-discovery-and-identity-model.md`: governs attempt-time role resolution, private native evidence, stale rejection, and no in-place migration.
- `docs/decisions/0015-consumer-discovery-and-identity-registry.md`: governs revision-bound snapshots, durable provider identity, privacy, and fail-closed behavior.
- `crates/resonance-agent/src/discovery.rs`, `identity.rs`, and `windows_discovery.rs`: authoritative private discovery, registry, and Windows endpoint mapping implementation.
- `crates/resonance-agent/src/windows.rs`: authoritative Windows capture startup and stream-event implementation.
- `docs/api.md`, `docs/architecture.md`, and `docs/roadmap.md`: current durable contract, ownership, and milestone boundaries.
- `docs/standards/ai-project-prompt-standard-v1.md` and `docs/standards/ai-project-handoff-standard-v1.md`: preflight, validation, publication, and handoff rules.
- Fetched `origin/main` and `FETCH_HEAD` at `ebb9b24ef2f7478ea2d6249823b5dbc5420521ef` before modification. Remote state is time-sensitive and requires readback after publication.
- Two fresh Arrakis runs of the normal diagnostic capture path on 2026-08-24: direct runtime evidence, host- and time-sensitive.

## Execution Context

- Host: `ARRAKIS`; Windows; PowerShell.
- Repository root: `D:\Aeons\Git\resonance-signal`.
- The production diagnostic created or reused private identity state under `%LOCALAPPDATA%\Resonance Signal\provider-state` as part of the requested capture-time registry integration.
- No dependency was added. The existing `wasapi` 0.24.0 API can open an endpoint by exact private endpoint ID.
- The supplied milestone attachment physically ended at line 258 after an incomplete `-` item in the real-Windows checklist. All complete requirements were implemented; no missing tail requirement was invented.

## Current Repository State

- Branch and verified starting HEAD: `main` at `ebb9b24ef2f7478ea2d6249823b5dbc5420521ef`.
- Working tree at handoff creation: ten intended milestone files modified or added; no unrelated changes were present.
- Upstream: `origin/main`.
- Synchronization before modification: `HEAD`, `origin/main`, and approved fetched `FETCH_HEAD` all matched `ebb9b24`; ahead/behind was `0/0`.
- Completion commit and authoritative remote readback: pending final validation at handoff creation.
- Preserved unrelated changes: none.

## Current Known-Good State

- Each Windows Default Playback attempt refreshes the active render-endpoint observation and resolves the `eConsole` role through the durable identity registry.
- One private binding contains the discovery identity/revision, mapped opaque `SourceId`, and exact native endpoint ID from the same observation.
- Capture opens that exact endpoint by ID, verifies the opened endpoint ID, registers endpoint/session notifications, refreshes the mapping, and rejects any stale or changed binding before publishing `Started`.
- `CaptureStarted` carries the resolved opaque ID into `StreamDescriptor`; the logical `default-playback` value is absent from emitted stream source metadata.
- Active capture remains on the exact opened endpoint and existing default-device notifications retain the explicit stream-ending behavior. No in-place migration or replacement attempt was added.
- Two fresh two-second Arrakis runs both captured `Realtek Digital Output (Realtek USB Audio)` at 48 kHz stereo, retained the same opaque registry-backed source identity across process restart, emitted distinct stream identities, and ended normally through `ProviderShutdown` / `StopRequested`.

## Completed Work

- Added an attempt-scoped private playback-capture binding to the existing discovery boundary.
- Added refresh, exact Default Playback resolution, and complete binding revalidation operations without adding explicit-source capture.
- Changed WASAPI startup from independently opening the current default endpoint to opening the exact endpoint bound by discovery.
- Added endpoint-ID equality and post-notification revision/binding checks before `Started` publication.
- Mapped registry/discovery failures through existing `Internal` or `SourceUnavailable` categories and existing retry hints without adding public Windows-specific errors.
- Passed the resolved `SourceId` through capture startup into `StreamDescriptor` and removed the legacy logical placeholder from that path.
- Added hardware-independent tests for role movement, stale rejection, unavailable default, duplicate names, no synthetic identity, endpoint mismatch, truthful descriptor emission, and stream/source identity separation.
- Reconciled README, API, architecture, ADR implementation state, and roadmap documentation with Milestone 6S.

## Decisions Made

- Open the exact endpoint from the mapped observation instead of independently asking WASAPI for the default endpoint a second time. This makes capture endpoint and descriptor provenance identical by construction.
- Revalidate the complete revision-bound binding after endpoint/session notification registration. Any discovery-visible change during startup fails closed before stream publication, including unrelated revision movement.
- Use the current user's Windows local application-data directory at `Resonance Signal\provider-state` for the diagnostic runtime's private durable registry. Native IDs and registry contents remain private and are not printed or exposed through `resonance-api`.
- Keep all explicit-source resolution methods disconnected from capture. Milestone 6S implements only Default Playback intent.
- Preserve existing lifecycle behavior after `Started`: a later default change terminates or reconfigures the current stream through existing notifications and never migrates it in place.

## Files Changed

- `README.md`: records Milestone 6S completion, mapped capture behavior, deferrals, and private runtime state location.
- `crates/resonance-agent/src/discovery.rs`: adds the private attempt binding, resolution/revalidation operations, and deterministic binding tests.
- `crates/resonance-agent/src/windows.rs`: opens the exact mapped endpoint, validates/revalidates it, emits the mapped `SourceId`, and adds capture-publication tests.
- `docs/api.md`: documents attempt-time mapped capture and fail-closed publication behavior.
- `docs/architecture.md`: records the exact-endpoint/revision binding and current constraints.
- `docs/decisions/0013-source-selection-model.md`: reconciles accepted source semantics with implemented Windows runtime behavior.
- `docs/decisions/0014-source-discovery-and-identity-model.md`: reconciles mapped capture implementation and remaining explicit-source work.
- `docs/decisions/0015-consumer-discovery-and-identity-registry.md`: records capture-time consumption of the private mapping.
- `docs/roadmap.md`: records Milestone 6S and advances explicit-source capture to the later-work list.
- `docs/handoffs/mapped-default-playback-capture-handoff-2026-08-24.md`: this continuation checkpoint.
- Generated build and rustdoc output remained under ignored `target/` storage and is excluded.

## Validation Completed

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets`: passed.
- Focused `cargo test -p resonance-agent discovery::tests`: passed, 14 tests plus one intentionally ignored real-device test.
- Focused `cargo test -p resonance-agent windows::tests`: passed, 17 tests.
- `cargo test --workspace`: passed, 135 tests total; one intentionally ignored real-device discovery test.
- `cargo doc --workspace --no-deps`: passed.
- `git diff --check`: passed with only the repository's expected LF-to-CRLF working-copy warnings.
- `cargo clippy --workspace --all-targets -- -D warnings`: not passed because Rust 1.98 reports a pre-existing `clippy::manual_is_multiple_of` finding at `crates/resonance-core/src/signal.rs:214`. That unrelated file was not modified.
- Normal live run 1: passed for two seconds; 196 waveform frames / 94,080 source frames; opaque mapped source present; placeholder absent; normal joined shutdown.
- Normal live run 2: passed for two seconds; 198 waveform frames / 95,040 source frames; same opaque source identity, new stream identity, placeholder absent; normal joined shutdown.
- `gitleaks dir . --no-banner --redact --no-color`: passed; approximately 270 MB scanned and no leaks found.
- Handoff structure and final unstaged scope review: passed. Staged scope review, commit, push, and remote readback were pending at handoff creation.

## Production State Versus Repository State

- Implemented: capture-time Default Playback refresh, durable identity mapping, exact-endpoint open, stale/mismatch rejection, truthful stream descriptor, tests, and documentation.
- Committed: pending at handoff creation.
- Pushed: pending at handoff creation.
- Deployed or activated: no service or deployment exists. The diagnostic executable was run directly from the workspace.
- Runtime-validated: two normal Arrakis capture attempts passed against the current Realtek playback endpoint.
- Local operational state: the private current-user identity registry was created or updated under Windows local application data; no device or system configuration was changed.
- Documented or planned only: explicit-source capture and all other excluded later milestones.
- Unverified: default-role movement during the narrow startup race on physical hardware, another Windows host, service-account state placement, and every excluded platform or consumer integration.

## Unresolved Issues and Unverified Assumptions

- The attachment's final real-Windows checklist item was truncated. Its unknown tail remains unverified.
- Clippy with warnings denied is blocked by one pre-existing Rust 1.98 lint in unchanged `resonance-core` code; all milestone code compiled and the complete test suite passed.
- The live runs did not mutate the Windows default endpoint, so real-hardware default-role movement during startup was not exercised. Deterministic tests cover stale movement and exact mismatch.
- The private registry location is appropriate for the current per-user diagnostic runtime. A future service installation or multi-user identity domain may require a separately approved state-location decision.
- Commit and authoritative publication state remain to be verified after final validation.

## Safety, Rollback, and Access Considerations

- No credential, secret, backend endpoint ID, registry namespace, tombstone, or registry file content is included in this handoff.
- Real-device validation read and captured the active playback endpoint and wrote only private provider identity state under the current user's local application data. It did not change device defaults, drivers, services, or system configuration.
- Source rollback should use a normal revert of the scoped completion commit; do not reset or rewrite shared history.
- Removing the private registry would intentionally reset the provider identity namespace and invalidate prior opaque IDs. Do not delete it as routine cleanup.
- Publishing to GitHub is the only pending external repository side effect and requires final remote SHA readback.

## Do Not Redo or Reopen

- Do not reintroduce `default-playback` as a `SourceId`; it is logical intent only.
- Do not reopen the separation among source intent, `SourceId`, and `StreamId` without contradictory evidence.
- Do not open the Windows default independently after identity resolution; capture must use the exact bound endpoint.
- Do not use friendly name, format, channel count, or default-role metadata as identity proof.
- Do not expose native endpoint IDs, discovery internals, registry state, or the private state path through consumer contracts.
- Do not implement explicit-source capture, endpoint watching, automatic migration, replacement creation, recovery execution, transport, UI, microphone capture, or PipeWire as part of Milestone 6S.
- Do not fix the unrelated Rust 1.98 Clippy finding merely to claim this milestone's lint gate passed.

## Next Recommended Action

Commit the ten intended Milestone 6S files together, push that scoped commit to `origin/main`, and verify the authoritative remote SHA before beginning explicit-source capture work.
