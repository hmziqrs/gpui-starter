use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::QueryClient;
use crate::core::{QueryKey, QueryResource, QuerySignal, QueryStatus, RetryPolicy};

use super::helpers::current_time_ms;

// ── Query hooks ─────────────────────────────────────────────────────────

/// Subscribe to a query resource and automatically re-render when it changes.
///
/// Call this in your component's constructor (not in `render`). It:
///
/// 1. Gets or creates a [`QueryResource`] entity from the global [`QueryClient`]
/// 2. Calls `cx.observe()` so your component re-renders on state changes
/// 3. Starts an async fetch if the resource is idle
/// 4. On failure, retries according to the resource's retry policy
///
/// # Returns
///
/// A tuple of `(Entity<QueryResource<T, E>>, Subscription)`:
/// - Store the entity to read state during render
/// - Store the subscription to keep the observation alive
pub fn use_query<T, E, C, F, Fut>(
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
    let (entity, subscription) = use_query_manual(key, cache_policy, request_policy, cx);

    // Start fetch if resource is idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        let weak = entity.downgrade();
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            fetch_with_retry(&fetcher, &retry_policy, &weak, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }

    (entity, subscription)
}

/// Like [`use_query`], but the fetcher receives a [`QuerySignal`] that it can
/// check periodically for cooperative cancellation.
///
/// The fetcher signature is `Fn(QuerySignal) -> Fut` instead of `Fn() -> Fut`.
/// On failure, retries according to the resource's retry policy by creating
/// a fresh signal for each retry attempt.
pub fn use_query_with_signal<T, E, C, F, Fut>(
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
    F: Fn(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let (entity, subscription) = use_query_manual(key, cache_policy, request_policy, cx);

    // Start fetch if resource is idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        let signal = entity.read_with(cx, |r, _| {
            r.signal().cloned().unwrap_or_else(QuerySignal::new)
        });
        let weak = entity.downgrade();
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            fetch_signal_with_retry(&fetcher, signal, &retry_policy, &weak, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }

    (entity, subscription)
}

/// Lower-level hook that sets up the entity and observation without starting a fetch.
///
/// Use this when you need full control over when and how fetching happens.
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
        cx.new(|_| QueryResource::new(key, cache_policy, request_policy))
    };

    let subscription = cx.observe(&entity, |_, _, cx| {
        cx.notify();
    });

    (entity, subscription)
}

/// Initiate a fetch on an existing query entity.
///
/// Call this when you want to refetch (e.g., on button click or timer).
/// Respects the resource's retry policy on failure.
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
    let weak = entity.downgrade();
    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    cx.spawn(async move |_this, cx| {
        fetch_with_retry(&fetcher, &retry_policy, &weak, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Like [`fetch_query`], but the fetcher receives a [`QuerySignal`] that it can
/// check periodically for cooperative cancellation.
///
/// The fetcher signature is `FnOnce(QuerySignal) -> Fut` instead of `FnOnce() -> Fut`.
/// Note: FnOnce fetchers cannot be retried because the closure is consumed on the first call.
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
        entity.update(cx, |resource, cx| {
            if let Some(guard) = resource.accept_current_request(
                resource
                    .active_request_id()
                    .unwrap_or(crate::core::RequestId::scoped(0, 0)),
            ) {
                match result {
                    Ok(data) => {
                        resource.complete_success(&guard, data, now_ms);
                    }
                    Err(error) => {
                        resource.complete_failure(&guard, error);
                    }
                }
            }
            cx.notify();
        });
        Ok::<_, ()>(())
    })
    .detach();
}

// ── Retry-aware fetch helpers ───────────────────────────────────────────

/// Execute a fetch with retry logic for a query resource.
///
/// Calls the fetcher. On failure, if the retry policy allows it, waits for the
/// configured delay and retries. Updates the entity state between attempts.
/// Resets the retry counter on success.
async fn fetch_with_retry<T, E, F, Fut>(
    fetcher: &F,
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
                    None => return,
                };
                entity.update(cx, |resource, cx| {
                    resource.reset_retry_count();
                    if let Some(guard) = resource.accept_current_request(
                        resource
                            .active_request_id()
                            .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                    ) {
                        resource.complete_success(&guard, data, now_ms);
                    }
                    cx.notify();
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
                    e.update(cx, |resource, cx| {
                        resource.increment_retry();
                        cx.notify();
                    });
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }
                    // Loop to retry
                } else {
                    // No more retries — complete with failure
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(
                            resource
                                .active_request_id()
                                .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                        ) {
                            resource.complete_failure(&guard, error);
                        }
                        cx.notify();
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
async fn fetch_signal_with_retry<T, E, F, Fut>(
    fetcher: &F,
    initial_signal: QuerySignal,
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
                    if let Some(guard) = resource.accept_current_request(
                        resource
                            .active_request_id()
                            .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                    ) {
                        resource.complete_success(&guard, data, now_ms);
                    }
                    cx.notify();
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
                    e.update(cx, |resource, cx| {
                        resource.increment_retry();
                        cx.notify();
                    });
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Get a fresh signal for the next attempt
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    signal = e.read_with(cx, |r, _| {
                        r.signal().cloned().unwrap_or_else(QuerySignal::new)
                    });
                } else {
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(
                            resource
                                .active_request_id()
                                .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                        ) {
                            resource.complete_failure(&guard, error);
                        }
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}
