use crate::core::{CachePolicy, NetworkMode, QueryKey, RefetchTrigger, RequestPolicy, RetryPolicy};

/// Configuration for a single query, inspired by TanStack Query's `queryOptions()`.
///
/// Use this to define reusable query configurations that can be shared
/// across components.
pub struct QueryOptions<T, E = crate::core::QueryError> {
    /// The hierarchical cache key for this query.
    pub key: QueryKey,
    /// How cached data is treated (TTL, stale-while-revalidate, or no cache).
    pub cache_policy: CachePolicy,
    /// How concurrent requests are handled (latest wins, or ignore duplicates).
    pub request_policy: RequestPolicy,
    /// Garbage collection time in milliseconds. Resources idle longer than this
    /// may be collected by [`QueryClient::gc`](crate::client::QueryClient::gc).
    pub gc_time_ms: u64,
    /// Whether to bypass cache on the next fetch.
    pub force_fetch: bool,
    /// When `true`, the previous resource's data is used as placeholder data
    /// when the query key changes, similar to TanStack Query's `keepPreviousData`.
    pub keep_previous_data: bool,
    /// Initial data to seed the query with before any fetch completes.
    /// If set, the resource starts with this data instead of `None`.
    pub initial_data: Option<T>,
    /// Retry policy for failed fetches. Defaults to no retries.
    pub retry_policy: RetryPolicy,
    /// Network connectivity mode for this query. Defaults to `Online`.
    pub network_mode: NetworkMode,
    /// When to automatically refetch data. Defaults to `OnMount`.
    pub refetch_on_mount: RefetchTrigger,
    _marker: std::marker::PhantomData<E>,
}

impl<T, E> QueryOptions<T, E> {
    /// Create a new query options with the given key and default policies.
    pub fn new(key: impl Into<QueryKey>) -> Self {
        Self {
            key: key.into(),
            cache_policy: CachePolicy::default(),
            request_policy: RequestPolicy::default(),
            gc_time_ms: 5 * 60 * 1_000,
            force_fetch: false,
            keep_previous_data: false,
            initial_data: None,
            retry_policy: RetryPolicy::no_retries(),
            network_mode: NetworkMode::default(),
            refetch_on_mount: RefetchTrigger::default(),
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

    /// Set the garbage collection time in milliseconds.
    pub fn gc_time_ms(mut self, ms: u64) -> Self {
        self.gc_time_ms = ms;
        self
    }

    /// Force a fresh fetch, bypassing any cache.
    pub fn force(mut self) -> Self {
        self.force_fetch = true;
        self
    }

    /// Enable keep-previous-data behavior.
    ///
    /// When the query key changes, the previous resource's data is used
    /// as placeholder data for the new resource, so the UI never shows
    /// an empty state during the transition.
    pub fn keep_previous_data(mut self, value: bool) -> Self {
        self.keep_previous_data = value;
        self
    }

    /// Set initial data for the query.
    ///
    /// When provided, the query resource starts with this data immediately
    /// instead of showing an empty/loading state. The initial data is used
    /// until the first fetch completes.
    pub fn initial_data(mut self, data: Option<T>) -> Self {
        self.initial_data = data;
        self
    }

    /// Set the retry policy for failed fetches.
    ///
    /// By default queries do not retry. Use this to enable automatic retries
    /// with optional exponential backoff on failure.
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Set the network mode for this query.
    ///
    /// Controls whether queries should fetch based on network connectivity.
    /// Defaults to `NetworkMode::Online`.
    pub fn network_mode(mut self, mode: NetworkMode) -> Self {
        self.network_mode = mode;
        self
    }

    /// Set the refetch trigger configuration.
    ///
    /// Controls when the query should automatically refetch data.
    /// Defaults to `RefetchTrigger::OnMount`.
    pub fn refetch_on_mount(mut self, trigger: RefetchTrigger) -> Self {
        self.refetch_on_mount = trigger;
        self
    }
}

// ── Mutation options ────────────────────────────────────────────────────

/// Configuration for a mutation, inspired by TanStack Query's mutation options.
///
/// Use this to define reusable mutation configurations including retry behavior
/// and garbage collection time.
///
/// # Builder pattern
///
/// ```
/// use gpui_query::hook::MutationOptions;
/// use gpui_query::core::RetryPolicy;
///
/// let opts: MutationOptions<String, i32> = MutationOptions::new()
///     .retry_policy(RetryPolicy::new(3).with_exponential_backoff())
///     .gc_time_ms(10 * 60 * 1_000);
/// ```
pub struct MutationOptions<V, T, E = crate::core::QueryError> {
    /// Retry policy for failed mutations.
    pub retry_policy: RetryPolicy,
    /// Garbage collection time in milliseconds. Mutation resources idle longer
    /// than this may be collected.
    pub gc_time_ms: u64,
    _marker: std::marker::PhantomData<(V, T, E)>,
}

impl<V, T, E> MutationOptions<V, T, E> {
    /// Create mutation options with sensible defaults: no retries, 5-minute GC.
    pub fn new() -> Self {
        Self {
            retry_policy: RetryPolicy::no_retries(),
            gc_time_ms: 5 * 60 * 1_000,
            _marker: std::marker::PhantomData,
        }
    }

    /// Set the retry policy for failed mutations.
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Set the garbage collection time in milliseconds.
    pub fn gc_time_ms(mut self, ms: u64) -> Self {
        self.gc_time_ms = ms;
        self
    }
}

impl<V, T, E> Default for MutationOptions<V, T, E> {
    fn default() -> Self {
        Self::new()
    }
}

// ── Mutation callbacks ──────────────────────────────────────────────────

/// Optional callbacks for mutation lifecycle events.
///
/// Use with [`use_mutation_with_callbacks`](crate::hook::use_mutation_with_callbacks)
/// to react to success, failure, or settlement of a mutation.
pub struct MutationCallbacks<T, E> {
    /// Called when the mutation completes successfully.
    pub on_success: Option<Box<dyn Fn(&T) + 'static>>,
    /// Called when the mutation fails.
    pub on_error: Option<Box<dyn Fn(&E) + 'static>>,
    /// Called when the mutation settles (either success or failure).
    pub on_settled: Option<Box<dyn Fn(Option<&T>, Option<&E>) + 'static>>,
}

impl<T, E> MutationCallbacks<T, E> {
    /// Create empty callbacks (no listeners).
    pub fn new() -> Self {
        Self {
            on_success: None,
            on_error: None,
            on_settled: None,
        }
    }

    /// Set the success callback.
    pub fn on_success(mut self, f: impl Fn(&T) + 'static) -> Self {
        self.on_success = Some(Box::new(f));
        self
    }

    /// Set the error callback.
    pub fn on_error(mut self, f: impl Fn(&E) + 'static) -> Self {
        self.on_error = Some(Box::new(f));
        self
    }

    /// Set the settled callback.
    pub fn on_settled(mut self, f: impl Fn(Option<&T>, Option<&E>) + 'static) -> Self {
        self.on_settled = Some(Box::new(f));
        self
    }
}

impl<T, E> Default for MutationCallbacks<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for CachePolicy {
    fn default() -> Self {
        CachePolicy::Ttl { ttl_ms: 60_000 }
    }
}

impl Default for RequestPolicy {
    fn default() -> Self {
        RequestPolicy::LatestWins
    }
}
