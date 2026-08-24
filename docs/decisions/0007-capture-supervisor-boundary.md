# ADR 0007: Capture supervisor boundary

- Status: Accepted
- Date: 2026-08-23

## Decision

Add a `CaptureSupervisor` boundary in `resonance-agent` above the existing single-use `CaptureOwner`. The supervisor is the recovery-policy owner. It creates and owns one `CaptureOwner` at a time for a supervised capture intent, observes that owner's typed lifecycle events and completion, and decides whether capture remains stopped or a later owner may be created.

`CaptureOwner` remains the resource owner for exactly one capture lifetime. Recovery never reopens or continues that lifetime. A replacement owner starts a new stream with a new `StreamId`, frame index zero, and stream time zero.

This decision defines responsibilities only. It does not implement the supervisor, reconnect, retry loops, endpoint following, or any capture behavior.

## Context

The production Windows boundary has a single-use `CaptureOwner`. Construction is inert; `start` may be called once; stop is explicit and idempotent; shutdown waits for joined completion while retaining ownership after a timeout. The owner worker runs provider callbacks and joins the nested WASAPI thread, while the WASAPI thread alone owns COM and endpoint resources. The owner retains a typed `CaptureOwnerCompletion` after termination.

This model deliberately excludes recovery. Endpoint removal, default-endpoint replacement, format change, interruption, or internal failure ends the current uninterrupted stream and releases its resources. Calling `start` again on the same owner is invalid.

A higher boundary is therefore required before reconnect behavior can be designed. Without it, retry and device-following policy would either leak into the single-run resource state machine or be duplicated by every application. Both outcomes would blur resource ownership, policy ownership, and consumer-visible stream boundaries.

The supervisor is platform-capture orchestration and remains in `resonance-agent`. It does not add Windows lifecycle types to `resonance-core` or `resonance-api`, and it does not change the rule that Resonance Signal provides audio data while consumers decide how to use it.

## Responsibilities

### `CaptureOwner`

`CaptureOwner` owns:

- one and only one capture lifetime;
- its stop token, ordinary worker, completion receiver, callback lifetime, and final completion state;
- the join obligation that ensures the nested WASAPI thread and its thread-affine resources are released;
- clean shutdown, including stop-before-start, idempotent stop requests, bounded waits, retrying a timed-out wait, and final drop cleanup;
- emission of the existing ordered `StreamEvent` lifecycle for a run that starts.

`CaptureOwner` does not:

- select retry policy or interpret a retry hint as a command;
- wait for a source, calculate backoff, or schedule another attempt;
- restart itself or create another owner;
- follow the default device or accept an endpoint replacement;
- preserve a stream identity across interruption;
- own service lifetime or transport behavior.

### `CaptureSupervisor`

`CaptureSupervisor` owns:

- creating, starting, stopping, and retaining the current `CaptureOwner`;
- ensuring a replacement is not created until the prior owner has completed and released its resources;
- observing the owner's typed events and `CaptureOwnerCompletion` without parsing diagnostic text;
- the decision to stop, wait, or create a new owner;
- retry-policy application, including whether an outcome is eligible for another attempt;
- backoff state and attempt history when a future policy enables retries;
- endpoint-replacement acceptance and default-device-following policy;
- recovery state for the supervised capture intent, including suppression of recovery after an explicit stop or service shutdown;
- forwarding every owner lifecycle event to consumers without hiding terminal boundaries;
- recording human diagnostics, logs, and evidence separately from machine-actionable state.

The supervisor owns policy, not native capture resources. The active `CaptureOwner` and its WASAPI thread retain their existing resource ownership. Platform notification callbacks continue only to classify and terminate the current run; they do not start another run.

Retry hints are inputs to supervisor policy rather than executable instructions. The supervisor must combine the typed error category, retry hint, stream end reason, owner completion, desired running state, and future configured policy before deciding what happens next. A normal explicit stop must not be mistaken for a recoverable interruption.

At this boundary, default-device following means deciding whether a later owner should resolve the then-current default endpoint. Detection of the old endpoint's termination remains in the Windows adapter. The exact policy and discovery mechanism are deferred.

## Recovery Model

Future recovery follows a sequence with no overlapping capture lifetimes:

```text
CaptureOwner emits terminal lifecycle event
        |
        v
CaptureOwner completes and releases all resources
        |
        v
CaptureSupervisor evaluates typed outcome + desired state + policy
        |
        +--> remain stopped
        |
        +--> enter a policy-owned wait/backoff state
        |
        +--> create and start a new CaptureOwner
```

The supervisor evaluates recovery only after joined owner completion. This prevents a replacement WASAPI lifetime from racing the endpoint resources and callbacks of the prior lifetime. An explicit supervisor stop first disables further recovery, then requests and joins shutdown of the current owner.

The design permits future event-driven source availability, scheduled retry, or operator-triggered restart. It does not choose among them. It also does not require every failure to be retried: invalid requests, unsupported formats, explicit shutdown, exhausted policy, or other future policy outcomes may remain stopped.

### Device recovery cases

All device cases end the current stream before any recovery decision:

- **Default playback endpoint removed:** the owner reports source unavailability and ends the stream. The supervisor may remain stopped or wait for a source according to future policy, then create a new owner if capture is still desired.
- **Default endpoint changed:** the owner reports source reconfiguration and ends the stream. A future default-following policy decides whether to resolve the new default endpoint with a new owner; the existing owner never switches endpoints.
- **Device disabled:** treat the current source as unavailable. Re-enablement detection and retry timing are deferred; any later attempt uses a new owner.
- **Format changed:** end the stream as reconfigured. A later owner must negotiate and validate the format again. Failure to obtain a supported mono or stereo representation remains an explicit unsupported-format outcome, not an in-stream conversion change.
- **Temporary interruption:** end the stream explicitly. Future policy may permit a new attempt after a wait, but the interruption is never presented as a pause and resume of the old stream.

These are policy classifications, not retry-loop or device-watcher implementation requirements.

## Stream Lifecycle

Each `CaptureOwner` represents at most one uninterrupted stream. If the supervisor creates a replacement owner:

- the replacement receives a new `StreamId`;
- its first accepted frame has frame index zero;
- its stream-relative time starts at zero;
- it emits its own `StreamEvent::Started` only after successful initialization;
- consumers observe the prior stream's terminal `Error` when applicable and `Ended` before any later stream starts.

The supervisor must not suppress the old terminal events, reuse its `StreamId`, continue its frame index, offset the new timeline, or synthesize continuity across attempts. Recovery state belongs to the supervisor and is distinct from stream state.

## Error and Event Flow

The `CaptureOwner` event callback enters the supervisor boundary. The supervisor observes typed lifecycle information and forwards the existing events unchanged and in order to the consumer-facing provider path. Once the callback has delivered the terminal event and the owner worker has completed, the supervisor receives `CaptureOwnerCompletion` and makes the recovery decision.

Startup failures that occur before a stream exists may have no `Started` or `Ended` pair. The supervisor evaluates their typed start/completion failure instead and must not invent a stream identity. A later successful owner still begins an independent stream.

Machine-actionable inputs are:

- `ErrorKind` and `ErrorScope`;
- `RetryHint`;
- `StreamEndReason` and the agent-owned `CaptureEnd` classification;
- `CaptureOwnerCompletion` and explicit desired-running/stopping state;
- future structured policy state such as attempt count or eligibility deadline.

Human diagnostics are:

- `ProviderError::message` and `Display` text;
- backend messages and stable error-code evidence when available;
- logs, diagnostic reports, and console/evidence output.

The supervisor must not parse human text to decide recovery. Consumers continue to observe provider lifecycle and data events. They may observe one stream end followed later by a separate stream start; they do not observe a false continuation or need Windows-specific recovery types.

## Alternatives Considered

### `CaptureOwner` handles everything

This would put single-run resource ownership, retry scheduling, device watching, replacement acceptance, and service policy into one state machine. It could hide reconnect from callers, but it would make owner completion ambiguous, complicate shutdown and drop semantics, and encourage reuse of an ended stream identity. It is rejected because recovery policy and one-lifetime resource cleanup have different responsibilities.

### Supervisor above `CaptureOwner`

This is the selected design. It preserves the validated owner as a small, single-use resource boundary while giving restart decisions, backoff, endpoint following, and recovery state one explicit home. It also makes each attempt independently testable and keeps stream boundaries visible. The cost is an additional orchestration component and the need to define its policy and service integration before reconnect can be implemented.

### External application owns recovery

An external application could create owners and implement its own retry behavior. This keeps the provider smaller, but it duplicates platform-capture recovery across applications, exposes agent lifecycle details to consumers, and makes consistent stream-boundary and shutdown behavior harder to enforce. It is rejected for capture recovery. External applications still own how they consume audio, and a future host process still owns whether the supervisor itself should be running.

## Consequences

Benefits:

- resource ownership and recovery-policy ownership remain separate and testable;
- no retry loop or device-following behavior is hidden inside capture;
- explicit stop can reliably suppress recovery before owner shutdown;
- every recovery attempt preserves honest consumer-visible stream identity;
- Windows-specific capture orchestration remains in `resonance-agent`;
- future policy can evolve without changing `resonance-core` or `resonance-api` lifecycle types.

Limitations:

- this milestone provides no reconnect or availability improvement;
- a future implementation must coordinate terminal events, joined completion, timers or notifications, and service shutdown;
- existing retry hints are guidance and do not by themselves define safe timing or attempt limits;
- consumers may experience gaps between independently identified streams.

Future implementation must introduce the supervisor as a narrow orchestration layer around, rather than inside, `CaptureOwner`. Tests will need to prove no overlapping owners, stop-suppresses-recovery behavior, typed outcome decisions, bounded policy state, and new identity/timeline semantics for every replacement.

## Deferred Decisions

- retry timing and attempt limits;
- backoff algorithm, jitter, reset conditions, and persistence;
- which outcomes are retryable under which policy;
- default-device following policy and endpoint-replacement acceptance criteria;
- source-availability detection or polling mechanism;
- supervisor public API and concrete recovery-state representation;
- service lifetime, startup mode, and shutdown deadline;
- transport behavior and whether recovery state is exposed beyond existing stream events;
- configuration ownership and operational evidence format.
