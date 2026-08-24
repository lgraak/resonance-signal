# ADR 0008: Recovery decision policy boundary

- Status: Accepted
- Date: 2026-08-23

## Decision

Recovery remains a `CaptureSupervisor` responsibility above the single-use `CaptureOwner`. The supervisor owns capture intent, mutable attempt and retry state, policy application, and enforcement. `CaptureOwner` only reports how one capture lifetime ended and completes cleanup; it never decides whether another owner should exist.

Define `RecoveryPolicy` as a deterministic, side-effect-free decision boundary inside `resonance-agent`. It evaluates a structured snapshot of intent, terminal outcome, completion, resource release, source-selection policy, and future retry state. It returns one of three decision classes:

- **remain stopped:** automatic recovery is not authorized;
- **wait:** recovery remains potentially allowed, but a policy-owned precondition such as a future retry deadline, cooldown, source-availability signal, or operator action has not been satisfied;
- **permit replacement:** the policy authorizes the supervisor to create one new owner after the supervisor revalidates current intent and ownership state.

A policy decision is authorization, not execution. `RecoveryPolicy` does not create owners, wait, sleep, register endpoint notifications, or mutate attempt state. `CaptureSupervisor` retains those orchestration responsibilities and must recheck that the decision still belongs to the current capture-intent generation before acting on it.

The initial future implementation may express this boundary as an internal pure function rather than a public trait or independently configurable component. The semantic boundary is required now; runtime polymorphism is not. This avoids premature abstraction while keeping the outcome matrix independently testable and preventing policy branches from becoming implicit supervisor control flow.

No recovery behavior is implemented by this decision.

## Context

`CaptureOwner` owns exactly one capture lifetime. It owns the ordinary worker, callback lifetime, stop token, completion receiver, nested WASAPI-thread join obligation, and final typed completion. A started owner cannot be restarted. Completion means its callbacks have ended and its native resources have been released.

`CaptureSupervisor` owns capture intent above that resource boundary. The current recovery-disabled implementation creates at most one owner, forwards lifecycle events, observes typed terminal information and joined completion, and records the mechanical boundary at which a replacement could be considered. It does not consume that eligibility or create a replacement.

The current contract provides useful but deliberately limited machine evidence: `ErrorKind`, `RetryHint`, `StreamEndReason`, `CaptureEnd`, `CaptureOwnerCompletion`, desired-running intent, and resource-release state. A retry hint is advice from the failing layer, not sufficient authorization. Diagnostic messages and backend text are not stable policy inputs.

Recovery needs a defined policy boundary because terminal conditions have different safety properties. Endpoint loss may be temporary; unsupported input is stable under unchanged conditions; resource exhaustion can be amplified by a tight loop; and a worker panic may indicate an invariant failure. Treating all of them as equivalent retries would obscure explicit stop intent and make capture behavior difficult to bound or explain.

## Recovery Ownership

### `CaptureOwner`

`CaptureOwner` owns and reports one attempt:

- native resources and one uninterrupted stream lifetime;
- lifecycle event emission and typed completion;
- stop observation, cleanup, and joins;
- provider-owned classification of platform outcomes into structured categories.

It does not track attempts, retain retry state, interpret retry hints as commands, calculate delays, resolve replacement endpoints, or create another owner.

### `CaptureSupervisor`

`CaptureSupervisor` owns recovery orchestration:

- desired running or stopped intent and its generation;
- invalidating pending recovery before requesting an explicit stop;
- the current owner or attempt identity and the no-overlap invariant;
- attempt history, consecutive-failure count, cooldown and retry state;
- calling the policy with a coherent structured snapshot;
- retaining and applying the policy decision;
- deciding when configured limits are exhausted;
- revalidating intent and evidence before any future owner creation;
- recording the applied decision for diagnostics and evidence.

The supervisor may delegate scheduling to a future clock or host integration, but it remains responsible for the resulting state transition. A timer firing is only new evidence for another policy evaluation; it is not permission to start capture.

### `RecoveryPolicy`

`RecoveryPolicy` owns classification and authorization, not lifecycle:

- map a coherent outcome and current state to remain-stopped, wait, or permit-replacement;
- apply configured attempt, cooldown, and reset rules;
- require evidence appropriate to the outcome;
- produce a stable decision reason suitable for tests and diagnostics.

The policy must be platform-capture-private to `resonance-agent`. Windows-specific cause types must not enter `resonance-core` or `resonance-api`. Provider-owned adapters may map native evidence into agent-internal structured causes before evaluation; the policy must never parse human-readable messages.

## Recovery Decision Model

### Evaluation order

The supervisor and policy use this precedence:

1. If desired-running intent is false, or the decision belongs to an obsolete intent generation, remain stopped regardless of the terminal outcome or retry hint.
2. If a started stream has not delivered its terminal boundary, completed, and released its resources, do not authorize a replacement.
3. If an attempt failed before a stream existed, require joined completion and proof that no attempt resources remain. An `Ended` event is not required because no stream started.
4. If required structured evidence is missing, inconsistent, or only available as diagnostic text, remain stopped and record the evidence gap.
5. Apply outcome-specific rules, retry-hint constraints, configured limits, cooldowns, and reset rules.
6. Immediately before future owner creation, recheck desired-running intent, intent generation, attempt identity, resource release, and the continuing validity of any source or time precondition.

`RetryHint::DoNotRetry` is a veto for automatic recovery under the unchanged intent. Other hints narrow the permitted decision but do not grant it: `WaitForSource` cannot produce an immediate replacement, `ChangeFormat` requires a changed precondition, and `RetryNow` remains subject to limits and intent.

### Terminal outcome decision table

| Terminal outcome | Potentially recoverable? | Why | Required conditions | Required structured evidence | Future policy inputs |
| --- | --- | --- | --- | --- | --- |
| Explicit user stop | No | Stop is authoritative intent, not a failure. | None. Invalidate pending decisions before requesting owner stop. | Current intent generation marked stopped; joined completion is still required for cleanup. | None; attempt counters may be retained only for diagnostics. |
| Normal shutdown | No | Service shutdown, provider shutdown, and a deliberately bounded run must remain stopped. | A later explicit start creates new intent rather than resuming recovery. | Stopped intent plus `ProviderShutdown`, `ConsumerCancelled`, `StopRequested`, or equivalent host shutdown evidence. | Future service-lifecycle mode, but never an automatic retry within the stopped intent. |
| Device removed | Conditional: wait | The selected source may return, but repeated immediate opens cannot establish availability. | Running intent remains active; prior owner is released; source-selection policy still permits that source. | `SourceUnavailable` / `SourceEnded`, `WaitForSource`, and a provider-owned structured removal or unavailable cause when available. | Source availability, source identity, wait/cooldown state, attempt limit, elapsed outage. |
| Device invalidated | Conditional: wait | Invalidation may be transient or may require resolving a fresh endpoint object. | Same as device removal; a future attempt must resolve the source again. | Typed unavailable outcome plus a provider-owned mapped invalidation cause; never a parsed backend message. | Availability evidence, invalidation recurrence, source-selection mode, cooldown and limit. |
| Default endpoint changed | Conditional | Following the replacement is valid only when the capture intent explicitly selects the current default, not a pinned endpoint. | Prior owner released; running intent current; future default-following policy enabled; new endpoint resolved after the terminal boundary. | `SourceReconfigured`, compatible retry hint, source-selection mode, and future structured old/new endpoint evidence. | Follow-default setting, endpoint identity, replacement acceptance, attempt history. |
| Format change | Conditional | A new owner may renegotiate, but the existing stream cannot change format in place. | Fresh negotiation is permitted and the source can produce a supported mono or stereo representation. | `SourceReconfigured` or structured format-change cause; later accepted format evidence belongs to the new attempt. | Reconfiguration limit, source identity, prior format, format stability/cooldown. |
| Interruption | Conditional: retry later | Some session or timing interruptions are transient; others indicate a recurring fault. | Running intent current; prior owner released; hint is not `DoNotRetry`; retry budget remains. | `StreamInterrupted` / `Failed`, retry hint, `Interrupted` or `DataDiscontinuity`, and structured cause when policy needs finer treatment. | Consecutive interruptions, recent stability, delay/cooldown, attempt limit. |
| Resource exhaustion | Conditional and guarded | A new attempt may succeed after pressure clears, but immediate retries can amplify overload. | Non-zero cooldown or other pressure-clear evidence; bounded attempts; running intent current. | `ResourceExhausted`, `RetryLater`, joined completion, and release evidence. | Recurrence window, cooldown, attempt cap, system-pressure signal, stable-run reset rule. |
| Unsupported format | Not under unchanged conditions | Repeating the same request against the same format is deterministic and cannot recover by timing alone. | Only a changed source, changed format, or separately approved conversion/configuration can create a new precondition. | `UnsupportedFormat` with `ChangeFormat` or `DoNotRetry`; requested and observed format as structured evidence when available. | Intent/config generation, source/format change evidence; no ordinary retry counter. |
| Internal failure | Conditional only with a retry-safe subtype | The category spans transient backend failures and invariant, contract, or protocol failures. The broad category alone is insufficient. | A structured subtype explicitly permits retry, hint is not `DoNotRetry`, resources are released, and a small bounded budget remains. | `Internal`, retry hint, joined completion, and a future structured retry-safe cause. Without that cause, remain stopped. | Subtype, recurrence, attempt cap, cooldown, stable-run reset, software version. |
| Owner startup failure | Conditional only with richer evidence | OS resource pressure may be transient, but the current `StartFailed` summary does not preserve a retry-safe cause. | No stream was created; joined completion proves no resources remain; future typed cause permits retry. | `StartFailed` plus a future structured OS/startup cause. Current coarse evidence is insufficient, so remain stopped. | Startup-failure subtype, attempt cap, cooldown, process pressure. |
| Worker panic | No automatic recovery | A panic may represent an invariant breach; restarting can hide or repeat corruption and currently has no terminal stream evidence. | Operator investigation and a later explicit start only. | `Panicked`, joined unwind/cleanup evidence, logs and panic evidence for humans. | No retry state; diagnostic occurrence count may be recorded. |

The table describes permission boundaries, not a complete taxonomy of native errors. Provider-owned platform code remains responsible for reliable mapping. If future experience shows that one row contains outcomes with materially different safety properties, add an agent-internal structured subtype and update this decision before enabling automatic behavior.

### Explicit intent and late events

An explicit stop is processed in this order:

```text
running intent
      |
      v
mark intent stopped and invalidate its generation
      |
      v
discard pending recovery authorization
      |
      v
request owner stop and join cleanup
      |
      v
remain stopped
```

Terminal events and completion that arrive after the stop request still belong to the old owner. They are forwarded in contract order, retained as evidence, and used to prove cleanup, but cannot restore desired-running intent or authorize recovery. A queued timer, availability notification, or previously computed decision is handled the same way: if its intent generation is no longer current and running, it is stale.

A later explicit start is a new capture intent with a new generation. It does not consume a stale recovery decision and is not counted as continuation of the stopped attempt chain unless a future service-lifecycle decision explicitly defines otherwise. With the current single-use supervisor, that later start may require a new supervisor instance; changing supervisor service lifecycle remains deferred.

## Retry Policy Boundary

Future retry policy owns rules, not mechanisms. Its configuration and state may include:

- maximum consecutive attempts and outcome-specific attempt budgets;
- immediate versus delayed eligibility;
- base delay, maximum delay, exponential or other backoff progression;
- jitter and its deterministic test seam;
- cooldowns for overload, rapid flapping, or repeated reconfiguration;
- source-availability or changed-precondition requirements;
- reset after a sufficiently stable run, explicit new intent, source change, or configuration change;
- whether attempt history survives supervisor recreation or process restart;
- an eligibility deadline and stable reason for remain-stopped or wait decisions.

The supervisor owns the mutable values that those rules examine. A future scheduler or endpoint watcher only supplies elapsed-time or availability evidence to the supervisor. It does not own policy, mutate intent, or create an owner directly.

No retry counts, delay values, backoff algorithm, jitter distribution, cooldown duration, or persistence rule are selected here. Those values require operational evidence and a separate implementation milestone.

## Stream Lifecycle

Recovery never changes the identity of an ended stream or resembles pause/resume. Every replacement capture:

- creates a new `CaptureOwner` and a new `StreamId`;
- starts its accepted frame index at zero;
- starts stream-relative time at zero;
- negotiates and emits its own `StreamDescriptor` and `Started` event;
- leaves the prior stream's `Error` when applicable and `Ended` event visible;
- does not offset timestamps, continue indexes, reuse descriptors, or synthesize continuity.

No replacement may begin until the prior owner has completed and released all resources. A startup failure that occurs before `Started` creates no consumer-visible stream to end; a later successful attempt still starts a wholly new stream.

## Consumer Visibility

Consumers observe capture facts, not supervisor policy internals.

Machine-actionable consumer-visible information remains:

- ordered `StreamEvent` lifecycle boundaries;
- platform-neutral `ErrorKind` and `ErrorScope`;
- advisory `RetryHint`;
- `StreamEndReason`;
- independent stream descriptors and identities for later streams.

Human diagnostics remain:

- `ProviderError::message` and display text;
- backend messages and stable native-code evidence;
- logs, panic reports, measurement summaries, and evidence reports;
- policy-decision explanations intended for operators.

Consumers must not parse diagnostics, infer attempt counts, reproduce the decision table, or treat a retry hint as a command. The provider may later expose recovery status through a separately designed platform-neutral orchestration contract, but it must not overload stream lifecycle or leak Windows recovery types into `resonance-api`.

## Alternatives Considered

### 1. `CaptureOwner` handles recovery

Rejected. It combines one-lifetime native-resource ownership with retry scheduling, endpoint selection, and service intent. Completion and drop semantics would become ambiguous, and it would be easier to accidentally reuse stream identity or overlap native resources.

### 2. `CaptureSupervisor` handles everything

Viable for a very small fixed matrix, and it avoids an additional named abstraction. It is not selected as the durable design because intent transitions, owner lifecycle, scheduling, outcome classification, and retry limits would become interleaved. Testing a decision would require driving orchestration state, and policy changes would be harder to review independently.

The future implementation may still begin with a private pure function inside the supervisor module. That is considered an implementation form of the `RecoveryPolicy` boundary, not permission to mix side effects into policy evaluation.

### 3. Separate `RecoveryPolicy` component

Selected as a semantic boundary. A deterministic evaluator makes the decision table directly testable, keeps timers and owner creation outside policy, and allows configuration to evolve without altering native-resource ownership. The costs are another vocabulary type and the risk of premature runtime polymorphism. Keeping it private to `resonance-agent` and not requiring a trait or dynamic dispatch addresses that cost.

## Consequences

Benefits:

- explicit stop and service shutdown have absolute precedence over recovery hints;
- each automatic action can be traced to typed evidence and a stable policy reason;
- retry state is bounded and has one owner;
- weak or missing classification fails closed instead of creating an unbounded loop;
- startup failure, runtime termination, and panic are not incorrectly flattened into one path;
- future policy tests can be hardware-independent and table-driven;
- platform-specific recovery details remain inside `resonance-agent`;
- consumer-visible stream boundaries and identities remain honest.

Limitations:

- this decision does not improve availability or create any replacement owner;
- some current completion summaries are too coarse to authorize safe retries;
- source availability, endpoint identity changes, and retry-safe internal subtypes need additional structured evidence before corresponding policy branches can be enabled;
- a future service must coordinate policy evaluation, clocks or notifications, shutdown, and configuration without weakening the intent-generation check;
- consumers may observe gaps between independent streams without provider-level recovery progress events.

Future implementation must preserve a pure decision seam, test every table row and precedence rule, and separately test supervisor enforcement against stop races, stale timers or notifications, attempt exhaustion, and no-overlap ownership.

## Deferred Decisions

- retry timing and concrete delay values;
- retry limits and outcome-specific budgets;
- backoff algorithm and maximum delay;
- jitter strategy and entropy source;
- cooldown durations and reset thresholds;
- persistence of retry history across process or supervisor lifetime;
- endpoint watching and source-availability detection;
- default-device following and endpoint-replacement acceptance;
- typed native-cause extensions needed for guarded recovery cases;
- service lifecycle, later explicit-start mechanics, and shutdown deadlines;
- transport or API exposure of recovery status;
- reconnect, replacement owner creation, and runtime acceptance testing.
