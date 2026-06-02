# GPUI Query MVP Plan

## Position

The starter should include a small, testable server-state layer, not a full React Query clone. The useful boundary is an internal `services::query` module that owns request/resource lifecycle primitives and can later be extracted into a `gpui-query` crate if more screens depend on it.

## MVP Scope

- Query keys and per-resource state.
- Idle, loading-empty, loading-with-data, success, failure, and cancelled states.
- Per-resource request ids so stale async results cannot overwrite newer state.
- Request scopes so reset state cannot collide with older in-flight request ids.
- Logical cancellation for blocking work that cannot be interrupted immediately.
- Cache policies: no cache, TTL cache, and stale-while-revalidate.
- Request policies: latest-wins and ignore-while-loading.
- Basic resource metrics: cache hits, cancellations, ignored stale results, and timestamps.
- Manual reset/invalidation can be built by owning services by clearing or replacing resources.

## Non-Goals For The Starter

- Devtools.
- Background polling, window-focus refetch, network reconnect refetch.
- Infinite queries, pagination helpers, optimistic mutation orchestration.
- Garbage collection timers.
- A task pool abstraction. GPUI already gives us foreground and background task APIs, and request lifecycle correctness is better handled by request ids plus entity/global state updates.

## Architecture

- `services::query` is transport-agnostic and knows nothing about HTTP, reqwest, or GPUI rendering.
- Feature services own domain keys and domain data, then store `QueryResource<T>` values.
- External consumers read resource state through accessors; lifecycle mutation goes through query transition methods.
- GPUI globals or entities remain responsible for reactivity and notification.
- Async work remains explicit at the feature-service layer with GPUI `spawn` and `background_executor`.
- UI components only read query state and dispatch feature actions.

## Extraction Criteria

Move this into a separate `gpui-query` crate only after at least two real screens need it and the API survives one refactor without changing public names. Before extraction, add mutation helpers, invalidation helpers, and entity-backed client helpers only if concrete screens need them.
