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

## Later milestones

- Implement agent-internal retry configuration and supervisor-owned mutable retry-state representation with hardware-independent transition tests, without timers, endpoint watchers, reconnect, or replacement capture unless separately approved.
- Select and validate concrete retry/backoff values only from operational evidence in a separately scoped milestone.
- Add microphone capture and the Linux PipeWire adapter only in separately scoped milestones.
- Add optional FFT, spectrum, and frequency-band processing after practical requirements are defined.
- Define an appropriate client transport only when contract requirements justify it.

Consumer applications and visualization remain outside this roadmap and repository.
