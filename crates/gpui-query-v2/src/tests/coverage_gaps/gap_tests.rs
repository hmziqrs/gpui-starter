//! Individual gap-filling tests (GAP-03 through GAP-20).
//!
//! Covers: begin_request_with_id + SWR + IgnoreWhileLoading, stale request ID
//! rejection, Force mode + IgnoreWhileLoading, QueryError sanitized, QueryKey
//! join/from/deref/serde/hash, InfiniteQuery IgnoreWhileLoading / cross-direction /
//! reset / bidirectional / prepend.

use crate::core::*;
use crate::tests::test_support::*;

// ═══════════════════════════════════════════════════════════════════════════
// GAP-03: begin_request_with_id + SWR + IgnoreWhileLoading + active request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_with_id_swr_ignore_while_loading_with_active_request() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "swr-ignore",
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 500,
            stale_ms: 1_000,
        },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = test_sequencer();

    // Seed cached data at t=100
    r.apply_success("cached", 100);

    // Start a fetch to create an active request
    r.begin_request(&mut seq, 1_500, QueryFetchMode::Force);
    assert!(r.is_loading());

    // Now call begin_request_with_id when data is stale and a request is active.
    // Should get StaleCacheHit with the EXISTING active_request_id (no new request started).
    let result = r.begin_request_with_id(
        Some(RequestId::scoped(99, 1)),
        1_500,
        QueryFetchMode::Normal,
    );

    match result {
        QueryBeginResult::StaleCacheHit {
            request_id,
            replaced_request_id,
            ..
        } => {
            // Should use the EXISTING active request id, not the provided 99:1
            assert!(
                replaced_request_id.is_none(),
                "no replacement under IgnoreWhileLoading"
            );
            // request_id should be the existing active request, not the one we passed
            assert_ne!(
                request_id,
                RequestId::scoped(99, 1),
                "should use existing active request id"
            );
        }
        other => panic!("expected StaleCacheHit, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-04: complete_current_optional_success rejects stale request ID
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complete_current_optional_success_rejects_stale_id() {
    let mut r = test_resource();
    let mut s = test_sequencer();

    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);

    // rid1 is stale
    assert!(
        !r.complete_current_optional_success(rid1, Some("stale"), 300),
        "stale ID should be rejected"
    );
    assert_eq!(r.ignored_results(), 1);

    // rid2 is current
    assert!(
        r.complete_current_optional_success(rid2, Some("fresh"), 300),
        "current ID should be accepted"
    );
    assert_eq!(r.data(), Some(&"fresh"));
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-05: complete_current_failure_with_data rejects stale request ID
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complete_current_failure_with_data_rejects_stale_id() {
    let mut r = test_resource();
    let mut s = test_sequencer();

    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let _rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);

    assert!(
        !r.complete_current_failure_with_data(rid1, "fallback", QueryError::response("stale"), 300),
        "stale ID should be rejected"
    );
    assert_eq!(r.ignored_results(), 1);

    // The current request is still active
    assert!(r.active_request_id().is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-06: Force mode respects IgnoreWhileLoading
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ignore_while_loading_rejects_forced_fetch_when_loading() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "test",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut s = test_sequencer();

    r.begin_request(&mut s, 100, QueryFetchMode::Normal);

    let result = r.begin_request(&mut s, 200, QueryFetchMode::Force);
    assert!(
        matches!(result, QueryBeginResult::IgnoredWhileLoading { .. }),
        "Force mode should still respect IgnoreWhileLoading"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-12: QueryError::sanitized() with mongodb connection string
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_error_sanitized_mongodb_connection() {
    let err = QueryError::transport("connect mongodb://admin:secret@host/db failed");
    let clean = err.sanitized();
    assert!(
        clean.message().contains("[REDACTED_CONNECTION]"),
        "mongodb connection string should be redacted"
    );
    assert!(
        !clean.message().contains("admin:secret"),
        "credentials should be removed"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-13: QueryError::sanitized() with empty string message
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_error_sanitized_empty_message() {
    let err = QueryError::response("");
    let clean = err.sanitized();
    assert_eq!(clean.message(), "");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-14: QueryError::new() with explicit kind
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_error_new_with_explicit_kind() {
    let err = QueryError::new(QueryErrorKind::Transport, "timeout");
    assert_eq!(err.kind(), QueryErrorKind::Transport);
    assert_eq!(err.message(), "timeout");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-15: record_cache_hit does not clear Cancelled status
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn record_cache_hit_does_not_clear_cancelled_status() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "cache-cancel",
        CachePolicy::Ttl { ttl_ms: 1_000 },
        RequestPolicy::LatestWins,
    );
    // Seed data at t=1000
    r.apply_success("data", 1_000);

    // Use Force mode to bypass the fresh cache and start a real request
    let mut seq = test_sequencer();
    r.begin_request(&mut seq, 1_100, QueryFetchMode::Force);
    r.cancel(QueryError::cancelled("abort"));
    assert_eq!(r.status(), QueryStatus::Cancelled);

    r.record_cache_hit();
    assert_eq!(
        r.status(),
        QueryStatus::Cancelled,
        "cache hit should not clear Cancelled status"
    );
    assert_eq!(r.cache_hits(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-16: QueryKey::join() appends segments
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn join_appends_segment() {
    let key = QueryKey::from(["users"]);
    let extended = key.join("42");
    assert_eq!(extended.parts().len(), 2);
    assert_eq!(extended.to_path(), "users::42");
    // Original unchanged
    assert_eq!(key.parts().len(), 1);
}

#[test]
fn join_chain_creates_multi_part_key() {
    let key = QueryKey::from("users").join("42").join("posts");
    assert_eq!(key.parts().len(), 3);
    assert_eq!(key.to_path(), "users::42::posts");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-17: QueryKey::from(Vec<String>)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn from_vec_string() {
    let key = QueryKey::from(vec!["users".to_string(), "42".to_string()]);
    assert_eq!(key.parts().len(), 2);
    assert_eq!(key.to_path(), "users::42");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-18: QueryKey Deref to [Arc<str>] allows indexing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deref_allows_indexing() {
    let key = QueryKey::from(["a", "b", "c"]);
    assert_eq!(&*key[0], "a");
    assert_eq!(&*key[2], "c");
    assert_eq!(key.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-19: QueryKey serde deserialize from single string
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_deserialize_single_string() {
    let json = "\"users\"";
    let key: QueryKey = serde_json::from_str(json).unwrap();
    assert_eq!(key.parts().len(), 1);
    assert_eq!(key.as_str(), "users");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-20: QueryKey Hash consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hash_consistency() {
    use std::collections::HashSet;
    let k1 = QueryKey::from(["users", "42"]);
    let k2 = QueryKey::from(["users", "42"]);
    let k3 = QueryKey::from(["users", "43"]);
    let mut set = HashSet::new();
    set.insert(k1.clone());
    assert!(set.contains(&k2), "equal keys must have equal hashes");
    assert!(!set.contains(&k3), "different keys should not match");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-07: InfiniteQuery begin_fetch_previous with IgnoreWhileLoading
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ignore_while_loading_prevents_previous_page_replacement() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = RequestSequencer::new();
    r.set_has_previous_page(true);

    let _id1 = r.begin_fetch_previous(&mut seq, 1_000).unwrap();
    assert!(r.is_fetching_previous_page());

    // Second call with IgnoreWhileLoading should return None
    let id2 = r.begin_fetch_previous(&mut seq, 2_000);
    assert!(
        id2.is_none(),
        "second begin_fetch_previous should be ignored"
    );
    assert_eq!(r.cancelled_count(), 0, "no cancellation on ignore");
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-08: Cross-direction IgnoreWhileLoading (next then previous)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ignore_while_loading_cross_direction_next_then_prev() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new_bidirectional(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = RequestSequencer::new();
    r.set_has_next_page(true);
    r.set_has_previous_page(true);

    let _id_next = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert!(r.is_fetching_next_page());

    // Cross-direction: begin_fetch_previous while next is active.
    // Under IgnoreWhileLoading, this checks is_fetching_previous_page (false),
    // so it should succeed despite is_fetching_next_page being true.
    // BUT active_request_id.is_some() => cancelled_count++
    let id_prev = r.begin_fetch_previous(&mut seq, 2_000);
    assert!(
        id_prev.is_some(),
        "cross-direction should succeed under IgnoreWhileLoading"
    );
    // The previous page fetch replaces the next page fetch (LatestWins-style
    // cross-direction replacement), so cancelled_count increments.
    assert!(r.is_fetching_previous_page());
    assert!(!r.is_fetching_next_page());
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-09: InfiniteQueryResource reset preserves retry_policy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn infinite_query_reset_preserves_retry_policy() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let policy = RetryPolicy::new(10)
        .with_delay(500)
        .with_exponential_backoff();
    r.set_retry_policy(policy.clone());
    r.reset();
    assert_eq!(
        r.retry_policy(),
        &policy,
        "retry_policy should survive reset"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-10: Bidirectional resource initial accessors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn bidirectional_resource_initial_accessors() {
    let r = InfiniteQueryResource::<Vec<String>>::new_bidirectional(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    assert_eq!(r.cache_policy(), CachePolicy::Ttl { ttl_ms: 60_000 });
    assert_eq!(r.request_policy(), RequestPolicy::LatestWins);
    assert_eq!(r.direction(), FetchDirection::Bidirectional);
    assert!(!r.has_next_page());
    assert!(!r.has_previous_page());
}

// ═══════════════════════════════════════════════════════════════════════════
// GAP-11: prepend with has_more=true preserves has_previous_page
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn prepend_with_has_more_true_preserves_has_previous() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    r.set_has_previous_page(true);

    let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["page0".to_string()], true, false, 4_000);

    assert!(
        r.has_previous_page(),
        "has_more=true should keep has_previous_page=true"
    );
    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
}
