# Roadmap

## Milestone 1: Foundation

- Buildable Rust workspace with `resonance-core`, `resonance-api`, and `resonance-agent`.
- Initial crate and module boundaries.
- Architecture, API, and roadmap documentation skeletons.
- Stable-Rust CI for Windows, Linux, formatting, checking, and builds.

## Milestone 2: Core contracts

- Define the provider-independent audio signal data model.
- Define the first client-facing contract.
- Establish compatibility and versioning expectations.
- Add focused tests for the published types and contracts.

## Later milestones

- Evaluate platform capture requirements and libraries.
- Implement capture providers behind platform-neutral boundaries.
- Implement signal-processing behavior.
- Define an appropriate client transport only when contract requirements justify it.

Consumer applications and visualization remain outside this roadmap and repository.
