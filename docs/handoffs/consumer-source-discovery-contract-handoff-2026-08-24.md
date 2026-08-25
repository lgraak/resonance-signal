# Consumer Source Discovery Contract Handoff

Date: 2026-08-24 (America/Los_Angeles)
Status: completed locally; authoritative publication blocked pending exact confirmation
Model: Codex (GPT-5)
Effort: unknown (not exposed to this session)
Repository: `resonance-signal` at `D:\Aeons\Git\resonance-signal`
Branch: `main`
HEAD: `fc200c024a002fd0dd77006e31eb6b5d0f12b273` (handoff-and-standards checkpoint; base of this final publication-state correction)
Authoritative remote: `origin` -> `https://github.com/lgraak/resonance-signal.git`

> This handoff is a continuation checkpoint, not authoritative truth. Current
> repository, remote, runtime, and test evidence wins if it conflicts with this
> document.

## Objective

Complete Milestone 6R by defining and implementing the portable consumer-facing source discovery contract over the proven private Windows playback discovery snapshot. The bounded objective was achieved locally: portable owned discovery types exist in `resonance-api`, `resonance-agent` converts its private snapshot to those types without exposing native identity evidence, stale revision use remains rejectable, and focused plus workspace validation passed.

Explicit-source capture, capture-time Default Playback mapping, endpoint watching, transport, serialization, UI, InfoPanel, microphone discovery, Linux/PipeWire, and recovery execution remained out of scope and were not implemented. The supplied task attachment ended in the middle of its final acceptance list after the text `- Windows`; no missing requirement was invented from the truncated tail.

## Authoritative Sources

- `crates/resonance-api/src/contract.rs`: portable consumer contract types and invariants.
- `crates/resonance-agent/src/discovery.rs`: private playback snapshot, portable conversion, revision binding, and exact-ID resolution boundary.
- `crates/resonance-agent/src/identity.rs`: private live/absent/retired identity registry state; native evidence and tombstones remain private.
- `crates/resonance-agent/src/windows_discovery.rs`: Windows WASAPI endpoint adapter and real-device validation seam.
- `docs/decisions/0013-source-selection-model.md`: accepted Default Playback versus Explicit Source intent distinction.
- `docs/decisions/0014-source-discovery-and-identity-model.md`: accepted provider-managed identity and discovery lifecycle.
- `docs/decisions/0015-consumer-discovery-and-identity-registry.md`: accepted portable descriptor, snapshot, registry privacy, and stale-resolution contract.
- `docs/api.md`, `docs/architecture.md`, and `docs/roadmap.md`: current documented contract, crate ownership, deferrals, and Windows-first sequence.
- `docs/standards/ai-project-handoff-standard-v1.md`: repository handoff format used for this checkpoint. Its SHA-256 matches the project-attached source at `D:\Aeons\Git\standards\ai-project-handoff-standard-v1.md`.
- `docs/standards/ai-project-prompt-standard-v1.md`: repository prompt-authoring and default handoff/publication workflow supplied with this checkpoint.
- Configured GitHub `origin`: authoritative publication destination. A direct read-only query on 2026-08-24 returned `a23d49dad550268f164c9f25c1b466c0697f4618` for `refs/heads/main`; remote state is time-sensitive and must be rechecked before publication.
- Fresh Arrakis real-device test output from this milestone: one portable active playback descriptor for `Realtek Digital Output (Realtek USB Audio)`, marked available and Default Playback, with Waveform as the only advertised product. Runtime/device evidence is host- and time-sensitive.

## Execution Context

- Host: `ARRAKIS`.
- Operating system: Microsoft Windows NT `10.0.26100.0`.
- Shell: PowerShell `7.6.4`.
- Repository root: `D:\Aeons\Git\resonance-signal`.
- Rust toolchain: `rustc 1.98.0 (88d9e12ae 2026-08-18)` and `cargo 1.98.0 (797e8a9bc 2026-08-05)`.
- No dependency was added and no isolated alternate toolchain was required.
- Real Windows discovery was read-only. The test used a temporary identity-registry directory and did not modify device state, drivers, services, or system configuration.
- A normal sandboxed remote query failed with Windows Schannel `SEC_E_NO_CREDENTIALS`; an approved read-only `git ls-remote` retry succeeded. The attempted push did not execute because external publication required explicit payload-and-destination confirmation.

## Current Repository State

- Branch and HEAD before this final publication-state correction: `main` at `fc200c024a002fd0dd77006e31eb6b5d0f12b273` (`Add project handoff standards and Milestone 6R checkpoint`).
- Working tree before handoff creation: clean.
- Working tree after handoff creation: the handoff and the two user-supplied standards under `docs/standards/` are the only three added files and form one checkpoint commit scope.
- Upstream: `main` tracks `origin/main`.
- Direct remote readback: `origin/main` was `a23d49dad550268f164c9f25c1b466c0697f4618` on 2026-08-24.
- Divergence before the handoff commit: local `main` was one implementation commit ahead of the verified remote.
- Commit created for implementation: `4fa90bdcd9f0643afeb0a5a7d928a48fb4b20381`.
- Push result: not published. The push request was rejected before execution because the external GitHub payload and destination required an explicit confirmation. Remote readback confirmed the implementation commit was absent.
- Handoff and standards commit: `fc200c024a002fd0dd77006e31eb6b5d0f12b273`; created as a separate local checkpoint because the implementation commit already existed. Shared history was not rewritten.
- Publication authorization: the user authorized committing and pushing the copied standards with this handoff. The external-publication safety gate requires additional explicit confirmation that the push necessarily includes parent implementation commit `4fa90bdcd9f0643afeb0a5a7d928a48fb4b20381` as well as the checkpoint commit.
- Preserved unrelated changes: none.

## Current Known-Good State

- Portable discovery types compile and are public from `resonance-api::contract` at implementation commit `4fa90bd`.
- The Windows mapping publishes only portable source ID, optional display name, `Playback` kind, three-state availability, Default Playback membership, and supported signal products.
- `DiscoveryRevision` is cloneable and equality-comparable but has no ordering, value accessor, or hash derivation; debug output redacts its private token.
- Known non-retired playback sources remain representable as `Unavailable` when absent from the active endpoint observation, while retired identities and native evidence remain private.
- Default Playback membership moves independently of `SourceId`; duplicate names remain distinct; metadata rename retains identity; old revisions are rejected; unavailable explicit IDs never substitute another source.
- Fresh workspace validation passed after the final code edit. Fresh real Windows validation also passed against the Arrakis endpoint set without device mutation.
- No consumer service or transport exists, so the known-good runtime evidence is the Rust contract, mapping, private resolution seam, automated tests, and bounded real-device discovery test rather than an externally callable discovery API.

## Completed Work

- Added `DiscoveryRevision`, `SourceAvailability`, portable `SourceDescriptor`, owned `DiscoverySnapshot`, and `DiscoveryContractError` to `resonance-api`.
- Enforced non-empty optional display names, duplicate-free supported products, duplicate-free default roles, and unique source IDs within a snapshot.
- Kept discovery revisions opaque and equality-only; their private value is redacted from `Debug` output.
- Added the private-to-portable mapping in `resonance-agent`, with owned returned values and no backend-native or registry fields.
- Added a private agent boundary that rejects stale portable revisions before exact-ID resolution.
- Extended private discovery output to retain known non-retired absent playback identities as unavailable without exposing tombstones.
- Preserved deterministic provider ordering without giving that ordering identity meaning.
- Updated the real Windows test to print only allowed portable fields.
- Added and extended tests for duplicate names, rename continuity, default-role movement, availability, conservative capability reporting, native privacy, snapshot ownership, stale behavior, and no substitution.
- Reconciled `README.md`, API documentation, architecture, roadmap, and ADR 0015 with the implemented Milestone 6R state and remaining capture integration gaps.
- Created this standards-compliant continuation checkpoint after the missing repository handoff was identified.

## Decisions Made

- The portable contract remains in `resonance-api`; provider/platform mapping and all registry operations remain in `resonance-agent`. The dependency direction remains `resonance-agent -> resonance-api -> resonance-core`.
- `DiscoveryRevision` is a freshness token, not source identity. Equality is supported; parsing, ordering, and hashing are deliberately absent from the consumer contract.
- The public descriptor uses the existing portable `SourceKind`, `DefaultSource`, and `SignalProduct` concepts. The current Windows mapping emits only `SourceKind::Playback`, `DefaultSource::Playback` when applicable, and `SignalProduct::Waveform`.
- A known non-retired playback source may remain in a complete snapshot as `Unavailable`. A source never observed successfully is not created merely from disabled, absent, or unplugged backend state.
- Default Playback remains logical intent. A descriptor's current default-role membership neither creates a synthetic source ID nor changes Explicit Source semantics.
- Public snapshots own all values and never borrow mutable discovery or registry state.
- No transport, wire encoding, paging, filtering, change notification, capture ownership, explicit-source capture, or recovery behavior was selected or implemented.
- The already-created implementation commit was not amended. This handoff is a separate checkpoint to avoid rewriting history without explicit authorization.

## Files Changed

Implementation commit `4fa90bdcd9f0643afeb0a5a7d928a48fb4b20381`:

- `README.md`: records Milestone 6R completion, portable discovery concepts, privacy boundary, and deferrals.
- `crates/resonance-api/src/contract.rs`: adds the portable discovery contract and API-level validation tests.
- `crates/resonance-agent/src/discovery.rs`: adds portable conversion, unavailable-source representation, stale portable revision rejection, and mapping tests.
- `crates/resonance-agent/src/identity.rs`: exposes a private normalized view of known non-retired registry sources to the discovery mapper.
- `crates/resonance-agent/src/lib.rs`: updates the private-discovery boundary comment after approval of the public value contract.
- `crates/resonance-agent/src/windows_discovery.rs`: converts real Windows validation output to allowed portable fields.
- `docs/api.md`: documents the concrete Rust discovery types, snapshot semantics, and remaining integration work.
- `docs/architecture.md`: documents crate ownership, portable mapping, unavailable-source retention, and native privacy.
- `docs/decisions/0015-consumer-discovery-and-identity-registry.md`: reconciles implementation impact without changing the accepted decision.
- `docs/roadmap.md`: records Milestone 6R and advances the next Windows steps.

This checkpoint:

- `docs/handoffs/consumer-source-discovery-contract-handoff-2026-08-24.md`: adds the required continuation handoff.
- `docs/standards/ai-project-handoff-standard-v1.md`: adds the project-neutral 14-section continuation-checkpoint standard.
- `docs/standards/ai-project-prompt-standard-v1.md`: adds the project-neutral prompt-authoring and default publication standard.

The follow-up documentation correction updates this handoff's publication status after the external push was rejected before execution; it does not alter implementation or standards content.

Generated build and rustdoc output remained under ignored `target/` storage and was not committed. No unrelated file was modified, staged, or removed.

## Validation Completed

- `cargo fmt --all --check`: passed after the final code edit.
- `cargo check --workspace --all-targets`: passed.
- `cargo test -p resonance-api`: passed, 6 tests.
- `cargo test -p resonance-agent discovery::tests`: passed, 12 tests; the separately gated real-device test was filtered as ignored in this focused run.
- `cargo test --workspace`: passed, 131 tests across workspace crates and targets; one intentionally ignored real-device test.
- `cargo doc --workspace --no-deps`: passed and generated workspace documentation under ignored `target/doc` output.
- `cargo test -p resonance-agent real_windows_playback_discovery_refresh_and_reopen -- --ignored --nocapture`: passed, 1 real Windows test. It observed one descriptor: `Realtek Digital Output (Realtek USB Audio)`, `Available`, Default Playback `true`, products `[Waveform]`. The printed opaque ID belonged to the temporary test registry and is not a durable installed-provider selection.
- Focused mapping tests explicitly checked duplicate names, rename, default-role movement, unavailable retention, Waveform-only capability, owned snapshot immutability, native endpoint/continuity string absence, stale revision rejection, and exact-ID no substitution.
- `git diff --check`: passed before the implementation commit; Git reported only the repository's LF-to-CRLF working-copy warnings.
- Final implementation scope review: exactly 10 intended files were staged and committed as `4fa90bd`; no unstaged changes remained.
- `git ls-remote --heads origin refs/heads/main`: direct approved readback succeeded and returned `a23d49dad550268f164c9f25c1b466c0697f4618`.
- `gitleaks dir docs --no-banner --redact --no-color`: passed; approximately 389 KB scanned and no leaks found.
- SHA-256 comparison: both repository standards exactly matched their project-attached sources under `D:\Aeons\Git\standards`.
- Publication attempt after checkpoint commit: rejected before execution because pushing the checkpoint would also publish its parent implementation commit and that exact combined payload-to-GitHub authorization was not explicit. No workaround was attempted.
- Handoff validation: required heading order, metadata, repository-relative references, UTF-8 without BOM, final newline, whitespace, secret scan, and exact final scope passed after the publication-state correction.
- Not run: Clippy, because it was optional in the packet and unrelated lint cleanup was explicitly excluded.
- Not run: consumer transport, explicit-source capture, capture-time mapped Default Playback, endpoint watching, microphone discovery, PipeWire, UI, InfoPanel, or recovery execution validation because those behaviors remain unimplemented and out of scope.

## Production State Versus Repository State

- Implemented: portable Rust discovery value types, Windows private-to-portable mapping, unavailable known-source representation, opaque revision comparison, stale exact-ID resolution rejection, tests, and documentation.
- Committed locally: implementation commit `4fa90bdcd9f0643afeb0a5a7d928a48fb4b20381` and handoff-and-standards checkpoint `fc200c024a002fd0dd77006e31eb6b5d0f12b273` on `main`; this corrected publication state is recorded in a follow-up documentation commit rather than by rewriting either commit.
- Pushed: no. Direct remote readback showed `origin/main` still at `a23d49dad550268f164c9f25c1b466c0697f4618`.
- Deployed or activated: not applicable. No service, transport, installer, endpoint watcher, capture integration, or consumer application was deployed or activated.
- Runtime-validated: the private Windows discovery adapter converted one real Arrakis playback endpoint to the portable contract in a temporary test registry. This does not establish a deployed consumer discovery service.
- Documented: Milestone 6R contract, privacy boundary, snapshot semantics, Default Playback distinction, capability ceiling, and remaining Windows capture integration gaps.
- Planned only: mapped Default Playback capture, explicit `SourceId` playback capture, Windows vertical-slice acceptance, later portable contract review, and Linux/PipeWire implementation.
- Unverified: behavior on another Windows host, a long-lived installed provider registry exposed through a future transport, and all later roadmap items.

## Unresolved Issues and Unverified Assumptions

- The implementation, handoff, and standards are not yet on the authoritative remote. The user authorized the standards/handoff commit and requested a push, but the safety gate requires explicit confirmation that the same push also publishes implementation commit `4fa90bdcd9f0643afeb0a5a7d928a48fb4b20381` to the configured GitHub destination.
- The task attachment was physically truncated in its final acceptance list after `- Windows`. All complete preceding requirements were implemented and validated; unknown missing tail text remains unverified.
- The real Windows test uses temporary registry storage. It proves portable conversion and same-run refresh/reopen behavior, not the identity of a separately installed long-lived provider instance.
- `origin/main` was directly verified as `a23d49d` on 2026-08-24, but remote state is time-sensitive and must be re-read before pushing.
- No consumer transport exists. Snapshot serialization, wire compatibility, paging, filtering, authorization, and change subscriptions remain deliberately unspecified.
- No production or cross-platform acceptance was claimed.

## Safety, Rollback, and Access Considerations

- No credentials, tokens, private endpoint IDs, registry continuity evidence, tombstones, or storage contents are recorded in this handoff.
- The real-device test enumerated playback state only and used temporary registry files removed at test completion. It did not change device availability or system configuration.
- No dependency, service, process configuration, deployment, external API, or consumer system was changed.
- Local rollback before publication is a Git review decision. Prefer a normal revert of the scoped local commit or a follow-up correction; do not use destructive reset or history rewriting without explicit authorization.
- Authoritative publication is the only pending external side effect. Before pushing, obtain explicit approval to send both the implementation commit and checkpoint history to `https://github.com/lgraak/resonance-signal.git` branch `main`, followed by remote SHA readback.
- GitHub credentials remain managed by the existing Git credential mechanism; no credential value was viewed or recorded.

## Do Not Redo or Reopen

- Do not reopen the separation among source intent, `SourceId`, and `StreamId` without concrete contradictory evidence.
- Do not expose WASAPI endpoint IDs, registry namespaces, continuity tokens, tombstones, schemas, or storage details through `resonance-api`, diagnostics, or a future transport.
- Do not treat display name, source kind, availability, default role, or supported products as identity proof.
- Do not represent Default Playback as a synthetic `SourceId`; current default membership is snapshot metadata only.
- Do not advertise Levels, Spectrum, FFT, microphone, or other capabilities from the Windows playback mapping until implementation and acceptance evidence exists.
- Do not silently truncate, relabel, or invent a downmix for multichannel capture; mono/stereo remains the product ceiling.
- Do not add explicit-source capture, capture-time mapping, endpoint watching, transport, UI, Linux/PipeWire, or recovery execution to Milestone 6R.
- Do not claim the rejected push published anything. Re-read `origin/main` before any later publication claim.
- Do not repeat the read-only Arrakis device check merely to prove the already-observed endpoint unless relevant device or implementation state changes.

## Next Recommended Action

Obtain explicit confirmation to publish implementation commit `4fa90bdcd9f0643afeb0a5a7d928a48fb4b20381` together with the handoff-and-standards checkpoint history to `https://github.com/lgraak/resonance-signal.git` branch `main`, then push and verify the authoritative remote SHA.
