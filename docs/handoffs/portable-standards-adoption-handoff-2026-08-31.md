# Resonance Signal Portable Standards Adoption Handoff

Date: 2026-08-31 America/Los_Angeles
Status: completed; containing commit and publication readback supplied by final Executor response
Project: `lgraak/resonance-signal`
Repository: GitHub `lgraak/resonance-signal`
Branch: `main`
Implementation HEAD: `83bc8b8d28d22bfd5f559e4544942eb7bc76b115` starting revision; adoption changes and handoff share the containing commit
Standards revision: `46278c6b5d5f1ea687c16fce473967e402fa3c52`
Executor: OpenAI Codex
Model: GPT-5 (Codex)
Effort: not exposed
Previous handoff: `docs/handoffs/resonance-signal-explicit-source-capture-handoff-2026-08-24.md`
Containing handoff commit: not self-recordable; use final Executor response and authoritative remote readback.

## Objective and Outcome

Formally adopt the portable project standards at the exact approved revision,
create minimal project-local workflow guidance, retire copied legacy standards
as active authority, and preserve project architecture and historical handoffs.

The bounded process/bootstrap milestone is complete. It changes no product,
runtime, protocol, identity, recovery, packaging, version, release, deployment,
or acceptance behavior.

## Governing References

- `.project-standards.toml` pins the portable workflow authority and exact
  revision.
- `AGENTS.md` contains Resonance Signal-specific ownership, architecture,
  safety, release, and validation rules.
- Adopted standards at
  `46278c6b5d5f1ea687c16fce473967e402fa3c52`:
  `standards/project-bootstrap-standard-v1.md`,
  `standards/project-prompt-standard-v1.md`, and
  `standards/project-handoff-standard-v1.md`.
- `README.md`, `CONTRIBUTING.md`, `docs/architecture.md`, `docs/api.md`,
  `docs/consumer-protocol.md`, `docs/windows-beta.md`, `docs/roadmap.md`, and
  accepted ADRs under `docs/decisions/` remain project authority.
- Previous handoff:
  `docs/handoffs/resonance-signal-explicit-source-capture-handoff-2026-08-24.md`.

## Current Verified State

- Starting Resonance Signal state was clean `main` at
  `83bc8b8d28d22bfd5f559e4544942eb7bc76b115`, tracking `origin/main` with
  ahead/behind `0/0`.
- Live authoritative GitHub readback matched the starting revision before the
  first project write.
- The project-standards checkout and authoritative Gitea `main` both resolved
  to `46278c6b5d5f1ea687c16fce473967e402fa3c52`.
- Every adopted standard and template path existed at that exact revision
  before bootstrap writes began.
- Existing root-level and `docs/handoffs/` historical handoffs are preserved
  unchanged.
- The exact containing commit and authoritative post-push readback must be
  taken from the final Executor response.

## Work Completed

- Added the v1 adoption record with exact project identity, standards
  authority, 40-character revision, adopted paths, project instructions, and
  canonical handoff directory.
- Added project-local instructions containing only Resonance Signal-specific
  product, architecture, identity, lifecycle, beta, safety, release, and
  validation boundaries.
- Declared `docs/handoffs/` canonical for future meaningful handoffs while
  preserving all historical handoffs in place and format.
- Removed the two copied legacy prompt and handoff standards from the active
  tree. Git history and historical handoff references preserve their
  provenance.
- Confirmed that no current non-historical workflow documentation depended on
  the removed copies.

## Decisions and Constraints

- GitHub `lgraak/resonance-signal` remains authoritative for project behavior,
  architecture, documentation, validation, release state, and history.
- Gitea `aeons/project-standards` is authoritative only for the portable
  workflow standards pinned by `.project-standards.toml`.
- Standards are adopted by exact-revision reference rather than copied into
  the project.
- Historical handoffs are evidence of their original workflow and are not
  rewritten when workflow authority changes.
- No project-local exception to the portable standards is defined.
- Windows-only beta scope, provider/consumer separation, loopback-only
  transport, identity and stream invariants, recovery-disabled behavior, and
  the source-free package boundary remain unchanged.

## Validation and Evidence

- Exact standards revision, authoritative Gitea readback, and all five
  required standard/template paths: verified before the first project write.
- Adoption TOML parse and required-field/path validation: passed.
- Project-local instruction scope review: passed; no portable standard was
  copied wholesale.
- Current non-historical workflow-reference search: passed; no active reference
  depends on the removed standards.
- Historical handoff preservation check: passed; no historical handoff changed.
- `git diff --check`: passed.
- Gitleaks directory scan: passed; no leaks found.
- Final diff and scope review: passed; only adoption/workflow files changed.
- Rust build, test, documentation, packaging, and runtime suites: not run
  because this milestone changes documentation and workflow only.

## Unresolved Items

- Normal tray launch console visibility remains an unresolved beta issue.
- Intermittent unexpected process termination remains unresolved.
- Durable crash, panic, and lifecycle diagnostics remain needed before public
  beta.
- No runtime, package, tag, GitHub Release, deployment, or acceptance state was
  changed or revalidated by this milestone.

## Files Changed

- `.project-standards.toml`: added exact-revision portable standards adoption.
- `AGENTS.md`: added project-specific repository instructions.
- `docs/standards/ai-project-prompt-standard-v1.md`: removed copied legacy
  workflow authority.
- `docs/standards/ai-project-handoff-standard-v1.md`: removed copied legacy
  workflow authority.
- `docs/handoffs/portable-standards-adoption-handoff-2026-08-31.md`: added this
  adoption continuation checkpoint.

## Publication and Runtime State

- Working-tree adoption and reconciliation: complete.
- Local containing commit: created after this handoff is finalized; exact SHA
  is supplied by the final Executor response.
- Authoritative GitHub publication: performed only after final validation;
  exact readback is supplied by the final Executor response.
- Deployment, activation, package publication, Git tag, and GitHub Release:
  unchanged.
- Runtime acceptance: not performed.

## Safety, Rollback, and Access

- No historical handoff, product source, package artifact, private identity
  registry, credential, runtime process, device, listener, or external service
  was modified.
- If rollback is later required, use a separately authorized normal revert of
  the containing commit; do not rewrite shared history or restore copied
  standards as parallel active authority.

## Do Not Redo

- Do not copy the portable standards back into this repository.
- Do not treat historical references to the removed files as current workflow
  authority.
- Do not move or rewrite historical root-level or `docs/handoffs/` handoffs
  merely to match the current standard.
- Do not begin beta stability, diagnostics, console-window, release, Linux,
  consumer, or runtime work from this adoption milestone.

## Next Recommended Action

Implement the Windows beta stability and diagnostics milestone covering silent
tray launch, durable crash/panic/lifecycle diagnostics, and investigation of
intermittent unexpected process termination.
