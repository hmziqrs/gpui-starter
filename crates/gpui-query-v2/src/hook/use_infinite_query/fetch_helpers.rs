//! Public fetch helpers for infinite query entities.
//!
//! These functions are called from event handlers (e.g., on scroll-to-bottom,
//! on button click) to fetch the next or previous page.

use gpui::{BorrowAppContext as _, Context, Entity};

use crate::client::QueryClient;
use crate::core::InfiniteQueryResource;

use super::fetch_runners::{run_fetch_next_page_with_id, run_fetch_previous_page_with_id};
use crate::hook::current_time_ms;

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

    // #fix: Use the bucket's persistent sequencer via QueryClient for
    // monotonic RequestIds. The pre-allocated ID is passed through to
    // begin_fetch_next_with_id so the resource's active_request_id matches
    // the bucket's counter. Falls back to None (transient sequencer) when
    // no QueryClient is available.
    let maybe_request_id = if cx.has_global::<QueryClient>() {
        let key = entity.read_with(cx, |r, _| r.key().clone());
        cx.update_global::<QueryClient, _>(|client, _| {
            client.next_request_id_for_infinite_key::<T, E>(&key)
        })
    } else {
        None
    };

    let request_id = entity.update(cx, |resource, _| {
        let now_ms = current_time_ms();
        resource.begin_fetch_next_with_id(maybe_request_id, now_ms)
    });

    // #fix #2: Removed unconditional cx.notify() here. The InfiniteQueryObserver
    // observes status changes and will trigger re-renders when status transitions
    // from Idle/Success to Loading.

    if let Some(request_id) = request_id {
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            run_fetch_next_page_with_id(&weak, &fetcher, request_id, &retry_policy, cx).await;
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

    // #fix: Use the bucket's persistent sequencer via QueryClient for
    // monotonic RequestIds. The pre-allocated ID is passed through to
    // begin_fetch_previous_with_id so the resource's active_request_id matches
    // the bucket's counter. Falls back to None (transient sequencer) when
    // no QueryClient is available.
    let maybe_request_id = if cx.has_global::<QueryClient>() {
        let key = entity.read_with(cx, |r, _| r.key().clone());
        cx.update_global::<QueryClient, _>(|client, _| {
            client.next_request_id_for_infinite_key::<T, E>(&key)
        })
    } else {
        None
    };

    let request_id = entity.update(cx, |resource, _| {
        let now_ms = current_time_ms();
        resource.begin_fetch_previous_with_id(maybe_request_id, now_ms)
    });

    // #fix #2: Removed unconditional cx.notify() here. InfiniteQueryObserver
    // handles re-rendering on status transitions.

    if let Some(request_id) = request_id {
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            run_fetch_previous_page_with_id(&weak, &fetcher, request_id, &retry_policy, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }
}
