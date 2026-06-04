//! Infinite query resource for managing paginated data.
//!
//! **v2 fixes**:
//! - Default `max_pages` is `Some(50)` instead of `None` (prevents unbounded growth)
//! - `enforce_max_pages_remove_front` uses `Vec::drain` instead of O(n²) `remove(0)`
//! - Old signal is cancelled before creating a new one on `begin_fetch_next`/`begin_fetch_previous`
//!
//! **Audit 2 fixes**:
//! - `max_pages` of 0 is treated as unbounded (no eviction) to prevent draining all pages
//! - Cross-direction request replacement (e.g. `begin_fetch_previous` while `begin_fetch_next` is
//!   active) is explicitly documented for `RequestPolicy::LatestWins`
//! - `retry_count` and `ignored_results` fields track diagnostics, matching `QueryResource`
//! - `complete_page_success`/`complete_page_failure` increment `ignored_results` when the request
//!   ID does not match
//! - `enforce_max_pages_remove_front`/`enforce_max_pages_remove_back` return evicted pages so
//!   callers can log or process them
//! - `reset()` preserves `max_pages` and resets `has_next_page` to its default (`true`);
//!   consumers should be aware that `has_next_page=true` after reset is an assumption
//!
//! **Audit 3 fixes**:
//! - Internal storage uses `VecDeque` instead of `Vec` so that `prepend_page` /
//!   `push_front` is O(1) amortized rather than O(n). `VecDeque::push_front`,
//!   `push_back`, `pop_back`, and `drain` are all O(1) amortized or better.
//! - `FetchDirection` controls default assumptions for `has_next_page` and
//!   `has_previous_page`. Forward-only queries (the common case) default
//!   `has_next_page = true`, while bidirectional queries default both to `false`
//!   and require explicit opt-in via the fetcher's `has_more` return value.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::{
    CachePolicy, QueryKey, QuerySignal, QueryStatus, QueryTimestamp, RequestGuard, RequestId,
    RequestPolicy, RequestSequencer, RetryPolicy,
};

/// Default maximum number of pages to retain.
const DEFAULT_MAX_PAGES: usize = 50;

/// Direction mode for an infinite query.
///
/// Controls the default assumptions for `has_next_page` and
/// `has_previous_page` on construction and after `reset()`.
///
/// - **ForwardOnly** (default): `has_next_page` starts `true`, `has_previous_page` starts `false`.
///   This is the common case for feed-style pagination where you only fetch next pages.
///   The `true` default for `has_next_page` assumes more pages exist until the fetcher says
///   otherwise.
///
/// - **Bidirectional**: Both `has_next_page` and `has_previous_page` start `false`.
///   The query will not attempt to fetch in either direction until the caller explicitly
///   sets `has_next_page(true)` or `has_previous_page(true)`, or the fetcher returns
///   `has_more = true` from a successful completion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FetchDirection {
    /// Fetch next pages only. `has_next_page` defaults to `true`.
    #[default]
    ForwardOnly,
    /// Fetch in both directions. Both flags default to `false`.
    Bidirectional,
}

/// An infinite query resource that manages paginated data.
///
/// Inspired by TanStack Query's `useInfiniteQuery`. Each "page" is a `T` —
/// typically a batch of items fetched from an API.
#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize, E: serde::Serialize"))]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned, E: serde::de::DeserializeOwned"))]
pub struct InfiniteQueryResource<T, E = super::QueryError> {
    key: QueryKey,
    #[serde(with = "vec_deque_serde")]
    pages: VecDeque<T>,
    status: QueryStatus,
    error: Option<E>,
    active_request_id: Option<RequestId>,
    cache_policy: CachePolicy,
    request_policy: RequestPolicy,
    started_at: Option<QueryTimestamp>,
    last_updated_at: Option<QueryTimestamp>,
    cache_hits: u64,
    cancelled_count: u64,
    ignored_results: u64,
    retry_count: u32,
    has_next_page: bool,
    has_previous_page: bool,
    is_fetching_next_page: bool,
    is_fetching_previous_page: bool,
    max_pages: Option<usize>,
    direction: FetchDirection,
    retry_policy: RetryPolicy,
    #[serde(skip)]
    signal: Option<QuerySignal>,
}

/// Serde helpers for `VecDeque` — serializes as a plain sequence and
/// deserializes into `VecDeque`. This keeps the wire format identical to the
/// old `Vec` representation so existing cached data remains compatible.
mod vec_deque_serde {
    use std::collections::VecDeque;

    use serde::de::{Deserialize, DeserializeOwned};
    use serde::ser::SerializeSeq;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S, T>(deque: &VecDeque<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: serde::Serialize,
    {
        let mut seq = serializer.serialize_seq(Some(deque.len()))?;
        for item in deque {
            seq.serialize_element(item)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<VecDeque<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: DeserializeOwned,
    {
        let vec: Vec<T> = Vec::<T>::deserialize(deserializer)?;
        Ok(vec.into())
    }
}

impl<T, E> InfiniteQueryResource<T, E> {
    /// Create a new infinite query resource.
    ///
    /// **v2**: `max_pages` defaults to `Some(50)` to prevent unbounded memory growth.
    ///
    /// **Audit 3**: Uses `FetchDirection::ForwardOnly` by default, meaning
    /// `has_next_page` starts `true`. Use [`new_bidirectional`](Self::new_bidirectional)
    /// for queries that paginate in both directions.
    pub fn new(
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
    ) -> Self {
        Self::with_direction(key, cache_policy, request_policy, FetchDirection::ForwardOnly)
    }

    /// Create a new infinite query resource configured for bidirectional paging.
    ///
    /// Both `has_next_page` and `has_previous_page` default to `false`. The
    /// query will not attempt to fetch in either direction until the caller
    /// explicitly enables it.
    pub fn new_bidirectional(
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
    ) -> Self {
        Self::with_direction(key, cache_policy, request_policy, FetchDirection::Bidirectional)
    }

    /// Create a new infinite query resource with an explicit [`FetchDirection`].
    fn with_direction(
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        direction: FetchDirection,
    ) -> Self {
        let (has_next, has_prev) = match direction {
            FetchDirection::ForwardOnly => (true, false),
            FetchDirection::Bidirectional => (false, false),
        };
        Self {
            key: key.into(),
            pages: VecDeque::new(),
            status: QueryStatus::Idle,
            error: None,
            active_request_id: None,
            cache_policy,
            request_policy,
            started_at: None,
            last_updated_at: None,
            cache_hits: 0,
            cancelled_count: 0,
            ignored_results: 0,
            retry_count: 0,
            has_next_page: has_next,
            has_previous_page: has_prev,
            is_fetching_next_page: false,
            is_fetching_previous_page: false,
            max_pages: Some(DEFAULT_MAX_PAGES),
            direction,
            retry_policy: RetryPolicy::default(),
            signal: None,
        }
    }
}

// ── Accessors ────────────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// All loaded pages, in order from first to last.
    ///
    /// **Note**: When `status()` is `Failure`, previously loaded pages are still
    /// present and valid — the failure applies only to the most recent page fetch.
    /// Use [`is_page_data_valid`](Self::is_page_data_valid) to check whether the
    /// current page data can be relied upon.
    ///
    /// **Audit 3**: Returns `&VecDeque<T>` instead of `&[T]`. `VecDeque` supports
    /// iteration and indexing, so most call sites are unaffected. Use `.as_slices()`
    /// if you need a split view, or `.iter()` for sequential access.
    pub fn pages(&self) -> &VecDeque<T> {
        &self.pages
    }

    /// Number of loaded pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The first loaded page, if any.
    pub fn first_page(&self) -> Option<&T> {
        self.pages.front()
    }

    /// The last loaded page, if any.
    pub fn last_page(&self) -> Option<&T> {
        self.pages.back()
    }

    /// Whether there are more pages after the last loaded page.
    pub fn has_next_page(&self) -> bool {
        self.has_next_page
    }

    /// Whether there are more pages before the first loaded page.
    pub fn has_previous_page(&self) -> bool {
        self.has_previous_page
    }

    /// Whether a `fetch_next_page` request is in flight.
    pub fn is_fetching_next_page(&self) -> bool {
        self.is_fetching_next_page
    }

    /// Whether a `fetch_previous_page` request is in flight.
    pub fn is_fetching_previous_page(&self) -> bool {
        self.is_fetching_previous_page
    }

    /// Maximum number of pages to retain.
    pub fn max_pages(&self) -> Option<usize> {
        self.max_pages
    }

    /// The fetch direction mode for this query.
    ///
    /// **Audit 3**: Controls the default assumptions for `has_next_page` and
    /// `has_previous_page` after construction and after `reset()`.
    pub fn direction(&self) -> FetchDirection {
        self.direction
    }

    /// Current status.
    pub fn status(&self) -> QueryStatus {
        self.status
    }

    /// Most recent error.
    pub fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    /// Whether loading.
    pub fn is_loading(&self) -> bool {
        self.status.is_loading()
    }

    /// Cache key.
    pub fn key(&self) -> &QueryKey {
        &self.key
    }

    /// Active request id.
    pub fn active_request_id(&self) -> Option<RequestId> {
        self.active_request_id
    }

    /// Cache policy.
    pub fn cache_policy(&self) -> CachePolicy {
        self.cache_policy
    }

    /// Request policy.
    pub fn request_policy(&self) -> RequestPolicy {
        self.request_policy
    }

    /// Set the cache policy.
    ///
    /// This allows policy updates on existing resources when the same key is
    /// reused with different policies (e.g., a different TTL).
    pub fn set_cache_policy(&mut self, policy: CachePolicy) {
        self.cache_policy = policy;
    }

    /// Set the request policy.
    ///
    /// This allows policy updates on existing resources when the same key is
    /// reused with different request behavior.
    pub fn set_request_policy(&mut self, policy: RequestPolicy) {
        self.request_policy = policy;
    }

    /// The retry policy for page fetches.
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    /// Set the retry policy.
    ///
    /// Stored by `use_infinite_query` from [`InfiniteQueryOptions::retry_policy`]
    /// so that fetch helpers can read it from the entity.
    pub fn set_retry_policy(&mut self, policy: RetryPolicy) {
        self.retry_policy = policy;
    }

    /// When the current request started (ms).
    pub fn started_at_ms(&self) -> Option<u128> {
        self.started_at.map(QueryTimestamp::as_millis)
    }

    /// When data was last updated (ms).
    pub fn last_updated_at_ms(&self) -> Option<u128> {
        self.last_updated_at.map(QueryTimestamp::as_millis)
    }

    /// Total cache hits.
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    /// Total cancelled requests.
    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    /// Total ignored results (completed requests whose ID no longer matched).
    ///
    /// Incremented when `complete_page_success` or `complete_page_failure`
    /// receives a stale request ID, i.e. the result was produced by a fetch
    /// that was subsequently replaced by a newer one.
    pub fn ignored_results(&self) -> u64 {
        self.ignored_results
    }

    /// Number of retry attempts for the current page fetch.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// Increment the retry counter.
    pub fn increment_retry_count(&mut self) {
        self.retry_count += 1;
    }

    /// Reset the retry counter to zero.
    pub fn reset_retry_count(&mut self) {
        self.retry_count = 0;
    }

    /// Whether any pages have been loaded.
    pub fn has_data(&self) -> bool {
        !self.pages.is_empty()
    }

    /// Whether the currently loaded page data is valid.
    ///
    /// Returns `true` when:
    /// - Status is `Success` (pages are up to date), or
    /// - Status is `LoadingWithData` or `LoadingEmpty` (pages from a previous
    ///   successful fetch are still valid while a new page is being fetched).
    ///
    /// Returns `false` when:
    /// - Status is `Idle` (no pages have been fetched yet), or
    /// - Status is `Cancelled` (data was explicitly cleared).
    ///
    /// **Important**: When status is `Failure`, this returns `true` if pages were
    /// previously loaded. A `Failure` status means the *last page fetch* failed,
    /// but all previously loaded pages remain valid. This is distinct from
    /// `QueryResource` where `Failure` invalidates the single data slot.
    pub fn is_page_data_valid(&self) -> bool {
        match self.status {
            QueryStatus::Success | QueryStatus::LoadingWithData => true,
            QueryStatus::Failure => !self.pages.is_empty(),
            QueryStatus::LoadingEmpty | QueryStatus::Idle | QueryStatus::Cancelled => false,
        }
    }

    /// Cancellation signal.
    pub fn signal(&self) -> Option<&QuerySignal> {
        self.signal.as_ref()
    }
}

// ── Page management ─────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// Set whether more pages are available after the last loaded page.
    pub fn set_has_next_page(&mut self, has_next: bool) {
        self.has_next_page = has_next;
    }

    /// Set whether more pages are available before the first loaded page.
    pub fn set_has_previous_page(&mut self, has_prev: bool) {
        self.has_previous_page = has_prev;
    }

    /// Set the fetch direction mode.
    ///
    /// This does not change the current `has_next_page` / `has_previous_page`
    /// flags — it only affects what `reset()` restores them to.
    pub fn set_direction(&mut self, direction: FetchDirection) {
        self.direction = direction;
    }

    /// Set the maximum number of pages to retain.
    ///
    /// A value of `Some(0)` is treated as unbounded (`None`) to prevent
    /// accidentally draining all pages. Callers that want no page retention
    /// should use `reset()` instead.
    ///
    /// Returns evicted pages (if any) so the caller can log or process them.
    pub fn set_max_pages(&mut self, max: Option<usize>) -> Vec<T> {
        // Treat 0 as unbounded to prevent draining all pages.
        self.max_pages = match max {
            Some(0) => None,
            other => other,
        };
        self.enforce_max_pages_remove_front()
    }

    /// Append a page to the end.
    ///
    /// **Audit 3**: Uses `VecDeque::push_back` — O(1) amortized.
    ///
    /// Returns evicted pages (if any) so the caller can log or process them.
    pub fn append_page(&mut self, page: T) -> Vec<T> {
        self.pages.push_back(page);
        self.enforce_max_pages_remove_front()
    }

    /// Prepend a page to the beginning.
    ///
    /// **Audit 3**: Uses `VecDeque::push_front` — O(1) amortized instead of
    /// the previous `Vec::insert(0, page)` which was O(n).
    ///
    /// Returns evicted pages (if any) so the caller can log or process them.
    pub fn prepend_page(&mut self, page: T) -> Vec<T> {
        self.pages.push_front(page);
        self.enforce_max_pages_remove_back()
    }

    /// **v2 fix**: Use `Vec::drain` instead of O(n²) `remove(0)`.
    ///
    /// **Audit 2 fix**: `max_pages` of 0 is treated as unbounded. At least 1
    /// page is always retained. Returns evicted pages for caller inspection.
    ///
    /// **Audit 3**: Uses `VecDeque::drain` — O(k) where k is the number of
    /// evicted pages.
    fn enforce_max_pages_remove_front(&mut self) -> Vec<T> {
        if let Some(max) = self.max_pages {
            if max > 0 && self.pages.len() > max {
                return self.pages.drain(..self.pages.len() - max).collect();
            }
        }
        Vec::new()
    }

    /// Evict pages from the back until within `max_pages`.
    ///
    /// **Audit 2 fix**: `max_pages` of 0 is treated as unbounded. At least 1
    /// page is always retained. Returns evicted pages for caller inspection.
    ///
    /// **Audit 3**: Uses `VecDeque::pop_back` — O(1) per eviction.
    fn enforce_max_pages_remove_back(&mut self) -> Vec<T> {
        let mut evicted = Vec::new();
        if let Some(max) = self.max_pages {
            if max > 0 {
                while self.pages.len() > max {
                    if let Some(page) = self.pages.pop_back() {
                        evicted.push(page);
                    }
                }
            }
        }
        evicted
    }
}

// ── Lifecycle ───────────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// Begin fetching the next page.
    ///
    /// **v2 fix**: Cancels the old signal before creating a new one.
    ///
    /// **Cross-direction replacement** (audit 2): When `RequestPolicy::LatestWins`
    /// is set and a `begin_fetch_previous` is currently active, calling this
    /// method will replace the previous-page request with this next-page request.
    /// The old signal is cancelled and the previous-page result will be silently
    /// discarded by `complete_page_success` (which returns `false` for stale IDs).
    /// This is intentional for `LatestWins` semantics — the most recent direction
    /// wins. Callers should check `is_fetching_next_page()` / `is_fetching_previous_page()`
    /// before completing if they need to detect direction changes.
    pub fn begin_fetch_next(
        &mut self,
        sequencer: &mut RequestSequencer,
        now_ms: u128,
    ) -> Option<RequestId> {
        if !self.has_next_page {
            return None;
        }

        if self.is_fetching_next_page {
            match self.request_policy {
                RequestPolicy::IgnoreWhileLoading => return None,
                RequestPolicy::LatestWins => {}
            }
        }

        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
        }

        // v2 fix: Cancel old signal before replacing
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }

        self.is_fetching_next_page = true;
        self.is_fetching_previous_page = false;

        let request_id = sequencer.next_request();
        self.active_request_id = Some(request_id);
        self.status = if self.pages.is_empty() {
            QueryStatus::LoadingEmpty
        } else {
            QueryStatus::LoadingWithData
        };
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(QuerySignal::new());

        Some(request_id)
    }

    /// Begin fetching the previous page.
    ///
    /// **v2 fix**: Cancels the old signal before creating a new one.
    ///
    /// **Cross-direction replacement** (audit 2): When `RequestPolicy::LatestWins`
    /// is set and a `begin_fetch_next` is currently active, calling this
    /// method will replace the next-page request with this previous-page request.
    /// The old signal is cancelled and the next-page result will be silently
    /// discarded by `complete_page_success` (which returns `false` for stale IDs).
    /// This is intentional for `LatestWins` semantics — the most recent direction
    /// wins. Callers should check `is_fetching_next_page()` / `is_fetching_previous_page()`
    /// before completing if they need to detect direction changes.
    pub fn begin_fetch_previous(
        &mut self,
        sequencer: &mut RequestSequencer,
        now_ms: u128,
    ) -> Option<RequestId> {
        if !self.has_previous_page {
            return None;
        }

        if self.is_fetching_previous_page {
            match self.request_policy {
                RequestPolicy::IgnoreWhileLoading => return None,
                RequestPolicy::LatestWins => {}
            }
        }

        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
        }

        // v2 fix: Cancel old signal before replacing
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }

        self.is_fetching_previous_page = true;
        self.is_fetching_next_page = false;

        let request_id = sequencer.next_request();
        self.active_request_id = Some(request_id);
        self.status = if self.pages.is_empty() {
            QueryStatus::LoadingEmpty
        } else {
            QueryStatus::LoadingWithData
        };
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(QuerySignal::new());

        Some(request_id)
    }

    /// Like [`begin_fetch_next`] but accepts an optional pre-generated `RequestId`
    /// instead of a `RequestSequencer`.
    ///
    /// When `maybe_request_id` is `Some`, uses that ID directly — this is the
    /// preferred call when the bucket's co-located sequencer has already
    /// pre-allocated an ID via `QueryClient::next_request_id_for_infinite_key`.
    /// When `None`, falls back to a transient `RequestSequencer::new()` for
    /// compatibility (e.g., when no `QueryClient` is available).
    ///
    /// Passing the pre-allocated ID through ensures the `RequestId` stored as
    /// the resource's `active_request_id` matches the one the bucket's sequencer
    /// already consumed, keeping the bucket's monotonic counter consistent with
    /// the resource's active request.
    pub fn begin_fetch_next_with_id(
        &mut self,
        maybe_request_id: Option<RequestId>,
        now_ms: u128,
    ) -> Option<RequestId> {
        if !self.has_next_page {
            return None;
        }

        if self.is_fetching_next_page {
            match self.request_policy {
                RequestPolicy::IgnoreWhileLoading => return None,
                RequestPolicy::LatestWins => {}
            }
        }

        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
        }

        // v2 fix: Cancel old signal before replacing
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }

        self.is_fetching_next_page = true;
        self.is_fetching_previous_page = false;

        let request_id = maybe_request_id
            .unwrap_or_else(|| RequestSequencer::new().next_request());
        self.active_request_id = Some(request_id);
        self.status = if self.pages.is_empty() {
            QueryStatus::LoadingEmpty
        } else {
            QueryStatus::LoadingWithData
        };
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(QuerySignal::new());

        Some(request_id)
    }

    /// Like [`begin_fetch_previous`] but accepts an optional pre-generated
    /// `RequestId` instead of a `RequestSequencer`.
    ///
    /// When `maybe_request_id` is `Some`, uses that ID directly — this is the
    /// preferred call when the bucket's co-located sequencer has already
    /// pre-allocated an ID via `QueryClient::next_request_id_for_infinite_key`.
    /// When `None`, falls back to a transient `RequestSequencer::new()` for
    /// compatibility (e.g., when no `QueryClient` is available).
    ///
    /// Passing the pre-allocated ID through ensures the `RequestId` stored as
    /// the resource's `active_request_id` matches the one the bucket's sequencer
    /// already consumed, keeping the bucket's monotonic counter consistent with
    /// the resource's active request.
    pub fn begin_fetch_previous_with_id(
        &mut self,
        maybe_request_id: Option<RequestId>,
        now_ms: u128,
    ) -> Option<RequestId> {
        if !self.has_previous_page {
            return None;
        }

        if self.is_fetching_previous_page {
            match self.request_policy {
                RequestPolicy::IgnoreWhileLoading => return None,
                RequestPolicy::LatestWins => {}
            }
        }

        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
        }

        // v2 fix: Cancel old signal before replacing
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }

        self.is_fetching_previous_page = true;
        self.is_fetching_next_page = false;

        let request_id = maybe_request_id
            .unwrap_or_else(|| RequestSequencer::new().next_request());
        self.active_request_id = Some(request_id);
        self.status = if self.pages.is_empty() {
            QueryStatus::LoadingEmpty
        } else {
            QueryStatus::LoadingWithData
        };
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(QuerySignal::new());

        Some(request_id)
    }

    /// Accept the current request for two-phase completion.
    ///
    /// Returns a [`RequestGuard`] if the request is still active, or `None`
    /// if it was replaced or cancelled. The guard is a capability token for
    /// the two-phase protocol (validate then complete).
    ///
    /// This mirrors [`QueryResource::accept_current_request`] for consistency.
    /// Use this when you need to inspect or transform data between validation
    /// and completion, or when integrating with frameworks that prefer explicit
    /// acceptance.
    pub fn accept_current_request(&mut self, request_id: RequestId) -> Option<RequestGuard> {
        if self.is_current_request(request_id) {
            self.active_request_id = None;
            Some(RequestGuard::new(request_id))
        } else {
            None
        }
    }

    /// Complete a page fetch with success using a guard (two-phase protocol).
    ///
    /// The guard proves that `accept_current_request` already validated the
    /// request is current.
    ///
    /// **Audit 3**: Uses `VecDeque::push_back` for append and `VecDeque::push_front`
    /// for prepend — both O(1) amortized.
    pub fn complete_success_with_guard(
        &mut self,
        _guard: &RequestGuard,
        page: T,
        has_more: bool,
        is_next: bool,
        now_ms: u128,
    ) {
        if is_next {
            self.pages.push_back(page);
            self.has_next_page = has_more;
            self.enforce_max_pages_remove_front();
        } else {
            self.pages.push_front(page);
            self.has_previous_page = has_more;
            self.enforce_max_pages_remove_back();
        }

        self.status = QueryStatus::Success;
        self.error = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;
    }

    /// Complete a page fetch with failure using a guard (two-phase protocol).
    ///
    /// The guard proves that `accept_current_request` already validated the
    /// request is current.
    ///
    /// **Note**: This does NOT clear previously loaded pages. A `Failure` status
    /// means the last page fetch failed, but previously loaded pages remain
    /// accessible via [`pages`](Self::pages). Use
    /// [`is_page_data_valid`](Self::is_page_data_valid) to check whether page
    /// data can be relied upon.
    pub fn complete_failure_with_guard(&mut self, _guard: &RequestGuard, error: E) {
        self.status = QueryStatus::Failure;
        self.error = Some(error);
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;
    }

    /// Complete a page fetch with success.
    ///
    /// Convenience method that accepts and completes in one call.
    ///
    /// **Audit 3**: Uses `VecDeque::push_back` for append and `VecDeque::push_front`
    /// for prepend — both O(1) amortized.
    pub fn complete_page_success(
        &mut self,
        request_id: RequestId,
        page: T,
        has_more: bool,
        is_next: bool,
        now_ms: u128,
    ) -> bool {
        if self.active_request_id != Some(request_id) {
            self.ignored_results += 1;
            return false;
        }

        if is_next {
            self.pages.push_back(page);
            self.has_next_page = has_more;
            self.enforce_max_pages_remove_front();
        } else {
            self.pages.push_front(page);
            self.has_previous_page = has_more;
            self.enforce_max_pages_remove_back();
        }

        self.status = QueryStatus::Success;
        self.error = None;
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;

        true
    }

    /// Complete a page fetch with failure.
    ///
    /// Convenience method that accepts and completes in one call.
    ///
    /// **Note**: This does NOT clear previously loaded pages. The `Failure` status
    /// applies to the most recent page fetch attempt only — previously loaded pages
    /// remain accessible via [`pages()`](Self::pages) and are still valid. Use
    /// [`is_page_data_valid()`](Self::is_page_data_valid) to check whether page
    /// data can be relied upon.
    pub fn complete_page_failure(&mut self, request_id: RequestId, error: E) -> bool {
        if self.active_request_id != Some(request_id) {
            self.ignored_results += 1;
            return false;
        }

        self.status = QueryStatus::Failure;
        self.error = Some(error);
        self.active_request_id = None;
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;

        true
    }

    /// Whether the given request id is the current active request.
    pub fn is_current_request(&self, request_id: RequestId) -> bool {
        self.active_request_id == Some(request_id)
    }

    /// Reset to idle, clearing everything.
    ///
    /// **Note** (audit 2): `max_pages` is preserved across resets — if it was
    /// changed via `set_max_pages()`, that value persists.
    ///
    /// **Audit 3**: `has_next_page` and `has_previous_page` are reset according
    /// to the current [`FetchDirection`](self.direction):
    /// - `ForwardOnly`: `has_next_page = true`, `has_previous_page = false`
    /// - `Bidirectional`: both reset to `false`
    ///
    /// If the resource was previously exhausted, the caller should set the
    /// flags again after reset if the direction-based defaults are incorrect.
    pub fn reset(&mut self) {
        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }
        self.pages.clear();
        self.status = QueryStatus::Idle;
        self.error = None;
        self.active_request_id = None;
        self.started_at = None;
        self.last_updated_at = None;
        self.cache_hits = 0;
        self.cancelled_count = 0;
        self.ignored_results = 0;
        self.retry_count = 0;
        // max_pages is intentionally preserved across resets.
        // direction is intentionally preserved across resets.
        let (has_next, has_prev) = match self.direction {
            FetchDirection::ForwardOnly => (true, false),
            FetchDirection::Bidirectional => (false, false),
        };
        self.has_next_page = has_next;
        self.has_previous_page = has_prev;
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;
    }

    /// Invalidate the cache (clear last-updated timestamp).
    pub fn invalidate(&mut self) {
        self.last_updated_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_resource() -> InfiniteQueryResource<Vec<String>> {
        InfiniteQueryResource::new(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        )
    }

    fn make_bidirectional_resource() -> InfiniteQueryResource<Vec<String>> {
        InfiniteQueryResource::new_bidirectional(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        )
    }

    #[test]
    fn new_resource_is_idle() {
        let r = make_resource();
        assert_eq!(r.status(), QueryStatus::Idle);
        assert!(r.pages().is_empty());
        assert_eq!(r.max_pages(), Some(50)); // v2: bounded default
    }

    #[test]
    fn begin_fetch_next_returns_request_id() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000);
        assert!(id.is_some());
        assert!(r.is_fetching_next_page());
    }

    #[test]
    fn complete_page_success_appends() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let accepted = r.complete_page_success(
            id,
            vec!["a".to_string()],
            true,
            true,
            2_000,
        );
        assert!(accepted);
        assert_eq!(r.page_count(), 1);
        assert_eq!(r.status(), QueryStatus::Success);
    }

    #[test]
    fn stale_request_is_rejected() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();
        assert!(!r.complete_page_success(id1, vec!["stale".to_string()], true, true, 3_000));
        assert!(r.complete_page_success(id2, vec!["fresh".to_string()], false, true, 3_000));
    }

    #[test]
    fn max_pages_enforced_on_append() {
        let mut r = make_resource();
        r.set_max_pages(Some(2));
        let mut seq = RequestSequencer::new();

        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);
        let id3 = r.begin_fetch_next(&mut seq, 5_000).unwrap();
        r.complete_page_success(id3, vec!["c".to_string()], false, true, 6_000);

        assert_eq!(r.page_count(), 2);
        assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
        assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
    }

    #[test]
    fn reset_clears_everything() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);
        r.reset();
        assert!(r.pages().is_empty());
        assert_eq!(r.status(), QueryStatus::Idle);
    }

    #[test]
    fn invalidate_clears_last_updated() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);
        assert!(r.last_updated_at_ms().is_some());
        r.invalidate();
        assert!(r.last_updated_at_ms().is_none());
        assert_eq!(r.page_count(), 1);
    }

    #[test]
    fn serde_roundtrip() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["a".to_string()], true, true, 2_000);
        let json = serde_json::to_string(&r).unwrap();
        let back: InfiniteQueryResource<Vec<String>> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.page_count(), 1);
        assert_eq!(back.status(), QueryStatus::Success);
        assert!(back.signal().is_none());
    }

    // ── Two-phase protocol tests ────────────────────────────────────────

    #[test]
    fn accept_current_request_returns_guard_for_active_request() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();

        let guard = r.accept_current_request(id);
        assert!(guard.is_some());
        assert_eq!(r.active_request_id(), None); // cleared on accept
    }

    #[test]
    fn accept_current_request_rejects_stale_request() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let _id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

        let guard = r.accept_current_request(id1);
        assert!(guard.is_none());
    }

    #[test]
    fn complete_success_with_guard_appends_page() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let guard = r.accept_current_request(id).unwrap();

        r.complete_success_with_guard(&guard, vec!["page1".to_string()], true, true, 2_000);
        assert_eq!(r.page_count(), 1);
        assert_eq!(r.status(), QueryStatus::Success);
        assert!(!r.is_fetching_next_page());
    }

    #[test]
    fn complete_failure_with_guard_preserves_pages() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        // Load one page successfully
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);
        assert_eq!(r.page_count(), 1);

        // Attempt next page but fail — using two-phase protocol
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        let guard = r.accept_current_request(id2).unwrap();
        r.complete_failure_with_guard(&guard, "network error".into());

        assert_eq!(r.status(), QueryStatus::Failure);
        assert_eq!(r.page_count(), 1); // pages preserved
        assert!(r.is_page_data_valid());
    }

    // ── is_page_data_valid tests ────────────────────────────────────────

    #[test]
    fn is_page_data_valid_idle_no_pages() {
        let r = make_resource();
        assert!(!r.is_page_data_valid());
    }

    #[test]
    fn is_page_data_valid_success_with_pages() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);
        assert!(r.is_page_data_valid());
    }

    #[test]
    fn is_page_data_valid_failure_preserves_pages() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        // Load a page
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

        // Fail next fetch
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        r.complete_page_failure(id2, "network error".into());

        // Pages are still valid despite failure
        assert!(r.is_page_data_valid());
        assert_eq!(r.pages().front(), Some(&vec!["page1".to_string()]));
    }

    #[test]
    fn is_page_data_valid_failure_no_pages() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        // Fail without ever loading a page
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_failure(id, "network error".into());

        assert!(!r.is_page_data_valid());
    }

    // ── Audit 2 tests ─────────────────────────────────────────────────

    #[test]
    fn max_pages_zero_treated_as_unbounded() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        // Load 3 pages
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);
        let id3 = r.begin_fetch_next(&mut seq, 5_000).unwrap();
        r.complete_page_success(id3, vec!["c".to_string()], true, true, 6_000);

        // Setting max_pages to 0 is treated as None (unbounded) — no pages evicted
        r.set_max_pages(Some(0));
        assert_eq!(r.max_pages(), None);
        assert_eq!(r.page_count(), 3);
    }

    #[test]
    fn set_max_pages_returns_evicted_pages() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);
        let id3 = r.begin_fetch_next(&mut seq, 5_000).unwrap();
        r.complete_page_success(id3, vec!["c".to_string()], true, true, 6_000);

        let evicted = r.set_max_pages(Some(2));
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0], vec!["a".to_string()]);
        assert_eq!(r.page_count(), 2);
    }

    #[test]
    fn append_page_returns_evicted_pages() {
        let mut r = make_resource();
        r.set_max_pages(Some(2));

        let evicted1 = r.append_page(vec!["a".to_string()]);
        assert!(evicted1.is_empty());

        let evicted2 = r.append_page(vec!["b".to_string()]);
        assert!(evicted2.is_empty());

        let evicted3 = r.append_page(vec!["c".to_string()]);
        assert_eq!(evicted3.len(), 1);
        assert_eq!(evicted3[0], vec!["a".to_string()]);
        assert_eq!(r.page_count(), 2);
    }

    #[test]
    fn prepend_page_returns_evicted_pages() {
        let mut r = make_resource();
        r.set_max_pages(Some(2));

        r.prepend_page(vec!["a".to_string()]);
        r.prepend_page(vec!["b".to_string()]);

        let evicted = r.prepend_page(vec!["c".to_string()]);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0], vec!["a".to_string()]);
        assert_eq!(r.page_count(), 2);
        // c, b are the remaining pages (c was prepended most recently)
        assert_eq!(r.first_page(), Some(&vec!["c".to_string()]));
    }

    #[test]
    fn stale_request_increments_ignored_results() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

        // id1 is stale — complete_page_success should increment ignored_results
        assert!(!r.complete_page_success(id1, vec!["stale".to_string()], true, true, 3_000));
        assert_eq!(r.ignored_results(), 1);

        // id2 succeeds
        assert!(r.complete_page_success(id2, vec!["fresh".to_string()], false, true, 3_000));
        assert_eq!(r.ignored_results(), 1); // no increment for successful completion
    }

    #[test]
    fn stale_failure_increments_ignored_results() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

        assert!(!r.complete_page_failure(id1, "stale error".into()));
        assert_eq!(r.ignored_results(), 1);

        assert!(r.complete_page_failure(id2, "fresh error".into()));
        assert_eq!(r.ignored_results(), 1);
    }

    #[test]
    fn retry_count_accessors() {
        let mut r = make_resource();
        assert_eq!(r.retry_count(), 0);
        r.increment_retry_count();
        r.increment_retry_count();
        assert_eq!(r.retry_count(), 2);
        r.reset_retry_count();
        assert_eq!(r.retry_count(), 0);
    }

    #[test]
    fn reset_clears_diagnostics_but_preserves_max_pages() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        // Load data and change max_pages
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);
        r.set_max_pages(Some(10));
        r.increment_retry_count();
        r.increment_retry_count();

        r.reset();

        // max_pages preserved
        assert_eq!(r.max_pages(), Some(10));
        // diagnostics cleared
        assert_eq!(r.retry_count(), 0);
        assert_eq!(r.ignored_results(), 0);
        assert_eq!(r.cancelled_count(), 0);
        // has_next_page reset to ForwardOnly default (true)
        assert!(r.has_next_page());
    }

    // ── Audit 3 tests ─────────────────────────────────────────────────

    #[test]
    fn forward_only_defaults_has_next_true() {
        let r = make_resource();
        assert_eq!(r.direction(), FetchDirection::ForwardOnly);
        assert!(r.has_next_page());
        assert!(!r.has_previous_page());
    }

    #[test]
    fn bidirectional_defaults_both_false() {
        let r = make_bidirectional_resource();
        assert_eq!(r.direction(), FetchDirection::Bidirectional);
        assert!(!r.has_next_page());
        assert!(!r.has_previous_page());
    }

    #[test]
    fn bidirectional_begin_fetch_next_returns_none_without_opt_in() {
        let mut r = make_bidirectional_resource();
        let mut seq = RequestSequencer::new();
        // has_next_page is false by default for bidirectional — fetch should be rejected
        let id = r.begin_fetch_next(&mut seq, 1_000);
        assert!(id.is_none());
    }

    #[test]
    fn bidirectional_fetch_works_after_opt_in() {
        let mut r = make_bidirectional_resource();
        let mut seq = RequestSequencer::new();
        r.set_has_next_page(true);
        let id = r.begin_fetch_next(&mut seq, 1_000);
        assert!(id.is_some());
    }

    #[test]
    fn reset_respects_direction() {
        let mut r = make_bidirectional_resource();
        let mut seq = RequestSequencer::new();
        r.set_has_next_page(true);
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);

        r.reset();
        // Bidirectional: both reset to false
        assert!(!r.has_next_page());
        assert!(!r.has_previous_page());
        assert_eq!(r.direction(), FetchDirection::Bidirectional);
    }

    #[test]
    fn set_direction_changes_reset_behavior() {
        let mut r = make_resource();
        // Start as ForwardOnly
        assert!(r.has_next_page());
        // Switch to Bidirectional
        r.set_direction(FetchDirection::Bidirectional);
        r.reset();
        // Now reset uses Bidirectional defaults
        assert!(!r.has_next_page());
        assert!(!r.has_previous_page());
    }

    #[test]
    fn vec_deque_serde_roundtrip() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

        let json = serde_json::to_string(&r).unwrap();

        // Verify the wire format is a plain array (backward compatible with old Vec format)
        assert!(json.contains("\"pages\":["));
        assert!(!json.contains("VecDeque"));

        let back: InfiniteQueryResource<Vec<String>> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.page_count(), 2);
        assert_eq!(back.first_page(), Some(&vec!["a".to_string()]));
        assert_eq!(back.last_page(), Some(&vec!["b".to_string()]));
    }
}
