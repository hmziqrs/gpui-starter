//! Request lifecycle helpers and retry-aware fetch functions for query resources.

use gpui::{BorrowAppContext as _, Context, Entity};

use crate::client::QueryClient;
use crate::core::{
    QueryBeginResult, QueryFetchMode, QueryKey, QueryResource, QuerySignal, RequestId, RetryPolicy,
};

use super::current_time_ms;

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
pub(crate) fn begin_request_on_entity<T, E, C>(
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
pub(crate) async fn fetch_with_retry<T, E, F, Fut>(
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
pub(crate) async fn fetch_signal_with_retry<T, E, F, Fut>(
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
