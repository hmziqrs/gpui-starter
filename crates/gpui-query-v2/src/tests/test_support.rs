//! Shared test infrastructure for gpui-query-v2.
//!
//! Provides:
//! - [`TestAppContext`] setup helpers via [`setup_test_cx`]
//! - [`QueryClient`] as a [`Global`] for tests via [`setup_query_client`]
//! - Mock fetcher functions: [`immediate_fetcher`], [`delayed_fetcher`], [`failing_fetcher`]
//! - Core resource constructors: [`test_resource`], [`test_resource_with_policies`]
//! - Assertion helpers: [`assert_status`], [`assert_data`], [`assert_error_message`]
//!
//! # Usage
//!
//! ```ignore
//! use crate::tests::test_support::*;
//!
//! #[gpui::test]
//! fn my_test(cx: &mut TestAppContext) {
//!     setup_query_client(cx);
//!     cx.update(|cx| {
//!         // ... test code using cx.global::<QueryClient>() ...
//!     });
//! }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{BackgroundExecutor, TestAppContext};

use crate::client::QueryClient;
use crate::core::{
    CachePolicy, QueryBeginResult, QueryError, QueryFetchMode, QueryKey, QueryResource,
    QueryStatus, RequestId, RequestPolicy, RequestSequencer,
};

// ── TestAppContext setup ───────────────────────────────────────────────

/// Install a default [`QueryClient`] as a [`Global`] on the given context.
///
/// Call this at the start of any integration test that needs the client
/// layer. After this, `cx.global::<QueryClient>()` and
/// `cx.update_global::<QueryClient, _>(…)` are available.
pub fn setup_query_client(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::new());
    });
}

/// Install a [`QueryClient`] with custom policies as a [`Global`].
pub fn setup_query_client_with_policies(
    cx: &mut TestAppContext,
    cache_policy: CachePolicy,
    request_policy: RequestPolicy,
) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(cache_policy, request_policy));
    });
}

/// Install a [`QueryClient`] with a custom GC time.
pub fn setup_query_client_with_gc(cx: &mut TestAppContext, gc_time_ms: u64) {
    cx.update(|cx| {
        cx.set_global(QueryClient::new().with_gc_time(gc_time_ms));
    });
}

// ── Core resource constructors ────────────────────────────────────────

/// Create a test resource with default policies (TTL 1s, LatestWins).
pub fn test_resource() -> QueryResource<&'static str> {
    QueryResource::new(
        "test",
        CachePolicy::Ttl { ttl_ms: 1_000 },
        RequestPolicy::LatestWins,
    )
}

/// Create a test resource with custom policies.
pub fn test_resource_with_policies(
    key: impl Into<QueryKey>,
    cache_policy: CachePolicy,
    request_policy: RequestPolicy,
) -> QueryResource<&'static str> {
    QueryResource::new(key, cache_policy, request_policy)
}

/// Create a typed test resource (for testing with non-string data).
#[allow(dead_code)]
pub fn typed_test_resource<T: Clone + Send + Sync + 'static>(
    key: impl Into<QueryKey>,
) -> QueryResource<T> {
    QueryResource::new(
        key,
        CachePolicy::Ttl { ttl_ms: 1_000 },
        RequestPolicy::LatestWins,
    )
}

/// Create a [`RequestSequencer`] for use in lifecycle tests.
pub fn test_sequencer() -> RequestSequencer {
    RequestSequencer::new()
}

// ── Mock fetcher functions ─────────────────────────────────────────────

/// A fetcher that immediately returns the given value.
///
/// Use in tests where you want a deterministic, instant result.
#[allow(dead_code)]
pub fn immediate_fetcher<T: Clone + Send + 'static>(
    value: T,
) -> impl Fn() -> std::future::Ready<Result<T, QueryError>> + Send + 'static {
    move || std::future::ready(Ok(value.clone()))
}

/// A fetcher that returns a value after the given delay (ms).
///
/// Uses the GPUI [`BackgroundExecutor::timer`] for an async-compatible delay
/// instead of blocking with `thread::sleep`. Callers must provide an
/// [`BackgroundExecutor`] reference (obtainable from `cx.background_executor()`
/// on any GPUI context).
///
/// In tests, pair this with `cx.executor().advance_clock(duration)` to
/// fast-forward through the delay without wall-clock waiting.
#[allow(dead_code)]
pub fn delayed_fetcher<T: Clone + Send + 'static>(
    value: T,
    delay_ms: u64,
    executor: BackgroundExecutor,
) -> Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<T, QueryError>> + Send>> + Send + 'static> {
    Box::new(move || {
        let value = value.clone();
        let executor = executor.clone();
        let delay = Duration::from_millis(delay_ms);
        Box::pin(async move {
            if !delay.is_zero() {
                executor.timer(delay).await;
            }
            Ok(value)
        }) as Pin<Box<dyn Future<Output = Result<T, QueryError>> + Send>>
    })
}

/// A fetcher that always returns the given error.
#[allow(dead_code)]
pub fn failing_fetcher(
    message: impl Into<String>,
) -> impl Fn() -> std::future::Ready<Result<(), QueryError>> + Send + 'static {
    let message = message.into();
    move || std::future::ready(Err(QueryError::response(message.clone())))
}

/// A fetcher that succeeds on the Nth call and fails before that.
///
/// Uses an `Arc<Mutex<>>` counter so the state is observable across calls.
#[allow(dead_code)]
pub fn flaky_fetcher(
    succeed_after: u32,
) -> (
    Arc<Mutex<u32>>,
    impl Fn() -> std::future::Ready<Result<&'static str, QueryError>> + Send + 'static,
) {
    let call_count = Arc::new(Mutex::new(0u32));
    let count_clone = call_count.clone();
    let fetcher = move || {
        let mut count = count_clone.lock().unwrap();
        *count += 1;
        if *count > succeed_after {
            std::future::ready(Ok("recovered"))
        } else {
            std::future::ready(Err(QueryError::transport("transient failure")))
        }
    };
    (call_count, fetcher)
}

/// A fetcher that records how many times it was called.
///
/// Returns `Ok("called")` on each invocation. Inspect `call_count` to verify.
#[allow(dead_code)]
pub fn counting_fetcher() -> (
    Arc<Mutex<u32>>,
    impl Fn() -> std::future::Ready<Result<&'static str, QueryError>> + Send + 'static,
) {
    let call_count = Arc::new(Mutex::new(0u32));
    let count_clone = call_count.clone();
    let fetcher = move || {
        let mut count = count_clone.lock().unwrap();
        *count += 1;
        std::future::ready(Ok("called"))
    };
    (call_count, fetcher)
}

// ── Signal-aware mock fetchers ─────────────────────────────────────────

/// A fetcher that respects the cancellation signal.
///
/// Checks `signal.is_cancelled()` before returning. If cancelled, returns
/// a cancellation error.
#[allow(dead_code)]
pub fn signal_aware_fetcher<T: Clone + Send + 'static>(
    value: T,
) -> impl Fn(crate::core::QuerySignal) -> std::future::Ready<Result<T, QueryError>> + Send + 'static
{
    move |signal| {
        if signal.is_cancelled() {
            std::future::ready(Err(QueryError::cancelled("fetch cancelled")))
        } else {
            std::future::ready(Ok(value.clone()))
        }
    }
}

/// A signal-aware fetcher that fails, allowing retry tests.
#[allow(dead_code)]
pub fn signal_aware_failing_fetcher(
    message: impl Into<String>,
) -> impl Fn(crate::core::QuerySignal) -> std::future::Ready<Result<(), QueryError>> + Send + 'static
{
    let message = message.into();
    move |_signal| std::future::ready(Err(QueryError::response(message.clone())))
}

// ── Assertion helpers ──────────────────────────────────────────────────

/// Assert that a resource has the expected status.
#[allow(dead_code)]
pub fn assert_status(resource: &QueryResource<impl Clone, impl Clone>, expected: QueryStatus) {
    let actual = resource.status();
    assert_eq!(
        actual, expected,
        "expected status {:?} but got {:?}",
        expected, actual
    );
}

/// Assert that a resource's data matches the expected value.
#[allow(dead_code)]
pub fn assert_data<T: PartialEq + std::fmt::Debug>(
    resource: &QueryResource<T, impl Clone>,
    expected: Option<&T>,
) {
    let actual = resource.data();
    assert_eq!(
        actual, expected,
        "expected data {:?} but got {:?}",
        expected, actual
    );
}

/// Extract the error message from a resource, if it has an error.
#[allow(dead_code)]
pub fn error_message<E: Clone>(resource: &QueryResource<impl Clone, E>) -> Option<String>
where
    E: std::fmt::Display,
{
    resource.error().map(|e| e.to_string())
}

/// Assert that a resource's error message matches the expected string.
#[allow(dead_code)]
pub fn assert_error_message<E: Clone + std::fmt::Display>(
    resource: &QueryResource<impl Clone, E>,
    expected: &str,
) {
    let msg = error_message(resource);
    assert_eq!(
        msg.as_deref(),
        Some(expected),
        "expected error message {:?} but got {:?}",
        expected,
        msg
    );
}

// ── Resource factories for state-transition tests ──────────────────────

/// Create a resource with `NoCache` + `LatestWins`.
///
/// Every `begin_request` on this resource will return `Started` (never `CacheHit`),
/// making it ideal for state-transition tests that want deterministic control
/// over every fetch lifecycle step without worrying about TTL freshness windows.
pub fn nocache_resource(key: impl Into<QueryKey>) -> QueryResource<&'static str> {
    QueryResource::new(key, CachePolicy::NoCache, RequestPolicy::LatestWins)
}

/// Create a fresh resource with a fixed key for state-transition invariant tests.
///
/// Convenience alias for [`nocache_resource`] with key `"invariant-test"`.
/// Every `begin_request` on this resource will return `Started` (never `CacheHit`).
#[allow(dead_code)]
pub fn fresh_resource() -> QueryResource<&'static str> {
    nocache_resource("invariant-test")
}

/// Begin a request on the resource and extract the `RequestId`.
///
/// Panics with a descriptive message if the result is anything other than `Started`.
/// Use this in tests that need the `request_id` for subsequent `complete_*` calls
/// but don't care about the full `QueryBeginResult`.
pub fn begin_request_id(
    r: &mut QueryResource<impl Clone, impl Clone>,
    seq: &mut RequestSequencer,
    now_ms: u128,
    mode: QueryFetchMode,
) -> RequestId {
    match r.begin_request(seq, now_ms, mode) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!(
            "begin_request_id() expected Started, got {:?} \
             (status={:?}, active_request_id={:?})",
            other,
            r.status(),
            r.active_request_id(),
        ),
    }
}

// ── Test fixture types ─────────────────────────────────────────────────

/// A simple user struct for integration tests.
#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: u32,
    pub name: String,
}

impl User {
    pub fn new(id: u32, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
        }
    }

    /// A default test user (id: 1, name: "Alice").
    pub fn default() -> Self {
        Self::new(1, "Alice")
    }
}

/// A simple post struct for integration tests.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub id: u32,
    pub title: String,
}

impl Post {
    #[allow(dead_code)]
    pub fn new(id: u32, title: &str) -> Self {
        Self {
            id,
            title: title.to_string(),
        }
    }

    /// A default test post (id: 1, title: "Hello World").
    #[allow(dead_code)]
    pub fn default() -> Self {
        Self::new(1, "Hello World")
    }
}

// ── Time helpers ───────────────────────────────────────────────────────

/// A fixed "now" timestamp for deterministic cache tests (ms since UNIX epoch).
#[allow(dead_code)]
pub const TEST_NOW_MS: u128 = 1_000_000;

/// Advance test time by the given number of milliseconds.
#[allow(dead_code)]
pub fn test_time_after(base_ms: u128, delta_ms: u64) -> u128 {
    base_ms + delta_ms as u128
}
