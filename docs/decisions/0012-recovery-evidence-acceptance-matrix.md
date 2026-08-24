# ADR 0012: Recovery evidence collection and acceptance matrix

- Status: Accepted
- Date: 2026-08-24

## Decision

Automatic recovery remains disabled until each proposed recoverable failure class has completed the measurement plan in this decision and has an independently reviewable evidence set that satisfies the recovery enablement criteria. Eligibility is granted per stable agent-level failure class and, where necessary, per narrower typed subtype. Evidence from one class, backend, source-selection mode, or deployment cohort does not authorize another.

The evidence set must establish more than eventual restart success. It must show that recovery is more likely to restore stable valid frame delivery than waiting for natural resumption or requiring operator action; characterize attempt ordinal, time-to-recovery, recurrence, user impact, and resource impact; and demonstrate that recovery does not create overlap, pressure amplification, rapid failure cycles, or synchronized retry behavior. Reliability and user-impact objectives, cohort definitions, observation windows, and stopping rules must be declared before an experiment begins. Numeric production thresholds and retry values are not selected here.

Evidence used for policy or future execution is typed, attributable, and bound to the current intent generation, immutable recovery-configuration identity, recovery episode, retry-state revision, source selection, and prior attempt identity. Stale, missing, contradictory, diagnostic-only, or unbound evidence fails closed. Logs may render evidence but do not authorize recovery.

Recovery internals remain private to `resonance-agent`. This decision changes neither `resonance-core` nor `resonance-api`, exposes no consumer-facing recovery state, and implements no fault injection, retry execution, clock, timer, backoff, reconnect, endpoint watcher, default-device following, or replacement owner.

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

`CaptureOwner` owns one capture lifetime and completes joined cleanup. `CaptureSupervisor` owns current intent, attempt and episode accounting, immutable recovery evaluation snapshots, and advisory policy decisions. ADR 0008 defines the pure policy boundary, ADR 0009 defines retry-state ownership, ADR 0010 defines validated immutable configuration, and ADR 0011 defines the operational parameter direction. The runtime remains bound to an explicit recovery-disabled configuration and consumes no permit decision.

The repository has strong lifecycle, policy, and normal real-device evidence but not representative failure evidence. It does not yet establish how frequently device loss, reconfiguration, interruption, pressure, or startup failures recover; whether a restart contributes to recovery; how many attempts are normally useful; how long stable audio takes to return; or whether attempts make an unstable system worse. Test-only configuration values and synthetic state transitions prove mechanics, not operational safety. Recovery therefore cannot be enabled from retry hints, familiar industry defaults, or a failure that merely appears transient.

## Failure Scenario Matrix

| Failure area and scenario | Evidence required | Expected observations | Recovery considerations and current posture |
| --- | --- | --- | --- |
| Device availability: selected playback device removed | Typed terminal cause; resolved source identity; availability transition for the current selection; terminal delivery; joined cleanup; restoration time; attempt outcomes; stable-frame return | The old stream ends, resources are released, and the same selected source either returns or remains absent | Guarded candidate only after current-source availability is proven. A different source must not silently satisfy a selected-source intent. |
| Device availability: Bluetooth device disconnected and reconnected | Disconnect and reconnect notifications or equivalent typed probes; source identity continuity or replacement; backend readiness; outage and stable-return timing; recurrence | Trials may distinguish short link interruptions, endpoint destruction/recreation, and identity changes | Evaluate subtypes separately. Reconnection is not authorization if source identity or selection semantics are unresolved. |
| Device availability: endpoint disabled and re-enabled | Typed disabled/available state; terminal cause; cleanup; operator action timing; supported format after re-enable | Disablement should terminate or prevent capture; re-enable may restore the same endpoint with changed capabilities | Guarded candidate after explicit availability and format evidence. Remains manual if platform evidence cannot distinguish disabled from permanently unavailable. |
| Device availability: default playback device changed | Source selector, old and new resolved source identities, typed default-change evidence, follow-default policy, old-owner cleanup, new-source format | A fixed selected source remains fixed; a future follow-default intent may resolve a replacement only after the old lifetime ends | Manual until default-device-following semantics and watcher evidence are separately designed. Never infer follow-default intent from device loss. |
| Source reconfiguration: format or sample-rate change | Old and new complete formats; typed change cause; supported-format validation; renegotiation outcome; stream identities; first-valid-frame and stable-return timing | The old stream ends; any accepted replacement has a new `StreamId`, frame index zero, and stream time zero | Guarded candidate only with fresh supported-format evidence. Never conceal the stream boundary or reuse old format evidence. |
| Source reconfiguration: endpoint replacement | Old/new resolved source identities, selection mode, replacement cause, capabilities, cleanup, operator-visible source change | Replacement may be valid for follow-default intent and invalid for a fixed-source intent | Manual until source-selection policy proves the replacement satisfies current intent; then measure as its own guarded subtype. |
| Interruption: temporary audio interruption | Typed interruption cause and duration; natural-resumption result without restart; restart result in comparable trials; backend-ready evidence where available; stable frame delivery | Some interruptions may resume naturally; others may require a new owner | Candidate only when controlled comparison shows restart improves restoration and recurrence stays bounded. Do not interrupt a naturally resuming stream unnecessarily. |
| Interruption: transient backend failure | Stable normalized subtype; native diagnostics retained only for investigation; backend readiness; attempt ordinal; cleanup and stable return | A retry-safe subtype should behave consistently across repeated and representative trials | Current broad internal/backend classifications remain manual. Only an evidence-backed stable subtype may become a candidate. |
| Resource pressure: bounded handoff exhaustion | Queue/pool state, drop/overload cause, processing latency, host load, pressure-clear transition, attempt resource deltas, recurrence | Restart during continuing pressure may immediately repeat failure and increase allocation or scheduling pressure | Manual until pressure-clear evidence exists and trials show retry does not amplify load or failure frequency. Delay alone is insufficient. |
| Resource pressure: sustained processing delays | Callback and processing timing, backlog, CPU/memory/thread measures, pressure duration, natural recovery, restart comparison | The bottleneck may be downstream of capture and unaffected by owner replacement | Manual unless a stable retry-safe subtype and measurable cleared precondition demonstrate benefit. Consumer pressure must not be reclassified as device failure. |
| Startup: owner creation failure | Stable typed construction subtype, configuration/source identity, resource state, precondition change, attempt outcome, diagnostics | Coarse failures can include permanent configuration defects, missing dependencies, and transient resource conditions | Manual with current coarse classification. A future transient subtype requires its own changed-precondition evidence and experiment set. |
| Startup: capture initialization failure | Stable typed initialization subtype, source availability, supported format, backend readiness, cleanup, attempt outcome | Unsupported or invalid configuration should repeat; a temporary backend condition may clear | Manual with current coarse classification. Never retry unchanged unsupported format or invalid configuration. |

Every row also requires the common evidence below. A row described as a candidate is not enabled by this decision; it identifies where evidence collection may justify a later configuration change.

## Evidence Requirements

### Common evidence

Each experiment and recovery episode records:

- experiment identifier, scenario definition, trigger procedure, cohort, host, operating system, backend, agent build, and test protocol version;
- current desired-running intent, intent generation, source selector, resolved source identity, recovery episode, retry-state revision, immutable configuration version/fingerprint, and prior attempt identity;
- owner-creation commitment, attempt ordinal, total and automatic-attempt accounting, and whether the attempt reached `Started` and valid frame delivery;
- typed terminal cause, error kind, retry hint, stream end reason, terminal delivery, joined owner completion, resource release, and proof that no prior owner remains active;
- monotonic timestamps for trigger, detection, terminal delivery, cleanup, precondition restoration, policy evaluation, future recovery commitment, replacement `Started`, first valid frame, and stable-delivery completion when applicable;
- prior and replacement stream identities, complete formats, frame-index/timeline reset, gaps, invalid frames, discontinuities, and recurrence during the declared observation window;
- user-visible outage duration, source changes, audible or visible disruption recorded by the experiment protocol, and operator intervention required;
- CPU, memory, threads, handles/native resources, queue or pool pressure, callback/processing timing, and synchronized-attempt observations at declared sampling points;
- policy decision and stable reason, missing or contradictory inputs, attempts consumed, exhaustion state, and the reason recovery was denied or an experiment was stopped.

Successful `start` or a `Started` event alone is not recovery success. Success requires valid frames from the intended source followed by uninterrupted delivery for the experiment's predeclared stability observation window. The window must be justified from the failure's observed recurrence distribution and the product reliability objective; this ADR does not assign its duration.

### Failure-class evidence

- **Device unavailable:** typed proof that the source required by current intent is available again. For follow-default behavior, the current intent must explicitly allow it and the replacement resolution must be fresh.
- **Format or source reconfiguration:** a fresh complete format, supported-format validation, and proof that the resolved source and format satisfy current intent.
- **Interruption:** typed interruption classification, backend-ready or equivalent precondition when available, and comparative evidence separating natural resumption from restart-assisted recovery.
- **Resource exhaustion or processing pressure:** typed proof that the causal pressure cleared, plus resource measurements showing an attempt does not recreate or worsen it.
- **Startup failure:** a stable retry-safe subtype and the exact changed precondition that makes a later construction or initialization attempt meaningfully different.

Backend messages, human-readable diagnostics, retry hints, elapsed time, source-friendly names, wall-clock timestamps, and unbound Boolean flags may support investigation but are not sufficient class evidence.

### Stale and conflicting evidence

Evidence is valid for one immutable evaluation context. A change to intent generation, source selection or resolution, configuration identity, recovery episode, retry-state revision, prior attempt identity, failure cause, or required precondition invalidates evidence captured for the old context. Timer delivery, watcher notification, or availability observation must be correlated to the exact pending requirement and revalidated immediately before any future owner-creation commitment.

Contradictory facts never resolve in favor of recovery. Examples include availability reported for a different source, a supported format paired with an old configuration, completion without resource release, pressure-clear evidence followed by renewed overload, or a current active owner paired with permit evidence. The policy records a stable denial reason and remains stopped until a fresh coherent snapshot exists.

## Measurement Plan

Experiments are planned here but are not implemented or executed by this milestone. Each run uses a written protocol, a declared expected failure class, synchronized monotonic observations, and a bounded manual abort procedure. Baseline trials precede recovery trials so natural behavior is known.

| Experiment | Setup and trigger | Required observations | Success criteria | Failure criteria |
| --- | --- | --- | --- | --- |
| Remove and restore selected playback device | Capture a known continuous signal; remove or disconnect the selected endpoint; restore it without changing intent | Detection/cleanup timing, availability state, identity continuity, attempts by ordinal, stable-frame return, recurrence, resources | Fresh availability evidence precedes recovery; intended source returns with a new valid stream and meets the declared reliability, recovery-time, impact, and stability objectives | Wrong source, overlapping owner, stale evidence, missing cleanup, repeated cycling, resource growth, or objective miss |
| Bluetooth disconnect/reconnect | Repeat controlled disconnect durations and reconnect paths across representative adapters/endpoints | Endpoint identity/recreation, outage distribution, natural resumption, guarded restart outcomes, operator actions | A stable subtype and precondition predict when a restart materially improves stable return | Results depend on untyped diagnostics, identity is ambiguous, or retry performs no better than natural behavior |
| Disable/re-enable endpoint | Disable and later re-enable a selected endpoint through an approved reversible procedure | Disabled/available evidence, terminal mapping, format after return, cleanup, stable return | Re-enabled state and supported format reliably gate successful recovery without hidden source substitution | Recovery begins while disabled, unchanged failure repeats, or platform state cannot be classified safely |
| Change default device | Use separate fixed-source and future follow-default protocols; change the system default during capture | Current selection semantics, old/new identity and format, terminal ordering, cleanup, source presented after change | Fixed source is preserved; any follow-default recovery uses explicit policy and a fresh resolved replacement | Silent source switching, old/new owner overlap, or ambiguous intent |
| Change format or sample rate | Change endpoint format while a known signal is active | Old/new formats, terminal cause, renegotiation, stream boundary, first valid frame, stability | Fresh supported format predicts successful new-stream capture with correct reset semantics | Format is guessed, old evidence is reused, invalid frames appear, or changed format repeats a permanent failure |
| Temporary interruption | Apply repeatable short interruptions over a declared duration range; compare no-restart baseline with guarded restart | Natural resume rate/time, restart-assisted rate/time, discontinuity, recurrence, user impact | Restart provides a material, repeatable benefit under the declared objective without worsening impact | Natural resumption is equal or better, restart adds disruption, or subtype/duration does not predict outcome |
| Transient backend failure | Use only a safe, repeatable, typed trigger once a validation mechanism exists | Normalized subtype, backend-ready evidence, cleanup, ordinal outcomes, diagnostics | The stable subtype and readiness evidence predict bounded successful recovery | Only broad internal failure is available, behavior is non-repeatable, or invariant failure is observed |
| Bounded handoff exhaustion | Apply controlled downstream delay sufficient to reach the bounded overload path, then remove pressure | Queue/pool occupancy, processing delay, host resources, pressure-clear point, restart comparison | Recovery occurs only after pressure clear and does not increase peak/steady resource use or recurrence | Retry occurs under active pressure, creates a loop, increases resource pressure, or cannot restore valid delivery |
| Sustained processing pressure | Apply representative CPU or processing contention, then compare natural clearing with restart | Callback/processing timing, backlog, resource measures, source continuity, restart effect | A typed cleared precondition plus restart consistently improves the declared outcome | Bottleneck remains external/downstream, retry has no causal benefit, or instability increases |
| Owner creation and initialization failures | Exercise only safe typed failure mechanisms once available; separate unchanged permanent inputs from changed transient preconditions | Failure subtype, causal precondition, attempt ordinal, cleanup/resource state, outcome after change | A narrow subtype with a measurable changed precondition repeatedly predicts safe recovery | Coarse errors, unchanged configuration, unsupported format, missing dependency, corruption, panic, or permanent recurrence |
| Multi-instance synchronization | Run representative concurrent agents through a common recoverable event after candidate delay/jitter behavior exists | Attempt-time distribution, host/backend load, collisions, recovery outcomes, resource peaks | Proposed scheduling avoids harmful alignment while meeting recovery objectives | Attempts remain synchronized, overload shared resources, or reduce recovery success |

For every candidate, trials span representative supported hosts, backend/device cohorts, source-selection modes, failure durations, and simultaneous-instance counts. Order is randomized where practical, environmental differences are recorded, and incomplete/aborted trials remain in the evidence set. A later review must distinguish laboratory fault triggers from naturally observed failures and explain any difference in classification or behavior.

## Recovery Enablement Criteria

A stable failure class or narrower subtype may move from **manual only** to **automatically recoverable** only when all of the following are true:

1. The failure and its enabling precondition are represented by stable typed evidence; no diagnostic-text parsing or native error value is required for authorization.
2. A pre-registered experiment protocol defines the deployment cohort, reliability objective, user-impact objective, stability window, observation duration, sample-size or statistical stopping rule, and acceptable uncertainty before results are observed.
3. Representative baseline and recovery trials show that the proposed action materially improves stable valid-frame restoration over natural resumption or operator-only handling. The confidence method and lower-bound requirement must be chosen from the reliability objective rather than fitted to the observed data.
4. Success by automatic-attempt ordinal is characterized sufficiently to justify a finite shared episode budget. No unobserved ordinal is assumed useful, and no result here selects the production budget.
5. Detection-to-stable-frame time and worst observed user impact meet the declared objective for the supported cohort. Stream boundaries remain visible and every replacement starts a new identity and timeline.
6. Repeated-failure and post-recovery observation show no unacceptable flapping, rapid cycling, hidden discontinuity, or premature episode reset.
7. Resource evidence shows bounded cleanup and no owner overlap, leak, pressure amplification, or harmful multi-instance synchronization.
8. Missing, stale, conflicting, exhausted, stopped, or unsupported states produce deterministic reasoned denial, and operators can inspect the evidence and attempt history without parsing prose.
9. The accepted evidence set, exclusions, raw results, analysis method, proposed class disposition, and configuration provenance receive explicit engineering review. Enabling configuration and execution remain separate approved changes.

An arbitrary pass percentage is insufficient. For rare or high-impact failures, the required evidence may be dominated by confidence bounds, adverse-event absence, and field observation rather than a simple success fraction. Failed, aborted, and no-op trials are included in denominators according to the predeclared protocol.

A class remains **manual only** when any criterion is unmet; behavior varies across an unrepresented cohort; recovery is not better than natural resumption; required evidence cannot be produced; a retry-safe subtype is unavailable; user or resource impact is unacceptable; or operational diagnosis is clearer and safer than automation. Device availability, reconfiguration, interruption, resource pressure, and startup subtypes are reviewed independently.

## Safety Boundaries

Automatic recovery must never occur when:

- explicit stop has cleared desired-running intent, or the intent generation is absent or stale;
- a worker panic, invariant violation, suspected corrupted state, invalid configuration, unsupported unchanged format, or broad unclassified internal failure occurred;
- terminal delivery, joined completion, resource release, or proof of no active owner is missing;
- evidence is missing, stale, contradictory, diagnostic-only, belongs to another source/attempt/configuration/episode, or cannot be attributed to a trusted producer;
- the automatic-attempt budget is exhausted, accounting is invalid, the one-shot authorization is stale or already consumed, or required cooldown/backoff evidence is absent;
- the causal precondition remains unchanged, including unavailable source, unsupported format, backend not ready, or uncleared resource pressure;
- repeated identical failures have reached the reviewed stopping boundary, violate the accepted recurrence objective, or indicate flapping; exact numeric boundaries remain deferred;
- recovery would silently change a fixed selected source, conceal a terminal lifecycle boundary, reuse a `StreamId`, preserve the prior timeline, or overlap the old owner;
- the candidate deployment, backend, device class, failure subtype, or simultaneous-instance pattern is outside the accepted evidence cohort;
- required operational observability or the manual abort/disable path is unavailable.

Worker panic, suspected corruption, and invariant failure require operator investigation and a new explicit intent after the cause is understood. Process restart, configuration reload, source change, or elapsed time alone never converts these conditions into recovery evidence.

## Operational Observability

Before execution can be accepted, operators need agent-internal structured visibility for:

- failure class and subtype, typed terminal outcome, retry hint, and source/configuration identity;
- current intent generation, recovery episode, retry-state revision, prior and current attempt identities, and recovery-configuration identity;
- attempts consumed and remaining, recovery episode start/end, exhaustion, and repeated-failure history;
- required, present, missing, stale, and contradictory common and class-specific evidence;
- policy decision, stable decision/denial reason, stale-decision rejection, and whether authorization was consumed;
- trigger-to-detection, cleanup, precondition-restoration, attempt-start, first-valid-frame, and stable-recovery timing;
- old/new stream identity and format, outage and user-impact measures, frame continuity facts, and recurrence;
- resource pressure, owner overlap checks, cleanup outcome, and multi-instance synchronization observations;
- configuration provenance, evidence cohort/protocol identity, and whether the current environment is covered by accepted evidence.

These facts remain private operational evidence in `resonance-agent`. Telemetry schema, storage, transport, retention, presentation, and alerting are deferred. Consumer APIs continue to expose audio products and platform-neutral stream lifecycle/error facts, not recovery policy internals.

## Alternatives Considered

### Enable recovery from assumptions

Rejected. Familiar retry defaults, retry hints, or a plausible transient label do not establish that a replacement owner helps, that a precondition changed, or that repeated attempts are safe. This approach would hide uncertainty inside production behavior and make numeric choices unauditable.

### Enable all failures equally

Rejected. Device loss, format change, interruption, resource pressure, invalid configuration, internal failure, and panic have different causal preconditions and safety properties. One permissive rule would retry permanent and invariant failures, while one conservative rule would prevent useful recovery from a demonstrated transient subtype.

### No automatic recovery

Retained as the current and fail-closed posture. It minimizes recovery storms and hidden state transitions, but leaves demonstrated transient failures to operator action. It remains the correct posture for unsupported classes and environments. Evidence may justify narrowly enabling selected classes later without making automatic recovery universal.

## Consequences

Benefits:

- recovery enablement becomes a reviewed evidence claim for a stable class and deployment cohort rather than an intuition or copied default;
- baseline comparison separates restart-assisted recovery from natural resumption;
- common and class-specific evidence preserve current intent, cleanup, source, format, pressure, and lifecycle invariants;
- predeclared objectives and uncertainty rules reduce post-hoc threshold selection;
- resource, recurrence, and multi-instance evidence address instability that a success rate alone would miss;
- reasoned denial and operator-visible episodes make future automatic behavior auditable;
- recovery details remain private to `resonance-agent` and consumer contracts remain unchanged.

Limitations and future implementation impact:

- representative device, backend, host, and concurrency trials require deliberate laboratory and field collection;
- some rare or destructive failures may remain manual because safe representative evidence is impractical;
- typed watcher, readiness, pressure-clear, timing, and resource evidence may need new private collection boundaries before experiments can run;
- cohort-specific acceptance may produce a narrower enabled configuration than desired;
- this decision provides no availability improvement and selects no numeric value;
- future execution must revalidate the exact accepted evidence bindings immediately before committing an attempt.

## Deferred Decisions

- implementation and execution of the experiments in this plan;
- safe fault-injection or validation mechanisms and operator procedures;
- reliability, recovery-time, user-impact, resource-impact, confidence, sample-size, observation-window, and recurrence thresholds;
- production automatic-attempt budget, cooldown, backoff, jitter, and stable-run values;
- deterministic delay calculation, clocks, timers, scheduling, entropy sampling, and authorization consumption;
- endpoint/source watchers, default-device-following semantics, backend-readiness and pressure-clear producers;
- typed retry-safe startup, initialization, construction, and internal-failure subtypes;
- evidence storage, schema, signing or provenance mechanism, retention, telemetry, presentation, and alerting;
- configuration source, activation, reload, rollback, and cohort-to-profile selection;
- reconnect, replacement-owner creation, and all automatic recovery execution.
