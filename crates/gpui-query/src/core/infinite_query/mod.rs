//! Infinite query resource for managing paginated data.
//!
//! [`InfiniteQueryResource`] manages a list of pages, each of type `T`,
//! supporting forward pagination (`fetch_next_page`), backward pagination
//! (`fetch_previous_page`), and optional page count limits.
//!
//! This module depends only on `serde` — zero framework coupling.

mod accessors;
mod lifecycle;
mod page_management;

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
    pub(crate) pages: Vec<T>,
    pub(crate) status: QueryStatus,
    pub(crate) error: Option<E>,
    pub(crate) active_request_id: Option<RequestId>,
    pub(crate) cache_policy: CachePolicy,
    pub(crate) request_policy: RequestPolicy,
    pub(crate) started_at: Option<QueryTimestamp>,
    pub(crate) last_updated_at: Option<QueryTimestamp>,
    pub(crate) cache_hits: u64,
    pub(crate) cancelled_count: u64,
    pub(crate) has_next_page: bool,
    pub(crate) has_previous_page: bool,
    pub(crate) is_fetching_next_page: bool,
    pub(crate) is_fetching_previous_page: bool,
    pub(crate) max_pages: Option<usize>,
    #[serde(skip)]
    pub(crate) signal: Option<QuerySignal>,
    /// Persistent sequencer that produces monotonically-increasing `RequestId`s
    /// across all fetch calls (initial, next-page, previous-page).
    ///
    /// Must live inside the entity so that every `begin_fetch_*` call advances
    /// the same counter. If a new `RequestSequencer` were created per call,
    /// every fetch would receive `scope_id=1, sequence=1`, making it impossible
    /// for `complete_page_success` / `complete_page_failure` to distinguish a
    /// stale in-flight response from the current one — effectively defeating
    /// the `LatestWins` cancellation policy.
    pub(crate) sequencer: RequestSequencer,
    key: QueryKey,
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
            sequencer: RequestSequencer::new(),
        }
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
