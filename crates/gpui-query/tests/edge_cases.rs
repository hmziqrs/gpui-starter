//! Edge-case integration tests for gpui-query.
//!
//! This file contains ONLY tests that add genuinely new coverage beyond the
//! comprehensive internal test suites in `src/tests/`. Each test here covers
//! a boundary value or scenario not exercised by:
//!
//! - `core_lifecycle.rs` — state transitions, cancel, reset, rollback, signal
//! - `core_cache.rs` — TTL/SWR/NoCache boundaries, invalidation, placeholder
//! - `core_infinite_query.rs` — max_pages, bidirectional, serde, is_page_data_valid
//! - `core_request.rs` — RequestId, RequestSequencer, RequestGuard, RequestPolicy
//! - `core_policy_types.rs` — RetryPolicy, CachePolicy accessors, should_retry
//! - `key.rs` — QueryKey construction, starts_with, to_path, serde
//!
//! Removed ~48 near-duplicate tests that were already covered thoroughly by
//! the internal suites. This reduces maintenance burden and avoids false
//! confidence from test count inflation.

use gpui_query::core::{
    CachePolicy, InfiniteQueryResource, QueryBeginResult, QueryError, QueryKey,
    QueryResource, RequestSequencer,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_resource(cache: CachePolicy) -> QueryResource<String, QueryError> {
    QueryResource::new(QueryKey::from("test"), cache, gpui_query::core::RequestPolicy::LatestWins)
}

fn begin_request_with_seq(
    resource: &mut QueryResource<String, QueryError>,
    seq: &mut RequestSequencer,
    now_ms: u128,
) -> QueryBeginResult {
    resource.begin_request(seq, now_ms, gpui_query::core::QueryFetchMode::Normal)
}

fn complete_success(
    resource: &mut QueryResource<String, QueryError>,
    request_id: gpui_query::core::RequestId,
    data: &str,
    now_ms: u128,
) -> bool {
    resource.complete_current_success(request_id, data.to_string(), now_ms)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. QueryKey: empty Vec panics (boundary not in key.rs internal tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn query_key_empty_vec_panics() {
    // QueryKey::new with an empty Vec should panic (invariant: at least one segment).
    let result = std::panic::catch_unwind(|| {
        let _ = QueryKey::new(Vec::<String>::new());
    });
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. CachePolicy TTL=0: every millisecond past completion triggers a new fetch
//    (unique boundary: core_cache.rs tests ttl_ms >= 1_000 only)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ttl_zero_every_begin_request_is_a_fetch() {
    // With ttl_ms = 0, data is fresh only at age=0 (age <= ttl_ms).
    // At age=1 the data is stale, so a new fetch is triggered.
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 0 });
    let mut seq = RequestSequencer::new();

    // First request starts normally.
    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    assert!(matches!(result, QueryBeginResult::Started { .. }));

    // Complete with success.
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 1_000));
    }

    // At the same timestamp, age=0 <= ttl_ms=0 => still a CacheHit.
    let result_same = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    assert!(matches!(result_same, QueryBeginResult::CacheHit));

    // But one millisecond later, age=1 > ttl_ms=0 => data is stale => new fetch.
    let result2 = begin_request_with_seq(&mut resource, &mut seq, 1_001);
    assert!(matches!(result2, QueryBeginResult::Started { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. CachePolicy TTL=u64::MAX: data is always fresh, never re-fetched
//    (unique boundary: core_cache.rs tests ttl_ms <= 60_000)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ttl_max_data_is_always_fresh() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: u64::MAX });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 2_000));
    }

    // Even a very large age should still be fresh under u64::MAX TTL.
    let result2 = begin_request_with_seq(&mut resource, &mut seq, u64::MAX as u128);
    assert!(matches!(result2, QueryBeginResult::CacheHit));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. CachePolicy SWR total_valid_ms saturates on overflow
//    (boundary value not tested by core_cache.rs or core_policy_types.rs)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn swr_total_valid_ms_saturates_on_overflow() {
    let policy = CachePolicy::StaleWhileRevalidate {
        ttl_ms: u64::MAX,
        stale_ms: u64::MAX,
    };
    assert_eq!(policy.total_valid_ms(), Some(u64::MAX));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Three concurrent requests with LatestWins: only the last completes
//    (core_request.rs tests 2-request replacement; this tests 3-request
//    accumulation of cancelled_count and ignored_results)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn three_concurrent_requests_latest_wins_only_last_completes() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r1 = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    let r2 = begin_request_with_seq(&mut resource, &mut seq, 2_000);
    let r3 = begin_request_with_seq(&mut resource, &mut seq, 3_000);

    if let QueryBeginResult::Started { request_id: id1, .. } = r1 {
        if let QueryBeginResult::Started { request_id: id2, .. } = r2 {
            if let QueryBeginResult::Started { request_id: id3, .. } = r3 {
                // Only id3 is current; id1 and id2 are stale.
                assert!(!complete_success(&mut resource, id1, "old1", 4_000));
                assert!(!complete_success(&mut resource, id2, "old2", 4_000));
                assert!(complete_success(&mut resource, id3, "newest", 4_000));
                assert_eq!(resource.data(), Some(&"newest".to_string()));
                assert_eq!(resource.cancelled_count(), 2);
                assert_eq!(resource.ignored_results(), 2);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. QueryResource serde round-trip (not covered by core_cache.rs which
//    tests InfiniteQueryResource serde and key.rs which tests key serde)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn resource_serde_roundtrip_preserves_state() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 5_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(complete_success(&mut resource, request_id, "hello", 2_000));
    }

    let json = serde_json::to_string(&resource).unwrap();
    let back: QueryResource<String, QueryError> = serde_json::from_str(&json).unwrap();

    assert_eq!(back.status(), gpui_query::core::QueryStatus::Success);
    assert_eq!(back.data(), Some(&"hello".to_string()));
    assert_eq!(back.cache_policy(), CachePolicy::Ttl { ttl_ms: 5_000 });
    assert_eq!(back.key(), &QueryKey::from("test"));
    // signal is #[serde(skip)] so it's None after deserialization.
    assert!(back.signal().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. InfiniteQueryResource serde with max_pages and has_next_page config
//    (core_infinite_query.rs serde test only checks page_count and status,
//    not max_pages preservation or has_next_page propagation)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn infinite_query_serde_roundtrip_preserves_pages_and_config() {
    let mut resource: InfiniteQueryResource<Vec<String>, QueryError> =
        InfiniteQueryResource::new(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            gpui_query::core::RequestPolicy::LatestWins,
        );
    let mut seq = RequestSequencer::new();
    resource.set_max_pages(Some(10));

    let id1 = resource.begin_fetch_next(&mut seq, 100).unwrap();
    resource.complete_page_success(id1, vec!["a".to_string()], true, true, 200);
    let id2 = resource.begin_fetch_next(&mut seq, 300).unwrap();
    resource.complete_page_success(id2, vec!["b".to_string()], false, true, 400);

    let json = serde_json::to_string(&resource).unwrap();
    let back: InfiniteQueryResource<Vec<String>, QueryError> =
        serde_json::from_str(&json).unwrap();

    assert_eq!(back.page_count(), 2);
    assert_eq!(back.max_pages(), Some(10));
    assert!(!back.has_next_page()); // last completion set has_more=false
    assert!(back.signal().is_none()); // signal is #[serde(skip)]
}
