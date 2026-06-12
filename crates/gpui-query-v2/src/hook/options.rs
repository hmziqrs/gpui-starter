//! Query and mutation options with builder pattern and sensible defaults.
//!
//! **v2**: All options use `Default` and `From<&str>` so users can pass just
//! a string key for the simplest case.

use std::sync::Arc;

use crate::core::{CachePolicy, RefetchTrigger, RequestPolicy, RetryPolicy};

/// Options for `use_query` and `fetch_query`.
///
/// # Quick Start
///
/// ```no_run
/// use gpui_query_v2::QueryOptions;
/// use gpui_query_v2::core::{CachePolicy, RetryPolicy};
/// use gpui_query_v2::hook::use_query;
/// # #[derive(Clone)]
/// # struct User;
/// # #[derive(Clone, Debug)]
/// # struct MyError;
/// # fn _doc(cx: &mut gpui::Context<()>) {
///
/// // Simplest: just a string key
/// let result = use_query("users", |signal| async move {
///     Ok::<Vec<User>, MyError>(vec![])
/// }, cx);
///
/// // With options:
/// let result = use_query(
///     QueryOptions::new("users")
///         .cache_policy(CachePolicy::Ttl { ttl_ms: 300_000 })
///         .retry_policy(RetryPolicy::new(5)),
///     |signal| async move {
///         Ok::<Vec<User>, MyError>(vec![])
///     },
///     cx,
/// );
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct QueryOptions {
    /// The query key. Can be a string or multi-segment key.
    pub key: crate::core::QueryKey,
    /// Cache policy. Default: Ttl { ttl_ms: 60_000 }.
    pub cache_policy: CachePolicy,
    /// Request policy. Default: LatestWins.
    pub request_policy: RequestPolicy,
    /// Retry policy. Default: 3 retries with exponential backoff.
    pub retry_policy: RetryPolicy,
    /// GC time in milliseconds. Default: 300_000 (5 minutes).
    ///
    /// **Note**: Per-query GC time is not yet wired into the client/bucket layer.
    /// The global GC time set via [`QueryClient::with_gc_time`] is used instead.
    /// This field is stored for forward compatibility and will be implemented in
    /// a future release.
    pub gc_time_ms: u64,
    /// Whether to keep previous data when the key changes.
    ///
    /// **Note**: This field is not yet consumed by `use_query` or
    /// `use_query_manual`. It is stored for forward compatibility and will be
    /// implemented in a future release.
    pub keep_previous_data: bool,
    /// Whether to force a fetch (ignore cache).
    ///
    /// When `true`, `use_query` passes `QueryFetchMode::Force` to
    /// `begin_request`, bypassing cache freshness checks and always starting
    /// a new fetch.
    pub force_fetch: bool,
    /// Refetch on mount trigger.
    ///
    /// **Note**: This field is not yet consumed. The event system integration
    /// for automatic refetching on component mount is not yet implemented.
    /// It is stored for forward compatibility.
    pub refetch_on_mount: RefetchTrigger,
    /// Refetch on window focus trigger.
    ///
    /// **Note**: This field is not yet consumed. The event system integration
    /// for automatic refetching on window focus is not yet implemented.
    /// It is stored for forward compatibility.
    pub refetch_on_window_focus: RefetchTrigger,
    /// Refetch on reconnect trigger.
    ///
    /// **Note**: This field is not yet consumed. The event system integration
    /// for automatic refetching on reconnect is not yet implemented.
    /// It is stored for forward compatibility.
    pub refetch_on_reconnect: RefetchTrigger,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            key: crate::core::QueryKey::from("default"),
            cache_policy: CachePolicy::default(),
            request_policy: RequestPolicy::default(),
            retry_policy: RetryPolicy::default(),
            gc_time_ms: 300_000,
            keep_previous_data: false,
            force_fetch: false,
            refetch_on_mount: RefetchTrigger::default(),
            refetch_on_window_focus: RefetchTrigger::default(),
            refetch_on_reconnect: RefetchTrigger::default(),
        }
    }
}

impl QueryOptions {
    /// Create options with just a key.
    pub fn new(key: impl Into<crate::core::QueryKey>) -> Self {
        Self {
            key: key.into(),
            ..Default::default()
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

    /// Set the retry policy.
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Set the GC time in milliseconds.
    ///
    /// **Note**: Per-query GC time is not yet wired into the client/bucket layer.
    /// The global GC time set via [`QueryClient::with_gc_time`] is used instead.
    /// This value is stored for forward compatibility.
    pub fn gc_time(mut self, ms: u64) -> Self {
        self.gc_time_ms = ms;
        self
    }

    /// Force a fetch, ignoring cache.
    ///
    /// When set, `use_query` passes `QueryFetchMode::Force` to `begin_request`,
    /// which bypasses cache freshness checks and always starts a new fetch.
    pub fn force(mut self) -> Self {
        self.force_fetch = true;
        self
    }

    /// Keep previous data when the key changes.
    ///
    /// **Note**: This option is not yet consumed by `use_query` or
    /// `use_query_manual`. It is stored for forward compatibility and will be
    /// implemented in a future release.
    pub fn keep_previous(mut self) -> Self {
        self.keep_previous_data = true;
        self
    }
}

impl From<&str> for QueryOptions {
    fn from(key: &str) -> Self {
        Self::new(key)
    }
}

impl From<String> for QueryOptions {
    fn from(key: String) -> Self {
        Self::new(key)
    }
}

impl From<crate::core::QueryKey> for QueryOptions {
    fn from(key: crate::core::QueryKey) -> Self {
        Self::new(key)
    }
}

/// Options for `use_mutation`.
#[derive(Clone, Debug)]
pub struct MutationOptions {
    /// Retry policy. Default: no retries.
    pub retry_policy: RetryPolicy,
    /// GC time in milliseconds.
    pub gc_time_ms: u64,
}

impl Default for MutationOptions {
    fn default() -> Self {
        Self {
            retry_policy: RetryPolicy::no_retries(),
            gc_time_ms: 300_000,
        }
    }
}

/// Lifecycle callbacks for mutations.
///
/// Not `Clone` because trait-object callbacks cannot be cloned.
/// Construct with `MutationCallbacks::new()` and the builder methods.
///
/// Callbacks are wrapped in `Arc` so they can be shared across concurrent
/// mutation invocations. `E` should implement `std::fmt::Debug` so that
/// callbacks can log or display error details.
pub struct MutationCallbacks<T, E> {
    pub on_success: Option<Arc<dyn Fn(&T) + Send + Sync>>,
    pub on_error: Option<Arc<dyn Fn(&E) + Send + Sync>>,
    pub on_settled: Option<Arc<dyn Fn(Option<&T>, Option<&E>) + Send + Sync>>,
    _phantom: std::marker::PhantomData<(T, E)>,
}

impl<T, E> Default for MutationCallbacks<T, E> {
    fn default() -> Self {
        Self {
            on_success: None,
            on_error: None,
            on_settled: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, E> MutationCallbacks<T, E> {
    /// Create empty callbacks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the success callback.
    pub fn on_success(mut self, f: impl Fn(&T) + Send + Sync + 'static) -> Self {
        self.on_success = Some(Arc::new(f));
        self
    }

    /// Set the error callback.
    pub fn on_error(mut self, f: impl Fn(&E) + Send + Sync + 'static) -> Self {
        self.on_error = Some(Arc::new(f));
        self
    }

    /// Set the settled callback (fires on both success and failure).
    pub fn on_settled(
        mut self,
        f: impl Fn(Option<&T>, Option<&E>) + Send + Sync + 'static,
    ) -> Self {
        self.on_settled = Some(Arc::new(f));
        self
    }
}

/// Options for infinite queries.
#[derive(Clone, Debug)]
pub struct InfiniteQueryOptions {
    /// The query key.
    pub key: crate::core::QueryKey,
    /// Cache policy.
    pub cache_policy: CachePolicy,
    /// Request policy.
    pub request_policy: RequestPolicy,
    /// Maximum pages to retain. Default: 50.
    pub max_pages: Option<usize>,
    /// Retry policy.
    pub retry_policy: RetryPolicy,
    /// GC time in milliseconds. Default: 300_000 (5 minutes).
    pub gc_time_ms: u64,
}

impl Default for InfiniteQueryOptions {
    fn default() -> Self {
        Self {
            key: crate::core::QueryKey::from("default"),
            cache_policy: CachePolicy::default(),
            request_policy: RequestPolicy::default(),
            max_pages: Some(50),
            retry_policy: RetryPolicy::default(),
            gc_time_ms: 300_000,
        }
    }
}

impl InfiniteQueryOptions {
    /// Create with just a key.
    pub fn new(key: impl Into<crate::core::QueryKey>) -> Self {
        Self {
            key: key.into(),
            ..Default::default()
        }
    }

    /// Set max pages. Pass a concrete number to cap retained pages.
    ///
    /// To allow unbounded pages, use [`InfiniteQueryOptions::unbounded_pages`]
    /// instead.
    pub fn max_pages(mut self, max: usize) -> Self {
        self.max_pages = Some(max);
        self
    }

    /// Allow unbounded page accumulation (no limit).
    ///
    /// Sets `max_pages` to `None`, meaning the infinite query will never
    /// evict old pages. Use with caution — unbounded page storage can grow
    /// without limit if the user scrolls far enough.
    pub fn unbounded_pages(mut self) -> Self {
        self.max_pages = None;
        self
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

    /// Set the retry policy.
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Set the GC time in milliseconds.
    pub fn gc_time(mut self, ms: u64) -> Self {
        self.gc_time_ms = ms;
        self
    }
}

impl From<&str> for InfiniteQueryOptions {
    fn from(key: &str) -> Self {
        Self::new(key)
    }
}
