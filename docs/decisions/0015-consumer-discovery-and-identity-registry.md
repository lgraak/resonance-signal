# ADR 0015: Consumer discovery and identity registry

- Status: Accepted
- Date: 2026-08-24
- Extends: [ADR 0014](0014-source-discovery-and-identity-model.md)

# Decision

Adopt a revisioned consumer discovery snapshot backed by a private, durable provider identity registry.

Consumers receive a complete replaceable snapshot containing an opaque discovery revision and one `SourceDescriptor` per source the provider can currently describe. A descriptor contains the opaque provider-managed `SourceId`, presentation metadata, source kind, current availability, supported signal products, and point-in-time default-role membership. Presentation fields and current state never establish identity.

`resonance-agent` owns the identity registry, backend-native continuity evidence, schema migration, retirement, and reset. The registry persists across process restart and compatible provider upgrades within one installation on one host. It never makes IDs portable across hosts or installations. Registry commits are atomic; corruption, incompatible schema, failed migration, or ambiguous continuity loses identity continuity safely rather than guessing. Retired IDs remain tombstoned for the lifetime of their identity namespace and are never recycled.

A discovery revision is a freshness precondition, not identity. A future operation that acts directly on a discovery snapshot must submit its revision and fail with stale-discovery semantics if provider observation or registry state has changed. A durable Explicit Source intent still identifies the source by `SourceId`; the provider revalidates the current live registry mapping and availability before capture and never substitutes another source.

This ADR defines data and persistence semantics only. It does not implement enumeration, storage, platform adapters, public registry access, UI, InfoPanel integration, or recovery behavior.

## Context

### Source intent model

[ADR 0013](0013-source-selection-model.md) defines two consumer intents:

- Default Playback follows the platform role across separately started streams; and
- Explicit Source permits only one provider-assigned `SourceId`.

[ADR 0014](0014-source-discovery-and-identity-model.md) defines a snapshot-oriented discovery model and scopes source identity to one provider installation on one host. It deliberately leaves the exact descriptor, snapshot revision, registry persistence, migration, corruption, and reset contracts for this decision.

Discovery is needed because a consumer cannot construct a `SourceId`, infer it from a display name, or safely select a backend-native key. It needs a provider-issued identity plus enough current descriptive state to present or filter available choices. Discovery does not change the meaning of source intent: choosing a descriptor creates Explicit Source intent, while choosing Default Playback remains role intent.

### `SourceId` model

`SourceId` remains opaque, provider-managed, and scoped to one installation-on-one-host identity namespace. Consumers may store it, compare it for equality, and submit it as Explicit Source intent. They may not parse it, display it as a friendly name, derive it from metadata, or treat it as authorization.

`SourceId` is independent from `StreamId`. One source may produce many uninterrupted streams, and a proven return always starts a new `StreamId` even when it retains the same `SourceId`. A default role may move between source IDs without changing Default Playback intent.

### Why persistence matters

Without durable provider mapping, every process restart would create new IDs, invalidate stored explicit selections, and make temporary disappearance indistinguishable from replacement. Persistence permits continuity only when the provider retains both its namespace and sufficient backend evidence. It does not strengthen weak evidence: a persisted guess is still unsafe.

## Consumer Discovery Model

### Discovery snapshot

A discovery response is a `DiscoverySnapshot` with:

- `revision`: an opaque token bound to the current provider observation and committed registry revision; and
- `sources`: the complete set of descriptors the provider chooses to expose for that request.

The snapshot replaces an earlier snapshot; it is not a delta, reservation, event log, or capture guarantee. Source ordering has no identity meaning. A backend may omit an absent source or retain it as unavailable when that state can be represented truthfully. Omission alone does not retire its ID.

The provider issues a different revision whenever a discovery-visible observation changes or a registry transaction changes any live mapping, retirement state, or identity namespace. Repeating discovery against unchanged state may return the same revision. Consumers compare revisions only for equality and must not parse, order, or persist them as identity.

A future request made directly from a snapshot supplies both the chosen `SourceId` and that snapshot's revision. If the current revision differs, the provider rejects the precondition as stale and performs no capture resolution. The consumer refreshes discovery and decides again. This conservative rule can reject a selection after an unrelated source changes, but it makes use of stale presentation or identity state detectable.

A previously stored Explicit Source intent is not a stored snapshot. It may submit its `SourceId` without claiming snapshot freshness, but the provider must resolve it through the current live registry and revalidate identity and availability before starting capture. A missing, retired, or ambiguous ID fails closed.

### `SourceDescriptor`

Each descriptor contains these conceptual fields:

- `source_id: SourceId`: the only source identity and explicit-selection key;
- `display_name: Option<String>`: a non-empty human-readable label when the backend can provide one;
- `kind: SourceKind`: the normalized playback, microphone, virtual, or other classification;
- `availability: SourceAvailability`: `Available`, `Unavailable`, or `Unknown` at snapshot time;
- `capabilities: Set<SignalProduct>`: the provider-supported signal products that may be requested for this source; and
- `default_roles: Set<DefaultSource>`: the logical platform roles currently held by the source.

Capabilities are provider-contract facts, not raw backend flags. They say that the provider can attempt the named signal product for the source under the current contract; they do not promise current availability, a particular format, successful negotiation, or uninterrupted capture. An unadvertised product must not be requested from that snapshot. Exact Rust and transport representations remain implementation decisions.

Availability is explicitly three-state. `Available` means the provider currently has affirmative evidence that the source can be considered for resolution. `Unavailable` means it has affirmative evidence that the known source cannot currently be resolved. `Unknown` means the provider cannot make either claim. Every state is advisory and must be revalidated at capture time.

Default-role membership is point-in-time presentation state. Selecting a descriptor by ID remains Explicit Source intent even if that descriptor currently owns the default role. Selecting Default Playback remains role intent and may resolve a different source on a later attempt.

### Presentation is not identity

Display names may change and may be duplicated. Kind, capabilities, availability, default roles, format, transport, and other descriptive properties may also change without changing identity when native continuity remains proven. None may be used alone or in combination to merge, resurrect, or substitute a source.

Two sources named `Headset` remain two descriptors with different `SourceId` values. Renaming `WH-1000XM5` to another label changes snapshot presentation and revision, not the ID. Consumers use display names for presentation only and must tolerate absence, duplicates, and change.

## Identity Registry Model

### Ownership and boundary

The registry is private operational state owned exclusively by `resonance-agent`. Platform discovery adapters produce backend observations and continuity evidence; the registry validates and maps those observations; only normalized descriptors cross the consumer boundary. `resonance-core` and `resonance-api` do not expose native keys, evidence records, registry schemas, tombstones, migration state, or reset controls.

One logical writer serializes registry transactions. Discovery reads one committed registry revision and one corresponding observation generation so it cannot publish a descriptor set assembled across partially applied identity changes.

### Registry namespace

A registry contains:

- a schema version;
- an opaque identity-namespace identifier;
- installation and host binding;
- backend and continuity-evidence schema identities;
- a monotonically changing registry revision;
- live mappings from validated backend evidence to `SourceId`;
- lifecycle and last-proven-continuity state needed to distinguish live, absent, and retired mappings; and
- retired-ID tombstones with typed retirement reasons.

Source IDs are allocated in the registry's namespace-qualified allocation domain. A new namespace cannot resolve or issue an ID from an earlier namespace, so an old explicit selection cannot collide with a post-reset source. The exact encoding and allocator are deferred, but consumers must not be able to infer either. An allocator must check both live IDs and tombstones and must never issue an ID already present in that namespace.

The namespace begins only after its initial registry image has been durably committed. If a new durable namespace cannot be committed, discovery and explicit-source resolution are unavailable; the provider must not publish restart-unstable ephemeral IDs as if they satisfied this contract.

### Lifecycle

An unseen observation receives a new ID only after the registry can commit the mapping. Temporary disappearance changes the mapping to absent without retirement. Proven return restores the live mapping with the same ID. Descriptive metadata changes update discovery state but not identity evidence unless the backend contract explicitly defines that metadata as continuity evidence.

Retirement is permanent within the namespace. Ordinary compaction, upgrade, and migration retain tombstones. An old ID never aliases a new mapping, and a request for a retired ID never falls back to a live source.

### Reset

Reset is a private, explicit provider administration operation, not a consumer discovery feature. It atomically creates a new identity namespace and invalidates every ID from the old namespace. It does not attempt to remap old IDs into the new namespace, even when the same sources are immediately rediscovered.

Missing or unusable registry state also requires a new namespace before discovery can resume. Reset therefore causes deliberate safe loss of continuity. Consumers must rediscover and create new explicit intent; old IDs fail rather than silently resolving in the replacement namespace.

## Persistence and Migration

### Persistence scope

Registry identity is intended to survive:

- provider process restart on the same host and installation;
- ordinary source disappearance and proven return; and
- provider application upgrade when a supported migration completes successfully.

It is not intended to survive:

- copying the registry to another host;
- a separate provider installation;
- operating-system reinstallation or loss of the host binding;
- explicit reset or loss of the registry namespace; or
- backend re-enumeration for which continuity cannot be proven.

A host-binding mismatch is not a migration opportunity. The provider refuses the copied namespace and initializes a new one. Host migration, export, import, merging registries, and backup-based portability are not supported by this contract.

### Atomic commits

The logical storage contract is whole-registry transactional replacement:

1. write a complete candidate image with schema, namespace, revision, mappings, tombstones, integrity metadata, and commit marker;
2. make the candidate durable;
3. atomically replace the prior committed image; and
4. make the containing metadata update durable where the platform requires it.

A crash or partial write before replacement leaves the last valid committed image authoritative. An incomplete candidate is ignored. The provider never merges fragments from two revisions or exposes mappings before their commit succeeds. The actual storage engine, file layout, checksum, locking primitive, and platform-specific durability calls are deferred, but an implementation must demonstrate these semantics with fault injection.

### Missing and corrupted state

On first start, a missing registry creates and commits a new namespace before publishing discovery. At any later start, missing state is indistinguishable from lost continuity and has the same result: a new namespace and invalid old IDs.

Integrity validation occurs before any mapping is trusted. If no complete valid committed image exists, the provider must not salvage individual mappings by friendly name, backend key alone, or best-effort parsing. It preserves or quarantines the unusable artifact for diagnostics without exposing its contents, then atomically creates a new namespace. If quarantine or new-namespace commit fails, discovery and explicit-source resolution remain unavailable.

### Upgrades and schema migration

A provider upgrade may preserve the namespace only through an explicit, versioned migration path. Migration reads the old committed image, writes a complete candidate image, validates it, and commits it atomically; the old image remains authoritative until success.

Migration preserves an individual `SourceId` only when the new backend/evidence schema can prove that its mapping means the same source. A presentation-only or lossless schema change may preserve all IDs. If only some mappings cannot be proven, those IDs are retired with tombstones while proven mappings continue. Migration must also preserve all existing tombstones and the no-reuse invariant.

An unsupported future schema, incompatible schema, or failed migration is never guessed through. The provider keeps the original artifact for diagnosis and starts a new namespace only after an atomic replacement can be committed. Failure to commit the replacement leaves discovery unavailable rather than falling back to transient mappings.

### Backend and discovery-algorithm changes

A backend change preserves IDs only when a purpose-built migration has authoritative equivalence evidence between old and new backend identities. Display name, source kind, transport, format, and capability similarity are insufficient. Without such proof, affected IDs are retired or the namespace is reset.

A discovery-algorithm change that affects presentation only changes snapshot revision, not identity. A change to native-key selection, normalization, precedence, or continuity rules increments the evidence-schema identity and requires migration. Entries that cannot satisfy the new proof are retired rather than coerced into continuity.

## Invalidation Rules

A `SourceId` must be retired when the provider observes any of the following:

- the identity evidence becomes ambiguous or insufficient after re-enumeration;
- two observations make conflicting claims to the same mapping;
- a backend-native key is reused by a different source;
- a mapping contradicts authoritative lifecycle or replacement evidence;
- a record cannot be trusted after integrity validation or entry migration; or
- continuity would require an impossible or internally inconsistent claim.

Ordinary disappearance, display-name change, default-role change, format change, capability change, or temporary unavailability does not by itself retire an ID.

Retirement removes the mapping from live resolution, records a tombstone, increments the registry revision, and invalidates any snapshot issued against the prior revision. A newly observed source may receive a new ID only after the conflict is resolved well enough to distinguish it; otherwise it remains unresolved and is not exposed as selectable. A retired ID is never restored, reassigned, or redirected.

Registry-wide corruption or reset invalidates the entire namespace rather than manufacturing per-entry retirement claims from untrusted data. All old IDs then fail current-namespace resolution.

## Fake Backend Validation

A future implementation must provide a deterministic private test seam in `resonance-agent` with:

- scripted backend observation batches containing separate native continuity evidence and presentation metadata;
- an injectable registry store that can replay committed images and fail each atomic-commit stage;
- a deterministic namespace and ID allocator that can prove no-reuse behavior; and
- explicit observation and registry revisions so stale-snapshot behavior can be asserted.

The seam validates at least these cases:

### Duplicate names

Two observations named `Headset` with distinct native evidence produce two descriptors with distinct IDs. Ordering or name equality never merges them.

### Metadata change

`WH-1000XM5` changes its display name while continuity evidence remains valid. The snapshot revision and descriptor presentation change; `SourceId` does not.

### Proven return

A source disappears, remains mapped as absent, and returns with matching unambiguous evidence. It reuses the same `SourceId`. Any later capture uses a new `StreamId`.

### Ambiguous return

A similar source appears without sufficient continuity evidence. The prior ID is retired or remains absent according to the known lifecycle facts; the observation receives a new ID only if it can be distinguished safely, otherwise it remains unresolved.

### Native key reuse

A backend key previously bound to one source is observed with contradictory evidence for another source. The old ID is retired, never reassigned, and the new source receives a different ID only after its identity is unambiguous.

### Stale snapshot

Snapshot A is issued, then an observation or registry transaction changes the revision. A request claiming snapshot A's revision is rejected as stale before source resolution or owner creation, even if its selected descriptor still appears unchanged.

### No substitution

An Explicit Source request for an absent, retired, unknown, ambiguous, or stale-snapshot source never resolves Default Playback, a same-named source, or another available source.

### Persistence failures

Restart reuses a valid committed namespace and mappings. A partial candidate write retains the last valid commit. Corruption, incompatible schema, or failed migration never yields salvaged or guessed mappings; either a new namespace commits or discovery stays unavailable.

## Alternatives Considered

### 1. Expose backend-native IDs

Consumers could select Windows endpoint IDs or PipeWire properties directly. This avoids a registry, but exposes platform internals, inherits unvalidated reuse behavior, prevents backend-independent consumers, and makes future mapping changes consumer-visible breaking changes.

### 2. Use display names

Names are convenient to present but mutable, localized, and non-unique. Duplicate devices would collapse and rename could look like replacement. Names remain descriptor presentation only.

### 3. No persistence

Process-local IDs remove storage and migration work. They also invalidate every explicit selection at restart and cannot preserve proven return across provider lifetimes. This contradicts the accepted installation-scoped continuity model.

### 4. Provider-managed registry

The provider owns opaque IDs, private native evidence, durable mappings, revisions, retirement, and migration. This adds operational state and corruption handling, but provides a platform-neutral consumer contract, restart continuity, conservative invalidation, stale-snapshot detection, and freedom to evolve backend rules without exposing them.

This alternative is selected.

## Consequences

### Benefits

- Consumers receive one explicit, platform-neutral discovery descriptor contract.
- Identity remains separate from mutable presentation and current availability.
- Explicit selections can survive restart and compatible upgrade when continuity remains proven.
- Revision preconditions make stale snapshot use detectable.
- Atomic commits and fail-closed reset avoid partial or guessed mappings.
- Tombstones prevent accidental ID reuse and silent substitution.
- Backend and evidence rules can evolve privately through versioned migration.

### Limitations

- Conservative global snapshot revisioning can require rediscovery after an unrelated source changes.
- Corruption, unsupported migration, host change, or reset deliberately invalidates stored selections.
- A human may recognize a returning device that the provider must assign a new ID.
- Durable private identity uses installation-bound state, atomic registry persistence, namespace/ID allocation, migration checks, and corruption handling; the storage implementation is now present in `resonance-agent`.
- IDs remain installation- and host-scoped, not portable account or hardware identities.

### Implementation impact

`resonance-api` represents the transport-independent consumer contract with owned `DiscoverySnapshot` and `SourceDescriptor` values, an opaque equality-only `DiscoveryRevision`, three-state `SourceAvailability`, duplicate-free supported-product and default-role sets. `resonance-agent` owns the private registry boundary and transactional storage implementation, including namespace and ID allocation, permanent tombstones, migration handling, reset, and fake-backend/fault-injection seams. The Windows discovery adapter supplies persistent `IMMDevice` endpoint IDs as private evidence, enumerates active render endpoints, resolves the console default role, retains known absent playback sources as unavailable, and converts private revision-aware snapshots to the portable value without native evidence or registry internals. Default Playback capture refreshes and revalidates that private mapping at attempt start; ADR 0016 maps only the portable result to the loopback v1 transport.

Milestones 6P through 6U implement durable registry persistence, continuity fallback, private Windows playback discovery, the portable Rust discovery contract, separate capture-time Default Playback and Explicit Source mappings, and the consumer-visible local service. Public storage schema, endpoint watching, and remote service behavior remain deferred.

## Deferred Decisions

- storage schema migration;
- discovery paging and change notification;
- Windows explicit-source resolution and capture;
- PipeWire discovery, stable-property mapping, and Linux implementation;
- UI, sorting, grouping, localization, and consumer source-selection flows;
- InfoPanel integration;
- authorization, multi-user visibility, and registry administration interface;
- registry export, import, backup, host transfer, or namespace merge;
- exact capability negotiation and format preflight beyond the `SignalProduct` set;
- endpoint watching, retry timing, replacement-owner creation, and every recovery behavior.

## Documentation Updates

- `README.md` records completion of the consumer-discovery-and-identity-registry design milestone and its runtime deferrals.
- `docs/api.md` records the snapshot, descriptor, revision, and stale-selection contract.
- `docs/architecture.md` records the private registry ownership, persistence, migration, and fail-closed boundary.
- `docs/roadmap.md` records Milestone 6O and the remaining implementation work.
