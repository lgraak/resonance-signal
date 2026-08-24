# Architecture

Resonance Signal is a standalone audio signal provider. It owns capture, signal processing, and the client-facing provider interface. External consumers remain outside the provider and consume exposed audio data rather than embedding capture implementations.

## Layer direction

```text
Audio Capture Layer
        |
        v
Audio Processing Layer
        |
        v
API / Client Interface
        |
        v
External Consumers
```

Data and dependencies should flow toward the client interface. Presentation and visualization concerns belong to consumers, not to Resonance Signal.

## Workspace boundaries

### `resonance-core`

Owns core data structures, shared types, and provider-independent logic. Its initial module skeleton separates signal concepts from future processing logic.

### `resonance-api`

Owns client-facing contracts, serialization types, and API definitions. No transport or network protocol is selected in the foundation milestone.

### `resonance-agent`

Provides the executable entry point. It will orchestrate capture and provider lifecycle work in later milestones.

## Foundation constraints

- No platform-specific capture library has been selected.
- No audio capture or processing behavior is implemented.
- No network service or transport is defined.
- No consumer or visualization code belongs in this repository.
