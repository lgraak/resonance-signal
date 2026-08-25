# Architecture

Resonance Signal is a standalone audio signal provider. It owns capture, signal processing, and the client-facing provider interface. External consumers remain outside the provider and consume exposed audio data rather than embedding capture implementations.

## Layer direction

```text
Audio Capture Layer
        |
        v
Audio Processing Layer
        |
        v
API / Client Interface
        |
        v
External Consumers
```

Data and dependencies should flow toward the client interface. Presentation and visualization concerns belong to consumers, not to Resonance Signal.

## Workspace boundaries

### `resonance-core`

Owns provider-independent signal values, validation, lightweight processing, and bounded analysis-window scheduling. The `signal` module defines waveform, level, and spectrum frame structures without capture or transport dependencies. The `scheduling` module accumulates contiguous waveform batches into complete windows with bounded retention and explicit discontinuity handling. The `processing` module provides zero-copy waveform subwindows, per-channel RMS and peak calculation, and opt-in peak normalization.

### `resonance-api`

Owns the consumer-facing semantic contract: source selection, subscriptions, stream lifecycle, product payloads, contract versioning, and platform-neutral failures. It depends on and re-exports `resonance-core` signal types. No transport or network protocol is selected.

### `resonance-agent`

Owns platform capture, capture-format enforcement, and provider lifecycle orchestration. Its production Windows playback-loopback boundary uses `wasapi` 0.24.0 behind a hardware-independent packet-to-frame seam. The executable is a bounded diagnostic client of that boundary. Official `pipewire-rs` bindings remain the selected but unimplemented Linux direction.

Dependency direction is one way:

```text
resonance-agent  --->  resonance-api  --->  resonance-core
       |                                        ^
       +------------- capture output -----------+
```

`resonance-core` cannot depend on capture backends, transports, or consumers. `resonance-api` cannot depend on an operating-system capture implementation. Consumer concerns never flow back into these crates.

## Stereo-first capture boundary

Resonance Signal supports capture products with one or two channels. Mono is one ordered channel. Known stereo is ordered front-left then front-right; an unknown two-channel layout may remain discrete without guessed speaker positions. Surround layouts, spatial/object metadata, and explicitly positioned non-stereo pairs are not supported capture products.

The enforcement point is the capture boundary owned by `resonance-agent`. Keeping the restriction there preserves the provider-independent `ChannelLayout` type and avoids a breaking core/API contraction while ensuring unsupported platform formats never enter an active product stream.

```text
platform source
      |
      v
backend negotiation and mono/stereo validation  -- unsupported --> ProviderError
      |
      v
bounded interleaved f32 AudioFrame batches
      |
      v
WindowScheduler --> signal processing --> resonance-api events --> consumers
```

A future backend may use a native platform format internally, but its accepted output must have an actual non-zero sample rate, one accepted channel layout, finite interleaved `f32` samples, contiguous source frame indices, and stream-relative monotonic timestamps. Capture batch sizes may vary and must remain bounded. The fixed format, source ID, and uninterrupted-stream ID are established before publishing `StreamEvent::Started`.

Platform-provided conversion is acceptable only when it yields a valid mono or stereo representation whose ordering can be reported truthfully. A source with more than two channels is otherwise rejected as `UnsupportedFormat`. Selecting the first two channels, relabelling known non-left/right positions, or performing a custom downmix is forbidden. Unknown one- and two-channel layouts may remain discrete because that preserves order without inventing semantics.

Interruption, restart, reconfiguration, timestamp discontinuity, or format change ends the stream. A resumed source receives a new stream ID, frame index zero, and a new monotonic timeline. This preserves the continuity rules already enforced by `WindowScheduler`; no backend-specific timing or identity type enters `resonance-core`.

Backend evaluation selected platform-specific safe wrappers that retain native evidence: `wasapi-rs` 0.24.0 for Windows and `pipewire-rs` 0.10.1 for Linux. A third-party cross-platform capture abstraction is not selected because the evaluated generic surface loses timing-validity or provenance needed by the uninterrupted-stream contract. The Windows playback-loopback capture boundary is productionized; Linux capture remains deferred.

The Windows adapter retains WASAPI device position, QPC timestamp, packet flags, endpoint identity, and endpoint/session notifications before mapping them to provider lifecycle. The future Linux adapter will retain negotiated SPA format, target properties, stream and registry events, buffer metadata, and graph timing. Only bounded samples and platform-neutral accepted format, source, stream, lifecycle, and diagnostic semantics cross the adapter boundary.

### Source intent and identity

Source selection uses the hybrid model in [ADR 0013](decisions/0013-source-selection-model.md), extended by the discovery and identity model in [ADR 0014](decisions/0014-source-discovery-and-identity-model.md) and the consumer discovery and private registry contract in [ADR 0015](decisions/0015-consumer-discovery-and-identity-registry.md). Default Playback is a logical role resolved before each future capture attempt; Explicit Source is pinned to one opaque provider-assigned `SourceId`. The provider retains intent separately from the resolved source so a later default resolution may truthfully select a new role owner while an explicit request can fail closed instead of silently substituting another device.

The identity domain is one provider installation on one host with one retained mapping namespace. `resonance-agent` maps private backend-native continuity evidence to an opaque `SourceId`; `StreamId` separately identifies one uninterrupted capture lifetime. The same source can therefore retain its source ID across discovery refreshes, process restarts, metadata or default-role changes, and proven temporary absence while producing many stream identities. Default Playback can resolve different source identities over time. Friendly names, platform classes, formats, and default status are mutable metadata, not identity proof.

A future consumer discovery snapshot exposes an opaque revision plus descriptors containing source ID, kind, optional display name, three-state availability, supported signal products, and point-in-time default roles. It neither reserves a source nor guarantees that metadata remains current. A direct selection from a snapshot carries the revision as a freshness precondition; revision mismatch is rejected before resolution. Backend-native endpoint or node identifiers stay private, retired IDs are never recycled, and missing or ambiguous return evidence invalidates continuity and requires rediscovery.

`resonance-agent` exclusively owns the durable identity registry. One logical writer atomically commits installation-and-host-bound namespace state, live and absent mappings, registry revision, private backend evidence, schema identity, and permanent retired-ID tombstones. Process restart and compatible provider upgrades retain identity; host change, namespace loss, corruption, incompatible schema, failed migration, or reset safely loses continuity rather than guessing. A new namespace must be durably committed before discovery can publish IDs, and failed persistence leaves discovery unavailable instead of emitting ephemeral identities. This transactional registry-storage behavior and the private refresh/snapshot resolver are implemented in `resonance-agent`.

Windows discovery calls `IMMDeviceEnumerator::EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)` through `wasapi` 0.24.0 and defensively rechecks each returned endpoint state. Only `Active` render endpoints become selectable descriptors; disabled, not-present, unplugged, capture, and microphone endpoints do not. The adapter resolves `GetDefaultAudioEndpoint(eRender, eConsole)` and annotates the matching active descriptor. Default Playback remains role intent and resolves through that annotation to the endpoint's mapped `SourceId`; it never receives a synthetic identity.

The private Windows continuity key is the `IMMDevice::GetId` endpoint ID plus the versioned evidence-schema token `windows-immdevice-endpoint-id-v1`. Microsoft documents endpoint IDs as persistent across restart and globally unique on the machine; the registry scopes that claim further to one installation and host. Display name, format, channel count, transport, active state, and default-role ownership are never identity evidence. The native endpoint ID is persisted only in the private registry and does not cross into `resonance-core`, `resonance-api`, descriptors, or diagnostics. If observations duplicate or contradict continuity evidence, the refresh omits them from selectable descriptors and invokes registry ambiguity retirement rather than guessing. No-op refreshes in one process retain their revision, role or descriptor changes advance it, and the first refresh after reopening advances an otherwise unchanged revision so pre-restart snapshots fail closed while `SourceId` mappings persist. The implementation uses the existing `wasapi` dependency only. See Microsoft's [endpoint persistence description](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-endpoint-builder-algorithm) and [`EnumAudioEndpoints` state semantics](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdeviceenumerator-enumaudioendpoints).

Disappearance, replacement, return, reconfiguration, and format change always end the current stream. A later attempt starts a new stream at frame index and stream-relative time zero. Default Playback resolves the then-current role owner; Explicit Source accepts only the same proven identity. Refresh is explicit and bounded; continuous watching, microphone discovery, PipeWire, consumer transport, capture-time discovery integration, and recovery execution remain deferred.

The current Windows capture adapter predates the resolved identity mapping: it emits the logical `default-playback` value as `SourceId` and retains the resolved endpoint name only in diagnostics. The private discovery layer now proves the correct mapped identity and Default Playback resolution, but capture-time integration remains a separate milestone. That integration must replace the placeholder before consumer discovery and capture are connected; explicit-source capture must remain identity-pinned and fail closed.

### Windows production capture data path

```text
default console rendering endpoint
        |
        v
WASAPI shared loopback event thread
        |
        | preallocated pool: 4 fixed-capacity byte buffers
        v
bounded non-blocking synchronous handoff
        |
        v
packet validation + f32 conversion + AudioFrame
        |
        v
StreamEvent processing callback
```

The WASAPI event thread owns COM and all non-`Send` endpoint/client objects. Its repeated work is limited to waiting, querying packet size, copying into an available fixed buffer, recording timing/flags, and attempting a non-blocking handoff. Conversion, finite-value validation, `AudioFrame` allocation, provider event construction, report aggregation, and consumer callbacks run on the ordinary processing thread. The CLI prints only the events and report it receives; console output is not part of capture ownership.

The pool and handoff each have four slots. This is an internal bound supported by real-device evidence: observed packets represented approximately 10 ms while callback work remained sub-millisecond. Every buffer is preallocated to the maximum frame count reported by the initialized WASAPI client, so packet size does not become a configuration knob. If either fixed-buffer ownership or channel capacity is exhausted, the stream ends with an explicit `ResourceExhausted` error before any packet can be silently discarded.

The adapter accepts the endpoint mix sample rate and requests float output. Native mono stays mono; all other non-zero native layouts are offered to the Windows audio engine as explicit front-left/front-right stereo. This is platform conversion, not silent first-two-channel extraction or a Resonance Signal downmix. If WASAPI cannot initialize that representation, no stream starts.

WASAPI device position validates packet adjacency. QPC timestamp validity and monotonicity are also enforced and its deltas are retained as evidence. Provider timestamps are normalized sample time: frame index zero and timestamp zero at the first accepted packet, then `frame_index * 1_000_000_000 / sample_rate`. This avoids importing the absolute Windows clock while keeping frame timestamps compatible with sample-contiguous scheduling.

Endpoint and session callbacks perform only an atomic first-reason update. Default playback replacement, format change, endpoint invalidation, device removal, session disconnect, later packet discontinuity, or timing failure stops the current WASAPI stream and emits an explicit provider error/end event. A normal owner stop is requested through a cloneable `CaptureStopToken` and maps to `ProviderShutdown`. Duration belongs only to the diagnostic runner and maps to `ConsumerCancelled`.

The adapter does not reconnect or continuously follow the default device. Invoking a later capture run is the explicit restart operation and establishes a new stream identity, frame index zero, and timestamp zero. This keeps reconnection policy at the supervisor boundary and prevents retry loops from being hidden inside platform capture.

### Capture owner lifecycle

The Windows `CaptureOwner` is the single public lifetime owner for one long-running run. Construction is inert. `start` transfers the event callback and blocking adapter runner to one ordinary owner worker. That worker owns the capture result and the obligation to join its nested WASAPI thread; the WASAPI thread exclusively owns COM initialization, endpoint/client objects, notification registrations, event handles, and the bounded native buffer pool.

```text
CaptureOwner
    |
    | start: transfer callback + runner
    v
ordinary owner worker
    |-- owns provider event processing and callback
    |-- owns capture completion
    |-- owns join obligation for WASAPI thread
    |
    v
WASAPI thread
    |-- owns COM and all endpoint/client resources
    |-- releases registrations, stream, handles, and COM before exit
```

The owner state machine is deliberately single-use:

```text
created --start--> running --terminal event or stop--> completed + joined
   |                  |
   | stop             | shutdown timeout
   v                  v
stopped-before-start  running, ownership retained
```

A stop request is an atomic, idempotent signal. A pre-start stop prevents initialization. During a run, the WASAPI event loop observes stop at least once per 100 ms wait interval, stops the audio client, emits the existing terminal event, releases its thread-affine resources, and exits. The ordinary owner worker finishes callbacks, joins the WASAPI thread, and reports completion. `shutdown` bounds only the caller's wait: a timeout never detaches the worker or moves the callback out of the owner, and the caller can wait again. Successful shutdown returns only after the worker is joined, so no event callback or capture resource remains live. Callbacks must return promptly; a callback that blocks can cause bounded shutdown to time out. Drop provides a final request-and-join cleanup path.

The owner does not contain retry, backoff, default-device following, or endpoint replacement policy. It cannot be restarted.

### Capture supervisor and recovery-policy boundary

Recovery policy belongs to `CaptureSupervisor` orchestration in `resonance-agent`, above the single-use owner. A side-effect-free `RecoveryPolicy` decision boundary classifies structured evidence; it does not own capture or scheduling:

```text
CaptureSupervisor
    |-- owns desired running/stopped state
    |-- owns the current CaptureOwner
    |-- owns intent generation, attempt identity, and RetryState
    |-- observes typed events and completion
    |-- creates immutable evaluation snapshots
    |-- records RecoveryPolicy decisions
    |
    +--> RecoveryPolicy: remain stopped, wait, or permit replacement
    |
    +--> no recovery action
    |
    v
CaptureOwner (one attempt)
    |
    v
WASAPI thread
```

The recovery-disabled supervisor creates and owns at most one single-use owner for a supervised capture intent. Its deterministic lifecycle states are `Idle`, `Running`, `Stopping`, and `Completed`. Start is permitted only from `Idle`; immediately before the narrow `CaptureOwnerFactory` call, the supervisor begins a nonzero intent generation and commits exactly one attempt identity. Construction failure, owner creation, startup failure, `StreamEvent::Started`, terminal failure, normal completion, terminal delivery, and joined cleanup are mutations of that same supervisor-owned attempt. None increments the attempt ordinal a second time, and `Started` does not reset recovery accounting. Stop first clears desired-running intent in `RetryState`, then requests owner stop and waits for joined completion. Stop before start transitions directly to `Completed` without creating an intent or owner, and repeated stops retain the same completion. A bounded wait timeout leaves the owner and `Stopping` state intact so the caller can wait again.

Every owner event remains visible to consumers. The supervisor records the actual `Started` event, typed error kind, retry hint, and end reason only after the consumer callback returns. `CaptureOwner::wait_for_completion` observes natural termination without requesting stop and joins the owner worker; successful completion therefore means callbacks have ended and nested WASAPI resources have been released. The supervisor then records the attempt outcome and cleanup, retains a typed completion summary and the full owner completion, and preserves retry state after the owner lifetime ends.

Replacement eligibility is only a recorded mechanical boundary: desired-running intent is still enabled, an `Ended` event was delivered, owner completion was received, and resources were released. No supervisor method consumes that eligibility or creates another owner, including after normal completion, construction failure, startup failure, capture failure, or panic.

The agent-internal recovery representation separates `RecoveryContext`, `RecoveryCause`, validated `RecoveryConfigurationSnapshot`, immutable `RetrySnapshot`, and typed `RecoveryEvidence` from `RecoveryDecision`. Its pure evaluator implements the ADR 0008 precedence order and returns a stable reason with remain-stopped, wait, or permit-replacement. Explicit stop and stale intent win before lifecycle checks. Configuration identity must match the current version/fingerprint and the retry-state binding; any mismatch returns a stale-configuration remain-stopped decision. A started stream requires its terminal event, joined owner completion, and resource release; a startup attempt requires joined completion and release but no stream terminal event. Missing or inconsistent structured evidence fails closed.

Policy first applies the configured stable failure-class disposition: non-retryable remains stopped, retryable continues through the cause-specific rule, and guarded classes require their configured typed source/format/pressure/changed-precondition evidence. It then derives remaining automatic attempts from the configured maximum and the recovery episode's automatic-attempt count; a sticky exhaustion record or count at the limit remains stopped. Required cooldown/backoff posture can progress only when retry state contains satisfied cooldown evidence. Caller-supplied `attempts_remaining` or `cooldown_complete` values are overwritten by these derived facts. Device removal and invalidation require availability evidence. Default-endpoint replacement requires follow-default policy and a resolved replacement, while format reconfiguration requires a supported fresh format. Resource exhaustion additionally requires pressure-clear evidence. Unsupported format under unchanged conditions, broad internal failure, unclassified startup failure, and worker panic remain stopped. Retry hints constrain policy but never command it. No timing value or backoff delay is calculated.

The implemented retry-state component gives each explicit start a new intent generation and each committed owner-creation call a monotonic attempt identity within that generation. Its transition model is:

```text
Idle --commit attempt--> Attempting --Started--> Running
                              |                    |
                              +------failure------+--> Failed
                                                       |
                                      terminal evidence + joined cleanup
                                                       |
                                                       v
                                                    Waiting
                                                    /     \
                                    commit recovery       exhaust
                                           |                 |
                                           v                 v
                                      Attempting          Exhausted

explicit stop from any current state --> Stopped
```

Construction, startup, `Started`, terminal failure, terminal delivery, and cleanup remain facts on one attempt and cannot increment it twice. Total attempts and automatic recovery attempts are distinct counters. A `Started` event alone does not reset recovery state; only a new explicit intent or explicit already-validated stable-run/changed-precondition evidence advances an episode. Recent typed failure records are structurally bounded while last-failure and aggregate counts survive eviction.

After joined completion, the supervisor copies current intent, intent generation, state revision, the full immutable configuration and identity, retry state, attempt identity, typed lifecycle evidence, and retry guidance into an owned immutable recovery-evaluation snapshot. Snapshot creation is side-effect free. `RecoveryPolicy` derives budget and cooldown conclusions while evaluating that snapshot, and the supervisor records the snapshot and decision together. A later explicit stop or other retry-state mutation makes the recorded evaluation detectably stale through its generation/revision binding; a configuration version or fingerprint change independently makes the old evaluation fail closed. Exhaustion remains sticky within an episode. A `Started` event, owner creation, or successful start is not reset evidence; only explicit typed reset transitions can advance an episode, while a new explicit intent creates fresh state. Checked counters and revisions are calculated before mutation so a rejected transition does not leave partially updated state.

Cooldown is represented as `NotRequired`, `Pending`, `Satisfied`, or `Invalidated`, correlated by opaque eligibility and satisfaction evidence. It does not contain a clock, deadline, duration, timer, or sleep. Satisfying cooldown only updates evidence; it does not allocate an attempt.

The recovery configuration boundary is a private sibling of retry state and recovery policy inside `resonance-agent`:

```text
untrusted complete definition
        |
        v
fail-closed structural validation
        |
        +-- reject missing, contradictory, zero-delay, or unbounded-loop inputs
        |
        v
owned immutable configuration snapshot
        |
        +-- explicit nonzero version
        +-- deterministic canonical-content fingerprint
        |
        v
supervisor recovery-evaluation snapshot
```

The configuration model represents the maximum count of automatic replacement attempts and exhaustion behavior; whether cooldown is required and its minimum duration; fixed, linear, or exponential backoff parameters and a maximum delay; forbidden or bounded-required jitter; retryable, non-retryable, or additional-evidence treatment for stable agent-level failure classes; and new-intent or evidenced stable-run reset behavior. It does not retain platform errors. It selects no production retry, cooldown, backoff, jitter, or stable-run values.

An enabled definition must pair a nonzero finite budget and an explicitly eligible or evidence-guarded failure class with nonzero cooldown, bounded nonzero backoff, and coherent delay/stable-run reset rules. An explicitly disabled definition has zero automatic attempts, classifies every failure as non-retryable, disables cooldown and backoff, and resets only on a new intent. Validation rejects instead of repairing any mismatch. The accepted object exposes no mutator and owns its values, so later mutation of the input definition cannot alter a captured snapshot.

Configuration identity includes both a caller-controlled nonzero version and a deterministic two-lane fingerprint over a canonical encoding of all typed fields. The fingerprint is an identity checksum, not an authentication primitive. Identical content at the same version is stable; changing content or version changes identity. Retry state records that identity when a capture intent begins, and the recovery-evaluation snapshot owns the full accepted configuration. Policy now compares the captured identity with both retry state and the current supervisor configuration, so an old Config A snapshot cannot authorize recovery after Config B becomes current.

The runtime `CaptureSupervisor` owns `RetryState`, retains an explicit validated recovery-disabled configuration, and invokes the configuration-aware evaluator after recording terminal and cleanup facts. That runtime definition has zero automatic attempts and classifies every failure as non-retryable, so advisory evaluations remain stopped. A permit-replacement result from a test-only enabled definition is still advisory data only: it is not converted into a recovery authorization, committed recovery attempt, factory call, or owner. The configuration and state modules have no owner factory, device integration, event sink, clock, entropy source, thread, or scheduling mechanism. No configuration loading or reload, delay calculation, timer, watcher, reconnect, default-device following, or replacement exists. The lifecycle and state integration implement [ADR 0007](decisions/0007-capture-supervisor-boundary.md), [ADR 0008](decisions/0008-recovery-policy-boundary.md), and [ADR 0009](decisions/0009-retry-state-and-policy-boundary.md); the accepted configuration boundary is recorded in [ADR 0010](decisions/0010-recovery-configuration-model.md). [ADR 0011](decisions/0011-operational-recovery-parameters.md) selects the operational policy direction: one finite shared episode budget, stable class-specific evidence gates, mandatory cooldown, bounded backoff, explicit jitter, and stable-run reset evidence, while deferring numeric values and every execution mechanism. [ADR 0012](decisions/0012-recovery-evidence-acceptance-matrix.md) defines the scenario matrix, typed evidence bindings, comparative experiments, per-class enablement criteria, and never-recover boundaries required before any enabled profile can be justified. It requires predeclared reliability and impact objectives and proof that restart improves stable frame restoration without causing recurrence, resource amplification, or synchronized attempts; it selects no acceptance percentage or production value.

A future replacement would still require a new `StreamId`, frame index zero, and stream time zero; the supervisor cannot conceal the prior `Error`/`Ended` transition or synthesize continuity. Consumers observe lifecycle facts and platform-neutral errors, not policy state, attempt counts, or parsed diagnostics.

## Contract flow

1. A consumer submits a `SubscriptionRequest` naming one or more source selectors and signal products.
2. The provider retains each source intent, resolves it before a capture attempt, and emits a `StreamDescriptor` with the actual source identity for every uninterrupted stream.
3. The provider emits bounded `SignalPacket` values containing requested waveform, level, or spectrum payloads.
4. Errors carry a platform-neutral category, scope, and recovery hint.
5. Interruption, source reconfiguration, or format change ends the stream. Resumption creates a new stream identity and timeline.

The semantic contract is intentionally independent of delivery mechanics. A future in-process, local IPC, or network transport can carry the same event model. That transport must define bounded buffering, backpressure, serialization, ordering, and authentication separately.

## Raw and processed responsibility

Resonance Signal always treats raw waveform data as the canonical flexibility boundary. Consumers may request only raw data and perform arbitrary analysis themselves.

The provider contract also permits opt-in levels and magnitude spectra. Computing these once in the provider prevents every consumer from repeating the same expensive work and ensures derived frames share explicit source windows. Derived products remain additive: they cannot make waveform access conditional, and visualization-specific aggregation does not belong in the provider.

The first scheduled processing path is deliberately direct:

```text
AudioFrame
    |
    v
bounded WindowScheduler --> complete AudioFrame window
    |                               |
    |                               +-- borrowed WaveformWindow --> RMS + peak --> LevelFrame
    |
    +-- raw waveform remains independently available
```

`WindowScheduler` uses configurable, non-overlapping tumbling windows. Its default is approximately 30 outputs per second; a 60 FPS target is a configuration change. Duration is rounded to the nearest whole sample frame for the active sample rate, so cadence follows the source sample clock. Complete owned output frames allow one window to span several capture batches while remaining directly consumable by the existing zero-copy `WaveformWindow` processing boundary.

The scheduler retains less than one window between calls and bounds accepted work by a configured maximum number of windows per push. It emits immediately on completion, retains partial slow input without padding or timeout flushing, and rejects oversized calls without mutating state. Queuing completed output for slower consumers belongs to a future transport and must have its own bounded backpressure policy.

Frame index, timestamp, fixed format, and caller-supplied uninterrupted-stream identity establish continuity. Gaps, overlaps, timestamp discontinuities, or same-stream format changes return errors, discard partial data, and require a new stream identity. A normal identity change reports the number of discarded partial frames. No invalid or cross-stream samples silently enter an analysis window.

`WaveformWindow` borrows a complete frame or a frame-aligned subwindow, avoiding waveform copies and exposing channel-specific iteration when needed. Level calculation remains synchronous and stateless; orchestration decides whether to schedule and publish the derived result. Peak normalization is an explicit slice helper and is never applied implicitly to provider output.

Future products remain separate branches from the waveform input. A later `SpectrumFrame`, frequency-band frame, or associated FFT metadata can be added as an independently requested product. Consumers that request only waveform or levels do not need to calculate or receive it, and the raw `AudioFrame` contract does not change.

## Current constraints

- The Windows default-playback loopback capture boundary has a single-use, lifecycle-managed owner and a recovery-disabled supervisor in `resonance-agent`; the supervisor owns its deterministic retry state and validated immutable recovery configuration, evaluates immutable snapshots, and records decision-only policy results. Recovery enablement remains blocked on representative per-class experiments and accepted evidence; no experiment tooling, configuration loading/reload, reconnecting service, retry loop, timer, backoff execution, replacement behavior, or service installation exists.
- Windows microphone capture and all Linux capture remain unimplemented.
- Supported future capture output is limited to mono and two-channel stereo; wider, spatial, and object-based formats are rejected unless the platform supplies a valid mono/stereo representation.
- Custom downmixing and silent first-two-channel extraction are prohibited.
- Processing is currently limited to bounded tumbling-window scheduling, waveform subwindows, RMS, sample peak, and explicit peak normalization.
- FFT, spectrum generation, frequency bands, smoothing, overlapping hops, transport queues, and output backpressure are deferred.
- No device-discovery interface or enumeration implementation exists; only its representation and lifecycle semantics are defined.
- The Windows adapter does not yet map the resolved endpoint identity to the consumer-visible `SourceId`.
- Source identity is scoped to one installation-on-one-host mapping domain. Its registry, atomicity, migration, corruption, retirement, and reset semantics are defined, but storage and platform continuity validation are not implemented.
- No serialization format, network service, IPC mechanism, or transport is defined.
- No consumer or visualization code belongs in this repository.
- End-to-end capture latency is not measured because cross-clock correlation is deferred.

See [ADR 0001](decisions/0001-audio-data-contract.md) for the audio contract, [ADR 0002](decisions/0002-bounded-window-scheduling.md) for scheduling and buffering decisions, [ADR 0003](decisions/0003-stereo-first-capture-boundary.md) for capture scope and enforcement, [ADR 0004](decisions/0004-capture-backend-selection.md) for backend evidence and implementation direction, [ADR 0005](decisions/0005-windows-capture-lifecycle-and-buffering.md) for the production Windows lifecycle and bounded handoff, [ADR 0006](decisions/0006-capture-owner-lifecycle.md) for explicit owner/thread/shutdown responsibility, [ADR 0013](decisions/0013-source-selection-model.md) for source intent and selection semantics, and [ADR 0014](decisions/0014-source-discovery-and-identity-model.md) for discovery and scoped source identity.
