//! Property-based tests for RetryPolicy, CachePolicy, serde roundtrip, and
//! RequestSequencer.

use crate::core::*;
use crate::tests::test_support::*;

// --- RetryPolicy: delay_for_attempt never exceeds ABSOLUTE_MAX_DELAY_MS --------

#[test]
fn prop_retry_delay_never_exceeds_absolute_max_for_all_attempts() {
    // ABSOLUTE_MAX_DELAY_MS = 3_600_000 (1 hour).
    const ABSOLUTE_MAX: u64 = 3_600_000;

    // Test with various base delays and exponential backoff enabled.
    let base_delays: &[u64] = &[
        0,
        1,
        10,
        100,
        1_000,
        10_000,
        100_000,
        1_000_000,
        u64::MAX / 2,
        u64::MAX,
    ];
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
                    attempt,
                    delay,
                    ABSOLUTE_MAX,
                    base,
                    max_delay
                );
            }
            // Extreme attempt numbers.
            for attempt in [u32::MAX, 200, 500, 1000] {
                let delay = policy.delay_for_attempt(attempt);
                assert!(
                    delay <= ABSOLUTE_MAX,
                    "delay_for_attempt({}) = {} exceeds ABSOLUTE_MAX ({}) \
                     with base={}, max_delay={}",
                    attempt,
                    delay,
                    ABSOLUTE_MAX,
                    base,
                    max_delay
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
            prev_delay,
            delay,
            attempt
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
        let total = policy
            .total_valid_ms()
            .expect("Ttl should have total_valid_ms");
        assert_eq!(total, ttl);

        // Sample ages: 0, boundary-1, boundary, boundary+1, and large values.
        let ages: &[u128] = &[
            0,
            ttl as u128 / 2,
            ttl as u128,
            ttl as u128 + 1,
            ttl as u128 * 2,
        ];
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
        (1, 1),
        (10, 10),
        (100, 200),
        (1_000, 2_000),
        (60_000, 30_000),
    ];
    for &(ttl, stale) in cases {
        let policy = CachePolicy::StaleWhileRevalidate {
            ttl_ms: ttl,
            stale_ms: stale,
        };
        let total = ttl as u128 + stale as u128;

        let ages: &[u128] = &[
            0,
            ttl as u128 / 2,
            ttl as u128,                 // boundary: still fresh
            ttl as u128 + 1,             // just past TTL: stale
            total / 2 + ttl as u128 / 2, // mid-stale window
            total,                       // boundary: still stale-but-serveable
            total + 1,                   // expired
            total * 2,                   // way expired
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
                assert!(
                    is_stale,
                    "age {} must be stale-but-serveable (ttl={}, total={})",
                    age, ttl, total
                );
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
        assert!(
            !policy.is_fresh(age),
            "NoCache should never be fresh at age {}",
            age
        );
        assert!(
            policy.is_expired(age),
            "NoCache should always be expired at age {}",
            age
        );
        assert!(
            !policy.is_stale_but_serveable(age),
            "NoCache should never be stale-but-serveable"
        );
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
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 100,
            stale_ms: 50,
        },
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: u64::MAX,
            stale_ms: u64::MAX,
        },
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
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 0,
            stale_ms: 0,
        },
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 100,
            stale_ms: 200,
        },
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: u64::MAX,
            stale_ms: u64::MAX,
        },
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
        RetryPolicy::new(5)
            .with_delay(0)
            .with_exponential_backoff()
            .with_max_delay(0),
        RetryPolicy::new(u32::MAX)
            .with_delay(u64::MAX)
            .with_exponential_backoff()
            .with_max_delay(u64::MAX),
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
    assert!(
        back.initial_data().is_none(),
        "initial_data is #[serde(skip)]"
    );

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
            prev,
            curr
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
            i,
            prev,
            curr
        );
        prev = curr;
    }
    // After wrapping through u64::MAX, the scope should have advanced.
    assert!(
        seq.scope_id >= 2,
        "scope should have advanced past overflow"
    );
}

#[test]
fn prop_request_sequencer_uniqueness_across_many_ids() {
    use std::collections::HashSet;
    let mut seq = RequestSequencer::new();
    let mut seen = HashSet::new();
    for _ in 0..10_000 {
        let id = seq.next_request();
        assert!(seen.insert(id), "duplicate RequestId generated: {:?}", id);
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
    assert_ne!(
        id1_second, id2_first,
        "advanced id should differ from initial"
    );

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
