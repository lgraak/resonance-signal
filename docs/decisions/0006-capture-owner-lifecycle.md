# ADR 0006: Capture owner lifecycle

- Status: Accepted
- Date: 2026-08-23

## Context

The production Windows capture boundary already accepts an explicit stop token, owns WASAPI resources on a dedicated COM thread, and emits bounded provider events on an ordinary processing thread. Its public entry point is blocking, however, so application code has no component that unambiguously owns startup, shutdown waiting, thread joins, callbacks, and the final result.

Milestone 6A needs one long-running owner without adding recovery policy. The owner must handle pre-start stop, repeated stop, shutdown races, worker failure, and deterministic cleanup. It must preserve the existing event and stream-identity contracts and keep all Windows types inside `resonance-agent`.

## Decision

Add a single-use Windows `CaptureOwner` in `resonance-agent`.

Construction is inert and owns only the callback, runner, and stop token. `start` may be called once. It transfers the callback and blocking capture run to a named ordinary worker thread. `CaptureOwnerStart::Started` reports worker creation; `StreamEvent::Started` continues to report successful capture initialization. If stop was requested before `start`, no worker or WASAPI resource is created and completion is `StoppedBeforeStart`.

Ownership is hierarchical:

1. `CaptureOwner` owns the ordinary worker handle, completion receiver, and stop token.
2. The ordinary worker owns provider-event processing, the consumer callback, the capture result, and the join obligation for its nested WASAPI thread.
3. The WASAPI thread alone owns COM initialization, endpoint/client objects, notification registrations, event handles, and native capture buffers.

`request_stop` is idempotent. `shutdown(timeout)` requests stop and waits for at most the caller-supplied duration. A timeout retains the worker handle, callback lifetime, and completion receiver in the same owner so the caller can wait again. A successful shutdown joins the worker and retains a typed completion state. Dropping a started owner is the final cleanup path: it requests stop and joins rather than detaching work.

Callbacks execute only on the ordinary worker and must return promptly. The bounded shutdown wait prevents the caller from being held indefinitely by a delayed event loop or blocked callback; after a timeout the owner must remain alive until another successful wait or final drop. Successful shutdown means all callbacks have finished and all nested capture resources have been released.

The blocking adapter also guards its WASAPI thread with request-and-join cleanup during error or panic unwinding. This prevents a processing failure or callback panic from detaching the COM/resource owner.

`CaptureOwner` contains no automatic reconnect, retry/backoff, default-device following, or endpoint-replacement acceptance policy. It cannot be restarted. A later recovery component must create a new owner explicitly, and the resulting capture run receives a new `StreamId`, frame index zero, and stream time zero.

## Consequences

- Application code has one obvious owner for startup, stop, completion, callbacks, and joins.
- Stop-before-start and repeated stop are defined without touching hardware.
- Shutdown timeout is observable without abandoning ownership or losing a later completion result.
- Successful shutdown and drop cleanup prevent capture threads, callbacks, and WASAPI resources from being detached.
- A callback that does not return promptly can cause shutdown to time out; callback behavior is therefore part of the owner contract.
- Reconnect remains visible, separately testable future policy rather than hidden behavior inside platform capture.

## Alternatives rejected

- Detach the worker after a shutdown timeout: rejected because callbacks and WASAPI resources could outlive their owner and completion would be lost.
- Block without a caller-supplied deadline: rejected because a delayed event loop or consumer callback could hold shutdown indefinitely.
- Put reconnect inside the owner: deferred because retry ownership, backoff, endpoint-following policy, and recovery acceptance require separate evidence.
- Move the lifecycle type into `resonance-api` or `resonance-core`: rejected because this owner controls a Windows platform implementation rather than the portable consumer contract.
