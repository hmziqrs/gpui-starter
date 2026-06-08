//! Query hook functions — `use_query`, `use_query_unsignalled`, `use_query_manual`,
//! `fetch_query`, and `fetch_query_with_signal`.

use gpui::{BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::{QueryClient, QueryObserver};
use crate::core::{
    QueryFetchMode, QueryKey, QueryResource, QuerySignal, QueryStatus,
};

use super::current_time_ms;
use super::fetch_retry::{begin_request_on_entity, fetch_signal_with_retry, fetch_with_retry};

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
    options: impl Into<crate::hook::QueryOptions>,
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
