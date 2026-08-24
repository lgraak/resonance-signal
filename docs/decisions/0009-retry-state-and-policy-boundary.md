# ADR 0009: Retry state and recovery policy configuration boundary

- Status: Accepted
- Date: 2026-08-23

## Decision

`CaptureSupervisor` owns all mutable retry state for one capture-intent generation. `RecoveryPolicy` evaluates an immutable snapshot of that state, the applicable immutable policy configuration, and typed lifecycle evidence. Future recovery execution remains a separate supervisor responsibility and may create at most one new `CaptureOwner` from a still-current authorization.

Retry state is private to `resonance-agent`. It is not part of the audio-data contract and must not be exposed through `resonance-core` or `resonance-api`.

An **attempt** begins when the supervisor commits to exactly one owner-creation call. The supervisor allocates an attempt identity and increments the attempts-started count immediately before invoking the owner factory. The attempt therefore counts even if owner construction fails, `start` fails, no stream emits `Started`, or the worker later terminates. Successful owner construction, successful `start`, `StreamEvent::Started`, and terminal failure are facts about that same attempt; none creates an additional attempt.

The initial explicit-start attempt and automatic recovery attempts share one monotonic attempt sequence within an intent generation, but the automatic recovery budget is tracked separately. This avoids making a configured maximum ambiguous about whether it includes the initial attempt. Concrete limits and timing values remain deferred.

No retry timer, wait, reconnect, replacement owner, endpoint watcher, or default-device-following behavior is implemented by this decision.

## Context

`CaptureOwner` owns exactly one capture lifetime. It owns its worker and callback lifetime, reports typed events and completion, releases its native resources, and cannot be restarted.

`CaptureSupervisor` owns the desired running or stopped intent above that one-lifetime boundary. It owns the current owner, observes terminal evidence and joined completion, and prevents overlapping capture lifetimes. An explicit stop clears running intent before owner shutdown so late events cannot authorize recovery.

`RecoveryPolicy` is the side-effect-free decision boundary defined by ADR 0008. It maps coherent structured evidence to remain stopped, wait, or permit replacement. The current representation can accept retry-budget and cooldown facts, but it deliberately does not define where those facts come from or how they evolve.

Without an explicit retry-state model, an implementation could increment counters at different lifecycle points, reset on a momentary `Started` event, reuse stale decisions, or retry repeatedly from the same failure evidence. Those ambiguities can create overlapping owners, conceal exhaustion, or produce a recovery storm. The state and configuration boundaries must therefore be defined before recovery execution.

## Ownership

### `CaptureOwner`

`CaptureOwner` owns only one attempt's resources and lifecycle facts:

- owner construction/start result;
- whether a stream emitted `Started`;
- ordered terminal events when a stream existed;
- typed completion and resource release.

It does not retain cross-attempt history, calculate delays, consume a retry budget, declare exhaustion, or reset recovery state.

### `CaptureSupervisor`

`CaptureSupervisor` owns the mutable state for the current intent generation:

- the monotonic attempt sequence and current attempt identity;
- the separately named count of automatic recovery attempts started;
- the most recent typed failure record;
- bounded retry history and aggregate failure counts;
- consecutive-failure and recovery-episode state;
- cooldown status and its eligibility marker;
- retry-budget exhaustion and the reason it became exhausted;
- the state revision used to invalidate previously evaluated snapshots;
- the policy-configuration identity applied to the intent.

History must be bounded. The durable model is a bounded collection of recent typed attempt summaries plus aggregate counts, not an unbounded event log and not human-readable diagnostic text. The concrete history capacity is deferred.

The supervisor is the only component that may mutate this state. A future clock, scheduler, endpoint watcher, or host integration may supply evidence, but it may not increment attempts, clear exhaustion, restore intent, or create an owner directly.

### `RecoveryPolicy`

`RecoveryPolicy` evaluates:

- a read-only retry-state snapshot;
- a read-only configuration snapshot;
- the current intent generation, recovery episode, and state revision;
- the completed attempt's typed outcome and release evidence;
- outcome-specific source, format, pressure, or availability evidence.

It returns a reasoned decision and, when waiting is permitted, a delay or changed-precondition requirement. It does not mutate the snapshot, read a clock, generate randomness, sleep, schedule work, or create an owner.

### Future recovery execution

Future execution remains inside `CaptureSupervisor`. Immediately before creating an owner it must atomically revalidate the running intent, intent generation, recovery episode, state revision, attempt identity, absence of an active owner, resource release, budget, cooldown, and any source precondition. It then consumes that authorization by advancing state and starting at most one attempt. Re-evaluating or replaying the same authorization cannot start a second attempt.

## Attempt Model

Each explicit start creates a new nonzero intent generation. Within that generation, attempts receive monotonically increasing ordinals. A stable identity is the pair `(intent_generation, attempt_ordinal)`; future recovery decisions additionally bind to the recovery episode and state revision from which they were evaluated.

The attempt lifecycle is:

```text
supervisor commits to owner creation
        |
        +-- allocate attempt identity
        +-- increment attempts started
        +-- if automatic, increment recovery attempts started
        |
        v
owner creation and start
        |
        +-- may fail before a stream exists
        +-- may emit Started and run
        |
        v
typed terminal/completion evidence
        |
        v
joined completion and resource release
        |
        v
supervisor records outcome and evaluates policy snapshot
```

The counters are intentionally distinct:

- **attempts started** is the audit sequence for every owner-creation call in the intent generation, including the initial explicit attempt;
- **automatic recovery attempts started** is the budget-consumption count for supervisor-authorized replacement attempts;
- **consecutive failed attempts** is a policy input updated only after a completed attempt is classified;
- **recovery episode** identifies the bounded failure/retry chain that may be reset after sufficient stability.

An owner creation or startup failure counts because repeated construction itself can consume resources or create load. `StreamEvent::Started` marks stream establishment but does not refund the attempt. A terminal failure updates the failure record and history after completion; it does not increment the attempts-started counters a second time.

Their reset behavior is also distinct. The attempts-started ordinal never resets within an intent generation; a new explicit intent begins again before its first attempt. Automatic-recovery attempts and consecutive failures reset when a new intent begins and may reset when a new recovery episode begins after the configured stable-run evidence. The episode identifier advances rather than being reused, and bounded history may retain the prior episode for diagnostics.

## Policy Configuration Model

The supervisor retains one immutable retry-policy configuration snapshot and stable configuration identity for an intent or recovery episode. Policy evaluation records that identity so a configuration reload or replacement invalidates outstanding decisions. A future configuration model may contain:

- outcome-specific automatic-recovery eligibility and vetoes;
- maximum automatic recovery attempts per episode;
- cooldown and backoff strategy inputs;
- maximum delay and jitter constraints;
- stable-run and changed-precondition reset rules;
- bounded history capacity;
- persistence and operator-reset behavior when those are later selected.

Configuration describes constraints; mutable observations and counters remain in supervisor-owned retry state. Configuration loading, validation, defaults, reload behavior, file or service schema, and invalid-configuration handling are deferred. No implicit permissive defaults are established by this ADR; a future implementation must fail closed when required configuration is absent or invalid.

## Retry Budget and Reset Model

Configuration will define an explicit automatic-recovery budget, potentially by outcome class. The configuration name must state whether the value is a count of automatic replacement attempts; it must not use an ambiguous field such as `retries` or `max_attempts` without that semantic definition.

Budget exhaustion is sticky within the current recovery episode. Once exhausted, automatic recovery remains stopped even if the same terminal evidence, cooldown expiry, or availability event is presented again. Exhaustion records the intent generation, recovery episode, state revision, attempt identity, policy reason, and configuration identity that produced it.

Reset rules are:

1. A new explicit start creates a new intent generation and fresh retry state. It never reuses authorization or counters from the stopped generation.
2. Explicit stop invalidates pending authorization and freezes the old generation for evidence; it does not silently clear history and resume that generation.
3. A successful owner construction, successful `start`, or `StreamEvent::Started` alone does not reset the recovery budget or consecutive-failure state.
4. A future configured stable-run condition may begin a new recovery episode only after typed evidence proves the required stability threshold. The threshold and evidence source are deferred.
5. A materially changed source or configuration may reset selected policy state only under a separately defined rule and a new state revision. It cannot revive a stale decision.
6. Process or supervisor recreation does not imply reset; persistence and service-lifetime behavior remain deferred and must fail closed until defined.

The distinction between intent generation and recovery episode permits a long-running intent to recover its budget after demonstrated stability without allowing a brief start to erase a flapping history.

## Cooldown Model

Cooldown prevents repeated owner creation from amplifying transient endpoint churn, resource pressure, recurring interruptions, or rapid reconfiguration. It is a policy precondition, not a sleeping thread and not evidence that a source is available.

The supervisor owns cooldown state. Its logical forms are:

- **not required** for the current recovery state;
- **pending**, with the failure/attempt that required it and an opaque eligibility deadline in a monotonic time domain;
- **satisfied**, with evidence that the same deadline was reached;
- **invalidated**, because intent, recovery episode, policy configuration, or another relevant precondition changed.

The policy operates on pending/satisfied evidence and does not read time. A future supervisor-owned scheduling integration will calculate or receive a monotonic current-time sample, compare it with the stored deadline, advance state, and request a fresh policy evaluation. A timer firing is never itself permission to create an owner. Wall-clock timestamps may be retained for human diagnostics, but they must not control an in-process cooldown because wall time can move.

No clock type, scheduler, timer, duration, or persistence representation is selected here. Persisted cooldown behavior across process restart is deferred because an in-memory monotonic deadline is not portable across lifetimes.

## Backoff Model

Future retry configuration may define:

- which failure classes require delay;
- a base delay;
- growth based on consecutive eligible failures or recovery attempts;
- a maximum delay cap;
- a jitter strategy and range;
- the stable-run or episode reset rule.

Delay calculation belongs to the deterministic policy/configuration boundary. The evaluator will receive the failure classification, applicable counters, configuration, and an explicit jitter sample from a testable supervisor-owned entropy seam. It returns the required delay; the supervisor records the resulting cooldown deadline. The policy does not obtain randomness or time as a side effect.

No exponential formula, delay value, cap, jitter distribution, entropy source, or rounding rule is selected in this milestone. Those require operational evidence and hardware-independent boundary tests before execution is enabled.

## Recovery Storm Prevention

Future recovery must preserve all of these safeguards:

- every owner-creation call consumes one attempt before the call is made, including construction and startup failures;
- automatic recovery has a finite configured budget and a sticky exhausted state;
- a mere `Started` event cannot reset a flapping failure chain;
- outcome classification can veto retry or require cooldown, source availability, changed format, cleared pressure, or operator action;
- resource exhaustion and repeated rapid failures cannot use an immediate zero-delay loop under an unchanged precondition;
- one immutable snapshot and decision can authorize at most one state transition and one owner-creation attempt;
- policy decisions bind to intent generation, recovery episode, state revision, and prior attempt identity;
- explicit stop and service shutdown take precedence over every retry hint, cooldown expiry, availability event, and prior authorization;
- the prior owner must have completed and released all resources, and no owner may be active, before replacement is considered;
- missing, inconsistent, stale, or diagnostic-only evidence fails closed;
- retry history is bounded, while aggregate counters preserve the evidence needed for limits after old entries are evicted.

These constraints break the unbounded `failure -> retry -> failure` cycle even before concrete timing values are chosen.

## Intent, Generations, and Required Execution Evidence

Retry state belongs to exactly one explicit capture-intent generation. A new explicit start creates a new generation and fresh state. Explicit stop first marks the current generation stopped and increments or otherwise invalidates its state revision, then discards pending policy authorization before requesting owner shutdown.

Before any future recovery execution, the supervisor must possess and revalidate:

- current desired-running intent;
- matching intent generation, recovery episode, state revision, and prior attempt identity;
- the configuration identity used for evaluation;
- typed terminal outcome and retry-hint constraints;
- terminal-event delivery when a stream existed;
- joined owner completion and resource release;
- no active owner or outstanding owner-creation transition;
- remaining automatic-recovery budget and non-exhausted state;
- satisfied cooldown when required;
- required source, format, endpoint, or pressure-clear evidence;
- a policy decision that permits exactly one replacement under that snapshot.

If any item changed or cannot be proved, execution must not create an owner. The supervisor must record the new state and obtain a fresh policy decision.

## Alternatives Considered

### 1. Retry state inside `CaptureOwner`

Rejected. `CaptureOwner` is deliberately single-use and owns one native-resource lifetime. Cross-attempt counters, cooldown, and exhaustion would either survive beyond its lifetime ambiguously or encourage the owner to restart itself. It would also mix resource cleanup with service policy and make new stream identity easier to conceal.

### 2. Retry state inside `RecoveryPolicy`

Rejected. Mutable policy state would make evaluation order-dependent and introduce hidden side effects. Tests could no longer evaluate the same snapshot deterministically, and timer, entropy, persistence, and state ownership would become coupled to classification. Policy remains a pure evaluator of explicit state and configuration.

### 3. Retry state inside `CaptureSupervisor`

Selected. The supervisor already owns capture intent, current-owner exclusivity, explicit-stop precedence, and future execution. It can serialize state transitions, bind them to intent generations, and revalidate decisions immediately before action while keeping `CaptureOwner` single-use and `RecoveryPolicy` pure. The cost is a more explicit supervisor state machine and the need for careful snapshot/revision tests in a later implementation milestone.

## Consequences

Benefits:

- attempt accounting has one precise increment point and distinguishes audit sequence from automatic-recovery budget;
- construction, startup, runtime, and terminal facts remain attributable to one attempt identity;
- explicit stop, stale decisions, and repeated evaluation cannot create replacement owners;
- cooldown, backoff, and exhaustion have one mutable owner without making policy stateful;
- brief stream startup cannot erase flapping evidence;
- the model is testable without audio hardware, clocks, timers, or reconnect execution;
- recovery internals remain private to `resonance-agent` and consumer contracts do not change.

Limitations:

- no availability improvement is delivered by this milestone;
- concrete limits and timing cannot be validated until operational evidence is selected;
- the current supervisor does not yet represent these fields or invoke the policy;
- persistence and service recreation may affect reset behavior and remain unresolved;
- future implementation must coordinate state revision, scheduling evidence, and shutdown without weakening fail-closed behavior.

Future implementation must first add agent-internal state/configuration representations and hardware-independent transition tests. Reconnect execution, timers, watchers, and replacement capture remain separately gated.

## Deferred Decisions

- numeric automatic-recovery attempt limits and outcome-specific budgets;
- base, growth, maximum, and rounding values for delays;
- exponential or alternative backoff formula;
- jitter distribution, range, entropy source, and deterministic sampling interface;
- stable-run duration and the exact evidence that permits a recovery-episode reset;
- bounded retry-history capacity and operational evidence format;
- configuration source, reload semantics, schema, and versioning;
- whether source or configuration changes reset any counters;
- retry-state and cooldown persistence across supervisor or process restart;
- service startup, shutdown, restart, and operator-reset behavior;
- endpoint availability/watch mechanisms and default-device-following policy;
- retry-safe subtypes for coarse startup and internal failures;
- all recovery execution, owner replacement, and reconnect behavior.
