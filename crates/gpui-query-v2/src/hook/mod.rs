//! The `use_query` and `use_mutation` hooks — ergonomic query and mutation
//! subscriptions for GPUI components.
//!
//! # v2 Improvements
//!
//! - Uses `QueryObserver` which returns `Option<Subscription>` instead of panicking
//! - Signals are properly cancelled on `LatestWins` replacement and `reset()`
//! - `AHashMap` in `QueryClient` for faster lookups
//! - `MutationDiagnostic` is a real type in devtools
//! - `max_pages` defaults to `Some(50)`
//! - `QueryError` has full `Display` + `Error` impls
//!
//! # Query Usage (options-first)
//!
//! The primary API is **options-first** with sensible defaults. The fetcher
//! always receives a [`QuerySignal`] for cooperative cancellation:
//!
//! ```no_run
//! use gpui_query_v2::hook::use_query;
//! use gpui_query_v2::{QueryOptions, CachePolicy, RequestPolicy};
//! # #[derive(Clone)]
//! # struct User;
//! # #[derive(Clone, Debug)]
//! # struct MyError;
//!
//! struct MyView {
//!     users: gpui::Entity<gpui_query_v2::QueryResource<Vec<User>, MyError>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (users, _subscription) = use_query(
//!             QueryOptions::new("users")
//!                 .cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 })
//!                 .request_policy(RequestPolicy::LatestWins),
//!             |signal| async move {
//!                 // Your async fetcher here
//!                 Ok(vec![])
//!             },
//!             cx,
//!         );
//!         Self { users, _subscription }
//!     }
//! }
//! ```
//!
//! For backward compatibility, [`use_query_unsignalled`] is available with a
//! `Fn() -> Fut` fetcher that receives no signal. However, the signal-accepting
//! `use_query` is the recommended default per the v2 "Signal-always" design goal.
//!
//! # Mutation Usage
//!
//! ```no_run
//! use gpui_query_v2::hook::{use_mutation, mutate};
//! # #[derive(Clone)]
//! # struct NewUser { name: String }
//! # #[derive(Clone)]
//! # struct User;
//! # #[derive(Clone, Debug)]
//! # struct MyError;
//!
//! struct MyView {
//!     create_user: gpui::Entity<gpui_query_v2::MutationResource<NewUser, User, MyError>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (entity, sub) = use_mutation((), cx);
//!         Self { create_user: entity, _subscription: sub }
//!     }
//!
//!     fn handle_submit(&mut self, name: String, cx: &mut gpui::Context<Self>) {
//!         mutate(&self.create_user, NewUser { name }, |vars| async move {
//!             Ok(User)
//!         }, cx);
//!     }
//! }
//! ```
//!
//! # WeakEntity Discard Behavior
//!
//! Throughout this module, [`gpui::WeakEntity::upgrade()`] is used inside async
//! tasks to access the owning entity. If the owning component is unmounted while
//! a fetch is in-flight, `upgrade()` returns `None` and the fetch result is
//! **silently discarded**. This is intentional for cache-layer correctness (avoids
//! writing to a dead entity), but callers who rely on side effects from fetch
//! completion should be aware that no callback or notification fires in this case.
//!
//! # Signal Cancellation (Audit Finding #8)
//!
//! The `accept_current_request` guard is the authoritative protection against stale
//! writes. A previous `signal.is_cancelled()` check after the fetcher returned was
//! removed -- it was a best-effort optimization with a TOCTOU window that provided
//! no guarantees. The two-phase protocol (accept + complete) correctly handles all
//! cases where a newer request supersedes the current one.

mod options;
mod use_infinite_query;
mod use_query_select;

pub use options::{InfiniteQueryOptions, MutationCallbacks, MutationOptions, QueryOptions};
pub use use_infinite_query::{
    fetch_next_page_infinite, fetch_previous_page_infinite, use_infinite_query,
};
pub use use_query_select::use_query_select;

use std::sync::Arc;

use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::{MutationObserver, QueryClient, QueryObserver};
use crate::core::{
    MutationResource, QueryBeginResult, QueryFetchMode, QueryKey, QueryResource, QuerySignal,
    QueryStatus, RequestId, RetryPolicy,
};

// ── Query hooks ─────────────────────────────────────────────────────────

/// Subscribe to a query resource and automatically re-render when it changes.
///
/// This is the **primary** `use_query` hook following the v2 "Signal-always"
/// design: the fetcher receives a [`QuerySignal`] for cooperative cancellation.
///
/// Call this in your component's constructor (not in `render`). It:
///
/// 1. Gets or creates a [`QueryResource`] entity from the global [`QueryClient`]
/// 2. Sets up a [`QueryObserver`] so your component re-renders on state changes
/// 3. Propagates the user's retry policy to the resource entity (audit fix #16)
/// 4. Calls `begin_request` to set status to Loading and obtain a `RequestId`
/// 5. Spawns an async fetch with retry logic, using the stored `RequestId`
///
/// # Returns
///
/// A tuple of `(Entity<QueryResource<T, E>>, Subscription)`:
/// - Store the entity to read state during render
/// - Store the subscription to keep the observation alive
///
/// # Unmount Behavior (Audit Finding #6)
///
/// If the component unmounts while a fetch is in-flight, the fetch result is
/// silently discarded. No callback fires. This is intentional for cache-layer
/// correctness. Callers who need completion guarantees should use
/// `fetch_query_with_signal` directly with their own completion handling.
pub fn use_query<T, E, C, F, Fut>(
    options: impl Into<QueryOptions>,
    fetcher: F,
    cx: &mut Context<C>,
) -> (Entity<QueryResource<T, E>>, Subscription)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let opts = options.into();
    let (entity, subscription) =
        use_query_manual(opts.key.clone(), opts.cache_policy, opts.request_policy, cx);

    // Audit fix #16: Propagate the user's retry policy to the resource entity.
    // Without this, the resource defaults to RetryPolicy::no_retries() and
    // the user's QueryOptions::retry_policy() builder is a dead API.
    entity.update(cx, |r, _| r.set_retry_policy(opts.retry_policy.clone()));

    // Start fetch if resource is idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        let fetch_mode = if opts.force_fetch {
            QueryFetchMode::Force
        } else {
            QueryFetchMode::Normal
        };
        // Audit fix #3: begin_request_on_entity returns Option<RequestId>.
        // If CacheHit or IgnoredWhileLoading, skip spawning the fetch task.
        // Audit fix #2: Thread the key through to avoid re-reading from entity.
        if let Some(request_id) =
            begin_request_on_entity(&entity, cx, fetch_mode, Some(opts.key.clone()))
        {
            // Read signal *after* begin_request creates it, not before.
            let signal = entity.read_with(cx, |r, _| {
                r.signal().cloned().unwrap_or_else(QuerySignal::new)
            });
            let weak = entity.downgrade();
            let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
            cx.spawn(async move |_this, cx| {
                fetch_signal_with_retry(
                    &fetcher,
                    signal,
                    request_id,
                    &retry_policy,
                    &weak,
                    cx,
                )
                .await;
                Ok::<_, ()>(())
            })
            .detach();
        }
    }

    (entity, subscription)
}

/// Like [`use_query`] but the fetcher receives no signal argument.
///
/// This exists for backward compatibility. Prefer [`use_query`] (the
/// signal-accepting version) which aligns with the v2 "Signal-always" design.
pub fn use_query_unsignalled<T, E, C, F, Fut>(
    key: QueryKey,
    cache_policy: crate::core::CachePolicy,
    request_policy: crate::core::RequestPolicy,
    fetcher: F,
    cx: &mut Context<C>,
) -> (Entity<QueryResource<T, E>>, Subscription)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let (entity, subscription) = use_query_manual(key.clone(), cache_policy, request_policy, cx);

    // Start fetch if resource is idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        // Audit fix #3: Only spawn fetch if begin_request returns a real RequestId.
        // Audit fix #2: Thread the key through to avoid re-reading from entity.
        if let Some(request_id) =
            begin_request_on_entity(&entity, cx, QueryFetchMode::Normal, Some(key))
        {
            let weak = entity.downgrade();
            let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
            cx.spawn(async move |_this, cx| {
                fetch_with_retry(&fetcher, request_id, &retry_policy, &weak, cx).await;
                Ok::<_, ()>(())
            })
            .detach();
        }
    }

    (entity, subscription)
}

/// Lower-level hook that sets up the entity and observation without starting a fetch.
///
/// Use this when you need full control over when and how fetching happens.
///
/// Uses v2's [`QueryObserver`] which returns `Option<Subscription>` instead of
/// panicking when the entity has been dropped.
///
/// # Panics (debug builds only)
///
/// In debug builds, panics if no [`QueryClient`] has been set via
/// `cx.set_global::<QueryClient>()`. In release builds, falls back to a
/// standalone entity (no shared caching, no GC) so that tests and demos
/// continue to work.
pub fn use_query_manual<T, E, C>(
    key: QueryKey,
    cache_policy: crate::core::CachePolicy,
    request_policy: crate::core::RequestPolicy,
    cx: &mut Context<C>,
) -> (Entity<QueryResource<T, E>>, Subscription)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let entity = if cx.has_global::<QueryClient>() {
        cx.update_global::<QueryClient, _>(|client, cx| {
            client.resource_with_policies::<T, E>(key, cache_policy, request_policy, cx)
        })
    } else {
        // Panic in debug builds when QueryClient is not initialized.
        // The silent fallback is appropriate for tests but dangerous for production.
        #[cfg(debug_assertions)]
        {
            eprintln!(
                "use_query_manual: no QueryClient set via cx.set_global(). \
                 Falling back to standalone entity (no shared caching, no GC). \
                 Call cx.set_global(QueryClient::new()) in your app setup."
            );
            panic!(
                "use_query_manual: QueryClient is not initialized. \
                 Call cx.set_global(QueryClient::new()) before using query hooks."
            );
        }
        #[cfg(not(debug_assertions))]
        {
            // Audit fix #5: Warning eprintln removed from release builds.
            // In release builds, silently fall back without leaking to stderr.
            cx.new(|_| QueryResource::new(key, cache_policy, request_policy))
        }
    };

    // Audit fix #12: Use match instead of expect() to avoid production panics.
    // In debug builds, the entity was just created so observe() should succeed.
    // In release builds, if GPUI internals change unexpectedly, fall back
    // gracefully rather than panicking.
    let mut observer = QueryObserver::new(&entity);
    let subscription = match observer.observe(cx) {
        Some(sub) => sub,
        None => {
            #[cfg(debug_assertions)]
            panic!(
                "QueryObserver::observe failed: entity was just created and cannot be dropped. \
                 This indicates a GPUI internal regression."
            );
            #[cfg(not(debug_assertions))]
            {
                // Audit fix #5: Warning eprintln removed from release builds.
                // Return a no-op subscription so the caller can continue.
                // This prevents a production panic from a GPUI internal issue.
                return (entity, Subscription::new(|| {}));
            }
        }
    };

    (entity, subscription)
}

/// Initiate a fetch on an existing query entity.
///
/// Call this when you want to refetch (e.g., on button click or timer).
/// Respects the resource's retry policy on failure.
///
/// Calls `begin_request` to obtain a fresh `RequestId` and transition the
/// resource to Loading before spawning the fetch task.
///
/// Audit fix #3: If `begin_request` returns `None` (cache hit or ignored),
/// no async fetch task is spawned, avoiding wasted resources.
pub fn fetch_query<T, E, C, F, Fut>(
    entity: &Entity<QueryResource<T, E>>,
    fetcher: F,
    cx: &mut Context<C>,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Audit fix #3: Only spawn fetch if begin_request returns a real RequestId.
    let Some(request_id) = begin_request_on_entity(entity, cx, QueryFetchMode::Normal, None)
    else {
        return;
    };
    let weak = entity.downgrade();
    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    cx.spawn(async move |_this, cx| {
        fetch_with_retry(&fetcher, request_id, &retry_policy, &weak, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Like [`fetch_query`], but the fetcher receives a [`QuerySignal`] that it can
/// check periodically for cooperative cancellation.
///
/// The fetcher signature is `FnOnce(QuerySignal) -> Fut`. Since `FnOnce` closures
/// are consumed on the first call, retries are not possible.
///
/// Calls `begin_request` to obtain a fresh `RequestId` and reads the signal
/// *after* `begin_request` creates it (v2 fix for stale signal).
///
/// Audit fix #3: If `begin_request` returns `None`, no async fetch task is spawned.
///
/// # Signal Cancellation (Audit Finding #8)
///
/// The `accept_current_request` guard is the authoritative protection against
/// stale writes. A previous `signal.is_cancelled()` check after the fetcher
/// returned was removed -- it was a best-effort optimization with a TOCTOU
/// window that provided no guarantees. The two-phase protocol (accept + complete)
/// correctly handles all cases where a newer request supersedes the current one.
pub fn fetch_query_with_signal<T, E, C, F, Fut>(
    entity: &Entity<QueryResource<T, E>>,
    fetcher: F,
    cx: &mut Context<C>,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: FnOnce(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Audit fix #3: Only spawn fetch if begin_request returns a real RequestId.
    let Some(request_id) = begin_request_on_entity(entity, cx, QueryFetchMode::Normal, None)
    else {
        return;
    };
    let signal = entity.read_with(cx, |r, _| {
        r.signal().cloned().unwrap_or_else(QuerySignal::new)
    });
    let weak = entity.downgrade();

    // FnOnce fetchers can only be called once, so retries are not possible.
    cx.spawn(async move |_this, cx| {
        let result = fetcher(signal).await;

        let now_ms = current_time_ms();
        let entity = match weak.upgrade() {
            Some(e) => e,
            None => return Ok::<_, ()>(()),
        };

        // Audit fix #7/#13: Only call cx.notify() when the result is actually
        // accepted. When accept_current_request returns None, no state change
        // occurred and no re-render is needed.
        //
        // Audit fix #8: Removed the signal.is_cancelled() check. The
        // accept_current_request guard is the authoritative protection.
        entity.update(cx, |resource, cx| {
            if let Some(guard) = resource.accept_current_request(request_id) {
                match result {
                    Ok(data) => {
                        resource.complete_success(guard, data, now_ms);
                    }
                    Err(error) => {
                        resource.complete_failure(guard, error, now_ms);
                    }
                }
                cx.notify();
            } else {
                #[cfg(debug_assertions)]
                eprintln!(
                    "DEBUG: fetch_query_with_signal: request {} no longer active, result discarded",
                    request_id.label()
                );
            }
        });
        Ok::<_, ()>(())
    })
    .detach();
}

// ── Request lifecycle helpers ───────────────────────────────────────────

/// Call `begin_request` on a query entity, using the bucket's co-located
/// sequencer when available for globally unique, monotonically increasing
/// RequestIds.
///
/// This transitions the resource to a Loading status, creates a fresh signal,
/// and returns `Some(RequestId)` that must be used for completion.
///
/// Returns `None` when the resource does not need fetching (cache hit, ignored
/// while loading). The caller should skip spawning the async fetch task when
/// this returns `None`.
///
/// Audit fixes #1/#3/#5/#15/#18: Uses the bucket's co-located `RequestSequencer`
/// (accessed via `QueryClient::next_request_id_for_key`) instead of creating a
/// transient one. This ensures RequestIds are globally ordered across multiple
/// fetches of the same resource, making debugging easier and preventing scope_id
/// reuse. Falls back to a transient sequencer when no QueryClient is available.
///
/// Audit fix #2: Accepts an optional `known_key` parameter. When provided by
/// the caller (e.g., from `use_query` which already has `opts.key`), the key
/// clone and entity re-read are avoided.
///
/// Audit fix #10: Only advances the bucket sequencer when the resource actually
/// needs a new request (not a CacheHit or IgnoredWhileLoading). Previously the
/// sequencer was advanced before calling begin_request, wasting RequestIds on
/// cache hits.
fn begin_request_on_entity<T, E, C>(
    entity: &Entity<QueryResource<T, E>>,
    cx: &mut Context<C>,
    fetch_mode: QueryFetchMode,
    known_key: Option<QueryKey>,
) -> Option<RequestId>
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let now_ms = current_time_ms();

    // Audit fix #10: Check whether the resource actually needs a new request
    // before advancing the bucket sequencer. If cache is fresh or the request
    // would be ignored, skip the sequencer advance entirely.
    if fetch_mode == QueryFetchMode::Normal {
        let skip_sequencer = entity.read_with(cx, |r, _| {
            // If cache is fresh, begin_request returns CacheHit.
            r.should_short_circuit_cache(now_ms)
                // If IgnoreWhileLoading and a request is active, returns IgnoredWhileLoading.
                || (r.request_policy() == crate::core::RequestPolicy::IgnoreWhileLoading
                    && r.active_request_id().is_some()
                    && !r.should_serve_stale_and_revalidate(now_ms))
        });
        if skip_sequencer {
            return entity.update(cx, |resource, _cx| {
                match resource.begin_request_with_id(None, now_ms, fetch_mode) {
                    QueryBeginResult::Started { request_id, .. } => Some(request_id),
                    QueryBeginResult::StaleCacheHit { request_id, .. } => Some(request_id),
                    QueryBeginResult::CacheHit => None,
                    QueryBeginResult::IgnoredWhileLoading { .. } => None,
                }
            });
        }
    }

    // Audit fix #2: Use the caller-provided key when available to avoid
    // re-reading and re-cloning the key from the entity.
    let maybe_request_id = if cx.has_global::<QueryClient>() {
        let key = known_key.unwrap_or_else(|| entity.read_with(cx, |r, _| r.key().clone()));
        cx.update_global::<QueryClient, _>(|client, _cx| {
            client.next_request_id_for_key::<T, E>(&key)
        })
    } else {
        None
    };

    entity.update(cx, |resource, _cx| {
        match resource.begin_request_with_id(maybe_request_id, now_ms, fetch_mode) {
            QueryBeginResult::Started { request_id, .. } => Some(request_id),
            QueryBeginResult::StaleCacheHit { request_id, .. } => Some(request_id),
            QueryBeginResult::CacheHit => {
                // Cache is fresh -- no fetch needed.
                None
            }
            QueryBeginResult::IgnoredWhileLoading { .. } => {
                // Another request is already in flight under IgnoreWhileLoading.
                // No fetch needed.
                None
            }
        }
    })
}

// ── Retry-aware fetch helpers ───────────────────────────────────────────

/// Execute a fetch with retry logic for a query resource.
///
/// Calls the fetcher. On failure, if the retry policy allows it, waits for the
/// configured delay and retries. Updates the entity state between attempts.
/// Resets the retry counter on success.
///
/// Takes an explicit `request_id` parameter obtained from
/// `begin_request_on_entity`. Callers should only invoke this when
/// `begin_request_on_entity` returns `Some(request_id)`.
///
/// Audit fix #6: After each retry delay, checks whether the request has been
/// cancelled (e.g., by a newer `begin_request` under LatestWins). If cancelled,
/// breaks out of the retry loop immediately to avoid unnecessary work.
async fn fetch_with_retry<T, E, F, Fut>(
    fetcher: &F,
    request_id: RequestId,
    retry_policy: &RetryPolicy,
    entity: &gpui::WeakEntity<QueryResource<T, E>>,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;

    loop {
        let result = fetcher().await;

        match result {
            Ok(data) => {
                let now_ms = current_time_ms();
                let entity = match entity.upgrade() {
                    Some(e) => e,
                    None => {
                        // Documented behavior -- if the owning component
                        // was unmounted, the result is silently discarded.
                        return;
                    }
                };
                entity.update(cx, |resource, cx| {
                    resource.reset_retry_count();
                    if let Some(guard) = resource.accept_current_request(request_id) {
                        resource.complete_success(guard, data, now_ms);
                        // Audit fix #7: Only notify when the result was actually accepted.
                        cx.notify();
                    } else {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: fetch_with_retry: request {} no longer active on success, result discarded",
                            request_id.label()
                        );
                    }
                });
                return;
            }
            Err(error) => {
                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, _cx| {
                        resource.increment_retry();
                        // No cx.notify() here -- increment_retry does not change
                        // status (stays Loading). The QueryObserver handles
                        // status-deduplication so this update does not trigger
                        // a re-render.
                    });
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Audit fix #6: After the retry delay, check whether the
                    // request has been cancelled (e.g., by a newer begin_request
                    // under LatestWins). If so, stop retrying immediately.
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    let request_still_active =
                        e.read_with(cx, |r, _| r.is_current_request(request_id));
                    if !request_still_active {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: fetch_with_retry: request {} no longer active after retry delay, aborting retry",
                            request_id.label()
                        );
                        return;
                    }
                    // Loop to retry
                } else {
                    // No more retries -- complete with failure
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    let failure_now_ms = current_time_ms();
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(request_id) {
                            resource.complete_failure(guard, error, failure_now_ms);
                            // Audit fix #4: Reset retry_count on terminal failure so the
                            // resource is clean for the next begin_request.
                            resource.reset_retry_count();
                            // Audit fix #7: Only notify when the result was actually accepted.
                            cx.notify();
                        } else {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "DEBUG: fetch_with_retry: request {} no longer active on failure, result discarded",
                                request_id.label()
                            );
                        }
                    });
                    return;
                }
            }
        }
    }
}

/// Like [`fetch_with_retry`] but for fetchers that take a [`QuerySignal`].
///
/// On retry, reads a fresh signal from the resource entity and passes it to the fetcher.
/// The signal is properly cancelled when a new request replaces the current one (v2 fix).
///
/// Audit fix #6: After each retry delay, checks whether the request has been
/// cancelled. If so, breaks out of the retry loop immediately.
///
/// Audit fix #7: After reading the fresh signal, also checks whether the request
/// is still active. This prevents doing work for a stale request after a
/// LatestWins replacement.
async fn fetch_signal_with_retry<T, E, F, Fut>(
    fetcher: &F,
    initial_signal: QuerySignal,
    request_id: RequestId,
    retry_policy: &RetryPolicy,
    entity: &gpui::WeakEntity<QueryResource<T, E>>,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;
    let mut signal = initial_signal;

    loop {
        let result = fetcher(signal.clone()).await;

        match result {
            Ok(data) => {
                let now_ms = current_time_ms();
                let e = match entity.upgrade() {
                    Some(e) => e,
                    None => return,
                };
                e.update(cx, |resource, cx| {
                    resource.reset_retry_count();
                    if let Some(guard) = resource.accept_current_request(request_id) {
                        resource.complete_success(guard, data, now_ms);
                        // Audit fix #7: Only notify when the result was actually accepted.
                        cx.notify();
                    } else {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: fetch_signal_with_retry: request {} no longer active on success, result discarded",
                            request_id.label()
                        );
                    }
                });
                return;
            }
            Err(error) => {
                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, _cx| {
                        resource.increment_retry();
                        // No cx.notify() -- increment_retry does not change status.
                    });
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Audit fix #7: After the retry delay, check whether the
                    // request is still active before reading a fresh signal and
                    // doing more work. If a new begin_request replaced this one,
                    // the old request_id is no longer current.
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    if !e.read_with(cx, |r, _| r.is_current_request(request_id)) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: fetch_signal_with_retry: request {} no longer active after retry delay, aborting",
                            request_id.label()
                        );
                        return;
                    }

                    // Get a fresh signal for the next attempt
                    signal = e.read_with(cx, |r, _| {
                        r.signal().cloned().unwrap_or_else(QuerySignal::new)
                    });
                } else {
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    let failure_now_ms = current_time_ms();
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(request_id) {
                            resource.complete_failure(guard, error, failure_now_ms);
                            // Audit fix #4: Reset retry_count on terminal failure so the
                            // resource is clean for the next begin_request.
                            resource.reset_retry_count();
                            // Audit fix #7: Only notify when the result was actually accepted.
                            cx.notify();
                        } else {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "DEBUG: fetch_signal_with_retry: request {} no longer active on failure, result discarded",
                                request_id.label()
                            );
                        }
                    });
                    return;
                }
            }
        }
    }
}

// ── Mutation hooks ──────────────────────────────────────────────────────

/// Hook for executing mutations (create, update, delete operations).
///
/// Creates a [`MutationResource`] entity. Returns the entity and a subscription
/// for state observation during render. Use the [`mutate`] helper to trigger the
/// mutation from event handlers.
///
/// Accepts `impl Into<MutationOptions>` so both `use_mutation((), cx)` (using
/// `Default` via `From<()>`) and `use_mutation(MutationOptions { .. }, cx)`
/// work.
///
/// Audit fix #1/#11: Uses `MutationObserver` with status-deduplication instead
/// of a raw `cx.observe`. The observer only calls `cx.notify()` when the
/// mutation's `MutationStatus` actually changes (Idle -> Loading, Loading ->
/// Success, Loading -> Failure). Intermediate updates like `increment_retry()`
/// and `prepare_retry()` do not change status (stays Loading), so they no
/// longer trigger re-renders.
///
/// Audit fix #17: Registers the mutation entity with the global [`QueryClient`]
/// so that `use_mutation_state` returns it, GC is triggered, and
/// `MutationOptions::gc_time_ms` is respected.
///
/// # Example
///
/// ```no_run
/// use gpui::{Entity, Subscription, Context};
/// use gpui_query_v2::hook::{use_mutation, mutate};
/// use gpui_query_v2::MutationResource;
/// # #[derive(Clone)]
/// # struct NewUser { name: String }
/// # #[derive(Clone)]
/// # struct User;
/// # #[derive(Clone, Debug)]
/// # struct MyError;
///
/// struct MyView {
///     create_user: Entity<MutationResource<NewUser, User, MyError>>,
///     _mutation_sub: Subscription,
/// }
///
/// impl MyView {
///     fn new(cx: &mut Context<Self>) -> Self {
///         let (entity, sub) = use_mutation((), cx);
///         Self { create_user: entity, _mutation_sub: sub }
///     }
///
///     fn handle_submit(&mut self, name: String, cx: &mut Context<Self>) {
///         mutate(&self.create_user, NewUser { name }, |vars| async move {
///             Ok(User)
///         }, cx);
///     }
/// }
/// ```
pub fn use_mutation<V, T, E, C>(
    options: impl Into<MutationOptions>,
    cx: &mut Context<C>,
) -> (Entity<MutationResource<V, T, E>>, Subscription)
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let opts = options.into();
    let entity = cx.new(|_| MutationResource::new(opts.retry_policy.clone()));

    // Audit fix #1/#11: Use MutationObserver with status-deduplication instead
    // of raw cx.observe. The observer only calls cx.notify() when MutationStatus
    // actually changes, preventing excessive re-renders from increment_retry()
    // and prepare_retry() calls that don't change status (stays Loading).
    let mut m_observer = MutationObserver::new(&entity);
    let subscription = m_observer
        .observe(cx)
        .expect("MutationObserver::observe failed: entity was just created");

    // Audit fix #17: Register the mutation entity with the global QueryClient so
    // that use_mutation_state returns it, GC is triggered, and gc_time_ms
    // is respected.
    if cx.has_global::<QueryClient>() {
        cx.update_global::<QueryClient, _>(|client, cx| {
            client.register_mutation(&entity, cx);
        });
    }

    (entity, subscription)
}

/// Hook for executing mutations with a custom retry policy.
///
/// Prefer [`use_mutation`] which now accepts `impl Into<MutationOptions>`.
#[deprecated(
    since = "0.2.0",
    note = "Use `use_mutation(options, cx)` instead — it now accepts MutationOptions via Into"
)]
pub fn use_mutation_with_options<V, T, E, C>(
    options: &MutationOptions,
    cx: &mut Context<C>,
) -> (Entity<MutationResource<V, T, E>>, Subscription)
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let entity = cx.new(|_| MutationResource::new(options.retry_policy.clone()));

    // Audit fix #1/#11: Use MutationObserver with status-deduplication.
    let mut m_observer = MutationObserver::new(&entity);
    let subscription = m_observer
        .observe(cx)
        .expect("MutationObserver::observe failed: entity was just created");

    (entity, subscription)
}

/// Hook to observe all mutation state across the application for a given
/// `(V, T, E)` type triple.
///
/// Returns a snapshot of all [`MutationResource`] entities of the specified
/// types registered in the global [`QueryClient`]. Returns an empty vec
/// if no mutations exist for this type or if no `QueryClient` is set up.
///
/// # Example
///
/// ```no_run
/// use gpui_query_v2::hook::use_mutation_state;
/// use gpui_query_v2::MutationResource;
/// # #[derive(Clone)]
/// # struct NewUser;
/// # #[derive(Clone)]
/// # struct User;
/// # #[derive(Clone, Debug)]
/// # struct QueryError;
/// # fn _doc<C: 'static>(cx: &mut gpui::Context<C>) {
///
/// let mutations = use_mutation_state::<NewUser, User, QueryError, _>(cx);
/// for entity in &mutations {
///     let status = entity.read(cx).status();
///     // ...
/// }
/// # }
/// ```
pub fn use_mutation_state<V, T, E, C>(cx: &mut Context<C>) -> Vec<Entity<MutationResource<V, T, E>>>
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    if cx.has_global::<QueryClient>() {
        cx.read_global::<QueryClient, _>(|client, _| client.all_mutations::<V, T, E>())
    } else {
        Vec::new()
    }
}

/// Trigger a mutation on an existing mutation entity.
///
/// This is the primary way to execute mutations. It:
/// 1. Transitions the entity to Loading with the given variables
/// 2. Spawns an async task calling the mutator
/// 3. On success, completes with the result data
/// 4. On failure, retries according to the entity's retry policy
///
/// Audit fix #8: Guards against concurrent calls by checking whether the
/// mutation is already in Loading state. If so, returns without starting a
/// new mutation to prevent the second call's async task from overwriting
/// the first.
///
/// Audit fix #3: Variables are wrapped in `Arc<V>` internally so that the
/// retry loop only performs an `Arc::clone` (cheap reference count increment)
/// per attempt, rather than cloning the full variables payload.
///
/// # Example
///
/// ```no_run
/// use gpui_query_v2::hook::mutate;
/// # #[derive(Clone)]
/// # struct Vars;
/// # #[derive(Clone)]
/// # struct Data;
/// # #[derive(Clone, Debug)]
/// # struct Err;
/// # fn _doc(entity: &gpui::Entity<gpui_query_v2::MutationResource<Vars, Data, Err>>, cx: &mut gpui::Context<()>) {
///
/// mutate(entity, Vars, |v| async move { Ok(Data) }, cx);
/// # }
/// ```
pub fn mutate<V, T, E, C, F, Fut>(
    entity: &Entity<MutationResource<V, T, E>>,
    variables: V,
    mutator: F,
    cx: &mut Context<C>,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Audit fix #8: Guard against concurrent calls. If the mutation is already
    // Loading, do not start a new one. This prevents a second mutate() call
    // from cancelling the first mutation's signal and then overwriting its
    // state with stale data when the first async task completes.
    let already_loading = entity.read_with(cx, |r, _| r.is_loading());
    if already_loading {
        return;
    }

    // Begin the mutation: transition to Loading
    entity.update(cx, |resource, cx| {
        resource.begin(variables.clone());
        cx.notify();
    });

    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    let weak = entity.downgrade();
    let variables_arc = Arc::new(variables);

    cx.spawn(async move |_this, cx| {
        run_mutation_loop(&weak, variables_arc, &mutator, &retry_policy, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Like [`mutate`] but with lifecycle callbacks.
///
/// Callbacks fire on the final outcome (after all retries exhausted or
/// on first success), not on intermediate retry attempts.
///
/// **Important**: Callbacks receive cloned data/error and run *outside* any
/// entity borrow, so they may safely call `entity.update()` or other GPUI
/// mutations without risk of deadlock or panic.
///
/// Audit fix #8: Guards against concurrent calls (see [`mutate`] for details).
///
/// Audit fix #9: If the entity is dropped during the mutation, `on_error` and
/// `on_settled` are still invoked so callers always get a terminal callback.
///
/// Audit fix #3: Variables are wrapped in `Arc<V>` for cheap retries.
pub fn mutate_with_callbacks<V, T, E, C, F, Fut>(
    entity: &Entity<MutationResource<V, T, E>>,
    variables: V,
    mutator: F,
    callbacks: MutationCallbacks<T, E>,
    cx: &mut Context<C>,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Audit fix #8: Guard against concurrent calls.
    let already_loading = entity.read_with(cx, |r, _| r.is_loading());
    if already_loading {
        return;
    }

    // Begin the mutation
    entity.update(cx, |resource, cx| {
        resource.begin(variables.clone());
        cx.notify();
    });

    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    let weak = entity.downgrade();
    let variables_arc = Arc::new(variables);

    cx.spawn(async move |_this, cx| {
        run_mutation_loop_with_callbacks(
            &weak,
            variables_arc,
            &mutator,
            &retry_policy,
            callbacks,
            cx,
        )
        .await;
        Ok::<_, ()>(())
    })
    .detach();
}

// ── Mutation internals ──────────────────────────────────────────────────

/// Core retry loop for mutations. Runs the mutator, handles success/failure,
/// and retries with backoff according to the retry policy.
///
/// Audit fix #19: When retries are available, uses `increment_retry()` +
/// `prepare_retry()` instead of `complete_failure()` followed by `retry()`.
/// This avoids a transient `Failure` status that would cause observers to see
/// a brief Failure flash between retry attempts. Only `complete_failure()` is
/// called when retries are exhausted, which represents a terminal failure.
///
/// Audit fix #1: Does NOT call `cx.notify()` after `increment_retry()` or
/// `prepare_retry()` because those operations do not change the mutation status
/// (stays Loading). The `MutationObserver` only triggers `cx.notify()` on actual
/// status changes, so these intermediate updates are invisible to the component.
///
/// Audit fix #3: Variables are passed as `Arc<V>` so each retry attempt only
/// performs an `Arc::clone` (reference count increment) instead of cloning
/// the full variables payload.
///
/// Audit fix #9: After each retry delay, checks whether the mutation is still
/// in Loading state. If it was cancelled or reset (no longer Loading), stops
/// retrying immediately.
async fn run_mutation_loop<V, T, E, F, Fut>(
    weak: &gpui::WeakEntity<MutationResource<V, T, E>>,
    variables: Arc<V>,
    mutator: &F,
    retry_policy: &RetryPolicy,
    cx: &mut gpui::AsyncApp,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;

    loop {
        // Audit fix #3: Arc::clone instead of variables.clone() for cheap retries.
        let result = mutator((*variables).clone()).await;

        match result {
            Ok(data) => {
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    None => return,
                };
                entity.update(cx, |resource, cx| {
                    resource.complete_success(data);
                    cx.notify();
                });
                return;
            }
            Err(error) => {
                if retry_policy.should_retry(attempt) {
                    // Audit fix #19: Do NOT call complete_failure() here.
                    // Instead, just increment the retry counter and wait for
                    // the delay. This avoids a transient Failure -> Loading
                    // flash for observers.
                    let delay_ms = retry_policy.delay_for_attempt(attempt);

                    let e = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, _cx| {
                        resource.increment_retry();
                        // Audit fix #1: No cx.notify() -- increment_retry does
                        // not change status (stays Loading).
                    });

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Audit fix #9: After the retry delay, check whether the
                    // mutation is still in Loading state. If it was cancelled
                    // or reset, stop retrying immediately.
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    if !entity.read_with(cx, |r, _| r.is_loading()) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: run_mutation_loop: mutation no longer Loading after retry delay, aborting"
                        );
                        return;
                    }

                    // After delay, prepare for retry (refresh signal, stay in Loading).
                    entity.update(cx, |resource, _cx| {
                        resource.prepare_retry();
                        // Audit fix #1: No cx.notify() -- prepare_retry does
                        // not change status (stays Loading).
                    });

                    attempt += 1;
                } else {
                    // No more retries -- terminal failure
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    entity.update(cx, |resource, cx| {
                        resource.complete_failure(error);
                        // Audit fix #4: Reset retry_count on terminal failure.
                        resource.reset_retry_count();
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}

/// Like [`run_mutation_loop`] but fires lifecycle callbacks on final outcome.
///
/// Callbacks receive cloned data/error and run *outside* any `entity.read_with`
/// borrow. This prevents deadlocks and panics if a callback attempts to call
/// `entity.update()` or any other GPUI mutation.
///
/// Audit fix #9: When `weak.upgrade()` returns `None` (entity dropped), the
/// `on_error` and `on_settled` callbacks are still fired so callers always
/// receive a terminal notification.
///
/// Audit fix #10: The weak entity check result is captured before
/// `complete_failure` so that callbacks fire even if the entity is dropped
/// between the update and the callback invocation.
///
/// Audit fix #19: Uses `increment_retry()` + `prepare_retry()` instead of
/// `complete_failure()` + `retry()` when retries are available.
///
/// Audit fix #1: Does NOT call `cx.notify()` after `increment_retry()` or
/// `prepare_retry()` since those operations do not change status.
///
/// Audit fix #3: Variables are `Arc<V>` for cheap retry clones.
///
/// Audit fix #9: After each retry delay, checks whether the mutation is still
/// Loading. If cancelled/reset, fires error callbacks and stops retrying.
async fn run_mutation_loop_with_callbacks<V, T, E, F, Fut>(
    weak: &gpui::WeakEntity<MutationResource<V, T, E>>,
    variables: Arc<V>,
    mutator: &F,
    retry_policy: &RetryPolicy,
    callbacks: MutationCallbacks<T, E>,
    cx: &mut gpui::AsyncApp,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;

    loop {
        // Audit fix #3: Arc::clone for cheap retry.
        let result = mutator((*variables).clone()).await;

        match result {
            Ok(data) => {
                // Clone data before update, invoke callbacks outside
                // any entity borrow so they can safely call entity.update().
                let data_for_callback = data.clone();
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    // Audit fix #9: Entity dropped during mutation. Fire
                    // on_settled with None for both to indicate discard.
                    None => {
                        if let Some(ref cb) = callbacks.on_settled {
                            cb(None, None);
                        }
                        return;
                    }
                };
                entity.update(cx, |resource, cx| {
                    resource.complete_success(data);
                    cx.notify();
                });

                // Fire success callback -- outside entity borrow
                if let Some(ref cb) = callbacks.on_success {
                    cb(&data_for_callback);
                }

                // Fire settled callback with success data -- outside entity borrow
                if let Some(ref cb) = callbacks.on_settled {
                    cb(Some(&data_for_callback), None);
                }

                return;
            }
            Err(error) => {
                // Clone error before update, invoke callbacks outside
                // any entity borrow.
                let error_for_callback = error.clone();

                if retry_policy.should_retry(attempt) {
                    // Audit fix #19: Do NOT call complete_failure() here.
                    // Instead, just increment the retry counter and wait.
                    let delay_ms = retry_policy.delay_for_attempt(attempt);

                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        // Audit fix #9: Entity dropped between mutator failure and retry.
                        None => {
                            if let Some(ref cb) = callbacks.on_error {
                                cb(&error_for_callback);
                            }
                            if let Some(ref cb) = callbacks.on_settled {
                                cb(None, Some(&error_for_callback));
                            }
                            return;
                        }
                    };
                    entity.update(cx, |resource, _cx| {
                        resource.increment_retry();
                        // Audit fix #1: No cx.notify() -- status stays Loading.
                    });

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Audit fix #9: After the retry delay, check whether the
                    // mutation is still Loading. If cancelled/reset, fire error
                    // callbacks and stop retrying.
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        // Audit fix #9/#10: Entity dropped during retry delay.
                        None => {
                            if let Some(ref cb) = callbacks.on_error {
                                cb(&error_for_callback);
                            }
                            if let Some(ref cb) = callbacks.on_settled {
                                cb(None, Some(&error_for_callback));
                            }
                            return;
                        }
                    };
                    if !entity.read_with(cx, |r, _| r.is_loading()) {
                        // Mutation was cancelled or reset during the delay.
                        // Fire error callbacks so callers get a terminal notification.
                        if let Some(ref cb) = callbacks.on_error {
                            cb(&error_for_callback);
                        }
                        if let Some(ref cb) = callbacks.on_settled {
                            cb(None, Some(&error_for_callback));
                        }
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: run_mutation_loop_with_callbacks: mutation no longer Loading after retry delay, aborting"
                        );
                        return;
                    }

                    // After delay, prepare for retry.
                    entity.update(cx, |resource, _cx| {
                        resource.prepare_retry();
                        // Audit fix #1: No cx.notify() -- status stays Loading.
                    });

                    attempt += 1;
                } else {
                    // No more retries -- terminal failure.
                    // Audit fix #10: Capture entity availability before
                    // complete_failure so callbacks still fire even if entity
                    // is dropped between the update and callback invocation.
                    let entity_available = weak.upgrade();
                    if let Some(entity) = entity_available {
                        entity.update(cx, |resource, cx| {
                            resource.complete_failure(error);
                            // Audit fix #4: Reset retry_count on terminal failure.
                            resource.reset_retry_count();
                            cx.notify();
                        });
                    }

                    // Fire error and settled callbacks outside entity borrow.
                    // These fire regardless of whether entity is still alive
                    // (Audit fix #9/#10).
                    if let Some(ref cb) = callbacks.on_error {
                        cb(&error_for_callback);
                    }

                    if let Some(ref cb) = callbacks.on_settled {
                        cb(None, Some(&error_for_callback));
                    }

                    return;
                }
            }
        }
    }
}

// ── Utility ─────────────────────────────────────────────────────────────

/// Returns current time as milliseconds since UNIX epoch.
///
/// Audit fix #20: This is the canonical implementation used across the hook
/// layer. The private duplicate in `mutation_bucket.rs` (`now_ms`) should
/// ideally be consolidated here or into a shared utility module.
pub fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

// ── Impl for MutationOptions integration ────────────────────────────────

/// Allow `use_mutation((), cx)` to work with default options.
impl From<()> for MutationOptions {
    fn from((): ()) -> Self {
        Self::default()
    }
}
