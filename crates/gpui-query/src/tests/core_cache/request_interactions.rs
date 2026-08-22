//! Cache interactions with request policies and policy accessor tests.

use crate::core::*;
use crate::tests::core_cache::*;
use crate::tests::test_support::*;

// ══════════════════════════════════════════════════════════════════════════
// CACHE INTERACTIONS WITH REQUEST POLICIES
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_short_circuits_fresh_ttl_cache() {
    let mut r = ttl_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "cached", STORED_AT_MS);
    let result = r.begin_request(&mut seq, STORED_AT_MS + 500, QueryFetchMode::Normal);
    assert_eq!(result, QueryBeginResult::CacheHit);
    assert_eq!(r.cache_hits(), 1);
    assert_eq!(r.active_request_id(), None);
}

#[test]
fn forced_begin_request_bypasses_fresh_ttl_cache() {
    let mut r = ttl_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "cached", STORED_AT_MS);
    let result = r.begin_request(&mut seq, STORED_AT_MS + 500, QueryFetchMode::Force);
    assert!(matches!(
        result,
        QueryBeginResult::Started {
            status: QueryStatus::LoadingWithData,
            replaced_request_id: None,
            ..
        }
    ));
}

#[test]
fn ignore_while_loading_rejects_duplicate_request() {
    let mut r: QueryResource<&'static str> = QueryResource::new(
        "demo",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = test_sequencer();
    let first = r.begin_request(&mut seq, 100, QueryFetchMode::Normal);
    assert!(matches!(first, QueryBeginResult::Started { .. }));
    let duplicate = r.begin_request(&mut seq, 200, QueryFetchMode::Normal);
    assert!(matches!(duplicate, QueryBeginResult::IgnoredWhileLoading { .. }));
}

#[test]
fn latest_wins_replaces_active_request() {
    let mut r = ttl_resource();
    let mut seq = test_sequencer();
    r.begin_request(&mut seq, 100, QueryFetchMode::Normal);
    let replacement = r.begin_request(&mut seq, 200, QueryFetchMode::Normal);
    assert!(matches!(
        replacement,
        QueryBeginResult::Started {
            replaced_request_id: Some(_),
            ..
        }
    ));
    assert_eq!(r.cancelled_count(), 1);
}

#[test]
fn ignore_while_loading_with_swr_still_serves_stale() {
    let mut r = QueryResource::new(
        "swr-ignore",
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: TTL_MS,
            stale_ms: STALE_MS,
        },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = test_sequencer();
    seed_data(&mut r, "cached", STORED_AT_MS);
    r.begin_request(&mut seq, STORED_AT_MS + 1_500, QueryFetchMode::Force);
    assert!(r.is_loading());
    let result = r.begin_request(&mut seq, STORED_AT_MS + 1_600, QueryFetchMode::Normal);
    assert!(matches!(
        result,
        QueryBeginResult::StaleCacheHit {
            replaced_request_id: None,
            ..
        }
    ));
}

#[test]
fn record_cache_hit_transitions_to_success() {
    let mut r = ttl_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    assert_eq!(r.status(), QueryStatus::Success);
    r.begin_loading(RequestId::scoped(1, 1), STORED_AT_MS + 100);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    r.record_cache_hit();
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.cache_hits(), 1);
}

#[test]
fn record_cache_hit_does_not_clear_failure_status() {
    let mut r = ttl_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    r.apply_failure("error", STORED_AT_MS + 100);
    assert_eq!(r.status(), QueryStatus::Failure);
    r.record_cache_hit();
    assert_eq!(r.status(), QueryStatus::Failure, "cache hit should not clear Failure status");
    assert_eq!(r.cache_hits(), 1, "cache_hits increments even when status is Failure (terminal state preserved)");
}

#[test]
fn cache_policy_accessor_roundtrip() {
    let mut r = ttl_resource();
    assert_eq!(r.cache_policy(), CachePolicy::Ttl { ttl_ms: TTL_MS });
    r.set_cache_policy(CachePolicy::StaleWhileRevalidate {
        ttl_ms: 5_000,
        stale_ms: 10_000,
    });
    assert_eq!(
        r.cache_policy(),
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 5_000,
            stale_ms: 10_000,
        }
    );
}

#[test]
fn swr_policy_accessors() {
    let policy = CachePolicy::StaleWhileRevalidate { ttl_ms: TTL_MS, stale_ms: STALE_MS };
    assert_eq!(policy.ttl_ms(), Some(TTL_MS));
    assert_eq!(policy.stale_ms(), Some(STALE_MS));
    assert_eq!(policy.total_valid_ms(), Some(SWR_TOTAL_MS));
    assert!(policy.can_short_circuit());
    assert!(policy.can_serve_stale());
}

#[test]
fn ttl_policy_accessors() {
    let policy = CachePolicy::Ttl { ttl_ms: 5_000 };
    assert_eq!(policy.ttl_ms(), Some(5_000));
    assert_eq!(policy.stale_ms(), None);
    assert_eq!(policy.total_valid_ms(), Some(5_000));
    assert!(policy.can_short_circuit());
    assert!(!policy.can_serve_stale());
}

#[test]
fn nocache_policy_accessors() {
    let policy = CachePolicy::NoCache;
    assert_eq!(policy.ttl_ms(), None);
    assert_eq!(policy.stale_ms(), None);
    assert_eq!(policy.total_valid_ms(), None);
    assert!(!policy.can_short_circuit());
    assert!(!policy.can_serve_stale());
}
