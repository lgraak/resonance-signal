# API

The `resonance-api` crate is reserved for the client-facing contract exposed by the provider.

## Foundation status

No public audio-data contract, serialization format, transport, or service endpoint is defined in this milestone. These choices are intentionally deferred until the core signal data model and consumer requirements can be evaluated together.

## Contract principles

- Contracts expose audio data and provider state, not visualization.
- Contracts remain independent of platform-specific capture implementations.
- Shared provider-independent types belong in `resonance-core`.
- Compatibility and versioning must be considered before the first usable contract is published.
