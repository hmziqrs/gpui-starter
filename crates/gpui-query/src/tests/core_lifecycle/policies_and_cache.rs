//! Request policies, stale data, cache behavior, and force fetch tests (sections 14-22).
//!
//! Covers: LatestWins, IgnoreWhileLoading, is_data_stale, optional success,
//! CacheHit, StaleWhileRevalidate, begin_request_with_id, force fetch.

use crate::core::*;
use crate::tests::test_support::*;
use crate::tests::core_lifecycle::transitions::*;

// ═══════════════════════════════════════════════════════════════════════
// 14. Double begin_loading: LatestWins cancels old request
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn latest_wins_second_begin_replaces_active_request() {
    let mut r = resource();
    let mut s = seq();

    let (rid1, _) = begin(&mut r, &mut s, 100);
    let (rid2, _) = begin(&mut r, &mut s, 200);

    // rid1 is no longer active
    assert_ne!(r.active_request_id(), Some(rid1));
    assert_eq!(r.active_request_id(), Some(rid2));
    assert_eq!(
        r.cancelled_count(),
        1,
        "replaced request increments cancelled_count"
    );

    // Completing rid2 succeeds
    assert!(r.complete_current_success(rid2, "fresh", 300));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"fresh"));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Double begin_loading: IgnoreWhileLoading ignores second
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ignore_while_loading_rejects_second_request() {
    let mut r = test_resource_with_policies(
        "test",
        CachePolicy::Ttl { ttl_ms: 1_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut s = seq();

    let result1 = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let rid1 = match result1 {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };

    let result2 = r.begin_request(&mut s, 200, QueryFetchMode::Normal);

    match result2 {
        QueryBeginResult::IgnoredWhileLoading {
            active_request_id,
        } => {
            assert_eq!(active_request_id, rid1);
        }
        _ => panic!(
            "expected IgnoredWhileLoading, got {:?}",
            result2
        ),
    }

    assert_eq!(r.active_request_id(), Some(rid1));
    assert_eq!(
        r.cancelled_count(),
        0,
        "no cancellation because request was ignored"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Stale data check (is_data_stale)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn is_data_stale_returns_true_when_loading_with_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    let _ = begin(&mut r, &mut s, 1_500);

    assert!(r.is_data_stale());
}

#[test]
fn is_data_stale_returns_true_on_failure_with_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_failure_with_data(
        rid2,
        "fallback",
        QueryError::response("err"),
        1_600
    ));

    assert!(r.is_data_stale(), "failure with data should be stale");
}

#[test]
fn is_data_stale_returns_false_on_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    assert!(!r.is_data_stale());
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Optional success: None -> Idle (not Success)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn optional_success_none_sets_idle_not_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_optional_success(rid, None, 200));

    assert_eq!(
        r.status(),
        QueryStatus::Idle,
        "None data should produce Idle, not Success"
    );
    assert_eq!(r.data(), None);
}

#[test]
fn optional_success_some_sets_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_optional_success(rid, Some("data"), 200));

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
}

#[test]
fn optional_success_none_clears_previous_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "old", 200));

    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_optional_success(rid2, None, 1_600));

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"old"));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Cache short-circuit: CacheHit result
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cache_hit_returns_no_fetch_when_fresh() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    // Within TTL (1000ms): should be a cache hit
    let result = r.begin_request(&mut s, 500, QueryFetchMode::Normal);

    assert_eq!(result, QueryBeginResult::CacheHit);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
    assert_eq!(r.cache_hits(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Stale-while-revalidate: StaleCacheHit result
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stale_while_revalidate_serves_stale_and_starts_background() {
    let mut r = test_resource_with_policies(
        "swr",
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 500,
            stale_ms: 1_000,
        },
        RequestPolicy::LatestWins,
    );
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "stale-data", 200));

    // At t=800: past TTL (500) but within stale window (500+1000=1500)
    let result = r.begin_request(&mut s, 800, QueryFetchMode::Normal);

    match result {
        QueryBeginResult::StaleCacheHit {
            request_id,
            status,
            replaced_request_id,
        } => {
            assert_eq!(status, QueryStatus::LoadingWithData);
            assert!(replaced_request_id.is_none(), "no prior request to replace");
            assert_eq!(r.active_request_id(), Some(request_id));
        }
        _ => panic!("expected StaleCacheHit, got {:?}", result),
    }

    assert_eq!(
        r.data(),
        Some(&"stale-data"),
        "stale data is still available"
    );
    assert_eq!(r.cache_hits(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 20. begin_request_with_id variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_with_id_uses_provided_id() {
    let mut r = resource();
    let custom_id = RequestId::scoped(42, 7);

    let result = r.begin_request_with_id(Some(custom_id), 100, QueryFetchMode::Normal);

    match result {
        QueryBeginResult::Started { request_id, .. } => {
            assert_eq!(request_id, custom_id);
        }
        _ => panic!("expected Started"),
    }
    assert_eq!(r.active_request_id(), Some(custom_id));
}

#[test]
fn begin_request_with_id_none_uses_transient_sequencer() {
    let mut r = resource();

    let result = r.begin_request_with_id(None, 100, QueryFetchMode::Normal);

    match result {
        QueryBeginResult::Started { request_id, .. } => {
            // Transient sequencer starts at scope 1, sequence 1
            assert_eq!(request_id, RequestId::scoped(1, 1));
        }
        _ => panic!("expected Started"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Force fetch mode bypasses cache
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn force_fetch_mode_bypasses_fresh_cache() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    // Even though cache is fresh (t=300 < TTL 1000), force fetch should proceed
    let result = r.begin_request(&mut s, 300, QueryFetchMode::Force);

    match result {
        QueryBeginResult::Started { .. } => {}
        _ => panic!("expected Started with Force mode, got {:?}", result),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Optimistic update: set_data / clear_data / rollback
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn set_data_saves_previous_for_rollback() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "real", 200));

    r.set_data("optimistic");

    assert_eq!(r.data(), Some(&"optimistic"));
    assert_eq!(r.previous_data(), Some(&"real"));

    assert!(r.rollback_to_previous());
    assert_eq!(r.data(), Some(&"real"));
    assert_eq!(r.previous_data(), None);
}

#[test]
fn clear_data_saves_for_rollback() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "real", 200));

    r.clear_data();

    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"real"));

    assert!(r.rollback_to_previous());
    assert_eq!(r.data(), Some(&"real"));
}

#[test]
fn rollback_returns_false_when_no_previous_data() {
    let mut r = resource();
    assert!(!r.rollback_to_previous());
}
