# Resonance Signal Task Handoff

## Objective

Define and implement Milestone 2, the Resonance Signal Audio Data Contract, before any platform-specific capture, transport, consumer integration, or visualization work.

## Execution Context

- Host: `Arrakis`
- OS: Windows 11, `Microsoft Windows NT 10.0.26100.0`
- Rust host: `x86_64-pc-windows-msvc`
- Working directory: `D:\Aeons\Git\resonance-signal`
- Repository: `https://github.com/lgraak/resonance-signal`
- Branch: `main`
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- Additional validation runtime: existing Debian WSL environment

## Repository State

- Branch: `main`
- HEAD: `21fa7167cb2f247a91b06f7d47179b9ae5208c88`
- Working tree: dirty with only the uncommitted Milestone 2 files listed below
- Remote: `origin` is `https://github.com/lgraak/resonance-signal.git`
- Remote state: live `git ls-remote` verification showed `origin/main` at `21fa7167cb2f247a91b06f7d47179b9ae5208c88`, matching local HEAD before these uncommitted changes
- Commit/push/PR state: no commit created, nothing pushed, and no pull request created

## Completed Work

- Added validated provider-independent audio types to `resonance-core`:
  - non-zero sample rate and channel-count value types;
  - positioned and discrete channel layouts;
  - stream-relative frame timestamps and non-empty source windows;
  - bounded interleaved `f32` waveform frames;
  - per-channel RMS/sample-peak level frames;
  - single-sided linear magnitude spectrum frames with explicit FFT and window metadata;
  - construction-time validation for incomplete frames, non-finite values, channel mismatches, invalid level values, and invalid spectrum shapes.
- Added the transport-independent consumer contract to `resonance-api`:
  - contract version `0.1`;
  - opaque source and uninterrupted-stream identities;
  - default and explicit source selectors;
  - multi-source, multi-product subscription requests;
  - stream descriptors, lifecycle events, and typed signal packets;
  - platform-neutral error categories, scopes, recovery hints, and end reasons;
  - re-exports of the shared `resonance-core` signal types.
- Added ten focused unit tests covering successful construction and rejected/error paths.
- Replaced placeholder API and architecture documentation with the implemented contract.
- Added ADR 0001 with the decision, alternatives, consequences, stability boundary, and deferred work.
- Reconciled README and roadmap status with the completed milestone.
- Added no capture dependency, operating-system API, serialization dependency, network service, consumer integration, or visualization logic.

## Decisions Made

### Audio data model decisions

- Canonical samples are finite normalized linear PCM `f32` values. `-1.0` and `1.0` are nominal full scale; finite values outside that range remain valid to preserve processing headroom.
- Waveform samples are interleaved in sample-frame-major, channel-minor order.
- Frames are independently owned, bounded, variable-sized batches containing complete sample frames.
- Channel order is explicit. Known positions use a positioned layout; unknown positions use a discrete layout without guessing semantics.
- Time is stream-relative. Each frame identifies a zero-based source-frame index and nanoseconds in one uninterrupted stream's monotonic clock domain.
- A format change or interruption ends the current `StreamId`; recovery creates a new stream identity and timeline.
- Wall-clock time is not part of the frame contract.

### API contract decisions

- Raw waveform is the flexibility baseline and must remain available.
- Levels and magnitude spectra are separate opt-in products so common expensive processing can be shared without forcing derived data on every consumer.
- Levels are per-channel RMS and sample peak over an explicit source window.
- Spectra are channel-major, single-sided, coherent-gain-corrected linear peak magnitudes. The periodic Hann and rectangular window definitions, zero-padding behavior, bin frequency, and Nyquist scaling are explicit.
- One subscription can request several products from several sources.
- Sources are selected by default playback, default capture, or opaque provider-assigned ID. The contract does not assume one source or expose platform device paths.
- Errors separate stable machine-actionable category/scope/retry guidance from non-stable diagnostic text.
- The semantic contract is version `0.1`. Waveform, timing, identity, subscription, stream-boundary, and error semantics are 1.0 stability targets; derived configuration, discovery, serialization, and transport remain experimental.

### Rejected alternatives

- Backend-native integer sample variants: rejected because they move conversion and format branching into every consumer.
- Planar waveform as the baseline: rejected in favor of one unambiguous frame-major layout.
- Wall-clock timestamps as the primary timeline: rejected because they can jump and do not prove sample continuity.
- Raw-only output: rejected because it duplicates common level and FFT work in every consumer.
- Derived-only output: rejected because it prevents arbitrary consumer processing.
- One implicit default output: rejected because it cannot represent selected playback devices, microphones, virtual devices, or concurrent sources.
- Platform error types: rejected because consumers must not depend on WASAPI, Win32, PipeWire, or portal-specific failures.
- Selecting serialization or transport now: rejected because no evidence yet justifies freezing delivery mechanics or a wire schema.

## Files Changed

Created:

- `docs/decisions/0001-audio-data-contract.md`
- `resonance-signal-audio-contract-handoff-2026-08-23.md`

Modified:

- `Cargo.lock`
- `README.md`
- `crates/resonance-api/Cargo.toml`
- `crates/resonance-api/src/contract.rs`
- `crates/resonance-core/src/signal.rs`
- `docs/api.md`
- `docs/architecture.md`
- `docs/roadmap.md`

Removed:

- None.

## Validation Completed

- `cargo fmt --all --check`
  - Passed.
- `cargo check --workspace --all-targets`
  - Passed on `x86_64-pc-windows-msvc` with no warnings.
- `cargo test --workspace`
  - Attempted on the native Windows target.
  - Test sources compiled, but native linking could not start because this host has no `link.exe`, Visual C++ toolchain, or Windows SDK libraries.
  - Rust's bundled `rust-lld` was also tried and confirmed the specific missing SDK libraries, beginning with `kernel32.lib`.
- `cargo test --workspace --target x86_64-unknown-linux-musl --no-run`
  - Passed using Rust's bundled linker and statically linked test binaries.
- Final cross-target test execution in Debian WSL
  - Passed: 10 tests, 0 failed.
  - `resonance-agent`: 0 tests passed.
  - `resonance-api`: 4 tests passed.
  - `resonance-core`: 6 tests passed.
- `cargo doc --workspace --no-deps`
  - Passed; public Rust documentation generated successfully.
- `git diff --check`
  - Passed with no whitespace errors. Git emitted only the checkout's existing LF-to-CRLF conversion warnings.
- Final diff and scope review
  - Passed. Only the files listed above are changed; ignored build artifacts are not part of the working tree changes.
- Remote verification
  - Passed using `git ls-remote` with Git's OpenSSL backend; `origin/main` matches local HEAD.

Validation added the `x86_64-pc-windows-gnullvm` and `x86_64-unknown-linux-musl` Rust standard-library target components to the host's existing stable toolchain. No repository configuration uses or requires those targets.

## Unresolved Issues and Assumptions

### Known limitations

- Native Windows test linking remains unavailable on this workstation until a Visual C++ linker and Windows SDK libraries are installed or exposed on `PATH`. The same final test binaries compiled and all tests executed successfully using the Linux MUSL target in WSL.
- No capture backend, signal-processing implementation, device discovery, serialization, transport, service, or consumer exists yet. These are intentionally outside Milestone 2.

### Deferred decisions

- Device discovery, friendly names, capabilities, and source-ID persistence.
- Consumer negotiation of level window, hop interval, FFT size, and spectrum window.
- Serialized schema, protocol version, delivery transport, ordering across streams, bounded backpressure, authentication, and authorization.
- Optional wall-clock or cross-host clock correlation.
- Platform capture libraries and backend-specific diagnostics.

### Assumptions

- Capture backends can normalize supported source formats into finite `f32` PCM without making native sample representation part of the consumer contract.
- Providers can assign opaque source and stream identifiers without promising portability across hosts or device removal.
- Backend evidence will determine useful derived-analysis defaults before parameter negotiation is stabilized.

## Do Not Redo or Reopen

- Do not optimize the provider contract for InfoPanel or another first consumer.
- Do not add platform capture details to `resonance-core` or `resonance-api`.
- Keep finite interleaved `f32` waveform data as the canonical raw product unless new evidence demonstrates a contract-level loss that cannot be handled additively.
- Keep raw waveform independently available; derived products remain opt-in and cannot replace it.
- Preserve stream-relative frame index plus monotonic time. Do not substitute wall-clock time as the primary continuity mechanism.
- Treat interruption and format change as stream boundaries with a new `StreamId`.
- Keep source and stream identifiers opaque; do not expose WASAPI, PipeWire, or other backend identifiers as portable semantics.
- Do not choose a serialization format or transport without a separate evidence-backed decision.

## Next Recommended Action

Implement provider-independent level and spectrum processing in `resonance-core` against synthetic `AudioFrame` fixtures, including golden tests for RMS, peak, FFT scaling, windowing, overlap, and discontinuity handling. Use that evidence to define derived-analysis configuration and capability negotiation before starting a platform capture backend.
