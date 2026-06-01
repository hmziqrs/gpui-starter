use crate::core::*;

fn make_resource() -> InfiniteQueryResource<Vec<String>> {
    InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    )
}

#[test]
fn test_infinite_query_new() {
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
    assert!(!r.has_data());
    assert!(r.signal().is_none());
}

#[test]
fn test_append_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string(), "b".to_string()], true, true, 2_000);

    assert_eq!(r.page_count(), 1);
    assert_eq!(r.last_page(), Some(&vec!["a".to_string(), "b".to_string()]));

    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["c".to_string()], true, true, 4_000);

    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["a".to_string(), "b".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
}

#[test]
fn test_prepend_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    // Load first page via next
    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    // Enable previous page and prepend
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
fn test_max_pages_eviction() {
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
    // Oldest page "a" was evicted
    assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
}

#[test]
fn test_max_pages_eviction_on_prepend() {
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
fn test_has_next_page_tracking() {
    let mut r = make_resource();
    assert!(r.has_next_page());

    r.set_has_next_page(false);
    assert!(!r.has_next_page());

    r.set_has_next_page(true);
    assert!(r.has_next_page());
}

#[test]
fn test_has_previous_page_tracking() {
    let mut r = make_resource();
    assert!(!r.has_previous_page());

    r.set_has_previous_page(true);
    assert!(r.has_previous_page());

    r.set_has_previous_page(false);
    assert!(!r.has_previous_page());
}

#[test]
fn test_begin_fetch_next() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000);
    assert!(id.is_some());
    assert!(r.is_fetching_next_page());
    assert!(r.is_loading());
    assert!(r.active_request_id().is_some());
    assert!(r.signal().is_some());
    assert!(r.started_at_ms().is_some());
}

#[test]
fn test_begin_fetch_next_returns_none_when_no_next_page() {
    let mut r = make_resource();
    r.set_has_next_page(false);
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000);
    assert!(id.is_none());
}

#[test]
fn test_begin_fetch_next_ignores_while_loading() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = RequestSequencer::new();
    let _ = r.begin_fetch_next(&mut seq, 1_000);

    let id = r.begin_fetch_next(&mut seq, 2_000);
    assert!(id.is_none());
}

#[test]
fn test_begin_fetch_next_replaces_with_latest_wins() {
    let mut r = make_resource(); // default is LatestWins
    let mut seq = RequestSequencer::new();
    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();

    let id2 = r.begin_fetch_next(&mut seq, 2_000);
    assert!(id2.is_some());
    assert_ne!(id1, id2.unwrap());
    assert_eq!(r.cancelled_count(), 1);
}

#[test]
fn test_complete_page_success() {
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
    assert!(r.signal().is_none());
}

#[test]
fn test_complete_page_failure() {
    let mut r: InfiniteQueryResource<Vec<String>, String> =
        InfiniteQueryResource::new("items", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let accepted = r.complete_page_failure(id, "network error".to_string());

    assert!(accepted);
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.error(), Some(&"network error".to_string()));
    assert!(!r.is_fetching_next_page());
    assert!(r.signal().is_none());
}

#[test]
fn test_complete_page_rejects_stale_request() {
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
fn test_reset_clears_pages() {
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
    assert!(!r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());
    assert!(r.signal().is_none());
    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
}

#[test]
fn test_invalidate_clears_last_updated() {
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
fn test_begin_fetch_previous_returns_none_when_no_previous_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();
    let id = r.begin_fetch_previous(&mut seq, 1_000);
    assert!(id.is_none());
}

#[test]
fn test_multiple_pages_accumulate() {
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
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page4".to_string()]));
}
