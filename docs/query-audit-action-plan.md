# gpui-query Audit — Action Plan

**Prioritized implementation plan** derived from the comprehensive audit (134 findings).
Each item includes the affected files, the fix, and the rationale.

---

## Phase 1: API Overhaul — Zero-Boilerplate Hooks

*Goal: Match TanStack Query's ergonomics. A simple query should go from ~10 lines to ~3 lines.*

---

### 1.1 Rewrite `use_query` to accept `QueryOptions` (B01, B08)

**Severity:** High | **Effort:** Medium | **Files:** `hook/mod.rs`, `hook/options.rs`

**Problem:** `use_query(key, cache_policy, request_policy, fetcher, cx)` takes 5 params. `QueryOptions` exists with all fields but is never used. Features like `retry_policy`, `initial_data`, `keep_previous_data` are inaccessible.

**Fix:**

```rust
// hook/mod.rs — new primary API
pub fn use_query<T, E, C, F, Fut>(
    options: impl Into<QueryOptions<T, E>>,
    fetcher: F,
    cx: &mut Context<C>,
) -> UseQueryResult<T, E>
where
    T: 'static,
    E: 'static,
    C: 'static,
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    let options = options.into();
    // ... existing logic, but now reads retry_policy, initial_data,
    // keep_previous_data, network_mode from options
}
```

**Migration:** Old signature becomes `use_query_raw()` or is deprecated. `impl Into<QueryOptions>` allows passing `QueryKey` directly for the simplest case.

---

### 1.2 Return named structs instead of tuples (B02)

**Severity:** High | **Effort:** Low | **Files:** `hook/mod.rs`, `hook/use_infinite_query.rs`

**Problem:** Every call site destructures `let (entity, _sub) = use_query(...)`.

**Fix:**

```rust
/// Result of `use_query` — holds the query entity and keeps the observation alive.
pub struct UseQueryResult<T, E> {
    /// The query state entity. Read data, status, error from this.
    pub entity: Entity<QueryResource<T, E>>,
    /// Keeps the observation alive. Store in your component struct.
    pub subscription: Subscription,
}

impl<T, E> UseQueryResult<T, E> {
    /// Convenience: read the current data, if any.
    pub fn data(&self, cx: &App) -> Option<T>
    where T: Clone { self.entity.read(cx).data().cloned() }

    /// Convenience: is the query currently loading?
    pub fn is_loading(&self, cx: &App) -> bool { self.entity.read(cx).is_loading() }
}

/// Result of `use_mutation` — holds the mutation entity.
pub struct UseMutationResult<V, T, E> {
    pub entity: Entity<MutationResource<V, T, E>>,
}

/// Result of `use_infinite_query` — holds pages and fetch methods.
pub struct UseInfiniteQueryResult<T, E> {
    pub entity: Entity<InfiniteQueryResource<T, E>>,
    pub subscription: Subscription,
    // Internal: stored fetcher for fetch_next/fetch_prev
    fetcher_next: Arc<dyn Fn(Option<&T>) -> Pin<Box<dyn Future<Output = Result<T, E>> + Send>> + Send + Sync>,
    fetcher_prev: Option<Arc<dyn Fn(Option<&T>) -> Pin<Box<dyn Future<Output = Result<T, E>> + Send>> + Send + Sync>>,
}
```

---

### 1.3 Fix `use_infinite_query` dead closure (B06)

**Severity:** High | **Effort:** Low | **Files:** `hook/use_infinite_query.rs`

**Problem:** The returned `fetch_next` closure is a no-op.

**Fix:** Return `UseInfiniteQueryResult` with stored fetchers:

```rust
impl<T, E> UseInfiniteQueryResult<T, E> {
    /// Fetch the next page. No-op if already fetching or no more pages.
    pub fn fetch_next<C: 'static>(&self, cx: &mut Context<C>) {
        let weak = self.entity.downgrade();
        let fetcher = self.fetcher_next.clone();
        let last_page = self.entity.read(cx).pages().last().cloned();

        cx.spawn(async move |cx| {
            let entity = weak.upgrade()?;
            let result = fetcher(last_page.as_ref()).await;
            entity.update(cx, |resource, cx| {
                match result {
                    Ok(page) => { resource.append_page(page); }
                    Err(e) => { /* handle error */ }
                }
                cx.notify();
            });
            Some(())
        }).detach();
    }

    /// Fetch the previous page. No-op if already fetching or no previous page.
    pub fn fetch_previous<C: 'static>(&self, cx: &mut Context<C>) {
        // Similar to fetch_next but prepends
    }
}
```

---

### 1.4 Merge `use_query` + `use_query_with_signal` (B10)

**Severity:** Medium | **Effort:** Medium | **Files:** `hook/mod.rs`

**Problem:** Two functions duplicating implementation; only difference is `Fn() -> Fut` vs `Fn(QuerySignal) -> Fut`.

**Fix:** Always pass `QuerySignal` to the fetcher. Users who don't need it can ignore the parameter:

```rust
pub fn use_query<T, E, C, F, Fut>(
    options: impl Into<QueryOptions<T, E>>,
    fetcher: F,
    cx: &mut Context<C>,
) -> UseQueryResult<T, E>
where
    F: Fn(QuerySignal) -> Fut + 'static,  // Always pass signal
    Fut: Future<Output = Result<T, E>> + 'static,
```

For backward compatibility, add a wrapper that wraps `Fn() -> Fut` into `Fn(QuerySignal) -> Fut`:

```rust
impl<F, Fut> From<F> for SignalFetcher<Fut>
where F: Fn() -> Fut + 'static {
    fn from(f: F) -> Self {
        Self::new(move |signal: QuerySignal| f())
    }
}
```

---

### 1.5 `use_mutation` should accept the mutator function (B03)

**Severity:** Critical | **Effort:** Medium | **Files:** `hook/mod.rs`

**Problem:** `use_mutation(cx)` returns a bare entity. User must call `mutate(&entity, vars, mutator_fn, cx)` each time, re-passing `cx`.

**Fix:**

```rust
pub fn use_mutation<V, T, E, C, F, Fut>(
    mutator: F,
    cx: &mut Context<C>,
) -> UseMutationResult<V, T, E>
where
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    let entity: Entity<MutationResource<V, T, E>> = cx.new(|_| MutationResource::new());
    // Store mutator in a way that mutate() can use it later
    UseMutationResult { entity }
}

impl<V, T, E> UseMutationResult<V, T, E> {
    pub fn mutate<C: 'static>(&self, variables: V, cx: &mut Context<C>) {
        // Uses the stored mutator function
    }

    pub fn mutate_with_callbacks<C: 'static>(
        &self,
        variables: V,
        callbacks: MutationCallbacks<T, E>,
        cx: &mut Context<C>,
    ) {
        // Uses the stored mutator function + callbacks
    }
}
```

---

### 1.6 Inline mutation callbacks (B05)

**Severity:** Medium | **Effort:** Low | **Files:** `hook/mod.rs`

```rust
// Target API:
result.mutate(vars, cx)
    .on_success(|data| { /* ... */ })
    .on_error(|err| { /* ... */ })
    .on_settled(|data, err| { /* ... */ });
```

---

### 1.7 `QueryClient::default()` (B11)

**Severity:** Medium | **Effort:** Trivial | **Files:** `client/mod.rs`

```rust
impl Default for QueryClient {
    fn default() -> Self {
        Self::new(CachePolicy::default(), RequestPolicy::default())
    }
}
```

Usage becomes: `cx.set_global(QueryClient::default());`

---

### 1.8 `ManagedQuery<T, E>` helper for direct-resource users (B14)

**Severity:** Medium | **Effort:** Medium | **Files:** New file `core/managed.rs`

```rust
/// Co-owns a QueryResource and its RequestSequencer.
/// Eliminates the need to manually manage sequencer pairs.
pub struct ManagedQuery<T, E = QueryError> {
    resource: QueryResource<T, E>,
    sequencer: RequestSequencer,
}

impl<T, E> ManagedQuery<T, E> {
    pub fn new(key: impl Into<QueryKey>, cache_policy: CachePolicy, request_policy: RequestPolicy) -> Self { ... }
    pub fn begin_request(&mut self, now_ms: u128, fetch_mode: QueryFetchMode) -> QueryBeginResult { ... }
    pub fn complete_success(&mut self, guard: &RequestGuard, data: T, now_ms: u128) { ... }
    pub fn complete_failure(&mut self, guard: &RequestGuard, error: E) { ... }
    // Delegate all QueryResource accessors...
}
```

---

## Phase 2: Correctness Fixes

---

### 2.1 Fix nested entity updates in bucket methods (GPQ-001)

**Severity:** High | **Effort:** Low | **Files:** `client/bucket.rs`

```rust
// Before:
fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
    for (key, entity) in &self.resources {
        if filter.matches(key) {
            let entity = entity.clone();
            entity.update(cx, |resource, _| { resource.invalidate(); });
        }
    }
}

// After:
fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
    let entities: Vec<_> = self.resources.iter()
        .filter(|(key, _)| filter.matches(key))
        .map(|(_, entity)| entity.clone())
        .collect();
    for entity in entities {
        entity.update(cx, |resource, _| { resource.invalidate(); });
    }
}
```

Apply same pattern to `reset_matching`.

---

### 2.2 Fix `RequestId::scoped(0, 0)` fallback (04)

**Severity:** High | **Effort:** Low | **Files:** `hook/mod.rs`

```rust
// Before:
if let Some(guard) = resource.accept_current_request(
    resource.active_request_id().unwrap_or(RequestId::scoped(0, 0)),
) { ... }

// After:
let Some(active_id) = resource.active_request_id() else {
    // Request was cancelled/reset while fetch was in flight.
    // Discard the result — this is expected behavior.
    cx.notify();
    return;
};
if let Some(guard) = resource.accept_current_request(active_id) {
    // ... complete the request
}
```

---

### 2.3 `QueryObserver::observe()` should return `Option` (03)

**Severity:** High | **Effort:** Low | **Files:** `client/observer.rs`

```rust
// Before:
pub fn observe<W: 'static>(&mut self, cx: &mut gpui::Context<W>) -> Subscription {
    let upgraded = self.entity.upgrade().expect("entity already dropped");
    // ...
}

// After:
pub fn observe<W: 'static>(&mut self, cx: &mut gpui::Context<W>) -> Option<Subscription> {
    let upgraded = self.entity.upgrade()?;
    // ...
    Some(subscription)
}
```

---

### 2.4 `QueryError` impl `Display + std::error::Error` (01)

**Severity:** High | **Effort:** Trivial | **Files:** `core/error.rs`

```rust
impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for QueryError {}

impl std::fmt::Display for QueryErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryErrorKind::Cancelled => write!(f, "cancelled"),
            QueryErrorKind::Response => write!(f, "response error"),
            QueryErrorKind::Transport => write!(f, "transport error"),
            QueryErrorKind::Unknown => write!(f, "unknown error"),
        }
    }
}
```

---

### 2.5 Replace `unwrap()` with `expect()` in `QueryClient` (02)

**Severity:** High | **Effort:** Trivial | **Files:** `client/mod.rs`

Extract a helper:

```rust
impl QueryClient {
    fn bucket_mut<T: 'static, E: 'static>(&mut self) -> &mut QueryBucket<T, E> {
        self.buckets
            .get_mut(&TypeId::of::<(T, E)>())
            .expect("QueryClient: bucket missing — call ensure_bucket first")
            .downcast_mut()
            .expect("QueryClient: type mismatch in bucket downcast")
    }
}
```

---

### 2.6 Fix mutation retry showing intermediate Failure status (GPQ-009)

**Severity:** Medium | **Effort:** Low | **Files:** `hook/mod.rs`

```rust
Err(error) => {
    let entity = match weak.upgrade() { Some(e) => e, None => return };

    if retry_policy.should_retry(attempt) {
        // Record failure WITHOUT notifying — skip intermediate UI state
        entity.update(cx, |resource, _| {
            resource.complete_failure(error);
        });
        // ... delay ...
        entity.update(cx, |resource, cx| {
            resource.retry();
            cx.notify(); // Only notify on meaningful transition
        });
    } else {
        // Final failure — notify observers
        entity.update(cx, |resource, cx| {
            resource.complete_failure(error);
            cx.notify();
        });
    }
}
```

---

## Phase 3: Performance

---

### 3.1 Remove `cx.notify()` from retry increments (PERF-002)

**Severity:** High | **Effort:** Low | **Files:** `hook/mod.rs`

In `fetch_with_retry`, `fetch_signal_with_retry`, and mutation loops — remove `cx.notify()` from `increment_retry()` calls. Only notify on terminal outcomes.

---

### 3.2 Default `max_pages` to bounded value (PERF-003)

**Severity:** High | **Effort:** Trivial | **Files:** `core/infinite_query.rs`

```rust
// Before:
max_pages: None,

// After:
max_pages: Some(50),
```

---

### 3.3 Replace `HashMap` with `AHashMap` (PERF-004)

**Severity:** High | **Effort:** Trivial | **Files:** `client/bucket.rs`, `client/mutation_bucket.rs`

```rust
// Before:
use std::collections::HashMap;

// After:
use ahash::AHashMap;
```

Add `ahash` to `Cargo.toml` dependencies.

---

### 3.4 Fix `Vec::remove(0)` in `enforce_max_pages` (06)

**Severity:** Medium | **Effort:** Trivial | **Files:** `core/infinite_query.rs`

```rust
fn enforce_max_pages_remove_front(&mut self) {
    if let Some(max) = self.max_pages {
        if self.pages.len() > max {
            self.pages.drain(..self.pages.len() - max);
        }
    }
}
```

---

### 3.5 Use `HashMap::retain` in `remove_matching` (07)

**Severity:** Medium | **Effort:** Trivial | **Files:** `client/bucket.rs`

```rust
fn remove_matching(&mut self, filter: &QueryKeyFilter) {
    self.resources.retain(|k, _| !filter.matches(k));
    self.sequencers.retain(|k, _| self.resources.contains_key(k));
}
```

---

## Phase 4: Documentation

---

### 4.1 Document all core types (DOC-001 through DOC-006)

**Severity:** High | **Effort:** Medium | **Files:** All files in `core/`

Priority order:
1. `CachePolicy`, `RequestPolicy`, `QueryBeginResult` (`core/policy.rs`)
2. `QueryStatus` (`core/status.rs`)
3. `QueryError` / `QueryErrorKind` (`core/error.rs`)
4. `QueryResource<T,E>` and key methods (`core/resource.rs`)
5. `RequestSequencer` / `RequestId` / `RequestGuard` (`core/request.rs`)
6. `SelectTransform` / `MappedQueryResource` (`core/select.rs`)

---

### 4.2 Add module-level docs

**Severity:** Medium | **Effort:** Low | **Files:** `core/mod.rs`, `client/mod.rs`, `hook/mod.rs`, `lib.rs`

Each module should have a `//!` doc comment explaining its purpose, layer, and how it fits in the architecture.

---

### 4.3 Fix inaccurate docs in `query-plan.md` (DOC-015, DOC-016)

**Severity:** Low | **Effort:** Trivial | **Files:** `docs/completed/query-plan.md`

- Update `use_mutation` signature to match actual implementation
- Change `.refetch_trigger()` to `.refetch_on_mount()`

---

## Phase 5: Test Coverage

---

### 5.1 Hook layer tests (COV-01)

**Severity:** High | **Effort:** High | **Files:** New `tests/hook_query.rs`, `tests/hook_mutation.rs`, `tests/hook_infinite.rs`

Priority tests:
1. `use_query` basic fetch-succeed lifecycle
2. `use_query` with retry (exponential backoff timing)
3. `use_query` with signal (cooperative cancellation)
4. `mutate` success / failure / retry
5. `mutate_with_callbacks` verifying callbacks fire
6. `use_infinite_query` initial fetch + fetch_next

---

### 5.2 Observer tests (COV-02)

**Severity:** High | **Effort:** Medium | **Files:** New `tests/observer.rs`

---

### 5.3 Remove duplicate infinite query tests (COV-14)

**Severity:** Low | **Effort:** Trivial

Remove inline tests from `core/infinite_query.rs`, keep the more comprehensive versions in `tests/core_infinite_query.rs`.

---

## Summary Table

| # | Action | Phase | Effort | Impact |
|---|--------|-------|--------|--------|
| 1.1 | `use_query` accepts `QueryOptions` | 1 | Medium | 🔴 Highest |
| 1.2 | Named return structs | 1 | Low | 🔴 High |
| 1.3 | Fix infinite query dead closure | 1 | Low | 🔴 High |
| 1.4 | Merge signal/non-signal variants | 1 | Medium | 🟠 Medium |
| 1.5 | `use_mutation` accepts mutator | 1 | Medium | 🔴 Critical |
| 1.6 | Inline mutation callbacks | 1 | Low | 🟠 Medium |
| 1.7 | `QueryClient::default()` | 1 | Trivial | 🟡 Low |
| 1.8 | `ManagedQuery<T,E>` helper | 1 | Medium | 🟠 Medium |
| 2.1 | Fix nested entity updates | 2 | Low | 🔴 High |
| 2.2 | Fix `RequestId` fallback | 2 | Low | 🔴 High |
| 2.3 | Observer returns `Option` | 2 | Low | 🔴 High |
| 2.4 | `QueryError` impl `Error + Display` | 2 | Trivial | 🔴 High |
| 2.5 | Replace `unwrap` with `expect` | 2 | Trivial | 🟠 Medium |
| 2.6 | Fix mutation retry UI flicker | 2 | Low | 🟠 Medium |
| 3.1 | Remove retry `cx.notify()` | 3 | Low | 🔴 High |
| 3.2 | Default `max_pages` | 3 | Trivial | 🔴 High |
| 3.3 | `AHashMap` | 3 | Trivial | 🟠 Medium |
| 3.4 | Fix O(n²) page removal | 3 | Trivial | 🟡 Low |
| 3.5 | Use `retain` | 3 | Trivial | 🟡 Low |
| 4.1 | Document core types | 4 | Medium | 🔴 High |
| 4.2 | Module-level docs | 4 | Low | 🟠 Medium |
| 4.3 | Fix docs accuracy | 4 | Trivial | 🟡 Low |
| 5.1 | Hook tests | 5 | High | 🔴 High |
| 5.2 | Observer tests | 5 | Medium | 🔴 High |
| 5.3 | Remove duplicate tests | 5 | Trivial | 🔵 Low |
