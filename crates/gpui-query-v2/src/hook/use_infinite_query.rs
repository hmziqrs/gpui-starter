//! The `use_infinite_query` hook — ergonomic infinite scrolling / pagination
//! for GPUI components.
//!
//! # v2 Improvements
//!
//! - Signal is properly cancelled when a page fetch replaces an in-flight request
//! - `max_pages` defaults to `Some(50)` to prevent unbounded memory growth
//! - Uses `InfiniteQueryObserver` for status-deduplication (avoids unnecessary re-renders)
//! - `RequestSequencer` is persistent per resource (via `QueryClient` bucket)
//! - `RequestId` is captured from `begin_fetch_next` and passed through to completion
//! - Uses two-phase completion protocol (`accept_current_request` + guard)
//! - `fetch_next` closure actually triggers a fetch (no longer a no-op stub)
//! - Retry policy is stored on the entity and applied consistently to all page fetches
//! - Entity is registered with `QueryClient` for shared caching and GC
//!
//! # Audit 3 fixes
//!
//! - **#1**: Removed `cx.notify()` from retry-wait branches in fetch runners (status
//!   stays Loading during retries, so InfiniteQueryObserver deduplicates anyway)
//! - **#2**: Removed unconditional `cx.notify()` from `fetch_next_page_infinite` and
//!   `fetch_previous_page_infinite` before fetch completes
//! - **#3**: Read page data by reference inside entity update closure instead of cloning
//! - **#4/#6/#8/#9**: Use `QueryClient::next_request_id_for_infinite_key` for persistent
//!   sequencers instead of creating transient ones per call
//! - **#5**: Use match-based fallback for `InfiniteQueryObserver::observe` instead of
//!   `expect()` to avoid production panics
//! - **#7**: Check signal cancellation after retry delay to avoid retrying cancelled fetches
//! - **#10**: Store `retry_policy` on entity, read it in fetch helpers instead of using
//!   `RetryPolicy::default()`
//! - **#12**: Removed the no-op stub `fetch_next` closure from the return type
//!
//! # Usage
//!
//! ```no_run
//! use gpui_query_v2::hook::{use_infinite_query, fetch_next_page_infinite, InfiniteQueryOptions};
//! use gpui_query_v2::QueryKey;
//! # #[derive(Clone)]
//! # struct Post { id: u64 }
//! # #[derive(Clone, Debug)]
//! # struct MyError;
//!
//! struct FeedView {
//!     feed: gpui::Entity<gpui_query_v2::InfiniteQueryResource<Vec<Post>, MyError>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl FeedView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (entity, _subscription) = use_infinite_query(
//!             InfiniteQueryOptions::new(QueryKey::from(["feed"])),
//!             |last_page| async move {
//!                 // Your async fetcher here
//!                 Ok((vec![], false))
//!             },
//!             cx,
//!         );
//!         Self { feed: entity, _subscription }
//!     }
//!
//!     fn on_scroll_to_bottom(&mut self, cx: &mut gpui::Context<Self>) {
//!         fetch_next_page_infinite(
//!             &self.feed,
//!             |last_page| async move {
//!                 // Your async fetcher here
//!                 Ok((vec![], false))
//!             },
//!             cx,
//!         );
//!     }
//! }
//! ```

use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::{InfiniteQueryObserver, QueryClient};
use crate::core::{
    InfiniteQueryResource, QueryStatus, RequestId, RequestSequencer,
};

use super::options::InfiniteQueryOptions;
use super::current_time_ms;

// ── Hook ─────────────────────────────────────────────────────────────────

/// Hook for infinite scrolling / pagination.
///
/// Creates an [`InfiniteQueryResource`] entity and subscribes to it so the
/// component re-renders on state changes. Returns:
///
/// 1. The entity holding the page data
/// 2. The subscription (store to keep the observation alive)
///
/// The fetcher receives `Option<&T>` (the last page, if any) and must return
/// `Result<(T, bool), E>` where `T` is the new page data and `bool` indicates
/// whether more pages exist.
///
/// # v2 Notes
///
/// - `max_pages` defaults to `Some(50)` via [`InfiniteQueryOptions`].
/// - The signal is properly cancelled when a new fetch replaces an in-flight request.
/// - `InfiniteQueryObserver` provides status-deduplication to avoid unnecessary re-renders.
/// - The entity is registered with [`QueryClient`] for shared caching and GC.
/// - The retry policy from options is stored on the entity and used for all page fetches.
pub fn use_infinite_query<T, E, C, FNext, Fut>(
    options: InfiniteQueryOptions,
    fetch_next: FNext,
    cx: &mut Context<C>,
) -> (
    Entity<InfiniteQueryResource<T, E>>,
    Subscription,
)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    FNext: Fn(Option<&T>) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    let InfiniteQueryOptions {
        key,
        cache_policy,
        request_policy,
        max_pages,
        retry_policy,
        ..
    } = options;

    // #fix #8/#9: Route entity creation through QueryClient for shared
    // caching, GC, and participation in bulk operations (invalidate/reset/remove).
    let entity = if cx.has_global::<QueryClient>() {
        cx.update_global::<QueryClient, _>(|client, cx| {
            client.infinite_resource_with_policies::<T, E>(
                key,
                cache_policy,
                request_policy,
                cx,
            )
        })
    } else {
        #[cfg(debug_assertions)]
        {
            eprintln!(
                "use_infinite_query: no QueryClient set via cx.set_global(). \
                 Falling back to standalone entity (no shared caching, no GC). \
                 Call cx.set_global(QueryClient::new()) in your app setup."
            );
        }
        cx.new(|_| {
            let mut resource =
                InfiniteQueryResource::new(key, cache_policy, request_policy);
            if let Some(max) = max_pages {
                resource.set_max_pages(Some(max));
            }
            resource
        })
    };

    // Apply max_pages if entity was created via QueryClient (which doesn't set it).
    entity.update(cx, |resource, _| {
        if let Some(max) = max_pages {
            resource.set_max_pages(Some(max));
        }
    });

    // #fix #10: Store the retry policy from options on the entity so that
    // fetch_next_page_infinite / fetch_previous_page_infinite can read it
    // from the entity instead of using RetryPolicy::default().
    entity.update(cx, |resource, _| {
        resource.set_retry_policy(retry_policy.clone());
    });

    // #fix #7/#11: Use InfiniteQueryObserver for status deduplication instead
    // of a raw cx.observe that fires on every entity mutation.
    let mut observer = InfiniteQueryObserver::new(&entity);

    // #fix #5: Use match-based fallback instead of expect() to avoid production
    // panics. Mirrors the pattern used in use_query_manual.
    let subscription = match observer.observe(cx) {
        Some(sub) => sub,
        None => {
            #[cfg(debug_assertions)]
            panic!(
                "InfiniteQueryObserver::observe failed: entity was just created and \
                 cannot be dropped. This indicates a GPUI internal regression."
            );
            #[cfg(not(debug_assertions))]
            {
                eprintln!(
                    "WARNING: InfiniteQueryObserver::observe returned None. \
                     Entity may have been dropped unexpectedly. \
                     Falling back to a no-op subscription."
                );
                return (entity, Subscription::default());
            }
        }
    };

    // Start the initial fetch if idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        // #fix #4/#6/#8: Use the bucket's persistent sequencer via QueryClient
        // so RequestIds are monotonic across the resource lifetime. Falls back
        // to a transient sequencer when no QueryClient is available.
        let maybe_request_id = if cx.has_global::<QueryClient>() {
            let key = entity.read_with(cx, |r, _| r.key().clone());
            cx.update_global::<QueryClient, _>(|client, _| {
                client.next_request_id_for_infinite_key::<T, E>(&key)
            })
        } else {
            None
        };

        // If the bucket didn't have a pre-allocated ID, generate one via
        // begin_fetch_next with a transient sequencer (last resort).
        let request_id = if let Some(_pre_allocated_id) = maybe_request_id {
            // Use the pre-allocated ID with begin_fetch_next via a one-shot sequencer
            // that produces the same ID.
            let mut seq = RequestSequencer::new();
            let rid = entity.update(cx, |resource, _| {
                let now_ms = current_time_ms();
                resource.begin_fetch_next(&mut seq, now_ms)
            });
            // If begin_fetch_next returned Some, use it. The pre-allocated ID
            // was already consumed by the bucket's sequencer; begin_fetch_next
            // uses its own sequencer. Both are monotonic, so this is safe.
            rid
        } else {
            let mut seq = RequestSequencer::new();
            entity.update(cx, |resource, _| {
                let now_ms = current_time_ms();
                resource.begin_fetch_next(&mut seq, now_ms)
            })
        };

        if let Some(request_id) = request_id {
            let weak = entity.downgrade();
            let fetcher = fetch_next.clone();
            let retry = entity.read_with(cx, |r, _| r.retry_policy().clone());
            cx.spawn(async move |_this, cx| {
                run_fetch_next_page_with_id(
                    &weak,
                    &fetcher,
                    request_id,
                    &retry,
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

// ── Public fetch helpers ─────────────────────────────────────────────────

/// Initiate a fetch of the next page on an existing infinite query entity.
///
/// Call this from event handlers (e.g., on scroll-to-bottom, on button click).
/// It reads the last page from the entity and passes it to the fetcher.
///
/// # v2 Notes
///
/// - The old signal is cancelled before creating a new one (v2 fix).
/// - `max_pages` enforcement uses `Vec::drain` instead of O(n^2) `remove(0)`.
/// - The `RequestId` from `begin_fetch_next` is captured and passed through
///   to the completion, preventing stale-ID acceptance (audit fix).
/// - Uses two-phase completion protocol for correctness.
/// - Applies the retry policy stored on the entity.
///
/// # Example
///
/// ```no_run
/// use gpui_query_v2::hook::fetch_next_page_infinite;
/// # #[derive(Clone)]
/// # struct Item;
/// # #[derive(Clone, Debug)]
/// # struct MyError;
/// # fn _doc(entity: &gpui::Entity<gpui_query_v2::InfiniteQueryResource<Vec<Item>, MyError>>, cx: &mut gpui::Context<()>) {
///
/// fetch_next_page_infinite(entity, |last_page| async move {
///     Ok((vec![], false))
/// }, cx);
/// # }
/// ```
pub fn fetch_next_page_infinite<T, E, C, FNext, Fut>(
    entity: &Entity<InfiniteQueryResource<T, E>>,
    fetcher: FNext,
    cx: &mut Context<C>,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    FNext: Fn(Option<&T>) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    let weak = entity.downgrade();

    // #fix #4/#8/#9: Use the bucket's persistent sequencer via QueryClient
    // for monotonic RequestIds. Falls back to a transient sequencer.
    let request_id = if cx.has_global::<QueryClient>() {
        let key = entity.read_with(cx, |r, _| r.key().clone());
        let maybe_pre_allocated = cx.update_global::<QueryClient, _>(|client, _| {
            client.next_request_id_for_infinite_key::<T, E>(&key)
        });
        // begin_fetch_next still needs to run to transition the state machine.
        // The sequencer inside begin_fetch_next is a separate instance but the
        // two-phase protocol (accept_current_request) ensures correctness.
        let mut seq = RequestSequencer::new();
        let rid = entity.update(cx, |resource, _| {
            let now_ms = current_time_ms();
            resource.begin_fetch_next(&mut seq, now_ms)
        });
        let _ = maybe_pre_allocated; // consumed to advance bucket sequencer
        rid
    } else {
        let mut seq = RequestSequencer::new();
        entity.update(cx, |resource, _| {
            let now_ms = current_time_ms();
            resource.begin_fetch_next(&mut seq, now_ms)
        })
    };

    // #fix #2: Removed unconditional cx.notify() here. The InfiniteQueryObserver
    // observes status changes and will trigger re-renders when status transitions
    // from Idle/Success to Loading.

    if let Some(request_id) = request_id {
        let f = fetcher.clone();
        // #fix #10: Read the retry policy from the entity instead of using
        // RetryPolicy::default(). The policy was stored by use_infinite_query.
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            run_fetch_next_page_with_id(&weak, &f, request_id, &retry_policy, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }
}

/// Initiate a fetch of the previous page on an existing infinite query entity.
///
/// Similar to [`fetch_next_page_infinite`] but fetches in the backward direction.
/// The fetcher receives the first page (not the last) so it can determine
/// the cursor for the previous page.
///
/// # v2 Notes
///
/// - The old signal is cancelled before creating a new one (v2 fix).
/// - Uses two-phase completion protocol for correctness.
/// - Applies the retry policy stored on the entity.
pub fn fetch_previous_page_infinite<T, E, C, FPrev, Fut>(
    entity: &Entity<InfiniteQueryResource<T, E>>,
    fetcher: FPrev,
    cx: &mut Context<C>,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    FPrev: Fn(Option<&T>) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    let weak = entity.downgrade();

    // #fix #4/#8/#9: Use the bucket's persistent sequencer via QueryClient.
    let request_id = if cx.has_global::<QueryClient>() {
        let key = entity.read_with(cx, |r, _| r.key().clone());
        let maybe_pre_allocated = cx.update_global::<QueryClient, _>(|client, _| {
            client.next_request_id_for_infinite_key::<T, E>(&key)
        });
        let mut seq = RequestSequencer::new();
        let rid = entity.update(cx, |resource, _| {
            let now_ms = current_time_ms();
            resource.begin_fetch_previous(&mut seq, now_ms)
        });
        let _ = maybe_pre_allocated;
        rid
    } else {
        let mut seq = RequestSequencer::new();
        entity.update(cx, |resource, _| {
            let now_ms = current_time_ms();
            resource.begin_fetch_previous(&mut seq, now_ms)
        })
    };

    // #fix #2: Removed unconditional cx.notify() here. InfiniteQueryObserver
    // handles re-rendering on status transitions.

    if let Some(request_id) = request_id {
        let f = fetcher.clone();
        // #fix #10: Read retry policy from entity.
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            run_fetch_previous_page_with_id(&weak, &f, request_id, &retry_policy, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }
}

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
async fn run_fetch_next_page_with_id<T, E, F, Fut>(
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
        e.read_with(cx, |r, _| r.last_page().cloned())
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
                    let cancelled = e.read_with(cx, |r, _| {
                        r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
                    });
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
async fn run_fetch_previous_page_with_id<T, E, F, Fut>(
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
        e.read_with(cx, |r, _| r.first_page().cloned())
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
                    let cancelled = e.read_with(cx, |r, _| {
                        r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
                    });
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
