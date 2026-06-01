//! Infinite query resource for managing paginated data.
//!
//! [`InfiniteQueryResource`] manages a list of pages, each of type `T`,
//! supporting forward pagination (`fetch_next_page`), backward pagination
//! (`fetch_previous_page`), and optional page count limits.
//!
//! This module depends only on `serde` — zero framework coupling.

use serde::{Deserialize, Serialize};

use super::{
    CachePolicy, QueryKey, QuerySignal, QueryStatus, QueryTimestamp, RequestId, RequestPolicy,
    RequestSequencer,
};

/// An infinite query resource that manages paginated data.
///
/// Inspired by TanStack Query's `useInfiniteQuery`. Each "page" is a `T` —
/// typically a batch of items fetched from an API. The resource tracks whether
/// more pages are available in either direction and enforces an optional
/// `max_pages` limit.
///
/// # Lifecycle
///
/// 1. **Idle** — initial state, no pages loaded.
/// 2. **Loading** — a page fetch is in progress.
/// 3. **Success** — at least one page has been loaded successfully.
/// 4. **Failure** — the most recent page fetch failed.
/// 5. **Cancelled** — the active request was cancelled.
///
/// # Serde
///
/// The `signal` field is skipped because [`QuerySignal`] wraps a shared atomic
/// flag with no meaningful persisted form.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InfiniteQueryResource<T, E = super::QueryError> {
    key: QueryKey,
    pages: Vec<T>,
    status: QueryStatus,
    error: Option<E>,
    active_request_id: Option<RequestId>,
    cache_policy: CachePolicy,
    request_policy: RequestPolicy,
    started_at: Option<QueryTimestamp>,
    last_updated_at: Option<QueryTimestamp>,
    cache_hits: u64,
    cancelled_count: u64,
    has_next_page: bool,
    has_previous_page: bool,
    is_fetching_next_page: bool,
    is_fetching_previous_page: bool,
    max_pages: Option<usize>,
    #[serde(skip)]
    signal: Option<QuerySignal>,
}

// ── Constructor ──────────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// Create a new infinite query resource with the given key and policies.
    ///
    /// Starts in `Idle` status with no pages. Set `has_next_page` to `true`
    /// before calling [`begin_fetch_next`](Self::begin_fetch_next) the first
    /// time, or let the hook layer set it based on the initial fetch result.
    pub fn new(
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
    ) -> Self {
        Self {
            key: key.into(),
            pages: Vec::new(),
            status: QueryStatus::Idle,
            error: None,
            active_request_id: None,
            cache_policy,
            request_policy,
            started_at: None,
            last_updated_at: None,
            cache_hits: 0,
            cancelled_count: 0,
            has_next_page: true,
            has_previous_page: false,
            is_fetching_next_page: false,
            is_fetching_previous_page: false,
            max_pages: None,
            signal: None,
        }
    }
}

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

    /// Set the maximum number of pages to retain.
    ///
    /// When set, appending or prepending pages beyond this limit causes the
    /// oldest pages to be dropped. Immediately trims if the current page
    /// count already exceeds the new limit (removes from the front).
    pub fn set_max_pages(&mut self, max: Option<usize>) {
        self.max_pages = max;
        self.enforce_max_pages_remove_front();
    }

    /// Append a new page of data to the end of the pages list.
    ///
    /// When `max_pages` is set and exceeded, removes pages from the front
    /// (oldest pages for forward pagination).
    pub fn append_page(&mut self, page: T) {
        self.pages.push(page);
        self.enforce_max_pages_remove_front();
    }

    /// Prepend a page of data to the beginning of the pages list.
    ///
    /// Useful for bidirectional pagination (loading older content above).
    /// When `max_pages` is set and exceeded, removes pages from the back
    /// (oldest pages for backward pagination).
    pub fn prepend_page(&mut self, page: T) {
        self.pages.insert(0, page);
        self.enforce_max_pages_remove_back();
    }

    /// Remove pages from the front (oldest in forward direction) beyond `max_pages`.
    fn enforce_max_pages_remove_front(&mut self) {
        if let Some(max) = self.max_pages {
            while self.pages.len() > max {
                self.pages.remove(0);
            }
        }
    }

    /// Remove pages from the back (oldest in backward direction) beyond `max_pages`.
    fn enforce_max_pages_remove_back(&mut self) {
        if let Some(max) = self.max_pages {
            while self.pages.len() > max {
                self.pages.pop();
            }
        }
    }
}

// ── Lifecycle ───────────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// Begin fetching the next page.
    ///
    /// Returns `None` (and does nothing) if `has_next_page` is `false`.
    /// If a previous-page fetch is in progress, it is cancelled and replaced.
    /// If a next-page fetch is already in progress with `LatestWins` policy,
    /// the old request is cancelled and a new one starts. With
    /// `IgnoreWhileLoading`, returns `None` instead.
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
                RequestPolicy::LatestWins => {
                    // Cancel the existing next-page fetch and start fresh
                }
            }
        }

        // If any request is active, count it as cancelled.
        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
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
    /// Returns `None` (and does nothing) if `has_previous_page` is `false`.
    /// Respects the same `RequestPolicy` as [`begin_fetch_next`](Self::begin_fetch_next)
    /// when a previous-page fetch is already in progress.
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
                RequestPolicy::LatestWins => {
                    // Cancel the existing previous-page fetch and start fresh
                }
            }
        }

        // Cancel any in-flight request.
        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
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

    /// Complete a page fetch with success.
    ///
    /// If `request_id` matches the active request:
    /// - Appends the page (if `is_next`) or prepends it (if previous).
    /// - Sets `has_next_page` / `has_previous_page` from the result.
    /// - Transitions to `Success` status.
    ///
    /// Returns `true` if the request was accepted, `false` if it was stale.
    pub fn complete_page_success(
        &mut self,
        request_id: RequestId,
        page: T,
        has_more: bool,
        is_next: bool,
        now_ms: u128,
    ) -> bool {
        if self.active_request_id != Some(request_id) {
            return false;
        }

        if is_next {
            self.pages.push(page);
            self.has_next_page = has_more;
            self.enforce_max_pages_remove_front();
        } else {
            self.pages.insert(0, page);
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
    /// If `request_id` matches the active request, stores the error and
    /// transitions to `Failure` status. Returns `true` if accepted.
    pub fn complete_page_failure(&mut self, request_id: RequestId, error: E) -> bool {
        if self.active_request_id != Some(request_id) {
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

    /// Check whether the given request id is the current active request.
    pub fn is_current_request(&self, request_id: RequestId) -> bool {
        self.active_request_id == Some(request_id)
    }

    /// Reset the resource back to idle, clearing all pages and state.
    pub fn reset(&mut self) {
        self.pages.clear();
        self.status = QueryStatus::Idle;
        self.error = None;
        self.active_request_id = None;
        self.started_at = None;
        self.last_updated_at = None;
        self.cache_hits = 0;
        self.cancelled_count = 0;
        self.has_next_page = true;
        self.has_previous_page = false;
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;
    }

    /// Invalidate the resource by clearing the last-updated timestamp.
    ///
    /// Pages are retained but the resource is considered stale, so the next
    /// access will trigger a refetch.
    pub fn invalidate(&mut self) {
        self.last_updated_at = None;
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

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

    #[test]
    fn new_resource_is_idle() {
        let r = make_resource();
        assert_eq!(r.status(), QueryStatus::Idle);
        assert!(r.pages().is_empty());
        assert_eq!(r.page_count(), 0);
        assert!(r.first_page().is_none());
        assert!(r.last_page().is_none());
        assert!(r.has_next_page());
        assert!(!r.has_previous_page());
        assert!(!r.is_fetching_next_page());
        assert!(!r.is_fetching_previous_page());
        assert!(r.max_pages().is_none());
        assert!(r.error().is_none());
        assert!(!r.is_loading());
    }

    #[test]
    fn begin_fetch_next_returns_request_id() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000);
        assert!(id.is_some());
        assert!(r.is_fetching_next_page());
        assert!(r.is_loading());
        assert!(r.active_request_id().is_some());
    }

    #[test]
    fn begin_fetch_next_returns_none_when_no_next_page() {
        let mut r = make_resource();
        r.set_has_next_page(false);
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000);
        assert!(id.is_none());
    }

    #[test]
    fn begin_fetch_next_returns_none_when_already_fetching() {
        let mut r = InfiniteQueryResource::<Vec<String>>::new(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::IgnoreWhileLoading,
        );
        let mut seq = RequestSequencer::new();
        let _ = r.begin_fetch_next(&mut seq, 1_000);
        let id = r.begin_fetch_next(&mut seq, 1_000);
        assert!(id.is_none());
    }

    #[test]
    fn begin_fetch_next_replaces_with_latest_wins() {
        let mut r = make_resource(); // default is LatestWins
        let mut seq = RequestSequencer::new();
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        let id2 = r.begin_fetch_next(&mut seq, 2_000);
        assert!(id2.is_some());
        assert_ne!(id1, id2.unwrap());
        assert_eq!(r.cancelled_count(), 1);
    }

    #[test]
    fn complete_page_success_appends_next_page() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();

        let accepted = r.complete_page_success(
            id,
            vec!["a".to_string(), "b".to_string()],
            true,
            true,
            2_000,
        );
        assert!(accepted);
        assert_eq!(r.page_count(), 1);
        assert_eq!(r.last_page(), Some(&vec!["a".to_string(), "b".to_string()]));
        assert!(r.has_next_page());
        assert_eq!(r.status(), QueryStatus::Success);
        assert!(!r.is_fetching_next_page());
        assert_eq!(r.last_updated_at_ms(), Some(2_000));
    }

    #[test]
    fn complete_page_success_prepends_previous_page() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        // Load first page
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

        // Enable previous page
        r.set_has_previous_page(true);
        let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
        let accepted = r.complete_page_success(
            id2,
            vec!["page0".to_string()],
            false,
            false,
            4_000,
        );
        assert!(accepted);
        assert_eq!(r.page_count(), 2);
        assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
        assert_eq!(r.last_page(), Some(&vec!["page1".to_string()]));
        assert!(!r.has_previous_page());
    }

    #[test]
    fn complete_page_failure_stores_error() {
        let mut r: InfiniteQueryResource<Vec<String>, String> =
            InfiniteQueryResource::new("items", CachePolicy::NoCache, RequestPolicy::LatestWins);
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();

        let accepted = r.complete_page_failure(id, "network error".to_string());
        assert!(accepted);
        assert_eq!(r.status(), QueryStatus::Failure);
        assert_eq!(r.error(), Some(&"network error".to_string()));
        assert!(!r.is_fetching_next_page());
    }

    #[test]
    fn stale_request_is_rejected() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();

        // Start a second fetch (cancels first)
        let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

        // Completing the first request should be rejected
        let accepted = r.complete_page_success(
            id1,
            vec!["stale".to_string()],
            true,
            true,
            3_000,
        );
        assert!(!accepted);

        // Completing the second request should succeed
        let accepted = r.complete_page_success(
            id2,
            vec!["fresh".to_string()],
            false,
            true,
            3_000,
        );
        assert!(accepted);
        assert_eq!(r.page_count(), 1);
        assert_eq!(r.last_page(), Some(&vec!["fresh".to_string()]));
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
        // Oldest page removed
        assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
        assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
    }

    #[test]
    fn max_pages_enforced_on_prepend() {
        let mut r = make_resource();
        r.set_max_pages(Some(2));
        let mut seq = RequestSequencer::new();

        let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
        let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
        r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

        // Prepend a page
        r.set_has_previous_page(true);
        let id3 = r.begin_fetch_previous(&mut seq, 5_000).unwrap();
        r.complete_page_success(id3, vec!["c".to_string()], false, false, 6_000);

        assert_eq!(r.page_count(), 2);
        // Prepend "c" -> ["c", "a", "b"], enforce max_pages=2 removes from back -> ["c", "a"]
        assert_eq!(r.pages()[0], vec!["c".to_string()]);
        assert_eq!(r.pages()[1], vec!["a".to_string()]);
    }

    #[test]
    fn reset_clears_everything() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
        r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);

        assert!(r.has_data());
        r.reset();
        assert!(r.pages().is_empty());
        assert_eq!(r.status(), QueryStatus::Idle);
        assert!(r.error().is_none());
        assert!(r.active_request_id().is_none());
        assert!(r.has_next_page());
        assert!(!r.has_previous_page());
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
        // Pages are still there
        assert_eq!(r.page_count(), 1);
    }

    #[test]
    fn begin_fetch_previous_returns_none_when_no_previous_page() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();
        let id = r.begin_fetch_previous(&mut seq, 1_000);
        assert!(id.is_none());
    }

    #[test]
    fn multiple_pages_accumulate() {
        let mut r = make_resource();
        let mut seq = RequestSequencer::new();

        for i in 0..5 {
            let id = r.begin_fetch_next(&mut seq, (i * 1_000) as u128).unwrap();
            let has_more = i < 4;
            r.complete_page_success(
                id,
                vec![format!("page{}", i)],
                has_more,
                true,
                ((i + 1) * 1_000) as u128,
            );
        }

        assert_eq!(r.page_count(), 5);
        assert!(!r.has_next_page());
        assert_eq!(
            r.first_page(),
            Some(&vec!["page0".to_string()])
        );
        assert_eq!(
            r.last_page(),
            Some(&vec!["page4".to_string()])
        );
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
        assert!(back.has_next_page());
        assert_eq!(
            back.last_page(),
            Some(&vec!["a".to_string()])
        );
        // Signal is skipped during serialization
        assert!(back.signal().is_none());
    }
}
