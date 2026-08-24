# ADR 0011: Operational recovery parameter policy

- Status: Accepted
- Date: 2026-08-24

## Decision

Future automatic recovery uses one finite automatic-recovery-attempt budget per recovery episode, stable failure-class dispositions, mandatory cooldown, bounded backoff, explicit jitter policy, and evidence-gated reset. The budget is shared across eligible failure classes so changing failure classification cannot evade the episode limit. Failure classes control whether recovery is prohibited, directly eligible, or eligible only after a typed precondition changes; they do not receive independent numeric budgets in the current model.

The preferred production backoff direction is capped exponential growth for repeated eligible failures. Fixed and linear strategies remain valid configuration representations for an evidence-backed operational profile, but they are not implicit fallbacks. An enabled production profile should require bounded jitter for time-based retries unless deployment evidence demonstrates that synchronization cannot occur and the exception is recorded explicitly.

No production attempt count, cooldown duration, backoff input, delay cap, jitter bound, or stable-run duration is selected by this decision. Those values require representative operational evidence. Until a complete enabled profile has been selected, reviewed, and loaded through a separately approved boundary, the runtime remains on its explicit recovery-disabled configuration.

This decision changes no runtime behavior. It adds no recovery execution, retry loop, clock, timer, sleep, delay calculation, random sampling, reconnect, owner replacement, endpoint watcher, or configuration loading. All recovery parameters remain private to `resonance-agent`; `resonance-core` and `resonance-api` remain unchanged.

## Context

The accepted recovery pipeline is:

```text
CaptureOwner
      |
      v
CaptureSupervisor
      |
      +-- RetryState
      +-- RecoveryConfig
      +-- RecoveryEvidence
      |
      v
RecoveryPolicy
      |
      v
RecoveryDecision
      |
      v
Future Recovery Execution
```

ADR 0008 defines the pure recovery decision boundary, ADR 0009 defines supervisor-owned attempt accounting and recovery episodes, and ADR 0010 defines complete validated immutable recovery configuration. The supervisor now evaluates configuration-bound retry snapshots, but the runtime configuration deliberately disables recovery. The remaining design risk is operational: an enabled profile with an excessive budget, insufficient delay, premature reset, or hidden randomness could cause a recovery storm, while overly conservative limits could leave a transient failure unrecovered.

The repository contains lifecycle and real-device evidence that establishes the capture and policy boundaries, but it does not yet contain representative failure-frequency, outage-duration, retry-success, or multi-instance collision evidence from which safe production numbers can be derived. Test-only values prove structural behavior only. This ADR therefore selects parameter semantics, class treatment, evidence requirements, and fail-closed rules while deferring numeric tuning.

## Retry Budget

### Attempt semantics

A recovery attempt is counted when `CaptureSupervisor` commits to exactly one automatic owner-creation call. The automatic-attempt counter advances immediately before that call, so construction failure, startup failure, absence of `Started`, and later runtime failure all consume one budget unit. Lifecycle facts from the same owner do not consume additional units.

The initial operator-requested start is part of the intent generation's total attempt sequence but does not consume the automatic-recovery budget. Every later supervisor-authorized owner-creation commitment does. Re-evaluation, a cooldown becoming satisfied, a source-availability notification, or a timer event is evidence only and consumes no budget unless execution eventually commits an owner-creation call.

Repeated identical failures consume one unit per committed automatic attempt, exactly like other eligible failures. They do not consume weighted or multiple units because an unmeasured weighting rule would obscure accounting and create another tuning surface. Instead, recurrence increases consecutive-failure pressure, advances the selected backoff progression, and remains visible in bounded history and aggregate counts.

### Budget ownership and scope

`CaptureSupervisor` is the sole owner and mutator of budget state. The configured maximum is a finite count of **automatic recovery attempts started per recovery episode**. It is not a count of failures, policy evaluations, timer firings, or total attempts including the initial explicit start.

One episode-wide budget is selected rather than independent per-class budgets. A shared ceiling is understandable, matches the validated configuration model, and prevents alternating classifications from multiplying the total number of automatic attempts. Stable per-class dispositions and typed prerequisites provide differentiated safety. A later per-class ceiling would require evidence that a class needs a smaller cap than the shared episode limit and a model change that still preserves the shared upper bound.

No default budget exists. A missing, zero, unbounded, overflowed, or otherwise invalid enabled value fails closed. Zero is valid only for the explicit recovery-disabled configuration.

### Exhaustion behavior

When automatic attempts started equals the configured maximum, the episode becomes exhausted before another automatic attempt can be committed. Exhaustion is sticky and records the intent generation, episode, state revision, last attempt identity, failure class, policy reason, configuration identity, limit, and consumed count.

An exhausted episode remains stopped until a valid reset creates a new episode or a new explicit intent generation. Cooldown satisfaction, backoff completion, repeated policy evaluation, process recreation, source availability, a configuration identity change, or a more permissive retry hint cannot revive the exhausted episode. Missing or inconsistent exhaustion evidence also fails closed.

## Cooldown

Cooldown provides a minimum separation between automatic owner-creation commitments. Every enabled automatic-recovery profile requires a nonzero cooldown and bounded backoff; no eligible failure class has an implicit immediate-retry path. Failure-class evidence may extend the wait indefinitely, but cannot waive the minimum cooldown.

`CaptureSupervisor` owns cooldown state and binds it to the current intent generation, recovery episode, state revision, configuration identity, and failure/attempt that required it. Its policy-visible states remain:

- **required/pending:** delay evidence has not established eligibility;
- **satisfied:** matching evidence proves the current requirement was met;
- **invalidated:** intent, episode, configuration, failure pressure, or another bound precondition changed;
- **not required:** valid only when automatic recovery is disabled or no automatic attempt is under consideration.

Cooldown satisfaction requires typed monotonic-time evidence from a future clock/scheduling boundary for the exact pending requirement. Wall-clock movement, timer delivery by itself, or an unbound Boolean is insufficient. Satisfaction permits fresh policy evaluation only; it is not authorization and does not create an owner.

The current model uses one cooldown policy for the enabled profile rather than failure-specific durations. Failure classes differ through their dispositions and additional evidence:

- device unavailability can remain pending beyond cooldown until source availability is proven;
- source reconfiguration can remain pending until a changed precondition is proven;
- resource exhaustion can remain pending until resource-pressure-clear evidence exists;
- repeated startup or other non-retryable failures remain stopped regardless of cooldown.

Failure-specific durations are deferred unless operational evidence shows that class prerequisites plus one bounded delay policy are insufficient. A new failure, explicit stop, new intent, episode reset, or configuration replacement invalidates prior cooldown evidence.

## Backoff

The configuration model supports fixed, linear, and exponential strategies, each with an explicit maximum delay and reset behavior. Future deterministic calculation receives only explicit inputs: the validated configuration, failure class, recovery episode, automatic-attempt and consecutive-failure counts, cooldown minimum, and an optional externally supplied jitter sample. It returns a bounded required delay and calculation evidence; it does not read time, sample entropy, mutate policy state, schedule work, or authorize recovery.

Capped exponential backoff is the preferred production direction because pressure should grow more rapidly when automatic attempts repeatedly fail, while the cap preserves a reviewable maximum recovery interval. The first automatic attempt uses the configured initial stage; each subsequent eligible failed automatic attempt advances the stage within the same episode. Fixed or linear backoff may be selected only when collected evidence demonstrates that they meet recovery-time and storm-prevention objectives for the deployment.

The future calculation contract must define overflow-safe arithmetic, rounding, stage indexing, jitter composition, and the relationship between the cooldown minimum and backoff result. Regardless of the selected formula:

- the required delay is never less than the configured cooldown minimum;
- the final delay never exceeds the configured maximum delay;
- increasing failure pressure cannot reduce the non-jittered delay;
- all calculations are deterministic for identical inputs;
- invalid or unrepresentable inputs fail closed rather than clamp silently;
- a reset changes the episode before the initial stage can be used again.

No calculation formula or numeric input is selected or implemented here.

## Jitter

Jitter exists to prevent synchronized agents, simultaneous host restarts, common endpoint events, or repeated deterministic collision patterns from producing aligned recovery attempts. It is part of delay calculation, not failure classification or retry-state mutation.

The future seam is an agent-internal explicit input:

```text
JitterSource
      |
      v
sample(bound, recovery_context) -> JitterSample
      |
      v
deterministic delay calculation
```

`JitterSource` is owned by future supervisor/execution integration, never by `RecoveryPolicy`. Production may provide entropy; tests provide fixed, boundary, and scripted samples. The sample is bound to configuration identity, intent generation, episode, attempt ordinal, and a configured maximum so it cannot be replayed silently across changed recovery state.

There is no hidden randomness and no implicit jitter default. An enabled configuration explicitly requires or forbids jitter. Required jitter must have a finite bound, and the calculated final delay remains within the configured maximum. The distribution, range interpretation, seeding, sampling lifetime, and additive or symmetric composition are deferred to the deterministic-delay milestone and must be selected from collision and recovery-latency evidence.

## Reset Policy

Failure pressure clears only through one of these typed transitions:

1. **New explicit intent.** Operator or host action creates a new nonzero intent generation and fresh episode. It never reuses an old decision, cooldown, jitter sample, or attempt identity.
2. **Demonstrated stable operation.** Within a continuing intent, a new episode may begin only after continuous frame-delivery evidence proves uninterrupted valid audio for the configured minimum stable duration. Supervisor `Running`, owner construction, successful `start`, or `StreamEvent::Started` alone is insufficient.

Stable-run evidence must bind the active attempt and stream identity, cover the full configured monotonic interval, and contain no terminal event, discontinuity, invalid frame, owner change, or gap in accepted frame progression. A future implementation may derive it from supervisor-owned observation of typed frame/lifecycle facts, but not from logs or a wall-clock timestamp alone.

Explicit stop invalidates recovery state and authorization for the old intent; a later start is a new intent. A source/precondition change may satisfy a class-specific guard but does not refill the shared budget. A configuration change invalidates existing evaluations but does not, by itself, prove recovery or reset an exhausted episode. Process or supervisor restart does not imply reset; persistence semantics remain deferred and must fail closed until defined.

An explicit administrative reset separate from a new intent is not selected. If later required, it must be an authenticated, audited state transition that invalidates all outstanding evidence and decisions; it cannot be modeled as counter mutation or configuration reload.

## Failure Classes

Recovery policy uses stable agent-level classes. Platform-specific errors remain diagnostic inputs to provider-owned normalization and never become configuration keys.

| Failure class | Operational treatment | Automatic-recovery expectation | Required evidence beyond common lifecycle/budget checks |
| --- | --- | --- | --- |
| Device unavailable | Additional evidence required | Wait; eligible only after the selected source is proven available under the current intent. | Typed source-availability evidence bound to source selection and current state. |
| Source reconfiguration | Additional evidence required | Wait; eligible only after a materially changed source/format precondition is proven. | Typed changed-precondition evidence; supported-format evidence where format caused the boundary. |
| Interruption | Retryable | Eligible after cooldown/backoff when the retry hint does not veto it; recurrence advances pressure. | Typed interruption outcome, terminal delivery, joined completion, and resource release. |
| Resource exhaustion | Additional evidence required | Wait; never retry solely because delay elapsed. | Typed resource-pressure-cleared evidence plus cooldown/backoff satisfaction. |
| Unsupported format | Additional evidence required | Remain stopped under unchanged conditions; eligible only after a supported format or source is proven. | Typed supported-format or changed-precondition evidence; never parsed diagnostic text. |
| Startup failure | Non-retryable with current evidence | Remain stopped until a future retry-safe subtype is designed. | Current coarse startup evidence is insufficient. |
| Internal failure | Non-retryable with current evidence | Remain stopped until a future retry-safe subtype is designed. | Current broad internal classification is insufficient. |
| Worker panic | Non-retryable | Remain stopped; require operator investigation and a new explicit intent. | Joined cleanup and human diagnostic evidence do not authorize automatic recovery. |
| Owner construction failure | Non-retryable with current evidence | Remain stopped until a future retry-safe subtype is designed. | Current construction error is too coarse to distinguish transient pressure from defects. |

Every potentially eligible class remains subject to current running intent, exact configuration identity, complete terminal/cleanup evidence, no active owner, remaining shared budget, current episode/revision, satisfied cooldown/backoff, retry-hint constraints, and one-shot decision revalidation. Missing, stale, contradictory, or diagnostic-only evidence remains stopped.

## Evidence Requirements

Future policy evaluation and execution must distinguish three evidence sets:

1. **Common attempt evidence:** current running intent; matching intent generation, episode, revision, configuration identity, and prior attempt identity; typed outcome and retry hint; terminal delivery when a stream existed; joined completion and resource release; no active owner; remaining budget; and a fresh one-shot decision.
2. **Eligibility evidence:** satisfied cooldown/backoff bound to the current failure plus the class-specific availability, changed-precondition, supported-format, or pressure-clear evidence identified above. Elapsed time never substitutes for a changed precondition.
3. **Reset evidence:** a new explicit intent or continuous valid frame delivery for the complete configured stable-run interval, bound to one active attempt and stream.

Evidence must be typed, immutable for one evaluation, attributable to its producer, and bound to the state revision that consumed it. Logs, display text, backend messages, retry hints alone, unbound Booleans, wall-clock timestamps, and stale timer or watcher notifications are not authorization evidence. Conflicting evidence fails closed and produces a reasoned stopped decision.

## Operational Observability

Future operators need one coherent recovery snapshot and reasoned transition history inside `resonance-agent`. At minimum it must make these facts inspectable without parsing prose:

- current desired-running intent and intent generation;
- current recovery episode, state revision, configuration version/fingerprint, and policy decision reason;
- current and prior attempt identities, total attempts started, automatic attempts consumed, configured limit, and remaining attempts;
- consecutive eligible failures, bounded recent typed history, aggregate class counts, and last failure class/outcome;
- class disposition, missing or satisfied additional evidence, and retry-hint vetoes;
- cooldown state, the attempt that established it, invalidation reason, and future monotonic eligibility/deadline evidence;
- selected backoff strategy and stage, deterministic pre-jitter delay, supplied jitter evidence, final bounded delay, and next eligible recovery time when calculation exists;
- exhaustion state and reason, including the evidence that made it sticky;
- stable-run progress and the typed reset evidence that began a new episode;
- stale-decision rejection, explicit stop, and other reasons an otherwise eligible attempt was not executed.

These are agent-internal operational facts, not audio data and not a consumer API. Human-readable logs may render them, but stable typed reasons remain authoritative. Telemetry, storage, transport, retention, and presentation are deferred.

Before numeric values can be approved, evidence must cover representative hosts, supported backends, expected simultaneous instance counts, and each enabled failure class. The evidence set must include failure frequency and sequences, outage or pressure duration, success probability by automatic-attempt ordinal, time to stable frame delivery, recurrence after apparent recovery, operator-visible outage cost, resource impact during attempts, and any synchronized-event patterns. Selection must document the reliability objective and show how the proposed budget, delay cap, jitter bound, and stable-run threshold satisfy it. Absence of evidence preserves recovery-disabled behavior.

## Alternatives Considered

### One global retry budget

Selected as one shared per-episode automatic-attempt budget. It provides an auditable hard ceiling across every eligible failure class and matches the current validated configuration model. Its limitation is that one class can consume the episode's remaining allowance; per-class evidence gates and backoff make that preferable to allowing classifications to multiply the ceiling.

### Per-error budgets

Rejected. Native error codes are unstable policy keys, and separate counters can let alternating failures exceed the intended total pressure. Stable-class sublimits could be added beneath the shared ceiling later, but only with evidence and an explicit model revision.

### No automatic retry

Retained as the current runtime posture and as a valid operational profile. It is safest against storms and unknown failure modes, but cannot restore service from known transient failures without explicit action. It remains mandatory whenever an enabled profile is absent, incomplete, invalid, or unsupported by evidence.

### Unlimited retry

Rejected. Cooldown and backoff reduce rate but do not bound cumulative resource use or permanent failure loops. Unlimited retry also makes exhaustion unobservable and prevents a deterministic fail-closed terminal state.

## Consequences

Benefits:

- one shared finite budget bounds cumulative automatic owner creation across changing failure classes;
- class-specific dispositions preserve safety without duplicating platform-specific errors or adding multiple numeric budgets;
- mandatory cooldown, growing bounded pressure, and explicit jitter address rapid and synchronized recovery storms;
- reset requires sustained audio delivery rather than optimistic lifecycle events;
- explicit typed inputs make future calculation, sampling, policy, and observability independently testable;
- missing configuration or operational evidence preserves the recovery-disabled posture;
- recovery internals remain private to `resonance-agent` and consumer contracts do not change.

Limitations and operational impact:

- no availability improvement occurs until separately approved configuration, calculation, scheduling, and execution work exists;
- a shared episode budget may stop recovery after mixed transient failures, requiring explicit operator action;
- conservative evidence gates can extend outages when availability, format, or pressure-clear evidence is unavailable;
- no production timing or budget values are yet justified;
- process restart and persistence behavior remain unresolved, so restart cannot safely be treated as a reset;
- future operators will need typed recovery-state visibility before execution can be accepted.

## Deferred Decisions

- exact automatic-attempt budget and any evidence-backed stable-class sublimits;
- cooldown minimum, backoff inputs, growth multiplier, maximum delay, arithmetic, rounding, and stage indexing;
- jitter distribution, range interpretation, composition, entropy source, seeding, and sampling lifetime;
- stable-run minimum duration and the exact observation component that produces continuous-delivery evidence;
- configuration source, loading, schema, provenance, runtime reload, and activation gates;
- retry-state, exhaustion, cooldown, and history persistence across supervisor or process restart;
- deterministic delay-calculation implementation and tests;
- clocks, timers, scheduling, telemetry, retention, and next-eligible-time presentation;
- source availability, endpoint watching, default-device following, and typed retry-safe startup/internal/construction subtypes;
- retry authorization consumption, owner replacement, reconnect, and all recovery execution.
