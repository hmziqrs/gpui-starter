//! Comprehensive tests for `InfiniteQueryResource` in gpui-query-v2.
//!
//! Covers:
//! - Page append (fetch_next_page) and prepend (fetch_previous_page)
//! - `max_pages` enforcement: eviction on append and prepend
//! - Edge cases: `max_pages = 0` (unbounded), `max_pages = 1`, default (`Some(50)`)
//! - Stale rejection for page fetches
//! - Signal cancellation on page fetch replacement
//! - Page data access: `pages()`, `first_page()`, `last_page()`
//! - Empty pages state
//! - Reset clears all pages
//! - `FetchDirection` modes (ForwardOnly vs Bidirectional)
//! - `is_page_data_valid` across statuses
//! - Two-phase completion protocol
//! - `ignored_results` / `cancelled_count` diagnostics
//! - Evicted pages returned from `append_page` / `prepend_page` / `set_max_pages`

use crate::core::*;

// ── Helpers ─────────────────────────────────────────────────────────────

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

/// Convenience: load N pages via `begin_fetch_next` + `complete_page_success`.
/// Each page contains a single element `format!("page{i}")`.
/// Returns the resource with pages loaded.
fn load_n_pages(n: usize) -> InfiniteQueryResource<Vec<String>> {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();
    for i in 0..n {
        let has_more = i < n - 1;
        let id = r.begin_fetch_next(&mut seq, (i * 100) as u128).unwrap();
        r.complete_page_success(
            id,
            vec![format!("page{i}")],
            has_more,
            true,
            ((i + 1) * 100) as u128,
        );
    }
    r
}

// ── 1. Initial state ────────────────────────────────────────────────────

#[test]
fn new_resource_has_idle_state_with_empty_pages() {
    let r = make_resource();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.pages().is_empty());
    assert_eq!(r.page_count(), 0);
    assert!(!r.has_data());
    assert!(r.first_page().is_none());
    assert!(r.last_page().is_none());
    assert!(r.error().is_none());
    assert!(!r.is_loading());
    assert!(r.signal().is_none());
    assert!(r.active_request_id().is_none());
    assert!(r.started_at_ms().is_none());
    assert!(r.last_updated_at_ms().is_none());

    // v2: ForwardOnly defaults
    assert!(r.has_next_page());
    assert!(!r.has_previous_page());
    assert!(!r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());

    // v2: bounded default
    assert_eq!(r.max_pages(), Some(50));
    assert_eq!(r.direction(), FetchDirection::ForwardOnly);

    // Diagnostics
    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.retry_count(), 0);

    // Empty pages means data is not valid
    assert!(!r.is_page_data_valid());
}

// ── 2. Page append (fetch_next_page) ────────────────────────────────────

#[test]
fn fetch_next_page_appends_page_and_updates_status() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.is_fetching_next_page());
    assert!(r.signal().is_some());

    let accepted = r.complete_page_success(
        id,
        vec!["a".to_string(), "b".to_string()],
        true,
        true,
        2_000,
    );
    assert!(accepted);
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.last_page(), Some(&vec!["a".to_string(), "b".to_string()]));
    assert_eq!(r.last_updated_at_ms(), Some(2_000));
    assert!(!r.is_fetching_next_page());
    assert!(r.signal().is_none());
}

#[test]
fn fetch_next_page_accumulates_multiple_pages() {
    let r = load_n_pages(5);

    assert_eq!(r.page_count(), 5);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page4".to_string()]));
    // The last page was loaded with has_more = false
    assert!(!r.has_next_page());
}

#[test]
fn fetch_next_page_returns_none_when_no_next_page() {
    let mut r = make_resource();
    r.set_has_next_page(false);
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000);
    assert!(id.is_none());
}

// ── 3. Page prepend (fetch_previous_page) ───────────────────────────────

#[test]
fn fetch_previous_page_prepends_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    // Load initial page via next
    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    // Enable previous and prepend
    r.set_has_previous_page(true);
    assert!(!r.is_fetching_previous_page()); // not fetching yet
    let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
    assert!(r.is_fetching_previous_page());

    let accepted = r.complete_page_success(
        id2,
        vec!["page0".to_string()],
        false,
        false, // is_next = false => prepend
        4_000,
    );
    assert!(accepted);
    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page1".to_string()]));
    // has_more from completion updated has_previous_page
    assert!(!r.has_previous_page());
}

#[test]
fn fetch_previous_page_returns_none_when_no_previous_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();
    // ForwardOnly: has_previous_page defaults to false
    let id = r.begin_fetch_previous(&mut seq, 1_000);
    assert!(id.is_none());
}

// ── 4. max_pages enforcement ────────────────────────────────────────────

#[test]
fn max_pages_evicts_oldest_page_on_append() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

    // Third page exceeds max_pages=2, evicts oldest ("a")
    let id3 = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    r.complete_page_success(id3, vec!["c".to_string()], false, true, 6_000);

    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
}

#[test]
fn max_pages_evicts_newest_page_on_prepend() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

    // Prepend a page: ["c", "a", "b"] enforced to 2 removes from back => ["c", "a"]
    r.set_has_previous_page(true);
    let id3 = r.begin_fetch_previous(&mut seq, 5_000).unwrap();
    r.complete_page_success(id3, vec!["c".to_string()], false, false, 6_000);

    assert_eq!(r.page_count(), 2);
    assert_eq!(r.pages()[0], vec!["c".to_string()]);
    assert_eq!(r.pages()[1], vec!["a".to_string()]);
}

// ── 5. max_pages edge cases ─────────────────────────────────────────────

#[test]
fn max_pages_zero_treated_as_unbounded() {
    let mut r = load_n_pages(3);

    // v2 audit 2: Some(0) is treated as None (unbounded) — no eviction
    r.set_max_pages(Some(0));
    assert_eq!(r.max_pages(), None);
    assert_eq!(r.page_count(), 3);
}

#[test]
fn max_pages_one_retains_only_latest_page() {
    let mut r = make_resource();
    r.set_max_pages(Some(1));
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

    // Only the last page is retained
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["b".to_string()]));
}

#[test]
fn max_pages_default_is_50() {
    let r = make_resource();
    assert_eq!(r.max_pages(), Some(50));
}

#[test]
fn max_pages_50_allows_50_pages_and_evicts_on_51st() {
    let mut r = make_resource();
    assert_eq!(r.max_pages(), Some(50));
    let mut seq = RequestSequencer::new();

    // Load 50 pages — all with has_more=true so has_next_page stays true
    for i in 0..50 {
        let id = r.begin_fetch_next(&mut seq, (i * 100) as u128).unwrap();
        r.complete_page_success(
            id,
            vec![format!("p{i}")],
            true, // always report more pages available
            true,
            ((i + 1) * 100) as u128,
        );
    }
    assert_eq!(r.page_count(), 50);
    assert_eq!(r.first_page(), Some(&vec!["p0".to_string()]));

    // 51st page evicts p0
    let id51 = r.begin_fetch_next(&mut seq, 5_000_000).unwrap();
    r.complete_page_success(id51, vec!["p50".to_string()], false, true, 5_000_100);
    assert_eq!(r.page_count(), 50);
    assert_eq!(r.first_page(), Some(&vec!["p1".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["p50".to_string()]));
}

#[test]
fn set_max_pages_returns_evicted_pages() {
    let mut r = load_n_pages(3);

    let evicted = r.set_max_pages(Some(2));
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["page0".to_string()]);
    assert_eq!(r.page_count(), 2);
}

#[test]
fn append_page_returns_evicted_pages() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));

    assert!(r.append_page(vec!["a".to_string()]).is_empty());
    assert!(r.append_page(vec!["b".to_string()]).is_empty());

    let evicted = r.append_page(vec!["c".to_string()]);
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["a".to_string()]);
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
    assert_eq!(r.first_page(), Some(&vec!["c".to_string()]));
}

// ── 6. Stale rejection for page fetches ─────────────────────────────────

#[test]
fn stale_request_success_is_rejected() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    // Completing the first (stale) request should fail
    assert!(!r.complete_page_success(id1, vec!["stale".to_string()], true, true, 3_000));
    // Completing the second (current) request should succeed
    assert!(r.complete_page_success(id2, vec!["fresh".to_string()], false, true, 3_000));
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.last_page(), Some(&vec!["fresh".to_string()]));
}

#[test]
fn stale_request_failure_is_rejected() {
    let mut r: InfiniteQueryResource<Vec<String>, String> = InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert!(!r.complete_page_failure(id1, "stale error".to_string()));
    assert!(r.complete_page_failure(id2, "fresh error".to_string()));
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.error(), Some(&"fresh error".to_string()));
}

#[test]
fn stale_request_increments_ignored_results() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert!(!r.complete_page_success(id1, vec!["stale".to_string()], true, true, 3_000));
    assert_eq!(r.ignored_results(), 1);

    assert!(r.complete_page_success(id2, vec!["fresh".to_string()], false, true, 3_000));
    assert_eq!(r.ignored_results(), 1); // no increment for accepted result
}

#[test]
fn stale_failure_increments_ignored_results() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert!(!r.complete_page_failure(id1, "stale".into()));
    assert_eq!(r.ignored_results(), 1);

    assert!(r.complete_page_failure(id2, "fresh".into()));
    assert_eq!(r.ignored_results(), 1);
}

// ── 7. Signal cancellation on page fetch replacement ────────────────────

#[test]
fn begin_fetch_next_cancels_previous_signal() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let old_signal = r.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    // Starting a new fetch cancels the old signal
    let _id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();
    assert!(old_signal.is_cancelled());
    // The new signal is not cancelled
    assert!(!r.signal().unwrap().is_cancelled());
    assert_ne!(r.signal().unwrap(), &old_signal);
}

#[test]
fn begin_fetch_previous_cancels_previous_signal() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let old_signal = r.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    // Switching direction cancels the old signal
    r.set_has_previous_page(true);
    let _id2 = r.begin_fetch_previous(&mut seq, 2_000).unwrap();
    assert!(old_signal.is_cancelled());
    assert!(!r.is_fetching_next_page());
    assert!(r.is_fetching_previous_page());
}

#[test]
fn latest_wins_allows_replacement() {
    let mut r = make_resource(); // LatestWins
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert_ne!(id1, id2);
    assert_eq!(r.cancelled_count(), 1);
}

#[test]
fn ignore_while_loading_prevents_replacement() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000);
    assert!(id2.is_none());
    assert_eq!(r.cancelled_count(), 0);
}

// ── 8. Page data access ────────────────────────────────────────────────

#[test]
fn pages_returns_vecdeque_in_order() {
    let r = load_n_pages(3);
    let pages = r.pages();

    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0], vec!["page0".to_string()]);
    assert_eq!(pages[1], vec!["page1".to_string()]);
    assert_eq!(pages[2], vec!["page2".to_string()]);
}

#[test]
fn first_and_last_page_on_single_page() {
    let r = load_n_pages(1);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page0".to_string()]));
    assert!(r.has_data());
}

#[test]
fn first_and_last_page_none_when_empty() {
    let r = make_resource();
    assert!(r.first_page().is_none());
    assert!(r.last_page().is_none());
    assert!(!r.has_data());
}

// ── 9. Empty pages state ───────────────────────────────────────────────

#[test]
fn empty_pages_state_is_idle() {
    let r = make_resource();
    assert!(r.pages().is_empty());
    assert_eq!(r.page_count(), 0);
    assert!(!r.has_data());
    assert!(!r.is_page_data_valid());
    assert_eq!(r.status(), QueryStatus::Idle);
}

// ── 10. Reset clears all pages ─────────────────────────────────────────

#[test]
fn reset_clears_all_pages_and_state() {
    let mut r = load_n_pages(3);
    assert!(r.has_data());
    assert_eq!(r.page_count(), 3);

    r.reset();

    assert!(r.pages().is_empty());
    assert_eq!(r.page_count(), 0);
    assert!(!r.has_data());
    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.error().is_none());
    assert!(r.active_request_id().is_none());
    assert!(r.signal().is_none());
    assert!(r.started_at_ms().is_none());
    assert!(r.last_updated_at_ms().is_none());
    assert!(!r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());

    // ForwardOnly defaults restored
    assert!(r.has_next_page());
    assert!(!r.has_previous_page());
}

#[test]
fn reset_preserves_max_pages() {
    let mut r = make_resource();
    r.set_max_pages(Some(10));
    r.reset();
    assert_eq!(r.max_pages(), Some(10));
}

#[test]
fn reset_preserves_direction() {
    let mut r = make_bidirectional_resource();
    r.reset();
    assert_eq!(r.direction(), FetchDirection::Bidirectional);
    assert!(!r.has_next_page());
    assert!(!r.has_previous_page());
}

#[test]
fn reset_clears_diagnostics() {
    let mut r = load_n_pages(2);
    r.increment_retry_count();
    r.increment_retry_count();

    r.reset();

    assert_eq!(r.retry_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.cache_hits(), 0);
}

// ── 11. FetchDirection modes ────────────────────────────────────────────

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
fn bidirectional_rejects_fetch_next_without_opt_in() {
    let mut r = make_bidirectional_resource();
    let mut seq = RequestSequencer::new();
    assert!(r.begin_fetch_next(&mut seq, 1_000).is_none());
}

#[test]
fn bidirectional_allows_fetch_after_opt_in() {
    let mut r = make_bidirectional_resource();
    let mut seq = RequestSequencer::new();
    r.set_has_next_page(true);
    assert!(r.begin_fetch_next(&mut seq, 1_000).is_some());
}

#[test]
fn set_direction_changes_reset_behavior() {
    let mut r = make_resource();
    assert!(r.has_next_page()); // ForwardOnly default

    r.set_direction(FetchDirection::Bidirectional);
    r.reset();
    assert!(!r.has_next_page()); // Bidirectional reset default
    assert!(!r.has_previous_page());
}

// ── 12. is_page_data_valid across statuses ──────────────────────────────

#[test]
fn is_page_data_valid_false_when_idle() {
    let r = make_resource();
    assert!(!r.is_page_data_valid());
}

#[test]
fn is_page_data_valid_true_when_success_with_pages() {
    let r = load_n_pages(1);
    assert!(r.is_page_data_valid());
}

#[test]
fn is_page_data_valid_true_when_failure_with_existing_pages() {
    let mut r = load_n_pages(2);
    let mut seq = RequestSequencer::new();

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let id = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    r.complete_page_failure(id, "network error".into());

    // Failure does not clear existing pages
    assert_eq!(r.page_count(), 2);
    assert!(r.is_page_data_valid());
    assert_eq!(r.status(), QueryStatus::Failure);
}

#[test]
fn is_page_data_valid_false_when_failure_no_pages() {
    let mut r: InfiniteQueryResource<Vec<String>, String> = InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_failure(id, "network error".to_string());

    assert!(!r.is_page_data_valid());
}

// ── 13. Two-phase protocol ─────────────────────────────────────────────

#[test]
fn accept_current_request_returns_guard_for_active_request() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let guard = r.accept_current_request(id);
    assert!(guard.is_some());
    assert!(r.active_request_id().is_none()); // cleared on accept
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
    let mut r = load_n_pages(1);
    let mut seq = RequestSequencer::new();

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let id = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    let guard = r.accept_current_request(id).unwrap();
    r.complete_failure_with_guard(&guard, "network error".into());

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.page_count(), 1);
    assert!(r.is_page_data_valid());
}

// ── 14. Failure does not clear existing pages ───────────────────────────

#[test]
fn page_failure_preserves_existing_pages() {
    let mut r = load_n_pages(2);
    let mut seq = RequestSequencer::new();

    assert_eq!(r.page_count(), 2);

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let id = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    r.complete_page_failure(id, "timeout".into());

    // Pages remain intact
    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page1".to_string()]));
    assert_eq!(r.status(), QueryStatus::Failure);
}

// ── 15. Loading status transitions ──────────────────────────────────────

#[test]
fn loading_empty_when_no_pages_exist() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let _id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.is_loading());
}

#[test]
fn loading_with_data_when_pages_exist() {
    let mut r = load_n_pages(1);
    let mut seq = RequestSequencer::new();

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let _id = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert!(r.is_loading());
}

// ── 16. has_more propagation ────────────────────────────────────────────

#[test]
fn has_more_false_stops_further_fetches() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id, vec!["only".to_string()], false, true, 2_000);

    assert!(!r.has_next_page());
    // Attempting to fetch next should return None
    let id2 = r.begin_fetch_next(&mut seq, 3_000);
    assert!(id2.is_none());
}

#[test]
fn has_more_propagated_on_prepend() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    r.set_has_previous_page(true);
    let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
    // has_more = false => has_previous_page set to false
    r.complete_page_success(id2, vec!["page0".to_string()], false, false, 4_000);

    assert!(!r.has_previous_page());
}

// ── 17. Invalidate ─────────────────────────────────────────────────────

#[test]
fn invalidate_clears_last_updated_but_preserves_pages() {
    let mut r = load_n_pages(2);
    assert!(r.last_updated_at_ms().is_some());

    r.invalidate();

    assert!(r.last_updated_at_ms().is_none());
    assert_eq!(r.page_count(), 2);
}

// ── 18. Serde roundtrip ────────────────────────────────────────────────

#[test]
fn serde_roundtrip_preserves_state() {
    let r = load_n_pages(3);

    let json = serde_json::to_string(&r).unwrap();
    let back: InfiniteQueryResource<Vec<String>> = serde_json::from_str(&json).unwrap();

    assert_eq!(back.page_count(), 3);
    assert_eq!(back.status(), QueryStatus::Success);
    assert_eq!(back.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(back.last_page(), Some(&vec!["page2".to_string()]));
    // Signal is skipped by serde
    assert!(back.signal().is_none());
}

#[test]
fn serde_wire_format_uses_plain_array() {
    let r = load_n_pages(2);
    let json = serde_json::to_string(&r).unwrap();
    // VecDeque serializes as a plain array, not a VecDeque-specific format
    assert!(json.contains("\"pages\":["));
    assert!(!json.contains("VecDeque"));
}
