# Roadmap

## Milestone 1: Foundation

- Buildable Rust workspace with `resonance-core`, `resonance-api`, and `resonance-agent`.
- Initial crate and module boundaries.
- Architecture, API, and roadmap documentation skeletons.
- Stable-Rust CI for Windows, Linux, formatting, checking, and builds.

## Milestone 2: Core contracts

- Defined the provider-independent waveform, level, and spectrum data model.
- Defined the first multi-source, multi-product client contract.
- Established pre-1.0 compatibility and versioning expectations.
- Added focused validation and contract tests.

## Milestone 3A: Practical signal processing primitives

- Added borrowed, frame-aligned waveform windows without copying sample data.
- Added scalar and per-channel RMS and maximum absolute peak calculations.
- Added explicit peak-normalization helpers that preserve out-of-range headroom.
- Added synthetic tests for silence, constant and known waveforms, multiple channels, subwindows, invalid input, and normalization.

## Milestone 3B: Bounded window scheduling and processing cadence

- Added configurable, non-overlapping analysis windows with 30 FPS and 60 FPS visualization cadences.
- Bounded retained partial samples and per-push work without introducing an output queue or async runtime.
- Added explicit frame-index, timestamp, format, and uninterrupted-stream boundary handling.
- Added synthetic tests for completion, accumulation, multiple outputs, oversized input, discontinuities, stream changes, and empty input.

## Milestone 4A: Stereo-first capture requirements and provider boundary

- Limited supported capture products to mono and two-channel stereo while preserving the wider provider-independent core layout model.
- Established front-left/front-right ordering for known stereo and conservative discrete layouts when one- or two-channel positions are unknown.
- Placed format enforcement at the future capture boundary in `resonance-agent`; unsupported wider sources fail before an active stream starts.
- Prohibited silent first-two-channel extraction and custom downmixing without a separate evidence-backed decision.
- Defined format, batch, timing, identity, lifecycle, diagnostic, and backend-evaluation requirements without selecting a dependency or implementing capture.

## Milestone 4B: Capture backend selection

- Evaluated direct WASAPI and PipeWire access, focused Rust bindings, CPAL, and GStreamer against the capture contract using current upstream documentation and source.
- Selected `wasapi-rs` 0.24.0 as the initial Windows direction and official `pipewire-rs` 0.10.1 bindings as the initial Linux direction.
- Rejected a third-party cross-platform capture layer for the first implementation because required native timestamp-validity, source-position, and provenance evidence would be lost.
- Defined one bounded Windows playback-loopback prototype as the next implementation milestone without adding dependencies or capture code.

## Milestone 5A: Windows WASAPI playback-loopback prototype

- Added the Windows-only `wasapi` 0.24.0 dependency to `resonance-agent`; `resonance-core` and `resonance-api` remain platform-independent.
- Opened the console-role default playback endpoint in event-driven shared loopback mode and requested mono or explicit front-left/front-right interleaved float output at the endpoint mix sample rate.
- Added a preallocated four-buffer pool and matching bounded, non-blocking handoff from the COM-owned WASAPI thread to ordinary processing.
- Converted native packets into validated `AudioFrame` and existing `StreamEvent` output with contiguous normalized frame indexes and sample-derived stream timestamps.
- Retained and validated WASAPI device positions, QPC timestamps, discontinuity/silence/timestamp flags, endpoint identity, endpoint/default-device notifications, and session-disconnect reasons.
- Made interruption, format change, device invalidation, timing discontinuity, and handoff exhaustion explicit stream boundaries; automatic reconnect remains deferred.
- Added hardware-independent conversion, frame-generation, unsupported-format, non-finite-value, and discontinuity tests plus runtime evidence reporting.

## Milestone 5B: Windows real-device validation

- Validated default-playback loopback against the WH-1000XM5 endpoint at 96 kHz, two-channel interleaved `f32`.
- Observed 960-frame packets representing approximately 10 ms of audio, approximately 10 ms QPC deltas, and sub-millisecond callback work.
- Confirmed that repeated capture runs create new stream identities and restart the stream-relative frame index and timestamp at zero.
- Classified the consistently observed first-packet discontinuity flag as startup history; later discontinuities remain stream-ending failures.

## Milestone 5C: Production Windows capture boundary

- Replaced duration-owned prototype orchestration with an explicit production stop token; retained duration only in the bounded diagnostic runner.
- Kept a four-slot, maximum-packet-sized preallocated pool and matching non-blocking handoff as internal implementation details rather than public tuning knobs.
- Formalized normal stop, source replacement, format change, endpoint loss, interruption, discontinuity, bounded overload, and internal-failure mappings to existing provider events.
- Separated machine-actionable terminal categories and retry hints from human diagnostics and console evidence output.
- Added hardware-independent tests for CLI validation, lifecycle/error mapping, bounded handoff delivery, explicit overload, and stream restart identity/timeline behavior.
- Preserved explicit owner-controlled restart; automatic reconnect remains deferred.

## Milestone 6A: Capture owner lifecycle

- Added an inert, single-use `CaptureOwner` with explicit startup, idempotent stop requests, bounded shutdown waiting, retained completion state, and joined cleanup.
- Assigned one unambiguous ownership chain: the public owner owns the ordinary worker, the worker owns provider callbacks and the WASAPI-thread join obligation, and the WASAPI thread owns all COM and endpoint resources.
- Made pre-start stop skip initialization, made a second start invalid, and retained worker ownership after a shutdown timeout so callers can safely wait again.
- Ensured successful shutdown and the drop fallback join all nested work before callbacks or capture resources can outlive the owner.
- Preserved existing `Started`, `Data`, `Error`, and `Ended` ordering and the new-stream identity/frame-index/timeline rules for later explicit runs.
- Added hardware-independent lifecycle tests for startup, stop-before-start, repeated stop, timeout/retry, completion, resource release, drop cleanup, event order, and restart semantics.
- Updated the diagnostic executable to exercise the long-running owner and explicit shutdown on real Windows capture.
- Validated that path on Windows 11 against Headphones (WH-1000XM5): 96 kHz stereo capture emitted 1,000 data frames over ten seconds, then joined with `ProviderShutdown` / `StopRequested`; observed WASAPI QPC deltas remained approximately 10 ms.
- Kept automatic reconnect, retry/backoff, and default-device following deferred to a separate milestone.

## Milestone 6B: Capture supervisor and recovery boundary design

- Selected a future `CaptureSupervisor` in `resonance-agent` as the owner of recovery policy above the existing single-use `CaptureOwner`.
- Kept one-run resource ownership, callbacks, joined cleanup, and terminal event emission in `CaptureOwner`; it does not restart, follow devices, or apply retry policy.
- Assigned owner creation, restart decisions, retry-policy application, backoff state, endpoint-replacement acceptance, default-device-following policy, and recovery state to the supervisor.
- Required the prior owner to complete and release its resources before a replacement is created, with explicit stop suppressing any further recovery.
- Preserved every terminal lifecycle boundary: each replacement owner creates a new `StreamId`, resets frame index and stream time to zero, and emits a separate `Started` event.
- Kept machine-actionable errors, retry hints, end reasons, and completion separate from human messages, logs, and evidence output.
- Defined future handling responsibilities for default-endpoint removal or replacement, device disablement, format change, and temporary interruption without implementing reconnect or device watching.
- Deferred retry timing, backoff algorithm, default-device-following policy, service lifetime, and transport behavior.

## Milestone 6C: Capture supervisor state boundary

- Added a recovery-disabled `CaptureSupervisor` in `resonance-agent` with deterministic `Idle`, `Running`, `Stopping`, and `Completed` states.
- Added a narrow `CaptureOwnerFactory`/`SupervisedCaptureOwner` seam so lifecycle coordination is testable without audio hardware while the production factory creates the existing WASAPI `CaptureOwner`.
- Made supervisor start single-use, owner creation explicit, stop-before-start owner-free, and repeated stop requests idempotent.
- Added natural owner completion observation that joins the worker without implicitly requesting stop; successful completion proves callbacks ended and nested resources were released.
- Recorded typed terminal event and owner completion state, while treating replacement eligibility only as an unconsumed boundary requiring delivered `Ended`, completion, resource release, and still-enabled running intent.
- Proved with hardware-independent tests that only one owner is created, no replacement occurs, explicit stop suppresses eligibility, terminal delivery precedes completion handling, and normal, failure, startup-failure, and panic outcomes are deterministic.
- Kept reconnect, retry timers, backoff, endpoint watchers, default-device following, and all recovery policy unimplemented.

## Milestone 6D: Recovery decision policy design

- Defined recovery as supervisor-owned orchestration above the one-lifetime `CaptureOwner`, with a deterministic side-effect-free `RecoveryPolicy` evaluation boundary inside `resonance-agent`.
- Defined remain-stopped, wait, and permit-replacement as policy decision classes; authorization remains separate from owner creation, timers, watchers, and other mechanisms.
- Made explicit stop invalidate the running-intent generation and every pending recovery authorization before owner shutdown; late events remain evidence and cannot restore intent.
- Defined outcome-specific permission and evidence rules for shutdown, endpoint loss or reconfiguration, format change, interruption, resource exhaustion, unsupported format, internal failure, startup failure, and panic.
- Required weak or diagnostic-only classification to fail closed, with retry hints treated as constraints rather than commands.
- Assigned bounded attempts, delays, backoff, jitter, cooldowns, reset rules, and persistence to future retry policy without selecting values or algorithms.
- Preserved independent replacement streams with a new `StreamId`, frame index zero, stream-relative time zero, and fully visible terminal/start lifecycle boundaries.
- Kept reconnect, replacement owner creation, retry scheduling, endpoint watching, default-device following, and all runtime capture behavior unimplemented.

## Milestone 6E: Recovery policy representation

- Added agent-internal `RecoveryContext`, stable recovery causes, explicit lifecycle/source/retry evidence, and reasoned remain-stopped, wait, or permit-replacement decisions.
- Implemented ADR 0008 precedence as a deterministic pure evaluator: explicit stop and stale intent win, incomplete lifecycle waits, and missing or inconsistent structured evidence fails closed.
- Represented conditional device availability, default-source replacement, fresh format, interruption budget/cooldown, and resource-pressure inputs without selecting timer, retry-count, or backoff values.
- Kept unsupported format under unchanged conditions, broad internal failure, coarse startup failure, worker panic, normal shutdown, exhausted budget, and retry vetoes stopped.
- Added hardware-independent tests for every ADR 0008 outcome row, precedence rule, evidence gap, guarded permission, and the decision-versus-action boundary.
- Kept the policy disconnected from `CaptureSupervisor`; reconnect, retry execution, timers, owner replacement, endpoint watching, default-device following, and service behavior remain unimplemented.

## Milestone 6F: Retry state and recovery policy configuration design

- Defined each owner-creation call as one attempt and fixed the increment point immediately before factory invocation, so construction, startup, and runtime failures share one attempt identity and cannot escape accounting.
- Separated the all-attempt audit sequence from the automatic-recovery budget, and bound both to one explicit capture-intent generation.
- Assigned mutable last-failure evidence, bounded retry history, recovery episodes, cooldown, exhaustion, and state revision to `CaptureSupervisor`; `RecoveryPolicy` evaluates immutable state and configuration snapshots.
- Required a new explicit intent or separately evidenced stable run to reset recovery state; a successful construction, `start`, or `Started` event alone cannot erase a flapping failure chain.
- Defined cooldown as supervisor-owned monotonic eligibility state and kept deterministic delay/backoff calculation, jitter inputs, scheduling, and expiration evidence separate from policy side effects.
- Required one-shot authorization bound to intent generation, recovery episode, state revision, and prior attempt identity, with explicit stop, no-overlap ownership, budget limits, typed failure classification, and fail-closed evidence preventing recovery storms.
- Deferred every numeric limit and timing value, configuration source, persistence rule, timer, watcher, reconnect, and replacement owner.

## Milestone 6G: Retry state representation and transition model

- Added an agent-internal, supervisor-owned retry-state component with explicit `Idle`, `Attempting`, `Running`, `Failed`, `Waiting`, `Exhausted`, and `Stopped` phases.
- Added nonzero intent generations, monotonic attempt identities, distinct total-attempt and automatic-recovery counters, typed attempt lifecycle facts, and state revisions that reject stale evaluated snapshots.
- Added recovery episodes with sticky exhaustion and explicit typed reset evidence; a successful owner start does not reset the episode or retry accounting.
- Added bounded recent typed failure history, last-failure evidence, aggregate failure counters, and checked transitions that do not partially mutate state when a counter is exhausted.
- Represented cooldown as pending, satisfied, invalidated, or not required using opaque evidence identities, without selecting or reading time.
- Added transition-table tests for intent invalidation, once-only attempt accounting, episode reset and exhaustion, bounded history, cooldown gating, immutable policy evaluation, and terminal/cleanup ordering.
- Kept the state model disconnected from the runtime `CaptureSupervisor`; it has no capture factory, device knowledge, timer, thread, event sink, reconnect, or replacement behavior.

## Milestone 6H: Supervisor retry-state integration

- Made runtime `CaptureSupervisor` the owner and sole mutator of its intent generation, attempt identity, bounded `RetryState`, recovery episode state, and state revision.
- Committed one attempt immediately before the owner-factory call, made construction explicitly fallible, and recorded construction failure, owner creation, startup failure, actual `Started`, terminal failure, normal completion, terminal delivery, and joined cleanup against that same identity.
- Preserved attempt and recovery accounting across the complete owner lifecycle; no lifecycle event increments the attempt twice and `Started` does not reset recovery state.
- Added an owned immutable recovery-evaluation snapshot containing current intent/generation/revision, configuration identity, retry state, attempt identity, lifecycle evidence, retry guidance, and applicable policy inputs. Snapshot creation is side-effect free and later state mutations are detectably stale.
- Invoked the pure `RecoveryPolicy` only after terminal and cleanup facts are recorded and retained the snapshot plus decision as advisory supervisor state.
- Proved with hardware-independent injected-owner tests that a permit decision creates no owner, explicit stop invalidates prior evaluation, late events cannot restore running intent, and factory creation remains exactly once.
- Kept retry counters, recovery state, decisions, snapshots, and cooldown private to `resonance-agent`; `resonance-core` and `resonance-api` remain unchanged.
- Kept all recovery execution disabled: no recovery authorization is consumed, no automatic attempt is committed, and no retry loop, timer, sleep, backoff execution, watcher, reconnect, default-device following, or replacement owner exists.

## Milestone 6I-A: Recovery configuration model

- Added a private `resonance-agent` validation boundary that converts a complete configuration definition into an owned immutable snapshot; missing fields and invalid definitions fail closed without repair or fallback defaults.
- Represented the maximum automatic recovery-attempt budget and exhaustion behavior, required cooldown and duration, backoff strategy and maximum delay, jitter requirements, stable agent-level failure classification, and stable-run reset evidence and duration without implementing any algorithm, clock, timer, scheduler, entropy source, or recovery action.
- Required enabled automatic recovery to have a finite nonzero budget, at least one explicitly eligible or evidence-guarded failure class, compatible typed evidence for guarded device/reconfiguration/resource/format failures, nonzero cooldown, bounded nonzero backoff, coherent jitter bounds, and consistent cooldown/backoff/stable-run reset rules. An explicitly disabled definition requires zero budget, all failures non-retryable, disabled delays, and new-intent-only reset.
- Assigned each accepted definition an explicit nonzero configuration version plus a deterministic fingerprint of its canonical typed content. Equivalent definitions with the same version have the same identity; changing content or version invalidates prior assumptions even if a caller reuses a version.
- Embedded the full immutable configuration snapshot and its identity in the supervisor's owned recovery-evaluation snapshot while retaining the identity in retry state for stale-state checks.
- Kept the diagnostic runtime on an explicit, validated, recovery-disabled internal definition. Numeric values used to exercise enabled configuration are test-only and do not establish production retry, cooldown, backoff, jitter, or stable-run values.
- Added hardware-independent tests for stable identity, missing fields, contradictory and unsafe combinations, immutable snapshot ownership, configuration-change invalidation, and the absence of retry-state mutation during validation.
- Kept configuration loading, file and environment sources, runtime reload, policy application of the new fields, retry execution, timers, reconnect, replacement owners, endpoint watching, and default-device following deferred.

## Milestone 6I-B: Recovery configuration to policy integration

- Connected validated immutable recovery configuration, configuration-bound `RetrySnapshot`, and typed lifecycle/source evidence to the pure recovery-policy entry point.
- Made stable failure-class dispositions deterministic policy gates: non-retryable classes remain stopped, retryable classes continue through the existing cause rules, and guarded classes require their configured typed evidence.
- Derived attempt availability from the configured maximum automatic-recovery attempts and recovery-episode accounting, including sticky exhaustion; caller-provided budget conclusions cannot override state.
- Required satisfied retry-state cooldown evidence whenever validated cooldown/backoff policy requires delay, without calculating a delay, reading a clock, running a timer, sleeping, scheduling, or executing backoff.
- Rejected stale configuration before permission when the current version/fingerprint, captured configuration, or retry-state binding disagree. A Config A evaluation cannot authorize after Config B becomes current.
- Preserved reset safety: owner creation, successful start, and `Started` do not clear exhaustion; invalid reset evidence leaves state unchanged, while a new explicit intent creates fresh accounting.
- Added hardware-independent tests for enabled and disabled classifications, guarded source evidence, budgets, cooldown, unsupported format, panic, configuration identity mismatch, Config A-to-B invalidation, reset behavior, and the decision-versus-execution boundary.
- Kept the runtime on its explicit recovery-disabled definition and retained decisions as advisory data only. No authorization is consumed, no automatic attempt is committed, and no owner, retry, timer, watcher, reconnect, or default-device-following behavior was added.

## Milestone 6J: Operational recovery parameter design

- Selected one finite shared automatic-attempt budget per recovery episode; stable failure classes control eligibility and typed prerequisites rather than owning independent counters that could multiply the episode ceiling.
- Defined one budget unit as one supervisor commitment to automatic owner creation, including construction and startup failure, while repeated identical failures advance backoff pressure instead of receiving arbitrary weighted charges.
- Required every enabled profile to use nonzero cooldown and bounded backoff, selected capped exponential growth as the preferred production direction, and retained fixed or linear strategies only for evidence-backed profiles.
- Required explicit, bounded, externally sampled jitter for time-based production retries unless a documented deployment-specific exception proves synchronization cannot occur; policy remains deterministic and randomness-free.
- Required a new explicit intent or sustained continuous frame delivery for an evidence-bound minimum duration to reset an episode. Owner construction, successful start, `Started`, source change, configuration change, and process restart alone do not reset failure pressure.
- Classified device unavailability, source reconfiguration, resource exhaustion, and unsupported format as changed-evidence cases; interruption remains conditionally retryable; coarse construction/startup/internal failures and worker panic remain non-retryable.
- Defined agent-internal operational observability and the representative failure, outage, retry-success, recurrence, resource-impact, and synchronization evidence required before numeric values can be approved.
- Kept exact values, deterministic delay calculation, jitter sampling, configuration loading/reload, persistence, telemetry, clocks, timers, scheduling, reconnect, endpoint watching, owner replacement, and all recovery execution deferred. The runtime remains explicitly recovery-disabled.

## Milestone 6K: Recovery evidence collection and acceptance matrix

- Defined representative device-availability, source-reconfiguration, interruption, resource-pressure, and startup-failure scenarios, including the evidence and baseline comparison required for each.
- Required common evidence to bind intent generation, configuration identity, recovery episode, retry-state revision, source selection, prior attempt, terminal delivery, joined cleanup, timing, user impact, and resource impact.
- Defined class-specific availability, supported-format, backend-ready, pressure-clear, and retry-safe startup evidence; stale, missing, contradictory, diagnostic-only, and cross-context evidence fails closed.
- Planned controlled experiments that compare natural resumption with restart-assisted recovery and measure success by attempt ordinal, detection-to-stable-frame time, recurrence, operator impact, resource behavior, and multi-instance synchronization.
- Required each candidate class or subtype to meet predeclared cohort-specific reliability, uncertainty, user-impact, stability, and stopping objectives before it can move from manual-only to automatically recoverable; no arbitrary acceptance percentage was selected.
- Kept explicit stop, panic, suspected corruption, invariant failure, incomplete cleanup, active ownership, exhausted accounting, unchanged causal preconditions, and unsupported cohorts outside automatic recovery.
- Kept experiments, fault-injection tooling, numeric thresholds, production retry values, telemetry, timers, watchers, reconnect, replacement owners, and every recovery execution mechanism deferred. Recovery evidence and policy internals remain private to `resonance-agent`.

## Milestone 6M: Source selection model

- Reviewed Windows default-playback, endpoint-removal, Bluetooth-availability, manual-restart, and source-identity evidence without changing runtime behavior.
- Selected a hybrid source intent model: Default Playback re-resolves the platform role for each future attempt, while Explicit Source accepts only one opaque provider-assigned identity.
- Separated durable selection intent, resolved `SourceId`, and uninterrupted `StreamId`; friendly names and backend-native identifiers are not portable identity.
- Defined disappearance, replacement, and return as visible stream boundaries. Later attempts always create a new stream identity and timeline, even when the same source returns.
- Mapped the portable model to the Windows default-role/endpoint distinction and set constraints for a future PipeWire default-sink and stable-node-property mapping.
- Kept discovery, UI, platform mapping details, identity persistence, Linux implementation, endpoint watching, retry timing, owner replacement, and all recovery execution deferred.

## Milestone 6N: Source discovery and identity model

- Defined discovery as a replaceable point-in-time snapshot of provider-managed opaque source IDs plus descriptive kind, label, availability, and default-role metadata.
- Scoped `SourceId` to one provider installation on one host with one retained mapping namespace; IDs remain stable across refreshes, process restarts, metadata changes, and temporary absence only while backend continuity is proven.
- Required conservative lifecycle handling: disappearance alone retains identity, proven return reuses the ID with a new stream, ambiguous return receives a new ID, and retired IDs are never reassigned.
- Mapped Default Playback to attempt-time Windows role resolution and future PipeWire session-manager policy while keeping backend-native endpoint and node identifiers private.
- Selected provider-managed opaque identity over backend-native IDs and display names.
- Kept enumeration, Windows adapter changes, PipeWire implementation, UI, authorization, endpoint watching, and recovery execution deferred.

## Milestone 6O: Consumer discovery API and identity registry design

- Defined a replaceable `DiscoverySnapshot` with an opaque revision and `SourceDescriptor` values containing ID, presentation name, kind, three-state availability, supported signal products, and current default roles.
- Required revision preconditions for operations made directly from a snapshot, while durable Explicit Source intent remains ID-based and is revalidated against current registry state.
- Assigned one private installation-and-host-bound identity registry, backend evidence, live/absent mappings, and permanent retired-ID tombstones to `resonance-agent`.
- Required whole-registry atomic commits and safe loss of continuity for missing or corrupted state, incompatible schemas, failed migration, host change, and explicit reset; discovery stays unavailable when a new namespace cannot be committed.
- Defined conservative entry migration and retirement for backend or evidence-rule changes, with no identity preservation based on display names or similarity.
- Defined a deterministic fake-backend and fault-injection validation seam for duplicate names, metadata changes, proven and ambiguous return, native-key reuse, stale snapshots, no substitution, and persistence failures.
- Kept enumeration, platform adapters, Rust API types, UI, InfoPanel integration, Linux implementation, and recovery behavior deferred.

## Milestone 6P: Private source identity registry storage

- Implemented the private installation-and-host namespace, opaque `SourceId` allocation, live/absent/retired mappings, permanent tombstones, revision gating, and atomic candidate/backup/current persistence in `resonance-agent`.
- Preserved proven continuity across restart and compatible schema handling while failing closed on corruption, incompatible evidence, reset, native-key reuse, and ambiguous return.
- Added deterministic allocator and fault-path tests without adding a serialization dependency or exposing registry schema, backend evidence, or storage paths through `resonance-api`.

## Milestone 6Q: Windows WASAPI playback source discovery

- Added explicit, bounded Windows render-endpoint refresh through the existing `wasapi` 0.24.0 dependency. Enumeration requests `DEVICE_STATE_ACTIVE` endpoints only and defensively rechecks state before mapping; microphone/capture endpoints are excluded.
- Resolved the Windows console default-render role separately from endpoint identity and annotated only its current mapped descriptor. Moving the role changes discovery revision and resolution without changing either endpoint's `SourceId`.
- Used private persistent `IMMDevice` endpoint IDs plus a versioned evidence-schema token as installation/host-scoped continuity evidence. Names, format, channel count, transport, and default status remain presentation or state only.
- Reconciled active observations through the existing registry, preserving IDs across no-op refresh, metadata rename, temporary absence/proven return, and registry reopen while retiring or omitting unsafe ambiguous/native-key-reuse cases. Reopen advances snapshot freshness without changing proven source identity.
- Added deterministic fake-source coverage for duplicate names, state filtering, rename, role movement, absence/return, ambiguity, native-key reuse, restart, stale snapshots, no substitution, and no-op revision behavior.
- Validated the real Arrakis endpoint set without modifying devices: one active endpoint, `Realtek Digital Output (Realtek USB Audio)`, owned Default Playback; immediate refresh and registry reopen retained its opaque `SourceId`.
- Kept the discovery snapshot private. Consumer descriptors/transport, capture-time registry integration, explicit-source capture, endpoint watching, microphone discovery, PipeWire, UI, InfoPanel, and recovery execution remain deferred.

## Milestone 6R: Consumer source discovery contract

- Added owned portable `DiscoverySnapshot`, opaque equality-only `DiscoveryRevision`, `SourceDescriptor`, and three-state `SourceAvailability` types to `resonance-api` without selecting serialization or transport.
- Represented mutable display names, source kind, supported signal products, and point-in-time default roles separately from opaque `SourceId`; duplicate names remain distinct and Default Playback remains role intent rather than a synthetic ID.
- Converted the private Windows playback snapshot in `resonance-agent` into the portable contract while stripping endpoint IDs, continuity evidence, registry namespaces, tombstones, schemas, and storage details.
- Retained known non-retired absent playback sources as `Unavailable`, advertised only the proven `Waveform` product, and preserved revision equality plus private stale-resolution rejection and exact-ID no-substitution behavior.
- Kept capture-time Default Playback mapping, explicit-source capture, endpoint watching, transport, serialization, UI, InfoPanel, microphone discovery, PipeWire, and recovery execution deferred.

## Milestone 6S: Mapped Default Playback capture

- Resolved the Windows console-role Default Playback owner through the private discovery and durable identity registry at the start of each capture attempt.
- Bound one opaque registry-backed `SourceId` to the exact private WASAPI endpoint ID, opened that endpoint explicitly, and revalidated the revision-bound mapping after notification registration before publishing `Started`.
- Replaced the legacy logical `default-playback` stream source with the actual mapped `SourceId` while retaining Default Playback as internal role intent.
- Failed closed before stream publication when the default role, discovery snapshot, registry durability, or opened endpoint could not prove the same binding.
- Added deterministic coverage for role movement, stale binding rejection, unavailable default, duplicate names, no synthetic identity, exact endpoint mismatch, truthful descriptors, and independent stream identities.
- Kept explicit-source capture, endpoint watching, automatic migration or replacement, transport, UI, microphone discovery, PipeWire, and recovery execution deferred.

## Milestone 6T: Explicit SourceId playback capture

- Added an agent-level Default Playback versus Explicit Source intent to Windows owner and supervisor construction without exposing native endpoint types.
- Resolved an explicit opaque `SourceId` through fresh private discovery and registry state, bound only its exact live WASAPI endpoint, and revalidated the revision-bound mapping before `Started`.
- Failed unknown, unavailable, retired, stale, and mismatched explicit mappings closed without substituting Default Playback, a same-named source, or another available endpoint.
- Reported the selected opaque `SourceId` in `StreamDescriptor`, retained private native identity, and preserved a new `StreamId` and timeline for every attempt.
- Kept Default Playback independently role-resolved and made default-role changes irrelevant to an active explicit capture while retaining endpoint and session termination boundaries.
- Added a narrow diagnostic `--source-id <opaque-id>` option, hardware-independent identity/binding tests, and real Arrakis validation for Default Playback, explicit repeat capture, and unknown-ID rejection.
- Kept endpoint watching, automatic migration or recovery, transport, UI, microphone discovery, and PipeWire deferred.

## Milestone 6U: Local consumer agent and waveform transport

- Added long-running `resonance-agent serve` mode with loopback-only `/v1/status`, `/v1/sources`, and `/v1/waveform` HTTP/WebSocket operations.
- Kept Default Playback as attempt-time role intent and Explicit Source as exact opaque identity with no substitution; every WebSocket capture receives an independent supervisor, fresh `StreamId`, and zero-based timeline.
- Mapped scheduled mono/stereo waveform windows to a versioned 40-byte little-endian header plus interleaved finite `f32` payload; JSON carries portable stream metadata, lifecycle, errors, and exact stop control.
- Added bounded 16-item per-client queues, a 16-session process limit, non-blocking capture-to-network handoff, two-second socket write deadlines, malformed-input limits, deterministic cleanup, and independent-session behavior.
- Added consumer-complete protocol documentation, a dependency-free external Python diagnostic, focused transport tests, and ADR 0016.
- Kept InfoPanel, browser UI, persistent consumer configuration, endpoint watching, automatic migration/recovery, Windows SCM installation, microphone/PipeWire, LAN listening, authentication, and TLS out of scope.

## Milestone 6V: Windows beta UX and packaging

- Made normal launch a tray-managed per-user runtime.
- Added actual service startup/running/failure state to the tray menu.
- Added truthful opt-in HKCU Start with Windows registration.
- Bounded Exit around graceful listener and active-session shutdown.
- Added capped local lifecycle diagnostics without audio or native identity data.
- Added a source-free Windows x64 ZIP layout and release checklist.
- Added tester-facing Windows beta setup and troubleshooting guidance.
- Deferred executable, tray, and README branding until approved assets were
  available in the repository.

## Milestone 6V.1: Windows beta branding integration

- Retained the approved full icon and wide banner in the repository-owned
  branding directory.
- Embedded multi-resolution application and tray icon resources directly in
  `resonance-agent.exe`.
- Added the approved banner near the top of the GitHub README.
- Preserved the existing three-file Windows beta package layout and runtime
  behavior.

## Later milestones

- Execute the accepted measurement plan, collect representative operational evidence, and review candidate failure classes without enabling recovery.
- Select exact retry, cooldown, backoff, jitter, and stable-run values only from accepted evidence; define deterministic delay calculation and jitter sampling; and design a configuration source/reload boundary in separately scoped milestones.
- Implement recovery execution only under separate approval, preserving one-shot stale-decision validation, no-overlap ownership, and new-stream identity requirements.
- Add microphone capture and the Linux PipeWire adapter only in separately scoped milestones.
- Add optional FFT, spectrum, and frequency-band processing after practical requirements are defined.
- Design non-loopback security only in a separate milestone that explicitly covers authentication, authorization, transport protection, abuse controls, and deployment behavior.

Consumer applications and visualization remain outside this roadmap and repository.
