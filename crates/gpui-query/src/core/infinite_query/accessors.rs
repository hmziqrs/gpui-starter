use super::{
    CachePolicy, InfiniteQueryResource, QueryKey, QuerySignal, QueryStatus, QueryTimestamp,
    RequestId, RequestPolicy,
};

// ── Accessors ────────────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// All loaded pages, in order from first to last.
    pub fn pages(&self) -> &[T] {
        &self.pages
    }

    /// Number of loaded pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The first loaded page, if any.
    pub fn first_page(&self) -> Option<&T> {
        self.pages.first()
    }

    /// The last loaded page, if any.
    pub fn last_page(&self) -> Option<&T> {
        self.pages.last()
    }

    /// Whether there are more pages available after the last loaded page.
    pub fn has_next_page(&self) -> bool {
        self.has_next_page
    }

    /// Whether there are more pages available before the first loaded page.
    pub fn has_previous_page(&self) -> bool {
        self.has_previous_page
    }

    /// Whether a `fetch_next_page` request is currently in flight.
    pub fn is_fetching_next_page(&self) -> bool {
        self.is_fetching_next_page
    }

    /// Whether a `fetch_previous_page` request is currently in flight.
    pub fn is_fetching_previous_page(&self) -> bool {
        self.is_fetching_previous_page
    }

    /// Maximum number of pages to retain, if set.
    ///
    /// When pages exceed this limit, the oldest pages are dropped.
    pub fn max_pages(&self) -> Option<usize> {
        self.max_pages
    }

    /// Current status of the resource.
    pub fn status(&self) -> QueryStatus {
        self.status
    }

    /// The most recent error, if any.
    pub fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    /// Whether the resource is in any loading state.
    pub fn is_loading(&self) -> bool {
        self.status.is_loading()
    }

    /// The cache key for this resource.
    pub fn key(&self) -> &QueryKey {
        &self.key
    }

    /// The active request id, if a request is in flight.
    pub fn active_request_id(&self) -> Option<RequestId> {
        self.active_request_id
    }

    /// The cache policy for this resource.
    pub fn cache_policy(&self) -> CachePolicy {
        self.cache_policy
    }

    /// The request policy for this resource.
    pub fn request_policy(&self) -> RequestPolicy {
        self.request_policy
    }

    /// When the current request started, in milliseconds since UNIX epoch.
    pub fn started_at_ms(&self) -> Option<u128> {
        self.started_at.map(QueryTimestamp::as_millis)
    }

    /// When the data was last updated, in milliseconds since UNIX epoch.
    pub fn last_updated_at_ms(&self) -> Option<u128> {
        self.last_updated_at.map(QueryTimestamp::as_millis)
    }

    /// Total number of cache hits.
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    /// Total number of cancelled requests.
    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    /// Whether any pages have been loaded.
    pub fn has_data(&self) -> bool {
        !self.pages.is_empty()
    }

    /// Returns a reference to the cancellation signal, if one exists.
    pub fn signal(&self) -> Option<&QuerySignal> {
        self.signal.as_ref()
    }

    /// Returns a mutable reference to the cancellation signal, if one exists.
    pub fn signal_mut(&mut self) -> Option<&mut QuerySignal> {
        self.signal.as_mut()
    }
}
