# gpui-query — Test Coverage Analysis

**Date:** 2026-06-03
**Current state:** ~135 tests in core/integration layers. Hook layer has **zero** tests.

---

## Coverage Map

### ✅ Well-Tested (core layer)

| File | Tests | Coverage |
|------|-------|----------|
| `core/key.rs` | 12 inline | Construction, prefix matching, join, clone, serde |
| `core/key_filter.rs` | 6 inline | Exact, prefix, all matching |
| `core/signal.rs` | 5 inline | New, cancel, clone, default |
| `core/retry.rs` | 11 inline + 11 in `tests/core_retry.rs` | Policy, backoff, builder, serde |
| `core/refetch.rs` | 2 inline | Default, labels |
| `core/network_mode.rs` | 4 in `tests/core_network_mode.rs` | Default, labels, equality, serde |
| `tests/core_cache.rs` | 12 | TTL freshness, SWR, NoCache, invalidation, cache hit |
| `tests/core_lifecycle.rs` | 16 | begin, cancel, stale rejection, signal, reset |
| `tests/core_request.rs` | 3 | Sequencer monotonicity, scope changes |
| `tests/core_data_retention.rs` | 18 | Placeholder, previous_data, display_data, rollback |
| `tests/core_mutation.rs` | 16 | Lifecycle, retry, cancel, signal, status |
| `tests/core_infinite_query.rs` | 20 | Pages, append/prepend, max_pages, stale rejection |
| `tests/core_select.rs` | 5 | Transform, mapped resource, clone |
| `tests/integration_client.rs` | ~30 | QueryClient, buckets, GC, mutations, diagnostics |

**Total: ~135 tests**

### 🔴 Zero Coverage (hook layer)

| Function | File | Risk Level |
|----------|------|-----------|
| `use_query` | `hook/mod.rs:88` | **Critical** — retry loop, signal, entity lifecycle |
| `use_query_with_signal` | `hook/mod.rs:125` | **Critical** — cooperative cancellation |
| `use_query_manual` | `hook/mod.rs:162` | **High** — observation wiring |
| `fetch_query` | `hook/mod.rs:192` | **High** — imperative refetch |
| `fetch_query_with_signal` | `hook/mod.rs:217` | **High** — signal propagation |
| `mutate` | `hook/mod.rs:538` | **Critical** — mutation execution |
| `mutate_with_callbacks` | `hook/mod.rs:572` | **High** — callback firing |
| `use_mutation` | `hook/mod.rs:497` | **Medium** — entity creation |
| `use_mutation_with_options` | `hook/mod.rs:512` | **Medium** — options wiring |
| `use_mutation_state` | `hook/mod.rs:482` | **Medium** — global query |
| `use_infinite_query` | `hook/use_infinite_query.rs:105` | **Critical** — pagination |
| `fetch_next_page_infinite` | `hook/use_infinite_query.rs:182` | **Critical** — next page fetch |
| `fetch_previous_page_infinite` | `hook/use_infinite_query.rs:220` | **High** — prev page fetch |
| `fetch_with_retry` (internal) | `hook/mod.rs:260` | **Critical** — retry logic |
| `fetch_signal_with_retry` (internal) | `hook/mod.rs:340` | **Critical** — retry + signal |
| `run_mutation_loop` (internal) | `hook/mod.rs:610` | **Critical** — mutation retry |
| `run_mutation_loop_with_callbacks` (internal) | `hook/mod.rs:690` | **High** — mutation retry + callbacks |

### 🔴 Zero Coverage (observer)

| Type | File | Risk Level |
|------|------|-----------|
| `QueryObserver` | `client/observer.rs` | **High** — callback wiring |
| `ObserverConfig` | `client/observer.rs` | **Medium** — builder |

---

## Untested Scenarios

Even in well-tested areas, these scenarios have no coverage:

### Concurrency & Timing

| ID | Scenario | Risk |
|----|----------|------|
| COV-08 | Concurrent query invalidation during active fetch | **High** — stale guard rejection |
| COV-11 | GC during active observation — resource should not be collected | **High** — correctness |
| COV-16 | `RequestPolicy::IgnoreWhileLoading` under concurrent access | **Medium** — race condition |
| COV-17 | `max_pages` eviction with active fetches on both sides | **Medium** — data loss |

### Edge Cases

| ID | Scenario | Risk |
|----|----------|------|
| COV-04 | `set_data` / `clear_data` / `rollback_to_previous` in integration | **Medium** — optimistic update lifecycle |
| COV-05 | `prefetch_query` edge cases (already cached, already loading) | **Medium** — double-fetch |
| COV-06 | `cancel_query` + signal propagation | **High** — cooperative cancellation |
| COV-07 | `set_query_data` + `rollback_query_data` full lifecycle | **Medium** — optimistic update |
| COV-09 | `SelectTransform` with `QueryClient` integration | **Low** — derived data |
| COV-10 | Multiple observers on same query — callback ordering | **Low** — non-determinism |
| COV-12 | Mutation with invalidation side effects | **Medium** — cache coherence |
| COV-13 | Infinite query error recovery mid-pagination | **Medium** — state recovery |
| COV-14 | Cache policy transitions during loading | **Low** — edge case |
| COV-15 | Network mode behavior differences | **Low** — feature not wired |

### Unsafe Code

| ID | Scenario | Risk |
|----|----------|------|
| COV-17 | `ErasedBucket` wrong-type downcast returns `None` | **Medium** — safety invariant |
| COV-18 | `RequestId::label()` formatting | **Low** — diagnostics |

---

## Duplicate Tests

`core/infinite_query.rs` has 16 inline tests that overlap significantly with `tests/core_infinite_query.rs` (18 tests). Both cover the same methods:

| Inline Test | Integration Test | Overlap |
|-------------|-----------------|---------|
| `test_infinite_query_new` | `new_resource_is_idle` | Identical |
| `test_begin_fetch_next` | `begin_fetch_next_returns_request_id` | Identical |
| `test_complete_page_success` | `complete_page_success_appends_next_page` | Identical |
| `test_max_pages_eviction` | `max_pages_enforced_on_append` | Identical |
| `test_reset_clears_pages` | `reset_clears_everything` | Identical |
| `test_invalidate_clears_last_updated` | `invalidate_clears_last_updated` | Identical |

**Action:** Remove inline tests from `core/infinite_query.rs`, keep the more comprehensive integration versions.

---

## Test Infrastructure

### Current helpers (`tests/test_support.rs`)

```rust
pub(crate) fn resource() -> QueryResource<&'static str> {
    QueryResource::new("demo", CachePolicy::Ttl { ttl_ms: 1_000 }, RequestPolicy::LatestWins)
}

pub(crate) fn error_message<'a>(resource: &'a QueryResource<&'static str>) -> Option<&'a str> {
    resource.error().map(QueryError::message)
}
```

### Recommended additions

```rust
/// Builder for creating test resources with custom configuration.
pub(crate) struct TestResourceBuilder<T> {
    key: QueryKey,
    cache_policy: CachePolicy,
    request_policy: RequestPolicy,
    _marker: PhantomData<T>,
}

impl<T> TestResourceBuilder<T> {
    pub fn new(key: &str) -> Self { ... }
    pub fn cache_policy(mut self, policy: CachePolicy) -> Self { ... }
    pub fn request_policy(mut self, policy: RequestPolicy) -> Self { ... }
    pub fn build(self) -> QueryResource<T> { ... }
}

/// Shared test types.
pub(crate) struct User { pub id: u32, pub name: String }
pub(crate) struct Post { pub id: u32, pub title: String }

/// Lifecycle helper: begin request -> complete success.
pub(crate) fn begin_and_succeed<T: Clone>(
    resource: &mut QueryResource<T>,
    sequencer: &mut RequestSequencer,
    data: T,
) -> RequestGuard {
    let result = resource.begin_request(sequencer, current_time_ms(), QueryFetchMode::Normal);
    let QueryBeginResult::Started { request_id, .. } = result else { panic!("expected Started") };
    let guard = resource.accept_current_request(request_id).unwrap();
    resource.complete_success(&guard, data, current_time_ms());
    guard
}
```

---

## Recommended Test Plan

### Priority 1: Hook Tests (Critical Path)

```rust
// tests/hook_query.rs

#[gpui::test]
async fn test_use_query_fetches_on_idle(cx: &mut TestAppContext) {
    // use_query on an idle resource should auto-fetch
}

#[gpui::test]
async fn test_use_query_caches_on_cache_hit(cx: &mut TestAppContext) {
    // use_query with fresh cache should not re-fetch
}

#[gpui::test]
async fn test_use_query_retry_on_failure(cx: &mut TestAppContext) {
    // Fetcher fails -> retry -> succeeds
}

#[gpui::test]
async fn test_use_query_signal_cancellation(cx: &mut TestAppContext) {
    // Signal cancelled during fetch -> entity returns to idle/previous state
}

#[gpui::test]
async fn test_fetch_query_imperative_refetch(cx: &mut TestAppContext) {
    // fetch_query on a loaded entity triggers refetch
}
```

### Priority 2: Mutation Tests

```rust
// tests/hook_mutation.rs

#[gpui::test]
async fn test_mutate_success(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_mutate_failure_with_retry(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_mutate_with_callbacks_fires_on_success(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_mutate_with_callbacks_fires_on_error(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_mutate_with_callbacks_fires_on_settled_both_paths(cx: &mut TestAppContext) { ... }
```

### Priority 3: Infinite Query Hook Tests

```rust
// tests/hook_infinite.rs

#[gpui::test]
async fn test_use_infinite_query_initial_fetch(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_fetch_next_page_appends(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_fetch_previous_page_prepends(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_max_pages_eviction_during_fetch(cx: &mut TestAppContext) { ... }
```

### Priority 4: Observer Tests

```rust
// tests/observer.rs

#[gpui::test]
async fn test_observer_on_success_fires(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_observer_on_error_fires(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_observer_on_loading_fires(cx: &mut TestAppContext) { ... }

#[gpui::test]
async fn test_observer_on_settled_fires_both_paths(cx: &mut TestAppContext) { ... }
```

### Priority 5: Edge Case Tests

- Concurrent invalidation during active fetch
- GC does not collect actively observed resources
- Multiple observers on same query
- Mutation + invalidation side effects
- Infinite query error recovery mid-pagination
- `RequestPolicy::IgnoreWhileLoading` under concurrent access
- `ErasedBucket` wrong-type downcast returns `None`

---

## Test Naming Convention

Adopt a consistent convention across all test files:

```rust
// ✅ Good: method_should_X_when_Y
#[test]
fn begin_request_should_return_started_when_cache_is_stale() { ... }

#[test]
fn complete_success_should_update_data_when_request_id_matches() { ... }

// ❌ Bad: vague names
#[test]
fn test_basic() { ... }

#[test]
fn works() { ... }
```

Current files already mostly follow this in `core_lifecycle.rs` and `core_cache.rs`, but `core_mutation.rs` and `core_retry.rs` use `test_` prefixes.
