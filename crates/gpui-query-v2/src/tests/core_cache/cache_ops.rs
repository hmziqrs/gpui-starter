//! Cache invalidation and reset tests.

use crate::core::*;
use crate::tests::core_cache::*;
use crate::tests::test_support::*;

// ══════════════════════════════════════════════════════════════════════════
// CACHE INVALIDATION
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn invalidate_clears_last_updated_but_retains_data() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    r.invalidate();
    assert_eq!(
        r.data(),
        Some(&"cached"),
        "data is retained after invalidate"
    );
    assert_eq!(
        r.last_updated_at_ms(),
        None,
        "last_updated_at cleared by invalidate"
    );
    assert!(
        !r.is_cache_fresh(STORED_AT_MS + 1),
        "after invalidate, data is not fresh even within TTL"
    );
}

#[test]
fn invalidate_then_begin_request_starts_fetch() {
    let mut r = ttl_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(r.should_short_circuit_cache(STORED_AT_MS + 500));
    r.invalidate();
    let result = r.begin_request(&mut seq, STORED_AT_MS + 500, QueryFetchMode::Normal);
    assert!(matches!(result, QueryBeginResult::Started { .. }));
}

#[test]
fn invalidate_on_no_data_is_noop() {
    let mut r = ttl_resource();
    r.invalidate();
    assert_eq!(r.data(), None);
    assert_eq!(r.last_updated_at_ms(), None);
}

#[test]
fn invalidate_then_refetch_refreshes_cache() {
    let mut r = ttl_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "v1", STORED_AT_MS);
    r.invalidate();
    let result = r.begin_request(&mut seq, STORED_AT_MS + 500, QueryFetchMode::Normal);
    let request_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started after invalidate"),
    };
    let completed_at = STORED_AT_MS + 600;
    r.complete_current_success(request_id, "v2", completed_at);
    assert_eq!(r.data(), Some(&"v2"));
    assert_eq!(r.last_updated_at_ms(), Some(completed_at));
    assert!(r.is_cache_fresh(completed_at + 200));
}

// ══════════════════════════════════════════════════════════════════════════
// CACHE RESET
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn reset_clears_data_and_error() {
    let mut r = ttl_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    r.apply_failure("something broke", STORED_AT_MS + 100);
    assert!(r.data().is_some());
    assert!(r.error().is_some());
    r.reset();
    assert_eq!(r.data(), None);
    assert_eq!(r.error(), None);
    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.last_updated_at_ms(), None);
}

#[test]
fn reset_clears_cache_hits_counter() {
    let mut r = ttl_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    let mut seq = test_sequencer();
    let result = r.begin_request(&mut seq, STORED_AT_MS + 500, QueryFetchMode::Normal);
    assert_eq!(result, QueryBeginResult::CacheHit);
    assert_eq!(r.cache_hits(), 1);
    r.reset();
    assert_eq!(r.cache_hits(), 0);
}

#[test]
fn reset_clears_all_counters() {
    let mut r = ttl_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    let mut seq = test_sequencer();
    r.begin_request(&mut seq, STORED_AT_MS + 500, QueryFetchMode::Force);
    r.begin_request(&mut seq, STORED_AT_MS + 600, QueryFetchMode::Force);
    assert_eq!(r.cancelled_count(), 1);
    r.reset();
    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.retry_count(), 0);
}

#[test]
fn reset_preserves_policies_and_key() {
    let mut r = QueryResource::new(
        "my-key",
        CachePolicy::Ttl { ttl_ms: 5_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    seed_data(&mut r, "data", STORED_AT_MS);
    r.reset();
    assert_eq!(r.key().as_str(), "my-key");
    assert_eq!(r.cache_policy(), CachePolicy::Ttl { ttl_ms: 5_000 });
    assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
}
