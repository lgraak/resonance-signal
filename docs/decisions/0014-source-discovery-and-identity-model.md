# ADR 0014: Source discovery and identity model

- Status: Accepted
- Date: 2026-08-24
- Extends: [ADR 0013](0013-source-selection-model.md)
- Evidence reviewed: 2026-08-24 Windows source availability, playback-device removal, Bluetooth availability, and source identity packets

# Decision

Adopt provider-managed opaque source identity and a snapshot-oriented discovery model.

Discovery exposes provider-assigned `SourceId` values with presentation and current-state metadata. Backend-native identifiers remain private evidence used by the provider to map a platform source into its identity domain. A `SourceId` is stable within one provider installation on one host while the provider retains its identity mapping and can prove continuity with the same backend source. It is not portable across hosts, installations, mapping-store resets, operating-system reinstallations, or unproven platform re-enumeration.

Temporary disappearance does not by itself invalidate a `SourceId`. A returning source keeps its prior ID only when the provider can prove native identity continuity. When continuity is missing, stale, contradictory, or ambiguous, the provider retires the prior mapping, assigns a new ID if the source is discovered again, and fails explicit requests for the old ID without substitution. Retired IDs are never reassigned to another source within the same identity domain.

Default Playback remains the logical role intent selected by [ADR 0013](0013-source-selection-model.md). The provider resolves that role immediately before each new capture attempt, reports the `SourceId` of the endpoint actually selected, and never treats a discovery-time default annotation as a reservation. An active stream never migrates in place.

This ADR defines discovery and identity semantics only. It does not add a Rust discovery API, change the Windows adapter, implement PipeWire, persist mappings, add UI, or define authorization.

## Context

### Source intent model

ADR 0013 separates two consumer intents:

- Default Playback follows the platform's logical playback role across separately started streams; and
- Explicit Source pins later resolution to one provider-assigned `SourceId`.

That model requires discovery to produce IDs that consumers can select and requires the provider to distinguish the logical default role from the physical endpoint currently owning it. Without an identity contract, Explicit Source cannot safely survive a refresh, temporary disappearance, or provider restart, and Default Playback cannot truthfully report which source was resolved.

### Windows evidence

The collected [source-availability](../evidence/resonance-signal-source-availability-evidence-2026-08-24.md), [playback-device-removal](../evidence/resonance-signal-playback-device-removal-evidence-2026-08-24.md), [Bluetooth-availability](../evidence/resonance-signal-bluetooth-source-availability-evidence-2026-08-24.md), and [source-identity](../evidence/resonance-signal-source-identity-evidence-2026-08-24.md) packets establish bounded Windows behavior:

- each capture run resolves the console-role default rendering endpoint at startup;
- endpoint removal or Bluetooth power loss ends the active stream as source unavailable instead of migrating it;
- a later manual run can resolve a different active endpoint;
- the original endpoint can be resolved again after it returns and becomes the intended default; and
- the evidence does not prove live in-place following after an arbitrary default-device switch.

The evidence distinguishes role resolution, endpoint identity, availability, and stream continuity. It supports a model in which endpoint return may preserve source identity while always creating a new `StreamId`. It does not prove that a friendly name, current default status, or one observed endpoint object is sufficient durable identity evidence.

### Why identity matters

Consumers need to distinguish three independent facts:

- what they asked for: logical default or an explicit source;
- which source the provider resolved: `SourceId`; and
- which uninterrupted capture lifetime produced the data: `StreamId`.

Conflating these facts permits silent source substitution, makes provenance unreliable, and can join data from different endpoints under one apparent identity. A stable but scoped `SourceId` lets consumers retain an explicit selection through ordinary refreshes and temporary absence without exposing platform identifiers or claiming portability the backend cannot support.

## Discovery Model

### Discovered source representation

A future discovery result is a replaceable snapshot of sources currently known to the provider. Each discovered-source entry conceptually contains:

- `SourceId`: the opaque selection and comparison key;
- `SourceKind`: playback, microphone, virtual, or other;
- an optional human-readable display label;
- current availability when the backend can report it; and
- the logical default roles the source owns in that snapshot, if any.

The display label, availability, default-role annotations, format, transport class, and capabilities are mutable descriptive metadata. None participates in identity comparison. Default-role membership is point-in-time information only: it can change immediately after discovery and does not convert an explicit source into Default Playback intent.

A backend may omit an absent source from a current snapshot or retain it as known but unavailable when the platform can represent that state. Absence from one snapshot is not identity invalidation. Discovery is not a reservation, a capture guarantee, or an event history; capture resolution must revalidate current availability and identity.

The exact Rust types, request filters, snapshot revision representation, change-notification mechanism, and capability schema are deferred to the enumeration implementation milestone.

### Provider responsibilities

The provider must:

- enumerate through the active platform adapter and normalize only platform-neutral source facts;
- assign and look up opaque IDs within the provider identity domain;
- keep backend-native identity evidence private to `resonance-agent`;
- map one proven backend source to one `SourceId` and never map distinct sources to the same live ID;
- preserve an ID across descriptive metadata, format, availability, and default-role changes when native continuity remains proven;
- retire rather than recycle an ID when continuity can no longer be proven;
- report snapshot metadata truthfully without implying that it remains current;
- revalidate identity and availability when resolving a capture request; and
- fail explicit selection closed rather than substitute a default, same-named, or similar source.

Discovery does not authorize recovery, endpoint watching, retry scheduling, or owner creation. Those remain separate orchestration decisions.

## Identity Model

### `SourceId`

`SourceId` is an opaque provider-assigned value. Consumers may store it, compare it for equality, and submit it as Explicit Source intent. They must not parse it, infer a platform, display it as a device name, construct values from backend identifiers, or use it as proof of authorization.

`SourceId` identifies a source, not a role or stream. The same source may own or lose the default role without changing ID and may produce many `StreamId` values. Default Playback may resolve different source IDs on different attempts.

### Identity scope

The identity domain is one provider installation on one host using one retained provider mapping namespace. Within that domain, IDs are intended to remain stable across:

- discovery refreshes;
- provider process restarts;
- source display-name and property changes that do not invalidate native identity;
- default-role changes; and
- temporary disappearance and proven return.

IDs are not promised to remain meaningful across another host, another provider installation, an identity-mapping reset or loss, an operating-system reinstall, a backend namespace reset, or device re-enumeration that does not preserve sufficient native identity evidence. Transport or serialization work must preserve opacity and must not broaden this scope.

### Continuity requirements

Identity continuity requires backend evidence that is stable for the relevant platform lifecycle and unambiguously identifies the same source. The provider must treat friendly name, source kind, current format, default-role ownership, transport type, and capability similarity as insufficient by themselves.

The provider preserves the same `SourceId` only when all available native evidence is consistent with the prior mapping. Missing, reused, contradictory, or ambiguous native evidence fails closed. The provider then invalidates the old mapping for future resolution and requires rediscovery; it must not guess continuity to make an explicit request succeed.

Persistence of the provider mapping is required for cross-process stability once discovery is implemented, but the storage format, location, migration, retention period, and reset mechanism are deferred.

## Platform Mapping

### Windows considerations

Default Playback maps to the Windows console-role default rendering endpoint at capture-attempt resolution time. The endpoint that currently owns that role is a discovered source with its own provider `SourceId`; the logical default is not itself a physical discovered source ID.

Windows mapping must use the native endpoint identifier and endpoint lifecycle evidence as private identity inputs. Friendly names such as a product label are descriptive and may collide or change. Endpoint state and default-device notifications may update discovery state and provide changed-condition evidence, but they do not migrate an active stream or independently authorize recovery.

The mapping implementation must validate how native endpoint identifiers behave across disable/enable, unplug/replug, Bluetooth reconnect, driver replacement, and operating-system re-enumeration. The current evidence supports temporary absence and later resolution, but it does not establish every persistence boundary. Ambiguous return receives a new provider ID.

The original Windows adapter emitted the logical value `default-playback` as its `SourceId`. Milestone 6S replaced that placeholder: the adapter now resolves the registry-backed endpoint identity at attempt start and reports its opaque `SourceId`.

### Linux/PipeWire considerations

Default Playback maps to the current default sink or monitor target selected by PipeWire session-manager policy immediately before a capture attempt. A discovery-time default annotation is advisory and must be re-resolved.

PipeWire registry global IDs and live node object addresses are graph-lifetime identifiers and must not be exposed as durable `SourceId` values. A future adapter must evaluate stable node and device properties, their ownership relationships, and session-manager metadata to construct private continuity evidence. Candidate properties such as `object.serial`, `node.name`, or device-backed properties may be used only after validation establishes their uniqueness, reuse, and restart behavior for the supported PipeWire environment.

If PipeWire cannot prove that a returned or recreated node represents the prior source, the provider issues a new ID. Exact property precedence, monitor-source representation, graph-change handling, and session-manager compatibility remain deferred to the PipeWire adapter milestone.

## Lifecycle Semantics

### Appearance

When a platform source appears, the provider first checks for an existing non-invalidated mapping supported by current native evidence. If continuity is proven, the source is a return. Otherwise the provider assigns a new `SourceId` and exposes it in a later discovery snapshot. Appearance does not start capture.

### Disappearance

When a source disappears or becomes unavailable, an active stream ends through the existing ordered error/end lifecycle. The provider marks or omits the source according to what the backend can truthfully report, but retains its identity mapping within the identity domain. Disappearance alone does not invalidate the ID, start a wait, or create a retry.

### Return

A returning source reuses its `SourceId` only when native continuity is proven. Any new capture uses a new `StreamId`, frame index zero, and stream-relative time zero, even when the source ID is unchanged. If continuity is not proven, the return is represented as a newly discovered source with a new ID.

### Invalidation

An ID is invalidated when the provider loses its mapping namespace, observes native identifier reuse or contradiction, receives authoritative platform evidence that the old object was replaced, or cannot prove continuity after re-enumeration. Invalidation is permanent within the identity domain: the old ID is never assigned to a different source.

An explicit request using an invalidated ID fails closed and requires rediscovery. The exact discovery status and consumer-visible error representation are deferred with the API implementation. Invalidation ends any active stream but does not itself authorize recovery.

## Default Playback Semantics

### Logical intent

Default Playback means “use the source that owns the platform's default playback role when a new capture attempt is committed.” It is a durable request intent, not an ID alias, discovery result, or promise to remain on one endpoint.

A consumer may observe which discovered source currently owns that role, but selecting Default Playback remains distinct from selecting that source's ID. If the default changes, the logical intent remains valid while the old and new endpoints retain their own source identities.

### Endpoint resolution

The provider resolves the role immediately before each capture attempt and reports the resolved endpoint's `SourceId` in the resulting `StreamDescriptor`. It revalidates availability and accepted format before publishing `Started`.

A default change never changes the source of an active stream in place. A later attempt may resolve a different endpoint and must create a new stream identity and timeline. Discovery-time default metadata may be stale and cannot override attempt-time resolution.

## Consumer Contract

Consumers request:

- a discovery snapshot when they need explicit source choices;
- Default Playback when they want platform-role intent; or
- Explicit Source with a `SourceId` obtained from discovery when they require one source identity.

Providers guarantee:

- opaque source IDs with equality semantics only;
- stability within the documented identity domain while continuity remains proven;
- truthful, point-in-time discovery metadata;
- attempt-time resolution and availability validation;
- reporting of the actual resolved source ID for every started stream;
- no silent substitution for Explicit Source;
- no ID recycling within an identity domain;
- fail-closed behavior on missing or ambiguous identity evidence; and
- visible stream boundaries for disappearance, return, replacement, reconfiguration, or format change.

Providers do not guarantee that a discovery result stays current, that a source remains available, that IDs are portable beyond the identity domain, or that discovery causes capture, authorization, recovery, persistence migration, or UI behavior.

## Alternatives Considered

### 1. Use backend-native IDs directly

Expose the Windows endpoint ID or PipeWire-native object/property identifier as `SourceId`.

This minimizes mapping state and makes backend diagnostics easy to correlate, but it leaks platform representation into the consumer contract, creates incompatible identity rules across backends, and can silently inherit native reuse or lifecycle behavior that has not been validated. It also makes future migration or composite identity repair a consumer-visible breaking change.

### 2. Use display names

Use the human-readable endpoint or node name as identity.

This is easy to present and inspect, but names are mutable, localized, non-unique, and frequently shared across identical devices. A rename could appear to replace a source, while two distinct devices could collapse into one identity. Display names remain metadata only.

### 3. Use provider-managed opaque identity

Map validated backend-native evidence to provider-assigned opaque IDs in a scoped identity domain.

This adds mapping persistence, invalidation, migration, and diagnostic-correlation responsibilities to the provider. In return, it preserves a platform-neutral contract, allows backend mapping rules to evolve, prevents consumers from depending on native identifiers, and provides explicit fail-closed continuity semantics.

This alternative is selected.

## Consequences

### Benefits

- Explicit source selections can survive refreshes, process restarts, and temporary source absence when continuity is provable.
- Default-role intent stays independent from endpoint identity.
- Consumers receive stable equality semantics without backend coupling.
- Friendly-name collisions and native-ID lifecycle quirks cannot silently substitute a source.
- Windows and future PipeWire adapters can use different private evidence while exposing one contract.
- Source provenance and uninterrupted-stream identity remain distinct.

### Limitations

- The provider must own durable mapping state and conservative invalidation logic.
- Stability cannot exceed the evidence available from each platform backend.
- IDs are deliberately not portable across hosts or provider identity domains.
- A source may receive a new ID after ambiguous re-enumeration even when a human considers it the same physical device.
- Discovery snapshots can become stale immediately and do not reserve a source.
- The public Rust API and Windows capture runtime do not yet expose discovery or report the resolved registry-backed endpoint identity. A private Windows discovery implementation now proves enumeration, mapping, and resolution semantics.

### Future implementation impact

The implemented private Windows discovery layer enumerates active render endpoints, maps persistent `IMMDevice` endpoint IDs through the durable registry, resolves the console Default Playback role, and rejects stale or unsafe resolution. The portable consumer snapshot exposes platform-neutral descriptors without backend-native keys. The Windows capture adapter opens the exact endpoint from the revision-bound private mapping and reports its opaque `SourceId`; the PipeWire adapter must establish validated stable-property rules before explicit-source capture.

Tests will need to cover duplicate display names, metadata changes, default-role changes, disappearance/return with and without continuity, native-key reuse, mapping reset, stale discovery snapshots, invalid explicit IDs, and no-substitution behavior. Runtime work requires separate approval.

The original design milestone changed no runtime behavior. Milestone 6Q subsequently implemented the private Windows discovery and identity-mapping boundary without changing capture behavior or adding dependencies.

## Deferred Decisions

- consumer discovery request filters, public snapshot representation, and change delivery;
- explicit-source Windows capture;
- the PipeWire adapter, stable-property selection, and default-sink/monitor mapping;
- mapping storage format, location, retention, migration, repair, reset, and backup behavior;
- UI and consumer presentation, sorting, grouping, and source selection flows;
- authorization, source visibility, permission prompts, and multi-user identity domains;
- capability negotiation and format preflight in discovery;
- transport and serialization representation;
- endpoint watching, retry timing, replacement-owner creation, and recovery execution.

## Documentation Updates

- `README.md` records completion of the source-discovery-and-identity design milestone and its implementation deferrals.
- `docs/api.md` defines the conceptual discovery representation, scoped identity contract, and current API gap.
- `docs/architecture.md` records provider-owned mapping, lifecycle, and platform boundaries.
- `docs/roadmap.md` records Milestone 6N and the implementation work that remains deferred.
