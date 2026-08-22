//! Internal async fetch runners for infinite query page fetches.
//!
//! These are the retry-aware async functions that execute the actual fetch
//! operations with captured `RequestId`s and two-phase completion protocol.

use crate::core::{InfiniteQueryResource, RequestId};

use crate::hook::{current_time_ms, read_entity};

// ── Internal fetch runners ───────────────────────────────────────────────

/// Execute a fetch-next-page operation with a captured `RequestId`.
///
/// #fix #5/#6: The `request_id` is the one returned from `begin_fetch_next`,
/// not re-read after the fetcher completes. This prevents stale-ID acceptance
/// when concurrent fetches are in flight.
///
/// #fix #12: Uses two-phase completion (`accept_current_request` then
/// `complete_success_with_guard`/`complete_failure_with_guard`) to close
/// the race window between reading active_request_id and completing.
///
/// #fix #13: Applies retry policy on fetch failure.
pub(super) async fn run_fetch_next_page_with_id<T, E, F, Fut>(
    entity: &gpui::WeakEntity<InfiniteQueryResource<T, E>>,
    fetcher: &F,
    request_id: RequestId,
    retry_policy: &crate::core::RetryPolicy,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(Option<&T>) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    // #fix #3: Read the last page reference inside the entity update closure
    // to avoid cloning the entire page data. We only need a reference for the
    // fetcher. However, since the fetcher is async and we can't hold a borrow
    // across .await, we clone only if needed. For the initial fetch (no pages),
    // no clone occurs.
    let last_page_data: Option<T> = {
        let e = match entity.upgrade() {
            Some(e) => e,
            None => return,
        };
        read_entity(&e, cx, |r, _| r.last_page().cloned()).flatten()
    };

    let mut attempt: u32 = 0;

    loop {
        let result = fetcher(last_page_data.as_ref()).await;

        let now_ms = current_time_ms();

        let e = match entity.upgrade() {
            Some(e) => e,
            None => return,
        };

        match result {
            Ok((page, has_more)) => {
                // #fix #12: Two-phase completion — accept then complete.
                e.update(cx, |resource, cx| {
                    if let Some(guard) = resource.accept_current_request(request_id) {
                        resource.complete_success_with_guard(
                            &guard, page, has_more, true, now_ms,
                        );
                        // Notify on terminal state change (success).
                        cx.notify();
                    } else {
                        eprintln!(
                            "DEBUG: run_fetch_next_page_with_id: request {} no longer active, result discarded",
                            request_id.label()
                        );
                    }
                });
                return;
            }
            Err(error) => {
                // #fix #13: Apply retry policy.
                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // #fix #7: After the retry delay, check whether the signal
                    // has been cancelled. A cancelled fetch should not retry.
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    let cancelled = read_entity(&e, cx, |r, _| {
                        r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
                    }).unwrap_or(true);
                    if cancelled {
                        return;
                    }

                    // #fix #1: No cx.notify() during retry wait. Status stays
                    // LoadingWithData/LoadingEmpty during retries, so the
                    // InfiniteQueryObserver deduplicates and no re-render is
                    // needed until terminal state (success or final failure).

                    // Loop to retry
                } else {
                    // No more retries — complete with failure using two-phase protocol
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(request_id) {
                            resource.complete_failure_with_guard(&guard, error);
                        } else {
                            eprintln!(
                                "DEBUG: run_fetch_next_page_with_id: request {} no longer active on failure, result discarded",
                                request_id.label()
                            );
                        }
                        // Notify on terminal state change (failure).
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}

/// Execute a fetch-previous-page operation with a captured `RequestId`.
///
/// Same fixes as `run_fetch_next_page_with_id`:
/// - Captured `RequestId` prevents stale-ID acceptance
/// - Two-phase completion protocol
/// - Retry policy on failure
pub(super) async fn run_fetch_previous_page_with_id<T, E, F, Fut>(
    entity: &gpui::WeakEntity<InfiniteQueryResource<T, E>>,
    fetcher: &F,
    request_id: RequestId,
    retry_policy: &crate::core::RetryPolicy,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(Option<&T>) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    // #fix #3: Read the first page reference inside entity update.
    let first_page_data: Option<T> = {
        let e = match entity.upgrade() {
            Some(e) => e,
            None => return,
        };
        read_entity(&e, cx, |r, _| r.first_page().cloned()).flatten()
    };

    let mut attempt: u32 = 0;

    loop {
        let result = fetcher(first_page_data.as_ref()).await;

        let now_ms = current_time_ms();

        let e = match entity.upgrade() {
            Some(e) => e,
            None => return,
        };

        match result {
            Ok((page, has_more)) => {
                e.update(cx, |resource, cx| {
                    if let Some(guard) = resource.accept_current_request(request_id) {
                        resource.complete_success_with_guard(
                            &guard, page, has_more, false, now_ms,
                        );
                        // Notify on terminal state change (success).
                        cx.notify();
                    } else {
                        eprintln!(
                            "DEBUG: run_fetch_previous_page_with_id: request {} no longer active, result discarded",
                            request_id.label()
                        );
                    }
                });
                return;
            }
            Err(error) => {
                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // #fix #7: Check signal cancellation after retry delay.
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    let cancelled = read_entity(&e, cx, |r, _| {
                        r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
                    }).unwrap_or(true);
                    if cancelled {
                        return;
                    }

                    // #fix #1: No cx.notify() during retry wait.
                } else {
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(request_id) {
                            resource.complete_failure_with_guard(&guard, error);
                        } else {
                            eprintln!(
                                "DEBUG: run_fetch_previous_page_with_id: request {} no longer active on failure, result discarded",
                                request_id.label()
                            );
                        }
                        // Notify on terminal state change (failure).
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}
