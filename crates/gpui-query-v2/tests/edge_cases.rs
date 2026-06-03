//! Edge-case integration tests for gpui-query-v2 based on audit findings.
//!
//! Covers boundary values and unusual inputs across:
//! - QueryKey (empty, single, very long)
//! - CachePolicy (TTL=0, TTL=u64::MAX, SWR edge cases)
//! - RetryPolicy (count=0, delay=0, overflow)
//! - QueryResource cancellation (after success, after cancel, double cancel)
//! - QueryResource reset (in every status)
//! - QueryResource begin_request (unregistered key, concurrent calls, IgnoreWhileLoading)
//! - InfiniteQueryResource (max_pages=0, max_pages=1, page eviction boundaries)
//! - RequestSequencer (overflow, scope advancement)
//! - QuerySignal (observer on dropped entity pattern)

use gpui_query_v2::core::{
    CachePolicy, FetchDirection, InfiniteQueryResource, QueryBeginResult, QueryError, QueryKey,
    QueryResource, QueryStatus, RequestId, RequestPolicy, RequestSequencer, RetryPolicy,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_resource(cache: CachePolicy) -> QueryResource<String, QueryError> {
    QueryResource::new(QueryKey::from("test"), cache, RequestPolicy::LatestWins)
}

fn make_resource_ignore_while_loading(
    cache: CachePolicy,
) -> QueryResource<String, QueryError> {
    QueryResource::new(QueryKey::from("test"), cache, RequestPolicy::IgnoreWhileLoading)
}

fn begin_request(resource: &mut QueryResource<String, QueryError>, now_ms: u128) -> QueryBeginResult {
    let mut seq = RequestSequencer::new();
    resource.begin_request(&mut seq, now_ms, gpui_query_v2::core::QueryFetchMode::Normal)
}

fn begin_request_with_seq(
    resource: &mut QueryResource<String, QueryError>,
    seq: &mut RequestSequencer,
    now_ms: u128,
) -> QueryBeginResult {
    resource.begin_request(seq, now_ms, gpui_query_v2::core::QueryFetchMode::Normal)
}

fn begin_request_force_with_seq(
    resource: &mut QueryResource<String, QueryError>,
    seq: &mut RequestSequencer,
    now_ms: u128,
) -> QueryBeginResult {
    resource.begin_request(seq, now_ms, gpui_query_v2::core::QueryFetchMode::Force)
}

fn complete_success(
    resource: &mut QueryResource<String, QueryError>,
    request_id: RequestId,
    data: &str,
    now_ms: u128,
) -> bool {
    resource.complete_current_success(request_id, data.to_string(), now_ms)
}

fn complete_failure(
    resource: &mut QueryResource<String, QueryError>,
    request_id: RequestId,
    msg: &str,
    now_ms: u128,
) -> bool {
    resource.complete_current_failure(request_id, QueryError::transport(msg), now_ms)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. QueryKey edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn query_key_empty_vec_panics() {
    // QueryKey::new with an empty Vec should panic (invariant: at least one segment).
    let result = std::panic::catch_unwind(|| {
        let _ = QueryKey::new(Vec::<String>::new());
    });
    assert!(result.is_err());
}

#[test]
fn query_key_single_segment_equals_from_single() {
    let from_array = QueryKey::from(["only"]);
    let from_single = QueryKey::from_single("only");
    assert_eq!(from_array, from_single);
    assert_eq!(from_array.as_single(), Some("only"));
    assert_eq!(from_single.as_single(), Some("only"));
    assert_eq!(from_array.to_path(), "only");
}

#[test]
fn query_key_very_long_key_does_not_overflow_path() {
    // Construct a key with 1000 segments.
    let segments: Vec<String> = (0..1000).map(|i| format!("seg_{i}")).collect();
    let key = QueryKey::new(segments);
    assert_eq!(key.parts().len(), 1000);

    let path = key.to_path();
    // Path should contain 999 separators (N-1 for N segments).
    assert_eq!(path.matches("::").count(), 999);
    assert!(path.starts_with("seg_0"));
    assert!(path.ends_with("seg_999"));
}

#[test]
fn query_key_join_preserves_original() {
    let original = QueryKey::from(["a", "b"]);
    let extended = original.join("c");
    // Original is untouched.
    assert_eq!(original.parts().len(), 2);
    assert_eq!(extended.parts().len(), 3);
    assert_eq!(extended.to_path(), "a::b::c");
}

#[test]
fn query_key_starts_with_self() {
    let key = QueryKey::from(["users", "42", "posts"]);
    assert!(key.starts_with(&key));
}

#[test]
fn query_key_does_not_start_with_unrelated_key() {
    let key = QueryKey::from(["users", "42"]);
    let other = QueryKey::from(["posts", "99"]);
    assert!(!key.starts_with(&other));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. CachePolicy TTL edge cases
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

#[test]
fn swr_serves_stale_within_window() {
    // TTL=100, stale=200 => fresh in [0,100], stale in (100,300], expired >300.
    let mut resource = make_resource(CachePolicy::StaleWhileRevalidate {
        ttl_ms: 100,
        stale_ms: 200,
    });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 0);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 100));
    }

    // At age=150, data is stale but serveable.
    let result2 = begin_request_with_seq(&mut resource, &mut seq, 250);
    assert!(matches!(result2, QueryBeginResult::StaleCacheHit { .. }));
}

#[test]
fn swr_expired_beyond_window_triggers_normal_fetch() {
    let mut resource = make_resource(CachePolicy::StaleWhileRevalidate {
        ttl_ms: 100,
        stale_ms: 200,
    });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 0);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 100));
    }

    // At age=500, data is fully expired (past ttl+stale=300).
    let result2 = begin_request_with_seq(&mut resource, &mut seq, 600);
    assert!(matches!(result2, QueryBeginResult::Started { .. }));
}

#[test]
fn swr_total_valid_ms_saturates_on_overflow() {
    let policy = CachePolicy::StaleWhileRevalidate {
        ttl_ms: u64::MAX,
        stale_ms: u64::MAX,
    };
    assert_eq!(policy.total_valid_ms(), Some(u64::MAX));
}

#[test]
fn no_cache_always_considers_data_expired() {
    let mut resource = make_resource(CachePolicy::NoCache);
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 0);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 1));
    }

    // With NoCache, data at last_updated=1 is expired at now=2.
    assert!(resource.is_cache_expired(2));
    // And it is never fresh.
    assert!(!resource.is_cache_fresh(1));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. RetryPolicy edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_policy_zero_retries_never_retries() {
    let policy = RetryPolicy::no_retries();
    assert_eq!(policy.max_retries, 0);
    assert!(!policy.should_retry(0));
}

#[test]
fn retry_policy_delay_zero_with_exponential_backoff_stays_zero() {
    let policy = RetryPolicy::new(3).with_delay(0).with_exponential_backoff();
    // 0 * 2^N = 0 for any N.
    assert_eq!(policy.delay_for_attempt(0), 0);
    assert_eq!(policy.delay_for_attempt(10), 0);
}

#[test]
fn retry_policy_exponential_overflow_saturates() {
    let policy = RetryPolicy::new(100)
        .with_delay(u64::MAX)
        .with_exponential_backoff()
        .with_max_delay(u64::MAX);
    // delay * factor overflows u64 => saturates to u64::MAX, then capped by
    // ABSOLUTE_MAX_DELAY_MS (1 hour = 3,600,000ms).
    let delay = policy.delay_for_attempt(62);
    assert_eq!(delay, 3_600_000);
}

#[test]
fn retry_policy_should_retry_boundary() {
    let policy = RetryPolicy::new(3);
    assert!(policy.should_retry(0));
    assert!(policy.should_retry(2));
    assert!(!policy.should_retry(3)); // exactly max_retries
    assert!(!policy.should_retry(100));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Cancel edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_after_success_returns_false() {
    // Cancelling when there is no active request should return false.
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 2_000));
    }

    // No active request anymore — cancel should return false.
    let cancelled = resource.cancel(QueryError::cancelled("late cancel"));
    assert!(!cancelled);
    assert_eq!(resource.status(), QueryStatus::Success);
}

#[test]
fn cancel_after_cancel_returns_false() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { .. } = result {
        // First cancel succeeds.
        assert!(resource.cancel(QueryError::cancelled("first")));
        // Second cancel fails — no active request.
        assert!(!resource.cancel(QueryError::cancelled("second")));
    }
}

#[test]
fn double_cancel_on_idle_returns_false() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    assert!(!resource.cancel(QueryError::cancelled("nope")));
    assert!(!resource.cancel(QueryError::cancelled("still nope")));
}

#[test]
fn cancel_preserves_previous_data_for_rollback() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    // First fetch succeeds.
    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "original", 2_000));
    }

    // Second fetch is a refetch (has data => LoadingWithData).
    let result2 = begin_request_with_seq(&mut resource, &mut seq, 3_000);
    if let QueryBeginResult::Started { .. } = result2 {
        // Cancel the refetch.
        assert!(resource.cancel(QueryError::cancelled("refetch cancelled")));
        // Data was cleared, but previous_data should hold "original".
        assert!(resource.data().is_none());
        assert_eq!(resource.previous_data(), Some(&"original".to_string()));
        assert_eq!(resource.status(), QueryStatus::Cancelled);
    }
}

#[test]
fn cancel_signal_is_observed_by_cloned_handles() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { .. } = result {
        let signal = resource.signal().unwrap().clone();
        assert!(!signal.is_cancelled());
        resource.cancel(QueryError::cancelled("done"));
        assert!(signal.is_cancelled());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Reset edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn reset_on_idle_clears_counters() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    // Manually inflate counters by doing some work.
    let mut seq = RequestSequencer::new();
    for _ in 0..3 {
        let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
        if let QueryBeginResult::Started { request_id: _, .. } = r {
            resource.cancel(QueryError::cancelled("bump counter"));
        }
    }
    assert!(resource.cancelled_count() > 0);

    resource.reset();
    assert_eq!(resource.status(), QueryStatus::Idle);
    assert_eq!(resource.cancelled_count(), 0);
    assert_eq!(resource.cache_hits(), 0);
    assert_eq!(resource.ignored_results(), 0);
    assert_eq!(resource.retry_count(), 0);
    assert!(resource.data().is_none());
    assert!(resource.error().is_none());
}

#[test]
fn reset_while_loading_cancels_signal() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let _result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    let signal = resource.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    resource.reset();
    assert!(signal.is_cancelled());
    assert_eq!(resource.status(), QueryStatus::Idle);
    assert!(resource.signal().is_none());
}

#[test]
fn reset_after_success_clears_data() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_success(&mut resource, request_id, "data", 2_000));
    }
    assert_eq!(resource.data(), Some(&"data".to_string()));

    resource.reset();
    assert!(resource.data().is_none());
    assert!(resource.previous_data().is_none());
    assert!(resource.last_updated_at_ms().is_none());
}

#[test]
fn reset_after_failure_clears_error() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let result = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = result {
        assert!(complete_failure(&mut resource, request_id, "boom", 2_000));
    }
    assert_eq!(resource.status(), QueryStatus::Failure);
    assert!(resource.error().is_some());

    resource.reset();
    assert!(resource.error().is_none());
    assert_eq!(resource.status(), QueryStatus::Idle);
}

#[test]
fn reset_preserves_policies_and_key() {
    let mut resource: QueryResource<String, QueryError> = QueryResource::new(
        QueryKey::from(["my", "key"]),
        CachePolicy::Ttl { ttl_ms: 5_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    resource.set_retry_policy(RetryPolicy::new(10));

    resource.reset();

    assert_eq!(resource.key(), &QueryKey::from(["my", "key"]));
    assert_eq!(resource.cache_policy(), CachePolicy::Ttl { ttl_ms: 5_000 });
    assert_eq!(resource.request_policy(), RequestPolicy::IgnoreWhileLoading);
    // Note: retry_policy is also preserved (it is configuration, not runtime state).
    assert_eq!(resource.retry_policy().max_retries, 10);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Fetch with unregistered / non-existent key pattern
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fetch_on_fresh_resource_always_starts() {
    // A resource that was just created (never fetched) should always start a request.
    let resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    assert_eq!(resource.status(), QueryStatus::Idle);
    assert!(resource.data().is_none());

    let mut resource = resource;
    let result = begin_request(&mut resource, 1_000);
    assert!(matches!(result, QueryBeginResult::Started { .. }));
    // First fetch is LoadingEmpty (no prior data).
    assert_eq!(resource.status(), QueryStatus::LoadingEmpty);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Concurrent begin_request calls with same key
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_begin_requests_latest_wins_replaces_active() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r1 = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    let r2 = begin_request_with_seq(&mut resource, &mut seq, 2_000);

    if let QueryBeginResult::Started {
        request_id: id1,
        replaced_request_id: None,
        ..
    } = r1
    {
        // First request has no predecessor (replaced_request_id is None, matched by pattern).

        if let QueryBeginResult::Started {
            request_id: id2,
            replaced_request_id: Some(replaced),
            ..
        } = r2
        {
            // Second request replaced the first.
            assert_eq!(replaced, id1);
            assert_ne!(id2, id1);
            assert_eq!(resource.cancelled_count(), 1);

            // Completing the stale request id1 should fail.
            assert!(!complete_success(&mut resource, id1, "stale", 3_000));
            assert_eq!(resource.ignored_results(), 1);

            // Completing the current request id2 should succeed.
            assert!(complete_success(&mut resource, id2, "fresh", 3_000));
            assert_eq!(resource.data(), Some(&"fresh".to_string()));
        } else {
            panic!("expected Started");
        }
    } else {
        panic!("expected Started");
    }
}

#[test]
fn concurrent_begin_requests_ignore_while_loading_ignores_second() {
    let mut resource = make_resource_ignore_while_loading(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r1 = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    let r2 = begin_request_with_seq(&mut resource, &mut seq, 2_000);

    if let QueryBeginResult::Started { request_id: id1, .. } = r1 {
        if let QueryBeginResult::IgnoredWhileLoading { active_request_id } = r2 {
            // Second request was ignored; active request is still id1.
            assert_eq!(active_request_id, id1);

            // Completing id1 still works.
            assert!(complete_success(&mut resource, id1, "data", 3_000));
        } else {
            panic!("expected IgnoredWhileLoading, got {:?}", r2);
        }
    } else {
        panic!("expected Started");
    }
}

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
// 8. Observer on dropped entity pattern (signal outlives resource reset)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn signal_clone_observes_cancel_after_resource_reset() {
    // Simulates an observer holding a signal clone while the resource is reset.
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    begin_request_with_seq(&mut resource, &mut seq, 1_000);
    let observer_signal = resource.signal().unwrap().clone();

    // Reset the resource — should cancel the signal.
    resource.reset();
    assert!(observer_signal.is_cancelled());
    assert!(resource.signal().is_none());
}

#[test]
fn signal_from_cancelled_request_is_cancelled() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    begin_request_with_seq(&mut resource, &mut seq, 1_000);
    let signal = resource.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    // Start a new request (LatestWins) — old signal is cancelled.
    begin_request_with_seq(&mut resource, &mut seq, 2_000);
    assert!(signal.is_cancelled());

    let new_signal = resource.signal().unwrap().clone();
    assert!(!new_signal.is_cancelled());
    assert_ne!(signal, new_signal);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Force fetch mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn force_fetch_ignores_fresh_cache() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    // Populate with data.
    let r1 = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r1 {
        assert!(complete_success(&mut resource, request_id, "cached", 2_000));
    }

    // Normal fetch at same time would be a CacheHit.
    let r2 = begin_request_with_seq(&mut resource, &mut seq, 2_000);
    assert!(matches!(r2, QueryBeginResult::CacheHit));

    // Force fetch ignores cache freshness.
    let r3 = begin_request_force_with_seq(&mut resource, &mut seq, 2_000);
    assert!(matches!(r3, QueryBeginResult::Started { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. RequestSequencer edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn request_sequencer_monotonically_increases() {
    let mut seq = RequestSequencer::new();
    let id1 = seq.next_request();
    let id2 = seq.next_request();
    let id3 = seq.next_request();

    // Sequence values should increase.
    assert!(id2.value() > id1.value());
    assert!(id3.value() > id2.value());
    // Scope should be the same for normal generation.
    assert_eq!(id1.scope_id(), id2.scope_id());
    assert_eq!(id2.scope_id(), id3.scope_id());
}

#[test]
fn request_sequencer_is_current_scope() {
    let mut seq = RequestSequencer::new();
    let id = seq.next_request();
    assert!(seq.is_current_scope(id));
}

// ═══════════════════════════════════════════════════════════════════════
// 11. InfiniteQueryResource edge cases
// ═══════════════════════════════════════════════════════════════════════

fn make_infinite() -> InfiniteQueryResource<Vec<String>, QueryError> {
    InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    )
}

#[test]
fn infinite_query_max_pages_zero_is_unbounded() {
    let mut resource = make_infinite();
    let mut seq = RequestSequencer::new();

    // Load 5 pages.
    for i in 0..5 {
        let id = resource.begin_fetch_next(&mut seq, i as u128 * 100).unwrap();
        assert!(resource.complete_page_success(
            id,
            vec![format!("p{i}")],
            true,
            true,
            i as u128 * 100 + 50,
        ));
    }

    // Set max_pages=0 => treated as None (unbounded).
    resource.set_max_pages(Some(0));
    assert_eq!(resource.max_pages(), None);
    assert_eq!(resource.page_count(), 5);
}

#[test]
fn infinite_query_max_pages_one_retains_only_last() {
    let mut resource = make_infinite();
    let mut seq = RequestSequencer::new();

    let id1 = resource.begin_fetch_next(&mut seq, 100).unwrap();
    resource.complete_page_success(id1, vec!["a".into()], true, true, 200);

    let id2 = resource.begin_fetch_next(&mut seq, 300).unwrap();
    resource.complete_page_success(id2, vec!["b".into()], true, true, 400);

    // Set max_pages=1 — should evict the oldest page.
    let evicted = resource.set_max_pages(Some(1));
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["a".to_string()]);
    assert_eq!(resource.page_count(), 1);
    assert_eq!(resource.last_page(), Some(&vec!["b".to_string()]));
}

#[test]
fn infinite_query_prepend_enforces_max_pages_from_back() {
    let mut resource = make_infinite();
    resource.set_max_pages(Some(2));

    resource.prepend_page(vec!["a".into()]);
    resource.prepend_page(vec!["b".into()]);
    // "c" pushes past max_pages=2, evicting from back.
    let evicted = resource.prepend_page(vec!["c".into()]);
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["a".to_string()]);
    // Pages are now [c, b].
    assert_eq!(resource.first_page(), Some(&vec!["c".to_string()]));
    assert_eq!(resource.last_page(), Some(&vec!["b".to_string()]));
}

#[test]
fn infinite_query_bidirectional_cannot_fetch_without_opt_in() {
    let mut resource = InfiniteQueryResource::<Vec<String>, QueryError>::new_bidirectional(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    // Both has_next_page and has_previous_page default to false.
    assert!(resource.begin_fetch_next(&mut seq, 1_000).is_none());
    assert!(resource.begin_fetch_previous(&mut seq, 1_000).is_none());

    // Opt in to next.
    resource.set_has_next_page(true);
    assert!(resource.begin_fetch_next(&mut seq, 1_000).is_some());
}

#[test]
fn infinite_query_reset_preserves_max_pages_and_direction() {
    let mut resource = make_infinite();
    resource.set_max_pages(Some(5));

    let mut seq = RequestSequencer::new();
    let id = resource.begin_fetch_next(&mut seq, 100).unwrap();
    resource.complete_page_success(id, vec!["page".into()], true, true, 200);

    resource.reset();
    assert_eq!(resource.max_pages(), Some(5));
    assert_eq!(resource.direction(), FetchDirection::ForwardOnly);
    assert!(resource.has_next_page());
    assert!(!resource.has_previous_page());
}

#[test]
fn infinite_query_page_data_valid_after_failure_with_existing_pages() {
    let mut resource = make_infinite();
    let mut seq = RequestSequencer::new();

    let id1 = resource.begin_fetch_next(&mut seq, 100).unwrap();
    resource.complete_page_success(id1, vec!["first".into()], true, true, 200);

    let id2 = resource.begin_fetch_next(&mut seq, 300).unwrap();
    resource.complete_page_failure(id2, QueryError::transport("timeout"));

    // Status is Failure but existing pages are still valid.
    assert_eq!(resource.status(), QueryStatus::Failure);
    assert!(resource.is_page_data_valid());
    assert_eq!(resource.page_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Optimistic update and rollback
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rollback_to_previous_restores_data() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(complete_success(&mut resource, request_id, "v1", 2_000));
    }

    // Optimistic update.
    resource.set_data("v2".to_string());
    assert_eq!(resource.data(), Some(&"v2".to_string()));
    assert_eq!(resource.previous_data(), Some(&"v1".to_string()));

    // Rollback.
    assert!(resource.rollback_to_previous());
    assert_eq!(resource.data(), Some(&"v1".to_string()));
    assert!(resource.previous_data().is_none());
}

#[test]
fn rollback_when_no_previous_returns_false() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    assert!(!resource.rollback_to_previous());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Complete with optional data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn complete_success_optional_none_transitions_to_idle() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(resource.complete_current_optional_success(request_id, None, 2_000));
    }

    assert_eq!(resource.status(), QueryStatus::Idle);
    assert!(resource.data().is_none());
}

#[test]
fn complete_success_optional_some_transitions_to_success() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(resource.complete_current_optional_success(
            request_id,
            Some("data".to_string()),
            2_000
        ));
    }

    assert_eq!(resource.status(), QueryStatus::Success);
    assert_eq!(resource.data(), Some(&"data".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Failure with retained data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn complete_failure_with_data_retains_data_and_sets_error() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(resource.complete_current_failure_with_data(
            request_id,
            "stale but visible".to_string(),
            QueryError::transport("refetch failed"),
            2_000
        ));
    }

    assert_eq!(resource.status(), QueryStatus::Failure);
    assert_eq!(resource.data(), Some(&"stale but visible".to_string()));
    assert!(resource.error().is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Stale data heuristic
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn is_data_stale_true_when_data_exists_and_status_is_failure() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    // Start a request and complete with failure but retained data.
    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        resource.complete_current_failure_with_data(
            request_id,
            "kept".to_string(),
            QueryError::transport("err"),
            2_000,
        );
    }

    // data exists, status is Failure => is_data_stale is true.
    assert_eq!(resource.status(), QueryStatus::Failure);
    assert!(resource.data().is_some());
    assert!(resource.is_data_stale());
}

#[test]
fn is_data_stale_false_when_success() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(complete_success(&mut resource, request_id, "data", 2_000));
    }

    assert!(!resource.is_data_stale());
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Cache hit on terminal failure state
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cache_hit_does_not_clear_terminal_failure() {
    // When a resource is in Failure status, a cache hit on old data should
    // NOT silently transition to Success (per the record_cache_hit invariant).
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        // Complete with failure AND retained data so we have data in Failure state.
        resource.complete_current_failure_with_data(
            request_id,
            "old_data".to_string(),
            QueryError::transport("failed"),
            2_000,
        );
    }
    assert_eq!(resource.status(), QueryStatus::Failure);
    assert!(resource.error().is_some());

    // Now try a cache hit (e.g., by calling begin_request when data is fresh).
    let r2 = begin_request_with_seq(&mut resource, &mut seq, 2_500);
    assert!(matches!(r2, QueryBeginResult::CacheHit));

    // Status should remain Failure — cache hit does not clear the error.
    assert_eq!(resource.status(), QueryStatus::Failure);
    assert!(resource.error().is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Serde round-trip edge cases
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

    assert_eq!(back.status(), QueryStatus::Success);
    assert_eq!(back.data(), Some(&"hello".to_string()));
    assert_eq!(back.cache_policy(), CachePolicy::Ttl { ttl_ms: 5_000 });
    assert_eq!(back.key(), &QueryKey::from("test"));
    // signal is #[serde(skip)] so it's None after deserialization.
    assert!(back.signal().is_none());
}

#[test]
fn infinite_query_serde_roundtrip_preserves_pages_and_config() {
    let mut resource = make_infinite();
    let mut seq = RequestSequencer::new();
    resource.set_max_pages(Some(10));

    let id1 = resource.begin_fetch_next(&mut seq, 100).unwrap();
    resource.complete_page_success(id1, vec!["a".into()], true, true, 200);
    let id2 = resource.begin_fetch_next(&mut seq, 300).unwrap();
    resource.complete_page_success(id2, vec!["b".into()], false, true, 400);

    let json = serde_json::to_string(&resource).unwrap();
    let back: InfiniteQueryResource<Vec<String>, QueryError> =
        serde_json::from_str(&json).unwrap();

    assert_eq!(back.page_count(), 2);
    assert_eq!(back.max_pages(), Some(10));
    assert!(!back.has_next_page()); // last completion set has_more=false
    assert!(back.signal().is_none()); // signal is #[serde(skip)]
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Placeholder and display data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn display_data_falls_back_to_placeholder() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    assert!(resource.display_data().is_none());

    resource.set_placeholder_data(Some("placeholder".to_string()));
    assert_eq!(resource.display_data(), Some(&"placeholder".to_string()));

    let mut seq = RequestSequencer::new();
    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(complete_success(&mut resource, request_id, "real", 2_000));
    }

    // data() takes priority over placeholder_data().
    assert_eq!(resource.display_data(), Some(&"real".to_string()));
}

#[test]
fn reset_clears_placeholder_data() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    resource.set_placeholder_data(Some("temp".to_string()));
    assert!(resource.placeholder_data().is_some());

    resource.reset();
    assert!(resource.placeholder_data().is_none());
    assert!(resource.display_data().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Invalidate cache
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn invalidate_makes_fresh_data_stale() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(complete_success(&mut resource, request_id, "data", 2_000));
    }

    // Data is fresh right now.
    assert!(resource.is_cache_fresh(3_000));

    // Invalidate clears last_updated_at.
    resource.invalidate();
    assert!(resource.last_updated_at_ms().is_none());

    // Now begin_request should NOT be a CacheHit (no timestamp => not fresh).
    let r2 = begin_request_with_seq(&mut resource, &mut seq, 4_000);
    assert!(matches!(r2, QueryBeginResult::Started { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Initial data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn initial_data_seeds_idle_resource() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    resource.set_initial_data("seed".to_string(), 1_000);

    assert_eq!(resource.data(), Some(&"seed".to_string()));
    assert_eq!(resource.initial_data(), Some(&"seed".to_string()));
    assert!(resource.last_updated_at_ms().is_some());
}

#[test]
fn initial_data_ignored_when_not_idle() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    // Start loading.
    begin_request_with_seq(&mut resource, &mut seq, 1_000);
    assert_eq!(resource.status(), QueryStatus::LoadingEmpty);

    // set_initial_data should be a no-op when not Idle.
    resource.set_initial_data("too-late".to_string(), 2_000);
    assert!(resource.data().is_none());
    assert!(resource.initial_data().is_none());
}

#[test]
fn initial_data_ignored_when_data_already_exists() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    let mut seq = RequestSequencer::new();

    let r = begin_request_with_seq(&mut resource, &mut seq, 1_000);
    if let QueryBeginResult::Started { request_id, .. } = r {
        assert!(complete_success(&mut resource, request_id, "existing", 2_000));
    }

    resource.set_initial_data("ignored".to_string(), 3_000);
    assert_eq!(resource.data(), Some(&"existing".to_string()));
}

#[test]
fn reset_clears_initial_data() {
    let mut resource = make_resource(CachePolicy::Ttl { ttl_ms: 60_000 });
    resource.set_initial_data("seed".to_string(), 1_000);
    assert!(resource.initial_data().is_some());

    resource.reset();
    assert!(resource.initial_data().is_none());
    assert!(resource.data().is_none());
}
