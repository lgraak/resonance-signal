# ADR 0010: Validated immutable recovery configuration

- Status: Accepted
- Date: 2026-08-24

## Decision

Recovery configuration is private to `resonance-agent`. An untrusted complete definition is accepted only after fail-closed structural validation and becomes an owned immutable snapshot. `CaptureSupervisor` retains that snapshot and binds retry state and recovery-evaluation snapshots to its identity. `resonance-core` and `resonance-api` do not expose recovery configuration.

Each accepted configuration identity contains:

- a caller-controlled nonzero configuration version;
- the internal configuration-schema version;
- a deterministic 128-bit fingerprint over a canonical encoding of every typed configuration field.

Equivalent content at the same version has the same identity. Any content or version change changes identity and invalidates prior recovery assumptions. The fingerprint prevents accidental reuse of an unchanged caller version from hiding changed content; it is not an authentication or integrity primitive.

Configuration loading, files, environment variables, serialization, runtime reload, persistence, policy application of the newly modeled values, retry timing, entropy, and recovery execution remain deferred.

## Context

ADR 0009 requires `CaptureSupervisor` to own mutable retry state while `RecoveryPolicy` evaluates immutable state and configuration snapshots. It also requires configuration identity to invalidate stale decisions, but it intentionally deferred the concrete model, validation, values, sources, and reload behavior.

An identity supplied independently from content would allow changed values to reuse an old identity. A content-only identity would not support explicit operator-controlled versioning. Mutable accepted configuration would allow a decision's assumptions to change after evaluation. Permissive defaults or silent repair could enable recovery under incomplete or contradictory inputs. The boundary therefore needs explicit completeness, structural safety, owned immutability, and identity derived from both revision and content before policy or execution uses it.

## Configuration Model

The private typed model represents:

- maximum automatic recovery attempts and exhaustion behavior;
- whether cooldown is disabled or required, its minimum duration, and reset behavior;
- disabled or required fixed, linear, or exponential backoff, including maximum delay and reset behavior;
- whether jitter is forbidden or required with a maximum bound;
- retryable, non-retryable, or additional-evidence treatment for stable agent-level failure classes;
- whether only a new explicit intent resets recovery state or whether typed stable-run evidence plus a minimum stable duration may begin a new episode.

Failure classification uses agent-level causes rather than native WASAPI, operating-system, or backend error codes. Platform errors remain diagnostic evidence and must be normalized before they reach recovery policy.

The model describes policy. It has no method or dependency capable of reading a clock, generating entropy, sleeping, scheduling work, accessing hardware, creating an owner, starting capture, reconnecting, or mutating supervisor state.

## Validation Boundary

Every required field is optional only in the untrusted input representation so absence can be rejected explicitly. Acceptance never fills a missing field, selects an enabled default, clamps a value, or repairs a contradiction.

Enabled automatic recovery requires all of these structural conditions:

- a finite nonzero automatic-recovery-attempt budget;
- at least one failure class explicitly retryable or guarded by additional evidence;
- device unavailability, source reconfiguration, resource exhaustion, and unsupported format either remain non-retryable or require compatible typed changed-precondition/source/format/pressure-clear evidence;
- a nonzero cooldown minimum;
- required bounded backoff with nonzero strategy inputs and maximum delay;
- initial delay, increment, cooldown minimum, and required jitter bounds no greater than the maximum delay where applicable;
- exponential growth multiplier of at least two when exponential strategy is selected;
- matching cooldown and backoff reset behavior;
- a stable-run rule consistent with delay reset behavior and a nonzero stable duration when stable-run reset is selected.

An explicitly disabled configuration requires zero automatic attempts, all failure classes non-retryable, disabled cooldown and backoff, and reset only through a new explicit intent. This is the runtime's current internal definition and preserves the recovery-disabled posture. It does not establish production values for enabled recovery.

Operational upper bounds and actual retry, cooldown, backoff, jitter, and stable-run values require collected evidence in a later milestone. Enabled values in unit tests are test-only examples.

## Identity and Snapshot Binding

The canonical fingerprint encoding includes the schema version, budget and exhaustion behavior, cooldown, backoff strategy and bounds, jitter, every stable failure-class disposition, and stable-run reset policy. Integer and duration components use explicit byte encodings rather than Rust's process-dependent hashing interfaces. Two independent fixed hash lanes form the 128-bit identity fingerprint.

An accepted snapshot owns its configuration values behind an immutable shared allocation. Mutating or discarding the input definition cannot alter the accepted snapshot. A recovery-evaluation snapshot owns a clone of that immutable configuration snapshot and retry state records the same identity. Future policy or execution must compare the complete current identity and reject a stale evaluation before action.

Identity collision resistance is sufficient for deterministic in-process stale-state detection but is not a security boundary. Authenticated configuration provenance, if later required, must be designed separately.

## Alternatives Considered

### Caller-supplied opaque identity only

Rejected. A caller could accidentally reuse an identity after changing content, leaving stale recovery assumptions apparently current.

### Content fingerprint only

Rejected. Semantically identical content may still need explicit version invalidation, and ADR 0009 requires identifiable configuration replacement.

### Mutable configuration owned directly by policy

Rejected. It would make evaluation order-dependent, permit assumptions to change after snapshot creation, and mix configuration state with the side-effect-free policy boundary.

### Permissive defaults and normalization

Rejected. Recovery controls repeated owner creation and resource use. Missing, contradictory, or zero-delay inputs must fail closed rather than silently becoming an enabled policy.

## Consequences

Benefits:

- configuration ownership remains inside the component that will eventually revalidate and execute recovery;
- accepted configuration is complete, explicit, owned, immutable, and safe to capture in evaluation snapshots;
- explicit version and content identity jointly invalidate stale decisions;
- unsafe immediate-loop structures and contradictory reset/budget/classification inputs are rejected before policy evaluation;
- validation and identity behavior are hardware-independent and deterministic.

Limitations:

- the runtime currently uses only an explicit recovery-disabled definition;
- the existing policy evaluator does not yet apply the newly modeled fields;
- no operational enabled values or upper bounds have been selected;
- no configuration source, reload, persistence, or cross-process identity contract exists;
- the fingerprint is not authentication, authorization, or tamper detection;
- no availability improvement or recovery execution is delivered.

## Deferred Work

- collect evidence and select operational budgets and duration bounds;
- map validated failure classifications and delay/reset rules into pure policy evaluation;
- define deterministic delay calculation and an explicit jitter-sample seam;
- define configuration source, schema exposure if any, loading, persistence, and reload semantics;
- define how an accepted configuration change advances supervisor state revision and invalidates recorded evaluations;
- implement timers, scheduling, retry authorization consumption, owner replacement, reconnect, endpoint watching, or default-device following only under separate approval.
