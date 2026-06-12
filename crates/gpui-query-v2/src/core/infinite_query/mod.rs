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

mod accessors;
mod lifecycle;
mod page_management;
mod resource;

pub use resource::{FetchDirection, InfiniteQueryResource};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CachePolicy, QueryKey, QueryStatus, RequestPolicy, RequestSequencer};

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
        let accepted = r.complete_page_success(id, vec!["a".to_string()], true, true, 2_000);
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
