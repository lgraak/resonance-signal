# Resonance Signal Repository Instructions

These instructions apply to all work in this repository.

## Project identity and portable workflow

- Project: `lgraak/resonance-signal`.
- GitHub `lgraak/resonance-signal` is authoritative for project source,
  architecture, validation, documentation, release state, and history.
- Read `.project-standards.toml`, then read the named portable standards at the
  exact adopted revision before substantive work.
- The adopted standards govern the Observer/Executor workflow. This file
  contains only Resonance Signal-specific ownership, architecture, safety,
  validation, and release rules.
- `docs/handoffs/` is canonical for future meaningful handoffs. Root-level and
  existing `docs/handoffs/` files remain historical evidence; do not move or
  rewrite them merely to match the current format.
- No project-local exceptions to the adopted portable standards are defined.

## Product and architecture boundaries

- Resonance Signal is the standalone audio signal provider. It owns capture,
  provider-side signal processing, portable contracts, and the local consumer
  service.
- Consumers, including Auraline and InfoPanel, are separate projects. Do not
  add consumer-specific presentation, visualization, or project coupling.
- Preserve dependency direction:
  `resonance-agent -> resonance-api -> resonance-core`. Platform capture and
  transport must not enter `resonance-core`; operating-system capture types
  must not enter `resonance-api`.
- Windows x64 is the current beta platform. Linux/PipeWire remains deferred
  until a separately authorized milestone after Windows consumer acceptance.
- Consumer transport remains numeric-loopback-only. Non-loopback operation
  requires a separately approved security and deployment design.

Read `README.md`, `docs/architecture.md`, `docs/api.md`,
`docs/consumer-protocol.md`, `docs/windows-beta.md`, `docs/roadmap.md`, and the
applicable accepted ADRs under `docs/decisions/` before changing the behavior
or boundary they govern.

## Identity, stream, and lifecycle invariants

- Default Playback is logical role intent resolved for each new attempt.
  Explicit Source selects one opaque provider-managed `SourceId` and must fail
  closed rather than substitute another source.
- Backend-native endpoint identifiers and identity-registry internals remain
  private to `resonance-agent`. Display names, default status, and formats are
  not identity proof.
- `SourceId` identifies a source only within the retained provider
  installation-and-host identity domain. `StreamId` identifies one
  uninterrupted capture lifetime.
- Interruption, restart, disappearance, replacement, reconfiguration, timing
  discontinuity, or format change ends the stream. A later attempt receives a
  new `StreamId`, frame index zero, and stream-relative timeline; active streams
  never migrate in place.
- Capture output remains finite interleaved `f32` mono or two-channel stereo.
  Do not silently select channels, invent positions, or introduce a custom
  downmix.
- Automatic recovery remains disabled. Do not add retry execution, timers,
  endpoint watching, reconnect, or replacement owners without a separately
  authorized evidence-backed milestone.

## Windows beta and release boundaries

- Normal beta launch is a per-user tray runtime that owns the loopback service
  and the explicit opt-in Start with Windows registration.
- Preserve the source-free Windows package contract unless a release milestone
  explicitly changes it. Package identity derives from Cargo metadata and must
  match executable `--version` output and the archive name.
- Do not create or change versions, tags, GitHub Releases, package publication,
  deployment, activation, or runtime state unless the current milestone
  explicitly authorizes that exact state change.
- Keep lifecycle diagnostics bounded and exclude audio samples, native endpoint
  identifiers, private identity-registry contents, credentials, and secrets.
- Do not delete or reset the private identity registry as routine cleanup;
  doing so intentionally invalidates prior opaque source identities.

## Project-specific validation

- For code changes, run `cargo fmt --all --check`,
  `cargo check --workspace --all-targets`, `cargo test --workspace`, and
  `cargo doc --workspace --no-deps`. Clippy is not currently a required clean
  workspace gate.
- For protocol, identity, lifecycle, concurrency, or failure-path changes, add
  focused coverage for both accepted and rejected/error paths and then run the
  relevant broader workspace checks.
- For packaging or runtime claims, follow `docs/windows-beta.md` and validate
  the packaged artifact or live runtime directly; source and workstation tests
  do not establish package or runtime acceptance.
- Documentation/process-only milestones do not require the Rust build suite
  unless they change executable behavior or another project instruction
  explicitly requires it.
- Always run `git diff --check`, review the final diff, and confirm that only
  intended files changed. Record only validation actually observed.
