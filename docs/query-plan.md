# gpui-query Reference Documentation

> A TanStack Query-inspired async state management library for GPUI.

---

## Table of Contents

1. [Overview](#overview)
2. [What's Done](#whats-done)
3. [What's Left](#whats-left)
4. [Consumer Usage](#consumer-usage)
5. [Architecture Decisions](#architecture-decisions)
6. [Test Coverage](#test-coverage)
7. [Migration Notes](#migration-notes)

---

## Overview

**gpui-query** is a Rust crate that brings the async state management patterns of [TanStack Query](https://tanstack.com/query) to the GPUI framework. It manages loading states, caching, deduplication, and stale data revalidation for asynchronous data fetching in desktop applications.

### Three-Layer Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                        HOOK LAYER                                │
│  use_query / use_query_manual / fetch_query / use_mutation      │
│  use_infinite_query / fetch_with_retry                          │
│  QueryOptions<T, E> / MutationOptions<V, T, E>                  │
│  InfiniteQueryOptions<T, E> / MutationCallbacks<T, E>           │
│  Component-facing ergonomic API                                 │
├──────────────────────────────────────────────────────────────────┤
│                       CLIENT LAYER                              │
│  QueryClient (Global registry)                                  │
│  QueryBucket<T, E> / QueryBucketTrait / BucketDefaults          │
│  MutationBucket<V, T, E> / MutationBucketTrait / MutationDefaults│
│  QueryObserver<T, E> / ObserverConfig<T, E>                     │
│  QueryDiagnostic / ClientDiagnostic (DevTools)                  │
│  Type-partitioned storage, bulk operations, GC, diagnostics     │
├──────────────────────────────────────────────────────────────────┤
│                        CORE LAYER                               │
│  QueryResource<T, E>    QueryKey        QueryStatus             │
│  MutationResource<V,T,E> MutationStatus  RetryPolicy            │
│  InfiniteQueryResource<T,E>  SelectTransform<T,U>               │
│  CachePolicy / RequestPolicy / QueryFetchMode / QueryBeginResult│
│  RequestId / RequestSequencer / RequestGuard / QueryTimestamp   │
│  QueryKeyFilter / QueryError / QuerySignal                      │
│  NetworkMode / RefetchTrigger / MappedQueryResource<T,U,E>      │
│  Framework-free (serde-only), pure state machines               │
└──────────────────────────────────────────────────────────────────┘
```

### Design Philosophy

| Principle | Implementation |
|---|---|
| **Framework-free core** | The core layer depends only on `serde`. All types are testable without GPUI. |
| **No stored fetcher** | QueryResource owns state, not behavior. Fetchers are provided at call sites. |
| **Type-partitioned storage** | Resources are grouped by `(T, E)` type pairs via `TypeId`, enabling type-safe access without downcasting. |
| **Monotonic request IDs** | `RequestSequencer` generates ever-increasing IDs. Stale completions are silently rejected. |
| **Scope-based invalidation** | `advance_scope()` invalidates all in-flight requests at once without iterating. |
| **Arc-based keys** | `QueryKey` uses `Arc<[Arc<str>]>` for cheap cloning across the entity graph. |
| **Retry with backoff** | `RetryPolicy` with fixed or exponential backoff, max delay cap, and per-resource configuration. |
| **Mutation subsystem** | `MutationResource<V, T, E>` with Idle/Loading/Success/Failure lifecycle, retry support, and `use_mutation` hook. |
| **Infinite pagination** | `InfiniteQueryResource<T, E>` with page append/prepend, bidirectional fetching, and `max_pages` eviction. |
| **Select transforms** | `SelectTransform<T, U>` for mapping cached data before returning, with `MappedQueryResource` view. |
| **QueryObserver** | Standalone observer with configurable callbacks (`on_success`, `on_error`, `on_loading`, `on_settled`). |
| **DevTools diagnostics** | `QueryClient::diagnostics()` and `query_diagnostics::<T, E>()` for introspecting all query state. |

### Project Stats

| Metric | Value |
|---|---|
| Total lines of code | 5,638 |
| Test lines of code | 3,127 |
| Number of tests (crate) | 222 |
| Number of tests (consumer) | 44 |
| Public types | 36 |
| Public methods | 248 |
| Feature completion (DONE) | 41 / 41 |
| Feature completion (PARTIAL) | 0 / 41 |
| Feature completion (TODO) | 0 / 41 |

---

## What's Done

All implemented features, organized by architectural layer.

### Core Layer

#### QueryKey — Structured Hierarchical Cache Key

`crates/gpui-query/src/core/key.rs:35`

An `Arc<[Arc<str>]>`-backed key supporting exact and prefix matching. Mirrors TanStack's array key pattern.

| Method | Line | Description |
|---|---|---|
| `QueryKey::new` | `key.rs:39` | Create a key from an iterator of string-like parts |
| `QueryKey::from_single` | `key.rs:48` | Create a single-segment key from a string |
| `QueryKey::parts` | `key.rs:53` | Returns the key segments as a slice of `Arc<str>` |
| `QueryKey::as_single` | `key.rs:60` | Returns the single segment if key has exactly one part, else `None` |
| `QueryKey::as_str` | `key.rs:71` | Returns the first segment as a string slice |
| `QueryKey::starts_with` | `key.rs:81` | Returns `true` if this key starts with the given prefix. Empty prefix matches everything |
| `QueryKey::join` | `key.rs:95` | Create a new key by appending an extra segment |

**Tests:** 12 tests covering construction, matching, and edge cases.

#### QueryKeyFilter — Bulk Operation Filter

`crates/gpui-query/src/core/key_filter.rs:19`

A filter enum for matching query keys in bulk operations.

```rust
pub enum QueryKeyFilter<'a> {
    Exact(&'a QueryKey),
    Prefix(&'a QueryKey),
    All,
}
```

| Method | Line | Description |
|---|---|---|
| `QueryKeyFilter::matches` | `key_filter.rs:30` | Returns `true` if the given key matches this filter |

**Tests:** 6 tests.

#### QueryStatus — Six-State Lifecycle

`crates/gpui-query/src/core/status.rs:4`

Richer than TanStack's four-state machine. Distinguishes loading with and without prior data, and includes an explicit `Cancelled` state.

```rust
pub enum QueryStatus {
    Idle,
    LoadingEmpty,
    LoadingWithData,
    Success,
    Failure,
    Cancelled,
}
```

| Method | Line | Description |
|---|---|---|
| `QueryStatus::label` | `status.rs:14` | Returns a human-readable static string label |
| `QueryStatus::is_loading` | `status.rs:25` | Returns `true` if `LoadingEmpty` or `LoadingWithData` |

#### QueryError — Error Classification

`crates/gpui-query/src/core/error.rs:4`

```rust
pub enum QueryErrorKind {
    Cancelled,
    Response,
    Transport,
    Unknown,
}

pub struct QueryError {
    kind: QueryErrorKind,
    message: String,
}
```

| Method | Line | Description |
|---|---|---|
| `QueryError::new` | `error.rs:18` | Construct from a kind and message |
| `QueryError::cancelled` | `error.rs:25` | Construct a Cancelled-variant error |
| `QueryError::response` | `error.rs:29` | Construct a Response-variant error |
| `QueryError::transport` | `error.rs:33` | Construct a Transport-variant error |
| `QueryError::unknown` | `error.rs:37` | Construct an Unknown-variant error |
| `QueryError::kind` | `error.rs:41` | Returns the error kind |
| `QueryError::message` | `error.rs:45` | Returns the error message string |

#### CachePolicy — Cache Strategy

`crates/gpui-query/src/core/policy.rs:6`

```rust
pub enum CachePolicy {
    NoCache,
    Ttl { ttl_ms: u64 },
    StaleWhileRevalidate { ttl_ms: u64 },
}
```

| Method | Line | Description |
|---|---|---|
| `CachePolicy::label` | `policy.rs:13` | Human-readable label string |
| `CachePolicy::can_short_circuit` | `policy.rs:23` | Returns `true` if this policy can serve cached data without starting a request |
| `CachePolicy::ttl_ms` | `policy.rs:27` | Returns the TTL in milliseconds, or `None` for `NoCache` |

#### RequestPolicy — Concurrency Policy

`crates/gpui-query/src/core/policy.rs:36`

```rust
pub enum RequestPolicy {
    LatestWins,           // Cancel previous, start new
    IgnoreWhileLoading,   // Deduplicate by ignoring new requests
}
```

| Method | Line | Description |
|---|---|---|
| `RequestPolicy::label` | `policy.rs:42` | Human-readable static string label |

#### QueryFetchMode — Cache Bypass

`crates/gpui-query/src/core/policy.rs:51`

```rust
pub enum QueryFetchMode {
    Normal,  // Respect cache
    Force,   // Bypass cache
}
```

#### QueryBeginResult — Request Start Outcome

`crates/gpui-query/src/core/policy.rs:58`

```rust
pub enum QueryBeginResult {
    Started { request_id: RequestId, status: QueryStatus, replaced_request_id: Option<RequestId> },
    CacheHit,
    IgnoredWhileLoading { active_request_id: RequestId },
}
```

#### RequestId / RequestSequencer / RequestGuard

`crates/gpui-query/src/core/request.rs`

The request identity and lifecycle system:

| Type | Line | Role |
|---|---|---|
| `RequestId` | `request.rs:4` | Opaque identifier: scope_id + sequence |
| `RequestSequencer` | `request.rs:30` | Monotonic ID generator with scope overflow handling |
| `QueryTimestamp` | `request.rs:70` | Millisecond-precision timestamp for cache age |
| `RequestGuard` | `request.rs:93` | Proof-of-ownership token for completing a request |

| Method | Line | Description |
|---|---|---|
| `RequestId::scoped` | `request.rs:12` | Create a request ID with explicit scope and sequence |
| `RequestId::value` | `request.rs:16` | Returns the sequence number component |
| `RequestId::scope_id` | `request.rs:20` | Returns the scope ID component |
| `RequestId::label` | `request.rs:24` | Returns a `scope:sequence` formatted string for debugging |
| `RequestSequencer::new` | `request.rs:42` | Create a new sequencer starting at scope 1, sequence 1 |
| `RequestSequencer::next_request` | `request.rs:49` | Allocate the next monotonic `RequestId` |
| `RequestSequencer::advance_scope` | `request.rs:59` | Increment the scope ID and reset the sequence counter |
| `RequestSequencer::is_current_scope` | `request.rs:64` | Returns `true` if the given ID belongs to the current scope |
| `QueryTimestamp::from_millis` | `request.rs:73` | Create a timestamp from milliseconds |
| `QueryTimestamp::as_millis` | `request.rs:77` | Returns the timestamp value as milliseconds |
| `RequestGuard::request_id` | `request.rs:102` | Returns the `RequestId` this guard wraps |

**Tests:** 3 tests for RequestSequencer.

#### QueryResource\<T, E\> — Core State Machine

`crates/gpui-query/src/core/resource.rs:13`

The central type. Owns cache and request state for one resource. Methods are split across submodules:

- `accessors.rs` — read-only state accessors
- `cache.rs` — cache freshness and invalidation
- `completion.rs` — request completion (success/failure)
- `lifecycle.rs` — request begin, cancel, reset

**Accessor Methods** (`accessors.rs`):

| Method | Line | Description |
|---|---|---|
| `is_loading` | `accessors.rs:8` | Returns `true` if in a loading state |
| `key` | `accessors.rs:12` | Returns a reference to the resource's `QueryKey` |
| `status` | `accessors.rs:16` | Returns the current `QueryStatus` |
| `data` | `accessors.rs:20` | Returns a reference to the cached data, if any |
| `error` | `accessors.rs:24` | Returns a reference to the last error, if any |
| `active_request_id` | `accessors.rs:28` | Returns the active request ID, if in flight |
| `cache_policy` | `accessors.rs:32` | Returns the resource's `CachePolicy` |
| `request_policy` | `accessors.rs:36` | Returns the resource's `RequestPolicy` |
| `started_at_ms` | `accessors.rs:40` | Returns the start timestamp in ms |
| `last_updated_at_ms` | `accessors.rs:44` | Returns the last update timestamp in ms |
| `cache_hits` | `accessors.rs:48` | Returns the number of cache hits |
| `cancelled_count` | `accessors.rs:52` | Returns the number of cancelled requests |
| `ignored_results` | `accessors.rs:56` | Returns the number of ignored stale results |
| `has_data` | `accessors.rs:60` | Returns `true` if the resource has cached data |

**Cache Methods** (`cache.rs`):

| Method | Line | Description |
|---|---|---|
| `cache_age_ms` | `cache.rs:6` | Returns cache age in ms relative to `now_ms` |
| `is_cache_fresh` | `cache.rs:10` | Returns `true` if cached data exists and is within TTL |
| `should_short_circuit_cache` | `cache.rs:20` | Returns `true` if cache policy allows short-circuiting and data is fresh |
| `invalidate` | `cache.rs:30` | Expire cache by clearing `last_updated_at` without removing data |

**Completion Methods** (`completion.rs`):

| Method | Line | Description |
|---|---|---|
| `complete_current_success` | `completion.rs:6` | Complete active request with success if `request_id` matches current |
| `complete_current_failure` | `completion.rs:19` | Complete active request with failure if `request_id` matches current |
| `complete_current_optional_success` | `completion.rs:27` | Complete with optional data (can be `None`) |
| `complete_current_failure_with_data` | `completion.rs:40` | Complete with both data and error |
| `complete_success` | `completion.rs:53` | Apply success using a `RequestGuard` |
| `complete_failure` | `completion.rs:57` | Apply failure using a `RequestGuard` |
| `complete_success_optional` | `completion.rs:61` | Apply optional success using a `RequestGuard` |
| `complete_failure_with_data` | `completion.rs:70` | Apply failure while preserving data using a `RequestGuard` |

**Lifecycle Methods** (`lifecycle.rs`):

| Method | Line | Description |
|---|---|---|
| `begin_request` | `lifecycle.rs:9` | Entry point: checks cache, applies `RequestPolicy`, allocates `RequestId` |
| `is_current_request` | `lifecycle.rs:57` | Returns `true` if the given ID matches the active request |
| `accept_current_request` | `lifecycle.rs:61` | Accept for completion if current; returns `RequestGuard` or `None` |
| `cancel` | `lifecycle.rs:74` | Cancel the active request, storing the error |
| `mark_ignored_result` | `lifecycle.rs:86` | Increment the ignored results counter |
| `reset` | `lifecycle.rs:90` | Reset all runtime state to `Idle` while preserving key and policies |

**Tests:** 25 tests across all submodules.

### Client Layer

#### QueryClient — App-Wide Global Registry

`crates/gpui-query/src/client/mod.rs:48`

Implements `gpui::Global`. Stores resources in type-partitioned buckets via `TypeId`.

| Method | Line | Description |
|---|---|---|
| `QueryClient::new` | `mod.rs:59` | Create a new client with default policies |
| `QueryClient::with_gc_time` | `mod.rs:72` | Builder-style setter for default GC time in ms |
| `QueryClient::resource` | `mod.rs:82` | Get or create a `QueryResource<T, E>` entity for the given key |
| `QueryClient::resource_with_policies` | `mod.rs:98` | Get or create with explicit cache and request policies |
| `QueryClient::contains` | `mod.rs:119` | Check if a resource exists for the given key and type |
| `QueryClient::invalidate_queries` | `mod.rs:137` | Invalidate (expire cache) for all resources matching the filter |
| `QueryClient::reset_queries` | `mod.rs:144` | Reset (clear all state) for all resources matching the filter |
| `QueryClient::gc` | `mod.rs:151` | Garbage-collect idle resources older than GC time |
| `QueryClient::total_count` | `mod.rs:159` | Total number of resources across all type buckets |
| `QueryClient::bucket_count` | `mod.rs:164` | Number of type-partitioned buckets |

**Tests:** 8 tests.

#### QueryBucketTrait / BucketDefaults / QueryBucket\<T, E\>

`crates/gpui-query/src/client/bucket.rs`

| Type | Line | Role |
|---|---|---|
| `QueryBucketTrait` | `bucket.rs:16` | Type-erased trait for bulk operations across all buckets |
| `BucketDefaults` | `bucket.rs:33` | Default policies applied when creating new resources |
| `QueryBucket<T, E>` | `bucket.rs:43` | Typed bucket storing `QueryResource` entities with co-located `RequestSequencer` |

| Method | Line | Description |
|---|---|---|
| `QueryBucketTrait::invalidate_matching` | `bucket.rs:18` | Invalidate all resources matching the filter |
| `QueryBucketTrait::reset_matching` | `bucket.rs:21` | Reset all resources matching the filter |
| `QueryBucketTrait::gc` | `bucket.rs:24` | Remove idle resources older than `gc_time_ms` |
| `QueryBucketTrait::count` | `bucket.rs:28` | Total number of resources in this bucket |
| `QueryBucket::new` | `bucket.rs:50` | Create a new typed bucket with the given defaults |
| `QueryBucket::resource` | `bucket.rs:59` | Get or create a `QueryResource` entity using default policies |
| `QueryBucket::resource_with_policies` | `bucket.rs:69` | Get or create with explicit policies; deduplicates by key |
| `QueryBucket::begin_request_for` | `bucket.rs:88` | Begin a request using the bucket's co-located sequencer |

**Tests:** 8 tests.

### Hook Layer

#### QueryOptions\<T, E\> — Reusable Query Configuration

`crates/gpui-query/src/hook/options.rs:7`

Inspired by TanStack's `queryOptions()`. Builder-pattern configuration struct.

| Method | Line | Description |
|---|---|---|
| `QueryOptions::new` | `options.rs:24` | Create with given key and defaults (TTL 60s, LatestWins, GC 5min) |
| `QueryOptions::cache_policy` | `options.rs:36` | Builder-style setter for cache policy |
| `QueryOptions::request_policy` | `options.rs:42` | Builder-style setter for request policy |
| `QueryOptions::gc_time_ms` | `options.rs:48` | Builder-style setter for GC time |
| `QueryOptions::force` | `options.rs:54` | Builder-style flag to force a fresh fetch bypassing cache |

#### Hook Functions

`crates/gpui-query/src/hook/mod.rs`

| Function | Line | Description |
|---|---|---|
| `use_query` | `mod.rs:54` | Ergonomic hook: get/create entity, observe, and start async fetch if idle. Returns `(Entity<QueryResource<T,E>>, Subscription)` |
| `use_query_manual` | `mod.rs:109` | Lower-level hook: set up entity and observation without starting a fetch |
| `fetch_query` | `mod.rs:138` | Initiate a fetch on an existing entity (for refetch on click or timer) |
| `current_time_ms` | `mod.rs:180` | Returns current time as milliseconds since UNIX epoch |

#### QuerySignal — Cooperative Cancellation Signal

`crates/gpui-query/src/core/signal.rs`

An `Arc<AtomicBool>`-backed cancellation signal. Clones share the same flag, so cancelling any clone cancels all of them. Created fresh on `begin_request`, cancelled on `cancel()`.

| Method | Line | Description |
|---|---|---|
| `QuerySignal::new` | `signal.rs` | Create a fresh un-cancelled signal |
| `QuerySignal::cancel` | `signal.rs` | Set the cancelled flag (visible to all clones) |
| `QuerySignal::is_cancelled` | `signal.rs` | Check if the signal has been cancelled |

Integrated into `QueryResource`:
- `signal() -> Option<&QuerySignal>` — accessor (created on begin_request, cleared on reset)
- `signal_mut() -> Option<&mut QuerySignal>` — mutable accessor
- `begin_request` creates a fresh signal; `cancel()` propagates to signal; `reset()` clears it

Integrated into `QueryClient`:
- `cancel_query<T,E>(key, error, cx) -> bool` — cancels resource + signal
- `signal_for<T,E>(key, cx) -> Option<QuerySignal>` — returns a signal clone

Hook-layer variants:
- `use_query_with_signal(fetcher: FnOnce(QuerySignal) -> Fut)` — signal-aware fetcher
- `fetch_query_with_signal(entity, fetcher: FnOnce(QuerySignal) -> Fut)` — signal-aware refetch

**Tests:** 5 inline tests (signal.rs) + 7 lifecycle tests + 5 integration tests = 17 tests.

#### Data Retention — placeholderData, previousData, displayData

`crates/gpui-query/src/core/resource/accessors.rs` and `lifecycle.rs`

Three new data accessors and lifecycle methods on `QueryResource`:

| Method | Layer | Description |
|---|---|---|
| `placeholder_data()` | accessor | Returns placeholder data (shown when no real data) |
| `previous_data()` | accessor | Returns the data held before the last success/optimistic write |
| `display_data()` | accessor | Returns `data` if present, falls back to `placeholder_data` |
| `set_placeholder_data(data: Option<T>)` | lifecycle | Set or clear placeholder data |
| `rollback_to_previous() -> bool` | lifecycle | Restore `previous_data` as current data, sets status to Success |

`apply_success` and `apply_success_optional` now store old data in `previous_data` before overwriting.

`QueryOptions` has a new `keep_previous_data: bool` field with builder setter.

**Tests:** 15 core unit tests + 2 integration tests = 17 tests.

#### Optimistic Updates — set_data, clear_data, rollback

`crates/gpui-query/src/core/resource/lifecycle.rs`

Two new methods for optimistic writes that store previous data for rollback:

| Method | Description |
|---|---|
| `set_data(data: T)` | Stores current data in `previous_data`, sets new data. Does NOT change status. |
| `clear_data()` | Stores current data in `previous_data`, sets data to None. Does NOT change status. |

Client-level methods on `QueryClient`:
- `set_query_data<T,E>(key, data, cx) -> bool`
- `rollback_query_data<T,E>(key, cx) -> bool`

**Tests:** 12 core unit tests + 5 integration tests = 17 tests.

#### Standalone fetchQuery — QueryClient imperative fetch

`crates/gpui-query/src/client/mod.rs` and `bucket.rs`

Imperative fetch methods on `QueryClient` that create resources and start requests without requiring a component subscription:

| Method | Description |
|---|---|
| `QueryClient::fetch_query<T,E>(key, cache, policy, now_ms, cx)` | Creates resource if needed, begins request. Returns `Some((entity, request_id))` on Started, `None` on cache hit or ignored. |
| `QueryClient::force_fetch_query<T,E>(...)` | Same as `fetch_query` but uses `QueryFetchMode::Force` to bypass cache. |
| `QueryBucket::fetch(key, cache, policy, now_ms, fetch_mode, cx)` | Bucket-level combined create-and-begin. |

**Tests:** 6 integration tests.

#### RetryPolicy — Configurable Retry with Backoff

`crates/gpui-query/src/core/retry.rs`

A configurable retry policy supporting fixed delays, exponential backoff, and max delay caps.

```rust
pub struct RetryPolicy {
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub exponential_backoff: bool,
    pub max_retry_delay_ms: u64,
}
```

| Method | Description |
|---|---|
| `RetryPolicy::no_retries()` | Create a policy with zero retries |
| `RetryPolicy::new(max_retries)` | Create with specified retry count |
| `with_delay(delay_ms)` | Builder: set base delay between retries |
| `with_exponential_backoff()` | Builder: enable exponential backoff |
| `with_max_delay(max_ms)` | Builder: cap maximum delay |
| `delay_for_attempt(attempt)` | Calculate delay for 0-based attempt number |
| `should_retry(current_retries)` | Whether to attempt another retry |

Integrated into `QueryResource`: `retry_count()`, `retry_policy()`, `set_retry_policy()`, `increment_retry()`, `reset_retry_count()`.

Default policy: 3 retries, exponential backoff, 1s base delay, 30s max.

**Tests:** 6 tests in `core_retry.rs`.

#### MutationResource\<V, T, E\> — Mutation State Machine

`crates/gpui-query/src/core/mutation.rs`

A state machine for tracking mutation operations (create, update, delete), with `V` as the variables/input type, `T` as the success output, and `E` as the error type.

```rust
pub enum MutationStatus {
    Idle,
    Loading,
    Success,
    Failure,
}

pub struct MutationResource<V, T, E = QueryError> {
    status, data, error, variables, key,
    retry_count, retry_policy, signal,
}
```

| Method | Description |
|---|---|
| `new(retry_policy)` | Create a new mutation in Idle state |
| `begin(variables)` | Transition to Loading, store input variables |
| `complete_success(data)` | Transition to Success with result data |
| `complete_failure(error)` | Transition to Failure, increment retry count |
| `retry()` | Reset to Loading if retries remain |
| `cancel(error)` | Cancel with error, propagate to signal |
| `reset()` | Return to Idle, clear all state |
| `status()`, `data()`, `error()`, `variables()` | Read-only accessors |
| `is_idle()`, `is_loading()`, `is_success()`, `is_failure()` | Status predicates |

Mutation keys: optional `QueryKey` for identifying specific mutations in the registry.

**Tests:** 9 tests in `core_mutation.rs`.

#### InfiniteQueryResource\<T, E\> — Paginated Query State Machine

`crates/gpui-query/src/core/infinite_query.rs`

A state machine for managing paginated data with bidirectional page loading and `max_pages` eviction.

```rust
pub struct InfiniteQueryResource<T, E = QueryError> {
    key, pages: Vec<T>, status, error,
    has_next_page, has_previous_page,
    is_fetching_next_page, is_fetching_previous_page,
    max_pages: Option<usize>, signal, ...
}
```

| Method | Description |
|---|---|
| `new(key, cache_policy, request_policy)` | Create with no pages |
| `pages()` | All loaded pages as a slice |
| `page_count()`, `first_page()`, `last_page()` | Page accessors |
| `append_page(page)` | Add page at end (forward pagination) |
| `prepend_page(page)` | Add page at start (backward pagination) |
| `set_has_next_page(bool)`, `set_has_previous_page(bool)` | Pagination cursors |
| `set_max_pages(Option<usize>)` | Cap stored pages, evict oldest |
| `begin_fetch_next(sequencer, now_ms)` | Start loading next page |
| `begin_fetch_previous(sequencer, now_ms)` | Start loading previous page |
| `complete_page_success(request_id, page, is_next, now_ms)` | Complete page fetch |
| `complete_page_failure(request_id, error)` | Fail page fetch |
| `reset()`, `invalidate()` | Lifecycle |

**Tests:** 11 tests in `core_infinite_query.rs`.

#### SelectTransform\<T, U\> — Data Mapping

`crates/gpui-query/src/core/select.rs`

An `Arc<dyn Fn>`-backed transform for mapping cached data of type `T` to output type `U`, inspired by TanStack Query's `select` option.

```rust
pub struct SelectTransform<T, U> {
    transform: Arc<dyn Fn(&T) -> U + Send + Sync>,
}

pub struct MappedQueryResource<T, U, E> {
    source_data: Option<T>,
    transform: SelectTransform<T, U>,
}
```

| Method | Description |
|---|---|
| `SelectTransform::new(closure)` | Create from any `Fn(&T) -> U + Send + Sync` |
| `apply(&self, data: &T) -> U` | Apply the transform |
| `MappedQueryResource::new(data, transform)` | Create mapped view |
| `MappedQueryResource::data()` | Returns `Option<U>` — transformed data |

**Tests:** 4 tests in `core_select.rs`.

#### NetworkMode — Connectivity Awareness

`crates/gpui-query/src/core/network_mode.rs`

```rust
pub enum NetworkMode {
    Online,       // Only fetch when online (default)
    Always,       // Always fetch regardless
    OfflineFirst, // Offline-first strategy
}
```

Available on `QueryOptions` via `.network_mode(mode)` builder. Desktop apps default to `Always`.

**Tests:** 2 tests in `core_network_mode.rs`.

#### RefetchTrigger — Automatic Refetch Configuration

`crates/gpui-query/src/core/refetch.rs`

```rust
pub enum RefetchTrigger {
    Never,
    OnMount,
    OnWindowFocus { stale_time_ms: Option<u64> },
    OnReconnect { stale_time_ms: Option<u64> },
    Always,
}
```

Available on `QueryOptions` via `.refetch_trigger(trigger)` builder.

### Client Layer (New Features)

#### MutationBucket\<V, T, E\> — Typed Mutation Storage

`crates/gpui-query/src/client/mutation_bucket.rs`

Type-partitioned storage for mutations, mirroring `QueryBucket`'s architecture:

```rust
pub struct MutationBucket<V, T, E> {
    resources: HashMap<QueryKey, Entity<MutationResource<V, T, E>>>,
    defaults: MutationDefaults,
}

pub struct MutationDefaults {
    pub retry_policy: RetryPolicy,
    pub gc_time_ms: u64,
}
```

`MutationBucketTrait` provides type-erased `count()` and `gc()`. `ErasedMutationBucket` wraps `Box<dyn MutationBucketTrait>` for storage in `QueryClient`.

#### QueryClient — New Methods

`crates/gpui-query/src/client/mod.rs`

| Method | Description |
|---|---|
| `mutation_resource::<V,T,E>(key, cx)` | Get or create a mutation entity |
| `mutation_resource_with_policy::<V,T,E>(key, retry_policy, cx)` | Get or create with explicit retry policy |
| `mutation_count()` | Total mutations across all type buckets |
| `gc_mutations(cx, now_ms)` | Garbage-collect idle mutations |
| `prefetch_query::<T,E>(key, cache, policy, now_ms, cx) -> bool` | Background prefetch; returns true if request started |
| `remove_queries(filter, cx)` | Remove resources matching filter entirely |
| `clear()` | Drop all resources and mutations across all buckets |
| `diagnostics(cx) -> ClientDiagnostic` | DevTools: introspect all query state |
| `query_diagnostics::<T,E>(cx) -> Vec<QueryDiagnostic>` | DevTools: per-type diagnostics |

#### QueryObserver\<T, E\> — Standalone Callback Observer

`crates/gpui-query/src/client/observer.rs`

A decoupled observer that watches a `QueryResource` entity and fires callbacks on state transitions, without requiring a component render.

```rust
pub struct ObserverConfig<T, E> {
    pub on_success: Option<Arc<dyn Fn(&T) + Send + Sync>>,
    pub on_error: Option<Arc<dyn Fn(&E) + Send + Sync>>,
    pub on_loading: Option<Arc<dyn Fn() + Send + Sync>>,
    pub on_settled: Option<Arc<dyn Fn(Option<&T>, Option<&E>) + Send + Sync>>,
}

pub struct QueryObserver<T, E> { entity, config, last_status }
```

| Method | Description |
|---|---|
| `QueryObserver::new(entity, config)` | Create observer for an entity |
| `observe(cx) -> Subscription` | Attach to context, returns keep-alive subscription |

#### DevTools Diagnostics

`crates/gpui-query/src/client/devtools.rs`

```rust
pub struct QueryDiagnostic {
    pub key: String,
    pub status: String,
    pub has_data: bool,
    pub has_error: bool,
    pub cache_policy: String,
    pub request_policy: String,
    pub cache_hits: u64,
    pub cancelled_count: u64,
    pub ignored_results: u64,
    pub last_updated_at_ms: Option<u128>,
    pub started_at_ms: Option<u128>,
}

pub struct ClientDiagnostic {
    pub total_resources: usize,
    pub bucket_count: usize,
    pub mutation_count: usize,
    pub queries: Vec<QueryDiagnostic>,
}
```

### Hook Layer (New Features)

#### use_mutation — Mutation Hook

`crates/gpui-query/src/hook/mod.rs`

Hook for executing mutations (create, update, delete operations). Returns the mutation entity and a trigger closure:

```rust
pub fn use_mutation<V, T, E, C, Fut>(
    mutator: impl Fn(V) -> Fut + 'static + Clone,
    cx: &mut C,
) -> (Entity<MutationResource<V, T, E>>, impl Fn(V) + 'static + Clone)
```

The returned closure, when called with variables:
1. Updates entity to Loading with variables
2. Spawns async task calling the mutator
3. On success: `complete_success`
4. On failure with retries: schedules retry with configured backoff
5. On failure without retries: `complete_failure`

With callbacks variant:
```rust
pub fn use_mutation_with_callbacks<V, T, E, C, Fut>(
    mutator, callbacks: MutationCallbacks<T, E>, cx
) -> (Entity, impl Fn(V) + 'static + Clone)
```

`MutationCallbacks<T, E>` provides `on_success`, `on_error`, `on_settled` callbacks.

#### use_infinite_query — Pagination Hook

`crates/gpui-query/src/hook/use_infinite_query.rs`

Hook for infinite scrolling / pagination:

```rust
pub fn use_infinite_query<T, E, C, FNext, Fut>(
    options: InfiniteQueryOptions<T, E>,
    fetch_next: FNext,
    cx: &mut C,
) -> (Entity<InfiniteQueryResource<T, E>>, impl Fn() + Clone + 'static, Subscription)
```

The `fetch_next` closure receives `Option<&T>` (last page) and returns `Future<Output = Result<(T, bool), E>>` where `T` is the new page and `bool` indicates `has_next_page`.

`InfiniteQueryOptions<T, E>` configures key, policies, and `max_pages`.

#### Retry-Aware Fetching

`crates/gpui-query/src/hook/mod.rs`

`use_query` and `use_query_with_signal` now check the resource's `retry_policy()`. On fetch failure with retries remaining, the hook schedules a retry with the configured backoff delay instead of immediately completing with failure.

#### MutationOptions — Mutation Configuration

`crates/gpui-query/src/hook/options.rs`

```rust
pub struct MutationOptions<V, T, E> {
    pub retry_policy: RetryPolicy,
    pub gc_time_ms: u64,
}
```

Builder methods: `.retry_policy()`, `.gc_time_ms()`.

---

## What's Left

All 41 planned TanStack Query features are now implemented.

### Feature Completion Summary

```
DONE    ██████████████████████████████████  41  (100%)
TODO    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   0  (0%)
                                    Total: 41 features
```

### Future Considerations

| Feature | Notes |
|---|---|
| **Hydration / SSR** | Not applicable to GPUI desktop apps. `QueryResource` is `Serde`-serializable which is a prerequisite. |
| **Query devtools UI** | DevTools diagnostics API exists (`ClientDiagnostic`). A GPUI component consuming it would be the next step. |
| **Cache explorer UI** | Bucket iteration is exposed via `diagnostics()`. A visual explorer would build on this. |

---

## Consumer Usage

The HTTP Lab service is the primary consumer of gpui-query. The following files form the integration surface.

### Module Responsibilities

```
http_lab/
├── types.rs              Policy configuration hub
├── state.rs              Central state container
├── transitions.rs        Request lifecycle engine
├── operations.rs         Orchestration layer
├── task_tracking.rs      Cancellation token registry
├── test_support.rs       Test helpers
├── cache.test.rs         Cache behavior tests
├── flow.test.rs          Full flow orchestration tests
├── tasks.test.rs         Task tracking tests
├── data_retention.test.rs  Placeholder, previousData, displayData, rollback tests
├── optimistic.test.rs     Optimistic updates (set_data, clear_data, rollback) tests
└── signal.test.rs         QuerySignal propagation tests
```

### Policy Configuration — types.rs

`src/services/http_lab/types.rs`

**Imports:** `CachePolicy`, `QueryKey`, `RequestPolicy`

`HttpLabAction::query_key()` builds a `QueryKey` from `["http_lab", action.id()]`. The `cache_policy()` and `request_policy()` methods map each action variant to its policy:

| Action | Cache Policy | Request Policy |
|---|---|---|
| GetText | `Ttl { ttl_ms: 60_000 }` | `LatestWins` |
| GetXml | `Ttl { ttl_ms: 60_000 }` | `LatestWins` |
| GetJson | `StaleWhileRevalidate { ttl_ms: 30_000 }` | `LatestWins` |
| PostJson | `NoCache` | `LatestWins` |
| PostForm | `NoCache` | `LatestWins` |
| PostMultipart | `NoCache` | `IgnoreWhileLoading` |
| Cookies | `NoCache` | `LatestWins` |
| Failure | `NoCache` | `LatestWins` |
| FullFlow | `NoCache` | `IgnoreWhileLoading` |

### State Management — state.rs

`src/services/http_lab/state.rs`

**Imports:** `QueryResource`, `QuerySignal`, `RequestId`, `RequestSequencer`

Stores a `BTreeMap<HttpLabAction, QueryResource<HttpExchange>>` (one resource per action), a `RequestSequencer` for scope management, and exposes `resource()` / `selected_resource()` / `active_count()`.

On `reset_for_user()`, calls `request_sequencer.advance_scope()` to invalidate all in-flight requests, then rebuilds all resources via `resource_for_action()`. The helper calls `QueryResource::new(action.query_key(), action.cache_policy(), action.request_policy())`.

**Data retention methods:**
- `display_resource(action) -> Option<&HttpExchange>` — delegates to `resource.display_data()`
- `previous_resource_data(action) -> Option<&HttpExchange>` — delegates to `resource.previous_data()`
- `set_placeholder_for_action(action, data: Option<HttpExchange>)` — sets placeholder data

**Optimistic update methods:**
- `set_action_data(action, data: HttpExchange)` — calls `resource.set_data(data)`
- `clear_action_data(action)` — calls `resource.clear_data()`
- `rollback_action_data(action) -> bool` — calls `resource.rollback_to_previous()`

**Signal method:**
- `signal_for_action(action) -> Option<QuerySignal>` — returns a clone of the resource's signal

### Request Lifecycle — transitions.rs

`src/services/http_lab/transitions.rs`

**Imports:** `CachePolicy`, `QueryBeginResult`, `QueryError`, `QueryFetchMode`, `QueryStatus`, `RequestGuard`, `RequestId`

The request lifecycle engine:

1. `begin_action()` calls `resource.begin_request(request_sequencer, now_ms, QueryFetchMode::Normal)` and pattern-matches `QueryBeginResult::{Started, CacheHit, IgnoredWhileLoading}`.
2. `apply_result_to_state()` calls `resource.accept_current_request(request_id)` to get a `RequestGuard`, then calls the appropriate completion method based on outcome:
   - `resource.complete_success(request_guard, exchange, now_ms)`
   - `resource.complete_failure(request_guard, error)`
   - `resource.complete_failure_with_data(request_guard, data, error)`
3. `cancel_action_in_state()` calls `resource.cancel(QueryError::cancelled(reason))`.
4. `finish_flow_resource()` uses `resource.complete_success_optional()`.

Also checks `resource.has_data()`, `resource.cache_policy()` (to detect `StaleWhileRevalidate` + has data for the "revalidating cached data" note), `resource.active_request_id()`, and `resource.status()`.

### Orchestration — operations.rs

`src/services/http_lab/operations.rs`

**Imports:** `RequestId`

`prepare_action()` obtains a `RequestId` from `begin_action()`, stores it in an `ActionHandle` alongside the `HttpLabAction`. `execute_action()` awaits the Tokio HTTP task then calls `apply_result()` which delegates to `apply_result_to_state()`. Uses `request_id.label()` for tracing throughout.

### Cancellation Sidecar — task_tracking.rs

`src/services/http_lab/task_tracking.rs`

**Imports:** `RequestId`

Maps `RequestId -> CancellationToken` in a global static `Mutex<BTreeMap>`. `register_request_flag()` creates and stores a token. `cancel_request_flag()` cancels one by `RequestId`. `remove_request_flag()` cleans up. Uses `request_id.label()` for tracing. No cache or request policies; purely a `RequestId`-keyed cancellation sidecar.

### UI Rendering — http_lab.rs

`src/features/pages/http_lab.rs`

**Imports:** `QueryResource`, `QueryStatus`, `RequestPolicy`

Reads `QueryResource<HttpExchange>` from global `HttpLabState` to drive rendering:

- `resource.status()` for status dots and chips (mapped to colors via `QueryStatus` variants)
- `resource.is_loading()` to show loading state
- `resource.request_policy()` to check `RequestPolicy::IgnoreWhileLoading` for disabling duplicate buttons
- `resource.data()` to render exchange panels
- `resource.error()` to show error callouts
- `resource.cache_policy().label()` and `resource.request_policy().label()` for info chips
- `resource.active_request_id().map(|id| id.label())` for request ID display
- `resource.cache_hits()`, `resource.cancelled_count()`, `resource.ignored_results()`, `resource.last_updated_at_ms()` for metrics

### Interactive Testing Page — http_lab_testing.rs

`src/features/pages/http_lab_testing.rs`

**Imports:** `CachePolicy`, `QueryBeginResult`, `QueryError`, `QueryFetchMode`, `QueryResource`, `RequestPolicy`, `RequestSequencer`

A comprehensive GPUI testing dashboard organized into 7 section cards, each validating a specific gpui-query feature area:

| Section | Buttons | What it tests |
|---|---|---|
| **Query Lifecycle** | Send query GET, Query TTL, Query ignore, Query latest | Real HTTP fetch, TTL cache hit, IgnoreWhileLoading dedup, LatestWins stale rejection |
| **Cancel Signal** | Query signal, Cancel active | QuerySignal creation, cancel propagation to clones |
| **Cache & Data Retention** | Placeholder data, Previous data, Rollback | display_data fallback, previous_data tracking, rollback_to_previous |
| **Optimistic Updates** | Optimistic set, Optimistic rollback, Full mutation | set_data, clear_data, rollback, full mutation lifecycle |
| **Standalone Client Fetch** | Client fetch, Client force | QueryClient.fetch_query(), force_fetch_query() without component |
| **Local Full Lab** | Per-action buttons, Local reset, Cancel | Real HTTP per action with proper policies, FullFlow orchestration |
| **Raw Baseline** | Send raw GET | Plain reqwest with no QueryResource — comparison baseline |

Each exercise button produces ✅/❌ verdicts per assertion and a final PASSED/FAILED bottom line.

Owns 4 dedicated `QueryResource<RawResponse>` instances with distinct policy combinations:

| Resource | Cache Policy | Request Policy |
|---|---|---|
| `query_resource` | `NoCache` | `LatestWins` |
| `query_ttl_resource` | `Ttl { ttl_ms: 30_000 }` | `LatestWins` |
| `query_ignore_resource` | `NoCache` | `IgnoreWhileLoading` |
| `query_latest_resource` | `NoCache` | `LatestWins` |

Also owns a `BTreeMap<HttpLabAction, QueryResource<RawResponse>>` for a local full-query lab that mirrors the main `HttpLabAction` policies. Directly calls `resource.begin_request()`, `resource.complete_current_success()`, `resource.complete_current_failure()`, `resource.cancel()`, `resource.accept_current_request()`. Exercises every `QueryBeginResult` variant and every `CachePolicy`/`RequestPolicy` combination interactively. Uses `QueryFetchMode::Force` for full-flow child resource seeding.

---

## Architecture Decisions

### Arc-Based Hierarchical Keys

```
QueryKey = Arc<[Arc<str>]>
```

**Rationale:** Keys are cloned frequently (stored in resources, passed to filters, used as map keys). `Arc<[Arc<str>]>` makes every clone a single atomic reference count increment. The hierarchical structure (array of segments) enables prefix matching for bulk invalidation without custom trie logic.

### TypeId-Partitioned Buckets

```
QueryClient {
    buckets: HashMap<TypeId, Box<dyn QueryBucketTrait>>,
}
```

**Rationale:** Resources of different types `(T, E)` are stored in separate buckets keyed by `TypeId`. This provides type-safe access (no downcasting at lookup time) while allowing type-erased bulk operations (`invalidate_queries`, `gc`, `count`) via the `QueryBucketTrait`. Each bucket co-locates its own `RequestSequencer`, keeping ID spaces independent per type.

### No Stored Fetcher

```
QueryResource<T, E> stores:
  - key, status, data, error, timestamps, counters
  - does NOT store a fetcher function
```

**Rationale:** TanStack Query stores the `queryFn` on the query object. In Rust, storing closures generically over `T` and `E` would require `Box<dyn Fn(...)>` or a trait object, adding allocation and indirection. Instead, fetchers are provided at call sites (`use_query`, `fetch_query`). This keeps `QueryResource` a pure data type with no behavior, making it trivially serializable and testable.

### Scope-Based Request Invalidation

```
RequestSequencer {
    scope_id: u64,
    sequence: u64,
}

advance_scope() -> {
    scope_id += 1;
    sequence = 1;
}
```

**Rationale:** When a user resets state (e.g., "reset for user" in HTTP Lab), all in-flight requests should be invalidated. Rather than iterating all resources and cancelling each one, `advance_scope()` makes every existing `RequestId` permanently stale. Any completion attempt with an old ID will fail `accept_current_request()` because the scope no longer matches. This is an O(1) invalidation of all in-flight work.

### RequestGuard Ownership Model

```
begin_request() -> RequestId
accept_current_request(request_id) -> Option<RequestGuard>
complete_success(guard: RequestGuard, data: T, now_ms: u64)
```

**Rationale:** Two-phase completion prevents use-after-free on request IDs. A `RequestId` alone is not enough to complete a request -- the caller must first `accept_current_request()` to obtain a `RequestGuard`, proving the ID is still current. The guard is consumed on completion. This eliminates the class of bugs where a stale request ID is used to overwrite newer data.

### Six-State Lifecycle

```
         ┌──────────┐
         │   Idle    │
         └────┬─────┘
              │ begin_request (no data)
              v
     ┌────────────────┐
     │  LoadingEmpty  │──── success ────> Success
     └────┬───────────┘         ^
          │                     │
          │ begin_request       │
          │ (has data)          │
          v                     │
   ┌─────────────────┐          │
   │ LoadingWithData  │─────────┘
   └────┬────────────┘
        │                     ┌──────────┐
        ├── failure ─────────>│ Failure  │
        │                     └──────────┘
        │                     ┌───────────┐
        └── cancel ──────────>│ Cancelled │
                              └───────────┘
```

**Rationale:** TanStack Query uses four states (`idle`, `pending`, `success`, `error`). GPUI-query adds `LoadingWithData` to distinguish "fetching with stale data on screen" from "fetching for the first time", and `Cancelled` to separate user-initiated cancellations from network failures. This gives UI code more information to render appropriate loading indicators.

---

## Test Coverage

### Statistics

| Metric | Value |
|---|---|
| Total tests (crate) | 222 |
| Total tests (consumer) | 36 |
| Test lines of code | 3,127 |

### Test Inventory by Type

| Type | Tests | File | Coverage |
|---|---|---|---|
| **QueryKey** | 12 | `core/key.rs` | Construction, matching, equality, edge cases |
| **QueryKeyFilter** | 6 | `core/key_filter.rs` | Exact, Prefix, All matching |
| **RequestSequencer** | 3 | `core/request.rs` | Monotonic generation, scope advance, overflow |
| **QuerySignal** | 5 | `core/signal.rs` | New/cancel, clone sharing, default |
| **QueryResource** | 25 | `core/resource/*.rs` | Full state machine lifecycle: begin, complete, cancel, reset, cache |
| **QueryResource (signal)** | 7 | `core/resource/*.rs` | Signal creation, cancel propagation, fresh on new request |
| **Data retention** | 15 | `core/resource/*.rs` | placeholder_data, previous_data, display_data, rollback |
| **Optimistic updates** | 12 | `core/resource/*.rs` | set_data, clear_data, rollback, mutation flow |
| **RetryPolicy** | 6 | `core/retry.rs` | No-retry, default, fixed delay, exponential backoff, max cap |
| **MutationResource** | 9 | `core/mutation.rs` | Full mutation lifecycle: begin, success, failure, retry, cancel, reset |
| **InfiniteQueryResource** | 11 | `core/infinite_query.rs` | Pages, append/prepend, max eviction, fetch next/previous, complete |
| **SelectTransform** | 4 | `core/select.rs` | Transform apply, mapped view, clone |
| **NetworkMode** | 2 | `core/network_mode.rs` | Default, labels |
| **QueryClient** | 40+ | `client/mod.rs` | Resource creation, invalidation, reset, GC, cancel_query, fetch_query, set_query_data, rollback, remove_queries, clear, prefetch, mutation_resource, diagnostics |
| **QueryBucket** | 8 | `client/bucket.rs` | Resource management, deduplication, bulk ops |

### Consumer Integration Tests

| File | Tests | Coverage |
|---|---|---|
| `http_lab/cache.test.rs` | 5 | TTL cache, stale-while-revalidate, LatestWins, IgnoreWhileLoading |
| `http_lab/flow.test.rs` | 6 | FullFlow, reset, scope advance, cookies |
| `http_lab/tasks.test.rs` | 5 | Stale results, task tracking, previous data preservation |
| `http_lab/data_retention.test.rs` | 5 | display_data, placeholder, previous_data, rollback |
| `http_lab/optimistic.test.rs` | 8 | set_data, clear_data, rollback, optimistic flow, failure rollback |
| `http_lab/signal.test.rs` | 5 | Signal creation, cancel propagation, clone sharing |
| `http_lab/response.test.rs` | 2 | UTF-8 truncation, parse helpers |
| `http_lab/retry.test.rs` | 4 | Retry policies per action, exponential backoff, retry count on failure |
| `http_lab/client_features.test.rs` | 4 | Key construction, all actions have policies, retry integration, reset preserves policies |

### Integration Test Coverage

| File | Focus | Key Assertions |
|---|---|---|
| `http_lab/cache.test.rs` | Cache policy behavior | TTL short-circuits (cache_hits == 1), SWR keeps data while re-fetching (LoadingWithData), LatestWins cancels previous (cancelled_count == 1), IgnoreWhileLoading rejects duplicates |
| `http_lab/flow.test.rs` | Full flow orchestration | FullFlow populates resources, reset returns RequestIds, scope advance prevents old IDs from colliding, cookie exchanges update snapshot |
| `http_lab/tasks.test.rs` | Task tracking and result application | Stale results ignored (ignored_results == 1), cancelled requests track until result arrives, successful results clean up, failed exchanges preserve previous data |

### Coverage Gaps

The following types have **no direct tests** (tested indirectly through integration):

- `QueryStatus` (tested through `QueryResource` lifecycle tests)
- `QueryErrorKind` / `QueryError` (constructors tested)
- `CachePolicy` / `RequestPolicy` (tested through cache behavior tests)
- `RefetchTrigger` (type definition only, actual refetch triggers require GPUI integration)
- `QueryObserver` (requires GPUI runtime for `cx.observe()`)
- `MutationCallbacks` (callback struct, tested through `use_mutation`)
- All hook functions (`use_query`, `use_query_manual`, `fetch_query`, `use_mutation`, `use_infinite_query`) — require GPUI test runtime

---

## Migration Notes

gpui-query was extracted from an embedded module within the HTTP Lab service into a standalone crate. The following changes were made during the extraction.

### Structural Changes

| Before (embedded) | After (crate) |
|---|---|
| Inline types in `http_lab/` | Dedicated crate at `crates/gpui-query/` |
| Mixed HTTP and query logic | Clean separation: core (no GPUI), client (GPUI entities), hook (component-facing) |
| Single resource type | Generic `QueryResource<T, E>` |
| Ad-hoc key strings | `QueryKey` with hierarchical segments |
| Manual request ID tracking | `RequestSequencer` with scope-based invalidation |

### API Changes

| Old Pattern | New Pattern |
|---|---|
| `HttpLabState` owns `BTreeMap<Action, Resource>` | `QueryClient` owns type-partitioned buckets; consumer owns typed resources |
| String-based cache invalidation | `QueryKeyFilter::Exact` / `Prefix` / `All` |
| Implicit cancellation via scope counter | Explicit `RequestSequencer::advance_scope()` + `QueryResource::cancel()` |
| No cache policy configuration | `CachePolicy { NoCache, Ttl, StaleWhileRevalidate }` |
| No request deduplication policy | `RequestPolicy { LatestWins, IgnoreWhileLoading }` |
| No garbage collection | `QueryClient::gc(cx, now_ms)` with configurable `gc_time_ms` |

### What the Consumer Still Owns

The crate manages state machines and caching. The consumer (HTTP Lab) still owns:

- **Fetcher functions** -- The actual HTTP calls via Tokio
- **Task orchestration** -- Spawning async tasks and awaiting results
- **Cancellation tokens** -- The `task_tracking.rs` sidecar for Tokio `CancellationToken`
- **UI rendering** -- Reading resource state and rendering GPUI components
- **Policy configuration** -- Deciding which `CachePolicy` and `RequestPolicy` each action uses

This separation is intentional: the crate is a state management library, not a networking library.

---

*Generated from gpui-query source analysis. Last updated: 2026-06-02 (all 41 features complete).*
