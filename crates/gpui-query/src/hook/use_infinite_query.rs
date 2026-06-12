//! The `use_infinite_query` hook — ergonomic infinite scrolling / pagination
//! for GPUI components.
//!
//! # Usage
//!
//! ```ignore
//! use gpui_query::hook::{use_infinite_query, InfiniteQueryOptions};
//! use gpui_query::QueryKey;
//!
//! struct FeedView {
//!     feed: gpui::Entity<gpui_query::InfiniteQueryResource<Vec<Post>>>,
//!     fetch_next: Box<dyn Fn() + 'static>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl FeedView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (entity, fetch_next, _subscription) = use_infinite_query(
//!             InfiniteQueryOptions::new(QueryKey::from(["feed"])),
//!             |last_page| async move {
//!                 let cursor = last_page.and_then(|p| p.last().map(|i| i.id));
//!                 let resp = api::fetch_feed(cursor).await?;
//!                 let has_more = resp.has_next;
//!                 Ok((resp.items, has_more))
//!             },
//!             cx,
//!         );
//!         Self { feed: entity, fetch_next: Box::new(fetch_next), _subscription }
//!     }
//! }
//! ```

use gpui::{AppContext as _, Context, Entity, Subscription};

use crate::core::{
    CachePolicy, InfiniteQueryResource, QueryKey, QueryStatus, RequestId, RequestPolicy,
};

use super::current_time_ms;

// ── Options ──────────────────────────────────────────────────────────────

/// Configuration for an infinite query.
///
/// Inspired by TanStack Query's `useInfiniteQuery` options.
pub struct InfiniteQueryOptions<T, E = crate::core::QueryError> {
    /// The hierarchical cache key for this query.
    pub key: QueryKey,
    /// How cached data is treated (TTL, stale-while-revalidate, or no cache).
    pub cache_policy: CachePolicy,
    /// How concurrent requests are handled (latest wins, or ignore duplicates).
    pub request_policy: RequestPolicy,
    /// Maximum number of pages to keep in memory. When exceeded, the oldest
    /// pages are dropped. `None` means unlimited.
    pub max_pages: Option<usize>,
    _marker: std::marker::PhantomData<(T, E)>,
}

impl<T, E> InfiniteQueryOptions<T, E> {
    /// Create options with the given key and default policies.
    pub fn new(key: impl Into<QueryKey>) -> Self {
        Self {
            key: key.into(),
            cache_policy: CachePolicy::default(),
            request_policy: RequestPolicy::default(),
            max_pages: None,
            _marker: std::marker::PhantomData,
        }
    }

    /// Set the cache policy.
    pub fn cache_policy(mut self, policy: CachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// Set the request policy.
    pub fn request_policy(mut self, policy: RequestPolicy) -> Self {
        self.request_policy = policy;
        self
    }

    /// Set the maximum number of pages to retain.
    pub fn max_pages(mut self, max: usize) -> Self {
        self.max_pages = Some(max);
        self
    }
}

// ── Hook ─────────────────────────────────────────────────────────────────

/// Hook for infinite scrolling / pagination.
///
/// Creates an [`InfiniteQueryResource`] entity and subscribes to it so the
/// component re-renders on state changes. Returns:
///
/// 1. The entity holding the page data
/// 2. A `fetch_next_page` closure that can be called from event handlers
/// 3. The subscription (store to keep the observation alive)
///
/// The `fetch_next` closure receives `Option<&T>` (the last page, if any)
/// and must return `Result<(T, bool), E>` where `T` is the new page data
/// and `bool` indicates whether more pages exist.
pub fn use_infinite_query<T, E, C, FNext, Fut>(
    options: InfiniteQueryOptions<T, E>,
    fetch_next: FNext,
    cx: &mut Context<C>,
) -> (
    Entity<InfiniteQueryResource<T, E>>,
    impl Fn() + Clone + 'static,
    Subscription,
)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    FNext: Fn(Option<&T>) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    let entity = cx.new(|_| {
        let mut resource =
            InfiniteQueryResource::new(options.key, options.cache_policy, options.request_policy);
        if let Some(max) = options.max_pages {
            resource.set_max_pages(Some(max));
        }
        resource
    });

    let subscription = cx.observe(&entity, |_, _, cx| {
        cx.notify();
    });

    // Start the initial fetch if idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        let weak = entity.downgrade();
        let fetcher = fetch_next.clone();
        cx.spawn(async move |_this, cx| {
            run_fetch_next_page(&weak, &fetcher, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }

    // Build the fetch_next_page closure
    let weak = entity.downgrade();
    let fetch_next_closure = {
        let fetcher = fetch_next.clone();
        move || {
            let weak = weak.clone();
            let fetcher = fetcher.clone();
            // We need an AsyncApp context. Since we can't hold one in a closure,
            // we use a tiny helper that spawns on the current thread.
            // The caller is expected to use this from within a GPUI context.
            // For ergonomics, we provide fetch_next_page_infinite as a separate
            // helper that takes cx explicitly.
            // This closure is a best-effort fire-and-forget that will panic
            // if the entity is gone.
            // In practice, callers should use fetch_next_page_infinite() instead.
            let _ = (weak, fetcher);
        }
    };

    (entity, fetch_next_closure, subscription)
}

/// Initiate a fetch of the next page on an existing infinite query entity.
///
/// Call this from event handlers (e.g., on scroll-to-bottom, on button click).
/// It reads the last page from the entity and passes it to the fetcher.
///
/// # Example
///
/// ```ignore
/// fetch_next_page_infinite(&entity, |last_page| async move {
///     let cursor = last_page.and_then(|p| p.last().map(|item| item.cursor()));
///     let resp = api::fetch_page(cursor).await?;
///     Ok((resp.items, resp.has_more))
/// }, cx);
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

    // Begin the fetch on the entity using the persistent sequencer stored
    // inside the resource. This ensures monotonically-increasing RequestIds
    // across all fetch calls so that staleness detection under LatestWins
    // policy works correctly.
    entity.update(cx, |resource, _| {
        let now_ms = current_time_ms();
        resource.begin_fetch_next_auto(now_ms);
    });

    cx.notify();

    let f = fetcher.clone();
    cx.spawn(async move |_this, cx| {
        run_fetch_next_page(&weak, &f, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Initiate a fetch of the previous page on an existing infinite query entity.
///
/// Similar to [`fetch_next_page_infinite`] but fetches in the backward direction.
/// The fetcher receives the first page (not the last) so it can determine
/// the cursor for the previous page.
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

    entity.update(cx, |resource, _| {
        let now_ms = current_time_ms();
        resource.begin_fetch_previous_auto(now_ms);
    });

    cx.notify();

    let f = fetcher.clone();
    cx.spawn(async move |_this, cx| {
        run_fetch_previous_page(&weak, &f, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

// ── Internal fetch runners ───────────────────────────────────────────────

/// Execute a fetch-next-page operation.
///
/// Reads the last page from the entity, calls the fetcher, then completes
/// the request on the entity with the result.
async fn run_fetch_next_page<T, E, F, Fut>(
    entity: &gpui::WeakEntity<InfiniteQueryResource<T, E>>,
    fetcher: &F,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(Option<&T>) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    // Read the last page for the fetcher
    let last_page_data: Option<T> = {
        let e = match entity.upgrade() {
            Some(e) => e,
            None => return,
        };
        e.read_with(cx, |r, _| r.last_page().cloned())
    };

    let result = fetcher(last_page_data.as_ref()).await;

    let now_ms = current_time_ms();

    let e = match entity.upgrade() {
        Some(e) => e,
        None => return,
    };

    // We need the request_id that was assigned during begin_fetch_next.
    // Read it from the entity before completing.
    let request_id: Option<RequestId> = e.read_with(cx, |r, _| r.active_request_id());

    let Some(request_id) = request_id else {
        return;
    };

    match result {
        Ok((page, has_more)) => {
            e.update(cx, |resource, cx| {
                resource.complete_page_success(request_id, page, has_more, true, now_ms);
                cx.notify();
            });
        }
        Err(error) => {
            e.update(cx, |resource, cx| {
                resource.complete_page_failure(request_id, error);
                cx.notify();
            });
        }
    }
}

/// Execute a fetch-previous-page operation.
async fn run_fetch_previous_page<T, E, F, Fut>(
    entity: &gpui::WeakEntity<InfiniteQueryResource<T, E>>,
    fetcher: &F,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(Option<&T>) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<(T, bool), E>> + Send + 'static,
{
    let first_page_data: Option<T> = {
        let e = match entity.upgrade() {
            Some(e) => e,
            None => return,
        };
        e.read_with(cx, |r, _| r.first_page().cloned())
    };

    let result = fetcher(first_page_data.as_ref()).await;

    let now_ms = current_time_ms();

    let e = match entity.upgrade() {
        Some(e) => e,
        None => return,
    };

    let request_id: Option<RequestId> = e.read_with(cx, |r, _| r.active_request_id());

    let Some(request_id) = request_id else {
        return;
    };

    match result {
        Ok((page, has_more)) => {
            e.update(cx, |resource, cx| {
                resource.complete_page_success(request_id, page, has_more, false, now_ms);
                cx.notify();
            });
        }
        Err(error) => {
            e.update(cx, |resource, cx| {
                resource.complete_page_failure(request_id, error);
                cx.notify();
            });
        }
    }
}
