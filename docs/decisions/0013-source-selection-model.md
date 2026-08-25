# ADR 0013: Source selection model

- Status: Accepted
- Date: 2026-08-24
- Evidence reviewed: 2026-08-24 Windows source availability, playback-device removal, Bluetooth availability, and source identity packets
- Extended by: [ADR 0014](0014-source-discovery-and-identity-model.md)

# Decision

Adopt a hybrid source intent model with two distinct consumer choices:

- **Default Playback** follows the platform's logical default playback role. The provider resolves that intent for each new capture attempt. A later attempt may therefore use a different physical endpoint.
- **Explicit Source** targets one provider-assigned source identity. A later attempt may use only that same source; it must not fall back to the platform default or to a similar replacement.

Source intent and resolved source identity are separate facts. Every active stream reports the source identity actually resolved for that stream. Neither intent permits an in-place endpoint migration: disappearance, replacement, reconfiguration, or return ends the current stream, and any later capture starts with a new `StreamId`, frame index zero, and stream-relative time zero.

This ADR defines selection semantics only. It does not implement discovery, persistence, endpoint watching, recovery, replacement-owner creation, or Linux capture.

## Context

### Windows evidence

The collected [source-availability](../evidence/resonance-signal-source-availability-evidence-2026-08-24.md), [playback-device-removal](../evidence/resonance-signal-playback-device-removal-evidence-2026-08-24.md), [Bluetooth-availability](../evidence/resonance-signal-bluetooth-source-availability-evidence-2026-08-24.md), and [source-identity](../evidence/resonance-signal-source-identity-evidence-2026-08-24.md) packets established the following bounded Windows facts:

- the production capture path resolves the console-role default rendering endpoint when a capture run starts;
- removing or powering off the resolved endpoint ends the active stream with source-unavailable lifecycle evidence rather than migrating that stream;
- a manual later run creates a new stream and can resolve a different active endpoint;
- after the original endpoint returns and again becomes the intended default, another new run can resolve it again; and
- the controlled evidence did not prove live in-place following after an arbitrary default-device switch.

These observations support startup-time resolution and explicit stream boundaries. They do not justify automatic recovery or claim that a running owner follows the Windows default continuously.

### Default device behavior

A platform default is an indirection, not a physical-device identity. Its purpose is to select whichever endpoint currently owns a platform role. Pinning the endpoint found during the first resolution would silently change that meaning from “follow default playback” to “use this device forever.” Conversely, treating every selection as default-following would make an explicit device request unreliable.

### Why source intent matters

The same endpoint can be reached through different user intents. A consumer that asks for default playback is choosing a role and accepts a later endpoint change across stream boundaries. A consumer that asks for a specific source is choosing identity and does not accept substitution. Resolved endpoint identity alone cannot recover which of those policies the consumer requested, so intent must remain explicit and survive independently of the most recently resolved endpoint.

## Source Intent Model

### Default Playback

Default Playback means “resolve the platform's current default playback source for a new capture attempt.” It is a durable logical intent within the lifetime of the request, not an alias for the endpoint returned by its first resolution.

- Resolution occurs before each new capture attempt.
- The provider records and reports the source identity actually selected.
- A changed platform default may resolve to a different source on a later attempt.
- An active stream never changes source identity in place.
- No attempt, watcher, or recovery action is implied by the intent itself.

The existing `Default(DefaultSource::Playback)` selector represents this intent at the consumer-contract boundary. The current Windows runtime implements one explicit capture attempt at a time and resolves the console-role default through the private registry immediately before that attempt. It reports the mapped opaque endpoint `SourceId`; automatic replacement and repeated recovery-driven attempts remain future orchestration work.

### Explicit Source

Explicit Source means “capture the source identified by this opaque provider-assigned `SourceId`.” It is an identity-pinned intent.

- Resolution must match the requested source identity.
- A different endpoint, even one with the same friendly name, device class, or capabilities, is not an acceptable substitute.
- If the source is absent, the provider reports or retains source-unavailable state according to the lifecycle contract; it does not fall back to Default Playback.
- If the same source returns and its identity can still be proven, a later attempt may resolve it and begins a new stream.
- If identity continuity cannot be proven, the provider must require rediscovery and a new explicit intent rather than guess.

The existing `Id(SourceId)` selector represents this intent at the consumer-contract boundary. Windows discovery and runtime explicit-source capture are implemented; consumer transport remains deferred.

## Source Identity Model

### Identity

`SourceId` is an opaque provider-assigned identity for one resolved capture source. It is distinct from:

- the source intent that selected it;
- a human-readable device name;
- a backend-native endpoint or node identifier; and
- `StreamId`, which identifies one uninterrupted stream lifetime.

Within the provider's documented identity scope, the same proven backend source maps to the same `SourceId`, and distinct backend sources must not share one `SourceId`. Friendly names, formats, transport type, and device class are descriptive properties, not identity proof. Backend-native identifiers remain private to `resonance-agent`.

### Lifecycle

A `SourceId` can outlive an individual `StreamId`. Restarting, returning after temporary absence, or changing accepted format always creates a new stream identity even when the resolved source identity is unchanged. Default Playback may resolve different `SourceId` values over time. Explicit Source may resolve only its requested `SourceId` while that identity remains valid and provable.

The provider must never reuse a `SourceId` for a different source within the same identity scope. When a platform cannot prove that a returned object is the prior source, the provider treats it as a different or newly discovered source.

### Persistence expectations

[ADR 0014](0014-source-discovery-and-identity-model.md) defines the identity domain as one provider installation on one host with one retained mapping namespace. Within that domain, implemented discovery preserves IDs across process restarts and temporary disappearance while native continuity remains proven. IDs are not portable across hosts, installations, mapping resets, operating-system reinstallations, or unproven backend re-enumeration. Windows registry storage plus Default Playback and Explicit Source runtime enforcement are implemented; other platform mappings remain deferred.

## Lifecycle Semantics

### Disappearance

When the resolved source disappears or becomes unavailable, the active stream ends through the existing ordered error/end lifecycle. The stream is never left open while waiting for the source. Default Playback retains its logical role intent; Explicit Source retains its identity-pinned intent only while that identity remains valid. This decision creates no automatic attempt to resume either intent.

### Replacement

A platform replacement is always a stream boundary.

- For Default Playback, a later resolution may accept the new role owner, report its `SourceId`, and create a new `StreamId`.
- For Explicit Source, a different source is rejected as a substitute even if it takes the same platform role or presents the same friendly name. The requested source remains unavailable.

No provider may conceal replacement by continuing the old stream identity or timeline.

### Return

If a disappeared source returns:

- Default Playback resolves whichever source owns the default role at the time of a later attempt; that may be the returned source or another source.
- Explicit Source may resolve the returned source only when backend evidence proves it has the requested identity.

Every return starts a new uninterrupted stream. Source return is changed-condition evidence for future recovery policy, not permission to create an owner or bypass recovery accounting.

## Cross-Platform Considerations

### Windows endpoint model

Windows separates the default rendering role from the endpoint that currently owns it. Default Playback maps to the console-role default rendering endpoint. The resolved WASAPI endpoint identity is retained privately as backend evidence and mapped to an opaque `SourceId`. Default-device, endpoint-state, removal, and session notifications can end the active stream and inform later resolution, but they do not migrate the current owner.

Windows endpoint identity must be based on the native endpoint identifier and lifecycle evidence, not friendly name. The exact mapping, caching scope, and persistence boundary remain implementation decisions.

### Future PipeWire mapping

PipeWire presents a dynamic graph whose default sink is normally session-manager policy, while explicit targets require stable node properties rather than an ephemeral registry global ID. A future adapter should map Default Playback to the current default sink/monitor target and map Explicit Source from stable properties such as `object.serial` or `node.name` only after their lifecycle and persistence properties are validated.

The portable contract remains intent plus opaque `SourceId`; PipeWire node IDs, metadata keys, and registry objects do not cross the provider boundary. The exact default-metadata and stable-property mapping is deferred to the Linux implementation milestone.

## Consumer Contract

Consumers request source intent, not backend routing policy:

- Default Playback to follow the platform playback role across separately started streams; or
- Explicit Source with an opaque `SourceId` obtained through future discovery.

For every started stream, the provider guarantees:

- truthful resolution of the requested intent;
- an opaque resolved `SourceId` and a distinct uninterrupted `StreamId`;
- no silent fallback from Explicit Source to Default Playback or another source;
- no in-place source migration or synthesized continuity;
- ordered lifecycle events when the source disappears, changes, or becomes unusable; and
- platform-neutral source, stream, format, and error semantics.

The provider now implements Windows playback discovery, persistent installation-scoped IDs, and separate capture-time Default Playback and Explicit Source mappings. It does not guarantee automatic waiting or retry, endpoint watching, replacement-owner creation, or equivalent behavior on other platforms.

## Alternatives Considered

### 1. Always follow default playback

Every new run would resolve the platform default, regardless of how the consumer originally selected the source.

This is simple and matches the current Windows diagnostic path, but it makes explicit selection impossible. A consumer monitoring a specific output could silently receive a different source after replacement, which violates identity and provenance expectations.

### 2. Always pin physical device

The provider would resolve once and keep targeting the first endpoint for all later runs.

This preserves device identity, but it breaks the meaning of a default selector. A user who changes the system default would continue capturing the old device, and an unavailable old endpoint would block capture even when the platform has a valid new default.

### 3. Hybrid source intent model

Default Playback remains role-following across stream boundaries; Explicit Source remains identity-pinned.

This adds explicit intent and resolution state to future orchestration, but it preserves both user meanings without guessing. It also maps naturally to the existing `Default` and `Id` selector shapes and keeps every source change visible through existing stream lifecycle semantics.

This alternative is selected.

## Consequences

### Benefits

- Preserves the user's selection meaning independently of the last resolved endpoint.
- Prevents silent substitution for explicitly selected sources.
- Allows default playback to follow platform policy on later attempts without hiding stream boundaries.
- Keeps backend identifiers private while preserving truthful source provenance.
- Uses the existing consumer selector shapes without changing Rust source in this milestone.
- Provides one cross-platform semantic model for Windows endpoints and future PipeWire nodes.

### Limitations

- Source discovery representation and durable identity scope are defined by ADR 0014, but enumeration and mapping persistence are not implemented.
- Current Windows runtime exposes only default-playback capture and does not execute replacement or follow-default recovery.
- Current Windows runtime reports the provider-mapped resolved endpoint `SourceId` for both Default Playback and identity-pinned Explicit Source capture; consumer transport remains unavailable.
- Exact equivalence between native identities across device removal, re-enumeration, process restart, or operating-system changes is platform-specific and unresolved.
- Consumers must handle new stream identities whenever capture resumes, even if the same source returns.

### Implementation impact

Future `resonance-agent` work must retain source intent separately from resolved source identity, bind both to attempt/evidence state, and resolve according to intent immediately before owner creation. Provider output must report the resolved `SourceId`. Explicit selection must fail closed on identity ambiguity. Default replacement and source return must remain typed changed-condition evidence rather than implicit authorization.

No current Rust source, dependency, or runtime behavior changes as a result of this design milestone.

## Deferred Decisions

- source discovery enumeration, snapshot API, friendly-name presentation, capabilities, and authorization;
- UI or consumer presentation for choosing and displaying sources;
- exact Windows endpoint and PipeWire node-property mapping;
- `SourceId` mapping storage, invalidation implementation, migration, and reset behavior;
- Linux PipeWire implementation and validation;
- recovery behavior, endpoint watching, wait/retry timing, owner replacement, and default-follow execution;
- microphone-default intent and whether it needs policy distinct from playback;
- transport and serialization representation of selection and identity.

## Documentation Updates

- `README.md` records the completed source-selection-model design milestone and its runtime deferrals.
- `docs/api.md` distinguishes default-role intent from explicit-source identity and records the current Windows mapping gap.
- `docs/architecture.md` places source intent and resolved identity at the provider boundary.
- `docs/roadmap.md` records Milestone 6M and the remaining discovery and platform-mapping work.
