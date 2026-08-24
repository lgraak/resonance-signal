# Architecture

Resonance Signal is designed as a standalone audio signal provider.

The provider captures audio, processes signal data, and exposes it to independent clients.

Initial planned layers:

- Core audio data model
- Platform capture providers
- Client API

Consumers should not depend on platform-specific capture implementations.
