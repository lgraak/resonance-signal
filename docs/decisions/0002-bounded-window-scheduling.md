# ADR 0002: Bounded analysis-window scheduling

- Status: Accepted
- Date: 2026-08-23

## Context

Capture backends will produce variable-sized `AudioFrame` batches, while downstream level and future spectrum processing need predictable, frame-aligned analysis windows. The scheduler must support real-time visualizations without embedding platform capture, transport, async runtime, or signal-processing policy. It must not accumulate input without bound or silently combine discontinuous samples.

Thirty and sixty updates per second are the immediate visualization cases. The design must allow later adjustment without turning Resonance Signal into a recording or sample-perfect offline engine.

## Decision

### Window and cadence model

Use configurable, non-overlapping tumbling windows driven by source sample frames.

- The default target duration is `33,333,333 ns`, approximately 30 outputs per second.
- A caller can select `16,666,667 ns` for approximately 60 outputs per second or another non-zero duration.
- Each stream converts duration to the nearest whole sample-frame count. At 48 kHz the 30 FPS and 60 FPS targets are exactly 1,600 and 800 frames.
- The first accepted sample begins the first window. A complete window is emitted immediately; the next begins at its end. No overlap or separate hop size is used.
- Partial input remains pending until completed. Empty or slow input does not produce padding, repetition, silence, or incomplete output.

Cadence is therefore predictable in the source clock domain. End-to-end latency is one target window plus capture-batch arrival and downstream processing time. The scheduler has no wall-clock timer and cannot flush when input stops.

### Buffer bounds

Retain fewer than one window of partial samples. Process complete windows synchronously during each push rather than queuing them internally.

`max_windows_per_push` bounds the sample frames accepted and the output vector created by one call. The default is eight, equivalent to at most about 267 ms of input at the default cadence. Oversized input is rejected before scheduler state changes. A caller can split a large capture batch or deliberately configure a different bound.

This bounds scheduler-owned retention and per-call work. It does not define transport output queues or consumer backpressure; those must be separately bounded when a transport exists.

### Continuity and stream boundaries

The orchestration layer supplies an uninterrupted-stream identity with every push. The scheduler remains generic over its representation so `resonance-core` does not depend on `resonance-api`.

Within one identity:

- sample rate and channel layout remain fixed;
- frame index must exactly equal the preceding batch's end;
- stream timestamp must equal the preceding batch's calculated end within one nanosecond of integer quantization;
- samples from several valid input batches may form one output window.

A gap, overlap, timestamp discontinuity, or format change returns an explicit scheduling error, discards the partial window, and invalidates the identity. Further data must use a new identity. A normal identity change discards any partial old window and reports a boundary with the discarded frame count before processing the new stream. Cross-stream or discontinuous samples are never combined.

### Processing boundary

The scheduler emits owned `AudioFrame` values with the calculated source window and preserved format. Existing `WaveformWindow::entire` and processing functions consume these frames directly. Scheduling does not duplicate RMS, peak, normalization, or future spectrum logic.

## Alternatives considered

### One output per capture batch

Rejected because backend period sizes are variable and platform-specific. It would make visualization cadence depend on capture implementation details.

### Wall-clock timer with partial flushes

Rejected for this milestone. Padding or emitting incomplete windows changes analysis semantics, while a timer adds async/runtime policy before capture evidence exists.

### Overlapping windows with a separate hop

Deferred. Overlap can improve spectrum smoothness but increases processing and complicates buffering. Current level and visualization use cases need predictable cadence more than spectral resolution.

### Unbounded accumulation or output queue

Rejected. Slow or absent consumers must not allow scheduler memory to grow indefinitely. Transport backpressure requires a separate explicit policy.

### Put `StreamId` in `resonance-core`

Rejected. The scheduler only requires equality and cloning; making it generic preserves the existing API ownership of opaque consumer-facing identities.

## Consequences

- Capture orchestration must attach the correct uninterrupted-stream identity and start a new one after any discontinuity.
- Capture batches larger than the configured per-push bound must be split or rejected by orchestration.
- Default level updates incur approximately 33.3 ms of windowing latency before capture and downstream overhead; selecting 60 FPS halves the windowing latency and sample count.
- Completed output is immediately available to existing processing primitives with no new DSP dependency.
- Output queuing, backpressure, timeout behavior, overlapping hops, and capture-thread integration remain deferred.
