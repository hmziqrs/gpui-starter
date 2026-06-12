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
use crate::core::{InfiniteQueryResource, QueryStatus};

use super::fetch_runners::run_fetch_next_page_with_id;
use crate::hook::current_time_ms;
use crate::hook::options::InfiniteQueryOptions;

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
) -> (Entity<InfiniteQueryResource<T, E>>, Subscription)
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
            client.infinite_resource_with_policies::<T, E>(key, cache_policy, request_policy, cx)
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
            let mut resource = InfiniteQueryResource::new(key, cache_policy, request_policy);
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
                return (entity, Subscription::new(|| {}));
            }
        }
    };

    // Start the initial fetch if idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        // #fix: Use the bucket's persistent sequencer via QueryClient so
        // RequestIds are monotonic across the resource lifetime. The
        // pre-allocated ID is passed through to begin_fetch_next_with_id so
        // the resource's active_request_id matches the bucket's counter.
        let maybe_request_id = if cx.has_global::<QueryClient>() {
            let key = entity.read_with(cx, |r, _| r.key().clone());
            cx.update_global::<QueryClient, _>(|client, _| {
                client.next_request_id_for_infinite_key::<T, E>(&key)
            })
        } else {
            None
        };

        // Pass the pre-allocated ID (or None) directly into
        // begin_fetch_next_with_id, which uses it instead of creating
        // a separate transient sequencer.
        let request_id = entity.update(cx, |resource, _| {
            let now_ms = current_time_ms();
            resource.begin_fetch_next_with_id(maybe_request_id, now_ms)
        });

        if let Some(request_id) = request_id {
            let weak = entity.downgrade();
            let fetcher = fetch_next;
            let retry = entity.read_with(cx, |r, _| r.retry_policy().clone());
            cx.spawn(async move |_this, cx| {
                run_fetch_next_page_with_id(&weak, &fetcher, request_id, &retry, cx).await;
                Ok::<_, ()>(())
            })
            .detach();
        }
    }

    (entity, subscription)
}
