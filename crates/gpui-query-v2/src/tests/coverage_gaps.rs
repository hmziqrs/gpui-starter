//! High-priority test coverage gaps for gpui-query-v2.
//!
//! # Test categories
//!
//! 1. **Property-based tests** (no external framework): Systematic checks over
//!    many inputs for RetryPolicy, CachePolicy, serde roundtrip, RequestSequencer.
//!
//! 2. **State-transition invariant tests**: Table-driven verification that
//!    status and data are never inconsistent after any state transition.
//!
//! 3. **Deterministic GC eviction tests**: Concrete assertions on GC behavior
//!    rather than "no panic" patterns.
//!
//! 4. **Concurrency guard tests**: Verify that the two-phase completion protocol
//!    maintains invariants even when requests are interleaved.

use crate::core::*;
use crate::tests::test_support::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Property-based tests
// ═══════════════════════════════════════════════════════════════════════════

// --- RetryPolicy: delay_for_attempt never exceeds ABSOLUTE_MAX_DELAY_MS --------

#[test]
fn prop_retry_delay_never_exceeds_absolute_max_for_all_attempts() {
    // ABSOLUTE_MAX_DELAY_MS = 3_600_000 (1 hour).
    const ABSOLUTE_MAX: u64 = 3_600_000;

    // Test with various base delays and exponential backoff enabled.
    let base_delays: &[u64] = &[0, 1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, u64::MAX / 2, u64::MAX];
    let max_delays: &[u64] = &[0, 100, 1_000, 30_000, ABSOLUTE_MAX, u64::MAX];

    for &base in base_delays {
        for &max_delay in max_delays {
            let policy = RetryPolicy {
                max_retries: 100,
                retry_delay_ms: base,
                exponential_backoff: true,
                max_retry_delay_ms: max_delay,
            };
            // Check attempts 0 through 100, plus some very large ones.
            for attempt in 0..=100u32 {
                let delay = policy.delay_for_attempt(attempt);
                assert!(
                    delay <= ABSOLUTE_MAX,
                    "delay_for_attempt({}) = {} exceeds ABSOLUTE_MAX ({}) \
                     with base={}, max_delay={}",
                    attempt, delay, ABSOLUTE_MAX, base, max_delay
                );
            }
            // Extreme attempt numbers.
            for attempt in [u32::MAX, 200, 500, 1000] {
                let delay = policy.delay_for_attempt(attempt);
                assert!(
                    delay <= ABSOLUTE_MAX,
                    "delay_for_attempt({}) = {} exceeds ABSOLUTE_MAX ({}) \
                     with base={}, max_delay={}",
                    attempt, delay, ABSOLUTE_MAX, base, max_delay
                );
            }
        }
    }
}

#[test]
fn prop_retry_delay_without_backoff_is_constant() {
    // Without exponential backoff, delay_for_attempt should return retry_delay_ms
    // regardless of attempt number.
    let delays: &[u64] = &[0, 1, 100, 1_000, 30_000, u64::MAX];
    for &base in delays {
        let policy = RetryPolicy {
            max_retries: 10,
            retry_delay_ms: base,
            exponential_backoff: false,
            max_retry_delay_ms: 0,
        };
        for attempt in 0..=50u32 {
            assert_eq!(
                policy.delay_for_attempt(attempt),
                base,
                "without backoff, delay should be constant for attempt {}",
                attempt
            );
        }
    }
}

#[test]
fn prop_retry_delay_monotonically_increases_or_capped() {
    // With exponential backoff and a reasonable base, delays should be
    // monotonically non-decreasing (they can plateau at the cap).
    let policy = RetryPolicy::new(100)
        .with_delay(100)
        .with_exponential_backoff()
        .with_max_delay(30_000);
    let mut prev_delay: u64 = 0;
    for attempt in 0..=50u32 {
        let delay = policy.delay_for_attempt(attempt);
        assert!(
            delay >= prev_delay,
            "delay decreased from {} to {} at attempt {}",
            prev_delay, delay, attempt
        );
        prev_delay = delay;
    }
}

// --- CachePolicy: is_fresh / is_expired / total_valid_ms relationships ------

#[test]
fn prop_cache_policy_fresh_and_expired_are_complementary_for_ttl() {
    // For Ttl, every non-negative age is either fresh or expired (no gap).
    // Boundary: age == ttl_ms is fresh (inclusive), age == ttl_ms + 1 is expired.
    let ttl_values: &[u64] = &[1, 10, 100, 1_000, 60_000, u64::MAX];
    for &ttl in ttl_values {
        let policy = CachePolicy::Ttl { ttl_ms: ttl };
        let total = policy.total_valid_ms().expect("Ttl should have total_valid_ms");
        assert_eq!(total, ttl);

        // Sample ages: 0, boundary-1, boundary, boundary+1, and large values.
        let ages: &[u128] = &[0, ttl as u128 / 2, ttl as u128, ttl as u128 + 1, ttl as u128 * 2];
        for &age in ages {
            let is_fresh = policy.is_fresh(age);
            let is_expired = policy.is_expired(age);

            if age <= ttl as u128 {
                assert!(is_fresh, "age {} <= ttl {} should be fresh", age, ttl);
                assert!(!is_expired, "fresh age {} should not be expired", age);
            } else {
                assert!(!is_fresh, "age {} > ttl {} should not be fresh", age, ttl);
                assert!(is_expired, "age {} > ttl {} should be expired", age, ttl);
            }
        }
    }
}

#[test]
fn prop_cache_policy_swr_three_way_partition() {
    // For StaleWhileRevalidate, every non-negative age falls into exactly one of:
    // fresh, stale-but-serveable, or expired. No gaps, no overlaps.
    let cases: &[(u64, u64)] = &[
        (1, 1), (10, 10), (100, 200), (1_000, 2_000), (60_000, 30_000),
    ];
    for &(ttl, stale) in cases {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms: ttl, stale_ms: stale };
        let total = ttl as u128 + stale as u128;

        let ages: &[u128] = &[
            0,
            ttl as u128 / 2,
            ttl as u128,        // boundary: still fresh
            ttl as u128 + 1,    // just past TTL: stale
            total / 2 + ttl as u128 / 2,  // mid-stale window
            total,              // boundary: still stale-but-serveable
            total + 1,          // expired
            total * 2,          // way expired
        ];
        for &age in ages {
            let is_fresh = policy.is_fresh(age);
            let is_stale = policy.is_stale_but_serveable(age);
            let is_expired = policy.is_expired(age);

            // Exactly one must be true.
            let count = is_fresh as u8 + is_stale as u8 + is_expired as u8;
            assert_eq!(
                count, 1,
                "age {} must be exactly one of fresh/stale/expired \
                 (fresh={}, stale={}, expired={}) for ttl={} stale={}",
                age, is_fresh, is_stale, is_expired, ttl, stale
            );

            if age <= ttl as u128 {
                assert!(is_fresh, "age {} <= ttl {} must be fresh", age, ttl);
            } else if age <= total {
                assert!(is_stale, "age {} must be stale-but-serveable (ttl={}, total={})", age, ttl, total);
            } else {
                assert!(is_expired, "age {} > total {} must be expired", age, total);
            }
        }
    }
}

#[test]
fn prop_cache_policy_nocache_always_expired_never_fresh() {
    let policy = CachePolicy::NoCache;
    assert_eq!(policy.total_valid_ms(), None);
    assert_eq!(policy.ttl_ms(), None);

    for age in [0u128, 1, 100, 1_000, u128::MAX] {
        assert!(!policy.is_fresh(age), "NoCache should never be fresh at age {}", age);
        assert!(policy.is_expired(age), "NoCache should always be expired at age {}", age);
        assert!(!policy.is_stale_but_serveable(age), "NoCache should never be stale-but-serveable");
    }
}

#[test]
fn prop_cache_policy_total_valid_ms_consistency() {
    // total_valid_ms must equal ttl_ms for Ttl, and ttl_ms + stale_ms for SWR.
    let cases: &[CachePolicy] = &[
        CachePolicy::NoCache,
        CachePolicy::Ttl { ttl_ms: 1 },
        CachePolicy::Ttl { ttl_ms: 60_000 },
        CachePolicy::Ttl { ttl_ms: u64::MAX },
        CachePolicy::StaleWhileRevalidate { ttl_ms: 100, stale_ms: 50 },
        CachePolicy::StaleWhileRevalidate { ttl_ms: u64::MAX, stale_ms: u64::MAX },
    ];
    for policy in cases {
        match policy {
            CachePolicy::NoCache => {
                assert_eq!(policy.total_valid_ms(), None);
                assert_eq!(policy.ttl_ms(), None);
                assert_eq!(policy.stale_ms(), None);
            }
            CachePolicy::Ttl { ttl_ms } => {
                assert_eq!(policy.total_valid_ms(), Some(*ttl_ms));
                assert_eq!(policy.ttl_ms(), Some(*ttl_ms));
                assert_eq!(policy.stale_ms(), None);
            }
            CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms } => {
                let expected = ttl_ms.saturating_add(*stale_ms);
                assert_eq!(policy.total_valid_ms(), Some(expected));
                assert_eq!(policy.ttl_ms(), Some(*ttl_ms));
                assert_eq!(policy.stale_ms(), Some(*stale_ms));
            }
        }
    }
}

// --- Serde roundtrip: decode(encode(x)) == x -----------------------------

#[test]
fn prop_serde_roundtrip_all_statuses() {
    let statuses = [
        QueryStatus::Idle,
        QueryStatus::LoadingEmpty,
        QueryStatus::LoadingWithData,
        QueryStatus::Success,
        QueryStatus::Failure,
        QueryStatus::Cancelled,
    ];
    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let back: QueryStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, status, "serde roundtrip failed for {:?}", status);
    }
}

#[test]
fn prop_serde_roundtrip_all_cache_policies() {
    let policies: Vec<CachePolicy> = vec![
        CachePolicy::NoCache,
        CachePolicy::Ttl { ttl_ms: 0 },
        CachePolicy::Ttl { ttl_ms: 1 },
        CachePolicy::Ttl { ttl_ms: 60_000 },
        CachePolicy::Ttl { ttl_ms: u64::MAX },
        CachePolicy::StaleWhileRevalidate { ttl_ms: 0, stale_ms: 0 },
        CachePolicy::StaleWhileRevalidate { ttl_ms: 100, stale_ms: 200 },
        CachePolicy::StaleWhileRevalidate { ttl_ms: u64::MAX, stale_ms: u64::MAX },
    ];
    for policy in &policies {
        let json = serde_json::to_string(policy).unwrap();
        let back: CachePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, policy, "serde roundtrip failed for {:?}", policy);
    }
}

#[test]
fn prop_serde_roundtrip_all_request_policies() {
    for policy in [RequestPolicy::LatestWins, RequestPolicy::IgnoreWhileLoading] {
        let json = serde_json::to_string(&policy).unwrap();
        let back: RequestPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}

#[test]
fn prop_serde_roundtrip_retry_policies() {
    let policies: Vec<RetryPolicy> = vec![
        RetryPolicy::no_retries(),
        RetryPolicy::default(),
        RetryPolicy::new(0),
        RetryPolicy::new(100),
        RetryPolicy::new(5).with_delay(0).with_exponential_backoff().with_max_delay(0),
        RetryPolicy::new(u32::MAX).with_delay(u64::MAX).with_exponential_backoff().with_max_delay(u64::MAX),
    ];
    for policy in &policies {
        let json = serde_json::to_string(policy).unwrap();
        let back: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, policy, "serde roundtrip failed for {:?}", policy);
    }
}

#[test]
fn prop_serde_roundtrip_query_error_all_kinds() {
    let errors = [
        QueryError::cancelled("abort"),
        QueryError::response("not found"),
        QueryError::transport("timeout"),
        QueryError::unknown("mystery"),
        QueryError::new(QueryErrorKind::Cancelled, ""),
        QueryError::new(QueryErrorKind::Response, "a".repeat(1000)),
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: QueryError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind(), err.kind());
        assert_eq!(back.message(), err.message());
    }
}

#[test]
fn prop_serde_roundtrip_query_resource_multiple_states() {
    // Use NoCache so begin_request always returns Started (no CacheHit).
    let mut r: QueryResource<String, QueryError> = QueryResource::new(
        "serde-test",
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut s = test_sequencer();

    // Test roundtrip in Success state.
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);

    r.complete_current_success(rid, "hello".to_string(), 200);

    let json = serde_json::to_string(&r).unwrap();
    let back: QueryResource<String, QueryError> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.status(), QueryStatus::Success);
    assert_eq!(back.data(), Some(&"hello".to_string()));
    assert_eq!(back.cache_policy(), CachePolicy::NoCache);
    assert_eq!(back.request_policy(), RequestPolicy::LatestWins);
    assert!(back.signal().is_none(), "signal is #[serde(skip)]");
    assert!(back.initial_data().is_none(), "initial_data is #[serde(skip)]");

    // Test roundtrip in Failure state.
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    r.complete_current_failure(rid2, QueryError::transport("fail"), 400);
    let json2 = serde_json::to_string(&r).unwrap();
    let back2: QueryResource<String, QueryError> = serde_json::from_str(&json2).unwrap();
    assert_eq!(back2.status(), QueryStatus::Failure);
    assert!(back2.error().is_some());
    assert!(back2.signal().is_none());
}

// --- RequestSequencer: IDs always monotonically increasing ----------------

#[test]
fn prop_request_sequencer_monotonic_within_scope() {
    let mut seq = RequestSequencer::new();
    let mut prev = seq.next_request();
    for _ in 0..1000 {
        let curr = seq.next_request();
        assert!(
            curr > prev,
            "RequestIds must be monotonically increasing: {:?} <= {:?}",
            prev, curr
        );
        prev = curr;
    }
}

#[test]
fn prop_request_sequencer_scope_advance_preserves_monotonicity() {
    // Force the sequencer to the edge of overflow and verify monotonicity
    // across the scope transition.
    let mut seq = RequestSequencer {
        scope_id: 1,
        next_request_id: u64::MAX - 5,
    };
    let mut prev = seq.next_request();
    for i in 0..20 {
        let curr = seq.next_request();
        assert!(
            curr > prev,
            "monotonicity broken at iteration {}: {:?} <= {:?}",
            i, prev, curr
        );
        prev = curr;
    }
    // After wrapping through u64::MAX, the scope should have advanced.
    assert!(seq.scope_id >= 2, "scope should have advanced past overflow");
}

#[test]
fn prop_request_sequencer_uniqueness_across_many_ids() {
    use std::collections::HashSet;
    let mut seq = RequestSequencer::new();
    let mut seen = HashSet::new();
    for _ in 0..10_000 {
        let id = seq.next_request();
        assert!(
            seen.insert(id),
            "duplicate RequestId generated: {:?}", id
        );
    }
}

#[test]
fn prop_request_sequencer_two_sequencers_no_collision() {
    let mut seq1 = RequestSequencer::new();
    let mut seq2 = RequestSequencer::new();
    // Different sequencers should produce different scope IDs or sequences,
    // so their first IDs should differ.
    // Both start at scope 1, seq 1, so they WILL produce the same first ID.
    // But advancing one should make them diverge.
    let id1_first = seq1.next_request(); // 1:1
    let id2_first = seq2.next_request(); // 1:1 (same scope/seq)
    assert_eq!(id1_first, id2_first, "both start at 1:1");

    // Now advance seq1 more.
    let id1_second = seq1.next_request(); // 1:2
    assert_ne!(id1_second, id2_first, "advanced id should differ from initial");

    // If we create a sequencer that's been advanced, it should produce
    // distinct ids from a fresh one.
    let mut seq3 = RequestSequencer {
        scope_id: 2,
        next_request_id: 1,
    };
    let id3 = seq3.next_request(); // 2:1
    assert_ne!(id3.scope_id(), id1_first.scope_id(), "different scopes");
}

#[test]
fn prop_request_sequencer_double_overflow_wraps_correctly() {
    // Force scope_id to u64::MAX and next_request_id to u64::MAX
    // to trigger double overflow.
    let mut seq = RequestSequencer {
        scope_id: u64::MAX,
        next_request_id: u64::MAX,
    };
    let id_before = seq.next_request(); // u64::MAX:u64::MAX
    assert_eq!(id_before.scope_id(), u64::MAX);
    assert_eq!(id_before.value(), u64::MAX);

    // The sequencer should have advanced scope. After u64::MAX scope,
    // checked_add overflows, so scope wraps to 1.
    // Verify the next id is from the new scope.
    let id_after = seq.next_request();
    // Scope should have wrapped to 1 or been advanced.
    assert!(
        id_after.scope_id() <= 2,
        "scope should wrap after u64::MAX: got {}",
        id_after.scope_id()
    );
    // The ids should still be unique (different scope or sequence).
    assert_ne!(id_before, id_after, "ids must differ across scope wrap");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. State-transition invariant tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn invariant_initial_state_is_consistent() {
    let r = fresh_resource();
    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.data().is_none(), "Idle => data must be None");
    assert!(r.error().is_none(), "Idle => error must be None");
    assert!(r.active_request_id().is_none());
}

#[test]
fn invariant_after_begin_loading_empty() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.data().is_none(), "LoadingEmpty => data must be None");
    assert!(r.error().is_none(), "begin_request clears error");
    assert!(r.active_request_id().is_some());
    assert!(r.signal().is_some());
}

#[test]
fn invariant_after_begin_loading_with_data() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    // First fetch succeeds to get data.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid1, "data1", 200);

    // Second fetch: resource has data, so status should be LoadingWithData.
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    // Data should still be present during refetch (optimistic).
    assert!(r.data().is_some(), "LoadingWithData => data should still be present");
    assert_eq!(r.data(), Some(&"data1"), "data preserved during refetch");
    assert!(r.error().is_none(), "begin_request clears error");
    assert_eq!(r.active_request_id(), Some(rid2));

    // Complete the refetch — old data should become previous_data.
    r.complete_current_success(rid2, "data2", 400);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data2"));
    assert_eq!(r.previous_data(), Some(&"data1"));
}

#[test]
fn invariant_after_complete_success() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "result", 200);

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"result"), "Success => data must be Some");
    assert!(r.error().is_none(), "Success => error must be None");
    assert!(r.active_request_id().is_none(), "completed => no active request");
    assert!(r.signal().is_some()); // signal remains but request is done
}

#[test]
fn invariant_after_complete_failure() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_failure(rid, QueryError::response("fail"), 200);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert!(r.data().is_none(), "Failure from LoadingEmpty => data must be None");
    assert!(r.error().is_some(), "Failure => error must be Some");
    assert!(r.active_request_id().is_none(), "completed => no active request");
}

#[test]
fn invariant_after_complete_failure_from_loading_with_data() {
    // When a refetch fails (apply_failure), data is RETAINED (not cleared).
    // apply_failure only sets status=Failure and error; it does NOT touch data.
    // This matches TanStack Query behavior where a failed refetch keeps stale data.
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    // First fetch succeeds.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid1, "original", 200);

    // Second fetch fails (refetch).
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    r.complete_current_failure(rid2, QueryError::transport("timeout"), 400);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert!(r.error().is_some(), "Failure => error must be Some");
    assert!(r.active_request_id().is_none());
    // Key invariant: apply_failure does NOT clear data or set previous_data.
    // The data from before the refetch is retained in-place.
    assert_eq!(r.data(), Some(&"original"), "apply_failure retains data in-place");
    assert!(r.previous_data().is_none(), "apply_failure does NOT set previous_data");
}

#[test]
fn invariant_after_cancel_from_loading_empty() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    r.begin_request(&mut s, 100, QueryFetchMode::Normal);

    let cancelled = r.cancel(QueryError::cancelled("abort"));
    assert!(cancelled, "cancel should return true when request is active");
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.data().is_none(), "Cancelled from LoadingEmpty => data must be None");
    assert!(r.error().is_some(), "Cancelled => error must be Some");
    assert!(r.active_request_id().is_none(), "cancelled => no active request");
}

#[test]
fn invariant_after_cancel_from_loading_with_data() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    // First fetch succeeds.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid1, "data", 200);

    // Start a refetch, then cancel.
    r.begin_request(&mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert_eq!(r.data(), Some(&"data"));

    let cancelled = r.cancel(QueryError::cancelled("abort"));
    assert!(cancelled);
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.data().is_none(), "cancel clears data (saved to previous_data)");
    assert!(r.error().is_some());
    assert_eq!(r.previous_data(), Some(&"data"), "cancel saves data to previous_data for rollback");
}

#[test]
fn invariant_cancel_returns_false_when_no_active_request() {
    let mut r = fresh_resource();
    assert!(!r.cancel(QueryError::cancelled("noop")));
    assert_eq!(r.status(), QueryStatus::Idle);
}

#[test]
fn invariant_after_reset() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "data", 200);
    r.set_placeholder_data(Some("placeholder"));
    r.increment_retry();

    r.reset();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.data().is_none(), "reset clears data");
    assert!(r.error().is_none(), "reset clears error");
    assert!(r.active_request_id().is_none());
    assert!(r.signal().is_none());
    assert!(r.placeholder_data().is_none());
    assert!(r.previous_data().is_none());
    assert!(r.initial_data().is_none());
    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.retry_count(), 0);
    // Policies are preserved.
    assert_eq!(r.cache_policy(), CachePolicy::NoCache);
    assert_eq!(r.request_policy(), RequestPolicy::LatestWins);
}

#[test]
fn invariant_complete_success_optional_none_yields_idle() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    // complete_success_optional with None should yield Idle, not Success.
    let guard = r.accept_current_request(rid).unwrap();
    r.complete_success_optional(guard, None, 200);

    assert_eq!(r.status(), QueryStatus::Idle, "None data => Idle (not Success)");
    assert!(r.data().is_none(), "Idle => data must be None");
    assert!(r.error().is_none());
}

#[test]
fn invariant_complete_success_optional_some_yields_success() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let guard = r.accept_current_request(rid).unwrap();
    r.complete_success_optional(guard, Some("data"), 200);

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
}

#[test]
fn invariant_complete_failure_with_data() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let guard = r.accept_current_request(rid).unwrap();
    r.complete_failure_with_data(guard, "fallback", QueryError::response("partial"), 200);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"fallback"), "Failure with data => data must be Some");
    assert!(r.error().is_some(), "Failure => error must be Some");
}

#[test]
fn invariant_stale_accept_rejected() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    // Start a second request (replaces the first under LatestWins).
    let rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);
    // rid1 is now stale. accept_current_request should return None.
    assert!(r.accept_current_request(rid1).is_none(), "stale request should be rejected");
    assert_eq!(r.ignored_results(), 1);

    // rid2 is current. accept_current_request should succeed.
    assert!(r.accept_current_request(rid2).is_some(), "current request should be accepted");
}

// --- Table-driven: all transitions from each starting state ---------------

/// Enumerate all possible state transitions and verify invariants.
#[test]
fn table_driven_all_transitions_from_idle() {
    let mut r = fresh_resource();
    assert_eq!(r.status(), QueryStatus::Idle);

    // Transition: Idle -> LoadingEmpty (begin_request)
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.data().is_none());

    // Transition: LoadingEmpty -> Success (complete_success)
    r.complete_current_success(rid, "data", 200);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
    assert!(r.error().is_none());

    // Transition: Success -> LoadingWithData (begin_request again)
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert_eq!(r.data(), Some(&"data"), "LoadingWithData preserves data");

    // Transition: LoadingWithData -> Failure (complete_failure)
    r.complete_current_failure(rid2, QueryError::response("fail"), 400);
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"data"), "apply_failure retains data in-place");
    assert!(r.previous_data().is_none(), "apply_failure does NOT set previous_data");

    // Transition: Failure -> LoadingWithData (begin_request when data is present)
    let _rid3 = begin_request_id(&mut r, &mut s, 500, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData, "data present => LoadingWithData even after Failure");
    assert!(r.error().is_none(), "begin_request clears error");

    // Transition: LoadingEmpty -> Cancelled (cancel)
    r.cancel(QueryError::cancelled("abort"));
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.data().is_none());
    assert!(r.error().is_some());

    // Transition: Cancelled -> Idle (reset)
    r.reset();
    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.data().is_none());
    assert!(r.error().is_none());
}

#[test]
fn table_driven_cancel_from_every_loading_state() {
    // Cancel from LoadingEmpty.
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        r.begin_request(&mut s, 100, QueryFetchMode::Normal);
        assert_eq!(r.status(), QueryStatus::LoadingEmpty);
        r.cancel(QueryError::cancelled("abort"));
        assert_eq!(r.status(), QueryStatus::Cancelled);
        assert!(r.data().is_none());
        assert!(r.error().is_some());
    }

    // Cancel from LoadingWithData.
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid, "data", 200);
        r.begin_request(&mut s, 300, QueryFetchMode::Normal);
        assert_eq!(r.status(), QueryStatus::LoadingWithData);
        r.cancel(QueryError::cancelled("abort"));
        assert_eq!(r.status(), QueryStatus::Cancelled);
        assert!(r.data().is_none(), "cancel clears data (saves to previous_data)");
        assert_eq!(r.previous_data(), Some(&"data"));
    }
}

#[test]
fn table_driven_rollback_from_every_state() {
    // rollback_to_previous only works when previous_data is set.
    // It sets status to Success and restores data.

    // From Success with previous_data (after a second successful fetch).
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid1, "v1", 200);
        let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
        r.complete_current_success(rid2, "v2", 400);
        assert_eq!(r.previous_data(), Some(&"v1"));

        let rolled_back = r.rollback_to_previous();
        assert!(rolled_back);
        assert_eq!(r.status(), QueryStatus::Success, "rollback sets Success");
        assert_eq!(r.data(), Some(&"v1"), "rollback restores previous data");
    }

    // From Cancelled with previous_data (cancel saves to previous_data).
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid1, "v1", 200);
        r.begin_request(&mut s, 300, QueryFetchMode::Normal);
        r.cancel(QueryError::cancelled("abort"));
        assert_eq!(r.previous_data(), Some(&"v1"));

        let rolled_back = r.rollback_to_previous();
        assert!(rolled_back);
        assert_eq!(r.status(), QueryStatus::Success);
        assert_eq!(r.data(), Some(&"v1"));
    }

    // From Failure with data retained (apply_failure does NOT clear data or set previous_data).
    // To have previous_data from a failure scenario, we need to use the optimistic update path.
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid1, "v1", 200);
        // Optimistic update sets previous_data.
        r.set_data("v2_optimistic");
        assert_eq!(r.data(), Some(&"v2_optimistic"));
        assert_eq!(r.previous_data(), Some(&"v1"));

        // Now rollback.
        let rolled_back = r.rollback_to_previous();
        assert!(rolled_back);
        assert_eq!(r.status(), QueryStatus::Success);
        assert_eq!(r.data(), Some(&"v1"));
    }

    // From Idle with no previous_data => rollback returns false.
    {
        let mut r = fresh_resource();
        assert!(!r.rollback_to_previous(), "no previous_data => rollback fails");
        assert_eq!(r.status(), QueryStatus::Idle);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Deterministic GC eviction tests (integration layer)
// ═══════════════════════════════════════════════════════════════════════════
//
// These tests use #[gpui::test] because they exercise QueryClient, which
// requires a GPUI AppContext. They only need the client layer, not hooks.

mod gc_tests {
    use gpui::{BorrowAppContext as _, TestAppContext};
    use crate::client::QueryClient;
    use crate::core::*;
    use crate::tests::test_support::*;

    /// Helper: create a client with GC and populate a success resource with a
    /// snapshot at a known timestamp. Returns the key used.
    fn create_success_with_snapshot(
        client: &mut QueryClient,
        cx: &mut gpui::App,
        key: &str,
        data: &str,
        success_time_ms: u128,
        _gc_time_ms: u64,
    ) {
        let key = QueryKey::from(key);
        let prepared = client
            .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
            .expect("should start");
        prepared.complete_success(data.to_string(), cx);

        client.update_query_snapshot::<String, QueryError>(
            &key,
            QueryStatus::Success,
            Some(success_time_ms),
            CachePolicy::Ttl { ttl_ms: 60_000 },
        );
    }

    #[gpui::test]
    fn test_gc_evicts_exactly_expired_resources(cx: &mut TestAppContext) {
        // gc_time=1000ms. Success threshold = 2*1000 = 2000ms.
        // Create 3 resources with different snapshot ages:
        // - "young": snapshot at t=2000, GC at t=2500 => age=500 < 2000 => preserved
        // - "middle": snapshot at t=1000, GC at t=2500 => age=1500 < 2000 => preserved
        // - "old": snapshot at t=100, GC at t=2500 => age=2400 > 2000 => evicted
        setup_query_client_with_gc(cx, 1_000);
        cx.update(|cx| {
            cx.update_global::<QueryClient, _>(|client, cx| {
                create_success_with_snapshot(client, cx, "young", "young_data", 2_000, 1_000);
                create_success_with_snapshot(client, cx, "middle", "middle_data", 1_000, 1_000);
                create_success_with_snapshot(client, cx, "old", "old_data", 100, 1_000);

                assert_eq!(client.all_queries::<String, QueryError>().len(), 3);

                client.gc_with_time(2_500, cx);

                // "young" and "middle" should survive; "old" should be evicted.
                assert_eq!(
                    client.all_queries::<String, QueryError>().len(),
                    2,
                    "exactly 1 of 3 resources should be evicted"
                );
                assert!(
                    client.query::<String, QueryError>(&QueryKey::from("young")).is_some(),
                    "young (age 500ms) should survive"
                );
                assert!(
                    client.query::<String, QueryError>(&QueryKey::from("middle")).is_some(),
                    "middle (age 1500ms) should survive"
                );
                assert!(
                    client.query::<String, QueryError>(&QueryKey::from("old")).is_none(),
                    "old (age 2400ms > success_threshold 2000ms) should be evicted"
                );
            });
        });
    }

    #[gpui::test]
    fn test_gc_eviction_counts_match(cx: &mut TestAppContext) {
        setup_query_client_with_gc(cx, 1_000);
        cx.update(|cx| {
            cx.update_global::<QueryClient, _>(|client, cx| {
                // Create 5 idle resources with no snapshot => all evicted.
                for i in 0..5 {
                    let _ = client.resource::<String, QueryError>(
                        format!("idle_{}", i),
                        cx,
                    );
                }
                assert_eq!(client.all_queries::<String, QueryError>().len(), 5);

                client.gc_with_time(5_000, cx);

                assert_eq!(
                    client.all_queries::<String, QueryError>().len(),
                    0,
                    "all 5 idle resources with no snapshot should be evicted"
                );
            });
        });
    }

    #[gpui::test]
    fn test_gc_preserves_loading_resource_with_snapshot(cx: &mut TestAppContext) {
        setup_query_client_with_gc(cx, 1_000);
        cx.update(|cx| {
            cx.update_global::<QueryClient, _>(|client, cx| {
                let key = QueryKey::from("loading_preserved");
                let prepared = client
                    .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                    .expect("should start");
                // Don't complete — leave in Loading state.

                client.update_query_snapshot::<String, QueryError>(
                    &key,
                    QueryStatus::LoadingEmpty,
                    Some(0),
                    CachePolicy::Ttl { ttl_ms: 5_000 },
                );

                // GC at t=1_000_000 — Loading resources are never evicted.
                client.gc_with_time(1_000_000, cx);

                let entity = client
                    .query::<String, QueryError>(&key)
                    .expect("loading resource must survive GC");

                // Now complete the fetch to verify the entity is still usable.
                prepared.complete_success("data".to_string(), cx);
                assert_eq!(
                    entity.read(cx).data(),
                    Some(&"data".to_string()),
                    "entity should be usable after surviving GC"
                );
            });
        });
    }

    #[gpui::test]
    fn test_gc_mixed_states_precise_eviction(cx: &mut TestAppContext) {
        // Create resources in various states and verify exact eviction counts.
        // gc_time=1000ms. Idle threshold=1000ms, Success threshold=2000ms.
        //
        // Use separate type buckets to avoid interactions between resources
        // sharing the same bucket during snapshot updates.
        setup_query_client_with_gc(cx, 1_000);
        cx.update(|cx| {
            cx.update_global::<QueryClient, _>(|client, cx| {
                // Loading (with snapshot at t=0) => preserved (loading never evicted).
                let prepared = client
                    .prepare_fetch_query::<String, QueryError>("loading", cx)
                    .expect("should start");
                client.update_query_snapshot::<String, QueryError>(
                    &QueryKey::from("loading"),
                    QueryStatus::LoadingEmpty,
                    Some(0),
                    CachePolicy::Ttl { ttl_ms: 5_000 },
                );

                // Success (snapshot at t=1000, GC at t=2500 => age=1500 < 2000) => preserved.
                create_success_with_snapshot(client, cx, "success_fresh", "data", 1_000, 1_000);

                // Success (snapshot at t=0, GC at t=2500 => age=2500 > 2000) => evicted.
                create_success_with_snapshot(client, cx, "success_old", "data", 0, 1_000);

                assert_eq!(client.all_queries::<String, QueryError>().len(), 3);

                client.gc_with_time(2_500, cx);

                // loading + success_fresh = 2 preserved.
                // success_old = 1 evicted.
                let remaining = client.all_queries::<String, QueryError>();
                assert_eq!(
                    remaining.len(),
                    2,
                    "exactly 1 of 3 resources should be evicted"
                );

                let remaining_keys: Vec<String> = remaining
                    .iter()
                    .map(|e| e.read(cx).key().to_path())
                    .collect();
                assert!(
                    remaining_keys.contains(&"loading".to_string()),
                    "loading should survive: {:?}",
                    remaining_keys
                );
                assert!(
                    remaining_keys.contains(&"success_fresh".to_string()),
                    "success_fresh should survive: {:?}",
                    remaining_keys
                );

                // Clean up: complete the loading fetch.
                prepared.complete_success("data".to_string(), cx);
            });
        });
    }

    #[gpui::test]
    fn test_gc_survive_then_evict_after_threshold_crossed(cx: &mut TestAppContext) {
        // Same resource survives GC at time T1, then gets evicted at T2.
        setup_query_client_with_gc(cx, 1_000);
        cx.update(|cx| {
            cx.update_global::<QueryClient, _>(|client, cx| {
                let key = QueryKey::from("aged");
                create_success_with_snapshot(client, cx, "aged", "data", 1_000, 1_000);

                // GC at t=2000: age=1000 < success_threshold(2000) => preserved.
                client.gc_with_time(2_000, cx);
                assert!(
                    client.query::<String, QueryError>(&key).is_some(),
                    "age=1000ms < success_threshold=2000ms => should survive"
                );

                // GC at t=3500: age=2500 > success_threshold(2000) => evicted.
                client.gc_with_time(3_500, cx);
                assert!(
                    client.query::<String, QueryError>(&key).is_none(),
                    "age=2500ms > success_threshold=2000ms => should be evicted"
                );
            });
        });
    }

    #[gpui::test]
    fn test_gc_boundary_success_threshold_exact(cx: &mut TestAppContext) {
        // Test the exact boundary: age == success_threshold.
        // gc_time=1000 => success_threshold=2000.
        // Snapshot at t=1000, GC at t=3000 => age=2000 == success_threshold.
        //
        // GC uses `age_ms < success_threshold` to retain (line ~437 in bucket.rs).
        // When age == threshold, the condition is false → evicted (>= semantics).
        setup_query_client_with_gc(cx, 1_000);
        cx.update(|cx| {
            cx.update_global::<QueryClient, _>(|client, cx| {
                let key = QueryKey::from("boundary");
                create_success_with_snapshot(client, cx, "boundary", "data", 1_000, 1_000);

                // GC at t=3000: age=3000-1000=2000 == success_threshold => evicted.
                client.gc_with_time(3_000, cx);
                assert!(
                    client.query::<String, QueryError>(&key).is_none(),
                    "age=2000ms == success_threshold=2000ms => must be evicted (>= boundary)"
                );
            });
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Concurrency / two-phase completion protocol tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn two_phase_protocol_accept_then_complete_is_consistent() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);

    // Phase 1: accept
    let guard = r.accept_current_request(rid).expect("should accept current request");
    assert!(r.active_request_id().is_none(), "accept clears active_request_id");

    // Phase 2: complete with success
    r.complete_success(guard, "result", 200);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"result"));
}

#[test]
fn two_phase_stale_accept_then_complete_does_not_corrupt() {
    // Begin two requests, try to complete the first (stale) — it should be
    // rejected, and the second should complete successfully.
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);

    // rid1 is stale. complete_current_success should return false.
    assert!(!r.complete_current_success(rid1, "stale_data", 300));
    assert_eq!(r.ignored_results(), 1);

    // rid2 is current. complete_current_success should return true.
    assert!(r.complete_current_success(rid2, "fresh_data", 400));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"fresh_data"));
}

#[test]
fn concurrent_replacements_increment_cancelled_count() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    // Each replacement increments cancelled_count.
    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 0);
    let _ = r.begin_request(&mut s, 200, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 1);
    let _ = r.begin_request(&mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 2);
    let _ = r.begin_request(&mut s, 400, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 3);
}

#[test]
fn ignore_while_loading_rejects_concurrent_requests() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "ignore-test",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut s = test_sequencer();

    // First request starts.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.active_request_id(), Some(rid1));

    // Second request is ignored.
    let result = r.begin_request(&mut s, 200, QueryFetchMode::Normal);
    match result {
        QueryBeginResult::IgnoredWhileLoading { active_request_id } => {
            assert_eq!(active_request_id, rid1);
        }
        _ => panic!("expected IgnoredWhileLoading, got {:?}", result),
    }
    assert_eq!(r.active_request_id(), Some(rid1), "active request should not change");
    assert_eq!(r.cancelled_count(), 0, "no cancellation on ignore");

    // Complete the first request.
    r.complete_current_success(rid1, "data", 300);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
}

#[test]
fn signal_cancelled_on_replacement() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let signal1 = r.signal().unwrap().clone();
    assert!(!signal1.is_cancelled());

    // Replace the request — the old signal should be cancelled.
    let _ = r.begin_request(&mut s, 200, QueryFetchMode::Normal);
    assert!(signal1.is_cancelled(), "old signal should be cancelled on replacement");
    let signal2 = r.signal().unwrap().clone();
    assert!(!signal2.is_cancelled(), "new signal should not be cancelled");
    assert_ne!(signal1, signal2, "signals should be different");
}

#[test]
fn signal_cancelled_on_explicit_cancel() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let signal = r.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    r.cancel(QueryError::cancelled("abort"));
    assert!(signal.is_cancelled(), "signal should be cancelled after explicit cancel");
}

#[test]
fn signal_cancelled_on_reset() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let signal = r.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    r.reset();
    assert!(signal.is_cancelled(), "signal should be cancelled on reset");
    assert!(r.signal().is_none(), "no signal after reset");
}

#[test]
fn display_data_falls_back_to_placeholder() {
    let mut r = fresh_resource();
    assert!(r.display_data().is_none());

    r.set_placeholder_data(Some("placeholder"));
    assert_eq!(r.display_data(), Some(&"placeholder"));

    // When data is present, data takes priority.
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "real_data", 200);
    assert_eq!(r.display_data(), Some(&"real_data"), "data takes priority over placeholder");
}

#[test]
fn initial_data_seeded_when_idle() {
    let mut r = fresh_resource();
    r.set_initial_data("seeded", 500);
    assert_eq!(r.data(), Some(&"seeded"), "initial data should populate data");
    assert_eq!(r.last_updated_at_ms(), Some(500));
    assert!(r.initial_data().is_some());

    // Seeding again while not Idle+None should be a no-op.
    r.set_initial_data("ignored", 600);
    assert_eq!(r.data(), Some(&"seeded"), "second seed should be ignored");
}

#[test]
fn initial_data_cleared_on_reset() {
    let mut r = fresh_resource();
    r.set_initial_data("seeded", 500);
    assert!(r.initial_data().is_some());
    r.reset();
    assert!(r.initial_data().is_none(), "initial_data cleared on reset");
    assert!(r.data().is_none(), "data cleared on reset");
}

#[test]
fn is_data_stale_heuristic() {
    let mut r = fresh_resource();
    assert!(!r.is_data_stale(), "no data => not stale");

    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "data", 200);
    assert!(!r.is_data_stale(), "Success with data => not stale");

    // Start a refetch — data is stale (LoadingWithData).
    r.begin_request(&mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert!(r.is_data_stale(), "LoadingWithData with data => stale");

    // Complete with failure — data still stale.
    let rid2 = begin_request_id(&mut r, &mut s, 400, QueryFetchMode::Normal);
    r.complete_current_failure_with_data(rid2, "fallback", QueryError::response("err"), 500);
    assert_eq!(r.status(), QueryStatus::Failure);
    assert!(r.is_data_stale(), "Failure with data => stale");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. GAP-03: begin_request_with_id + SWR + IgnoreWhileLoading + active request
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
// 6. GAP-04: complete_current_optional_success rejects stale request ID
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
// 7. GAP-05: complete_current_failure_with_data rejects stale request ID
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complete_current_failure_with_data_rejects_stale_id() {
    let mut r = test_resource();
    let mut s = test_sequencer();

    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let _rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);

    assert!(
        !r.complete_current_failure_with_data(
            rid1,
            "fallback",
            QueryError::response("stale"),
            300
        ),
        "stale ID should be rejected"
    );
    assert_eq!(r.ignored_results(), 1);

    // The current request is still active
    assert!(r.active_request_id().is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. GAP-06: Force mode respects IgnoreWhileLoading
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
// 9. GAP-12: QueryError::sanitized() with mongodb connection string
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
// 10. GAP-13: QueryError::sanitized() with empty string message
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_error_sanitized_empty_message() {
    let err = QueryError::response("");
    let clean = err.sanitized();
    assert_eq!(clean.message(), "");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. GAP-14: QueryError::new() with explicit kind
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_error_new_with_explicit_kind() {
    let err = QueryError::new(QueryErrorKind::Transport, "timeout");
    assert_eq!(err.kind(), QueryErrorKind::Transport);
    assert_eq!(err.message(), "timeout");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. GAP-15: record_cache_hit does not clear Cancelled status
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
// 13. GAP-16: QueryKey::join() appends segments
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
// 14. GAP-17: QueryKey::from(Vec<String>)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn from_vec_string() {
    let key = QueryKey::from(vec!["users".to_string(), "42".to_string()]);
    assert_eq!(key.parts().len(), 2);
    assert_eq!(key.to_path(), "users::42");
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. GAP-18: QueryKey Deref to [Arc<str>] allows indexing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deref_allows_indexing() {
    let key = QueryKey::from(["a", "b", "c"]);
    assert_eq!(&*key[0], "a");
    assert_eq!(&*key[2], "c");
    assert_eq!(key.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. GAP-19: QueryKey serde deserialize from single string
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_deserialize_single_string() {
    let json = "\"users\"";
    let key: QueryKey = serde_json::from_str(json).unwrap();
    assert_eq!(key.parts().len(), 1);
    assert_eq!(key.as_str(), "users");
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. GAP-20: QueryKey Hash consistency
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
// 18. GAP-07: InfiniteQuery begin_fetch_previous with IgnoreWhileLoading
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
    assert!(id2.is_none(), "second begin_fetch_previous should be ignored");
    assert_eq!(r.cancelled_count(), 0, "no cancellation on ignore");
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. GAP-08: Cross-direction IgnoreWhileLoading (next then previous)
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
// 20. GAP-09: InfiniteQueryResource reset preserves retry_policy
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
// 21. GAP-10: Bidirectional resource initial accessors
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
// 22. GAP-11: prepend with has_more=true preserves has_previous_page
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
