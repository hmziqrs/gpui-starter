//! Property-based tests for CachePolicy and RetryPolicy.
//!
//! Uses proptest to verify structural properties hold for all possible inputs,
//! including edge cases like u64::MAX, zero values, and overflow scenarios.

use proptest::prelude::*;

use crate::core::*;

// ── Helpers ─────────────────────────────────────────────────────────────

/// Arbitrary CachePolicy strategy covering all three variants with wide value ranges.
fn arb_cache_policy() -> impl Strategy<Value = CachePolicy> {
    prop_oneof![
        Just(CachePolicy::NoCache),
        (any::<u64>()).prop_map(|ttl_ms| CachePolicy::Ttl { ttl_ms }),
        (any::<u64>(), any::<u64>()).prop_map(|(ttl_ms, stale_ms)| {
            CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms }
        }),
    ]
}

/// Arbitrary RetryPolicy strategy with physically plausible values.
/// Ensures max_retry_delay_ms >= retry_delay_ms so the max-delay cap is meaningful.
fn arb_retry_policy() -> impl Strategy<Value = RetryPolicy> {
    (any::<u32>(), 1u64..=60_000, any::<bool>())
        .prop_flat_map(|(max_retries, retry_delay_ms, exponential_backoff)| {
            let max_delay_lo = retry_delay_ms;
            (
                Just(max_retries),
                Just(retry_delay_ms),
                Just(exponential_backoff),
                max_delay_lo..=3_600_000u64,
            )
        })
        .prop_map(
            |(max_retries, retry_delay_ms, exponential_backoff, max_retry_delay_ms)| RetryPolicy {
                max_retries,
                retry_delay_ms,
                exponential_backoff,
                max_retry_delay_ms,
            },
        )
}

/// RetryPolicy strategy that always has exponential backoff enabled.
fn arb_exponential_retry_policy() -> impl Strategy<Value = RetryPolicy> {
    (any::<u32>(), 1u64..=60_000)
        .prop_flat_map(|(max_retries, retry_delay_ms)| {
            (
                Just(max_retries),
                Just(retry_delay_ms),
                retry_delay_ms..=3_600_000u64,
            )
        })
        .prop_map(
            |(max_retries, retry_delay_ms, max_retry_delay_ms)| RetryPolicy {
                max_retries,
                retry_delay_ms,
                exponential_backoff: true,
                max_retry_delay_ms,
            },
        )
}

/// RetryPolicy strategy that always has linear (non-exponential) backoff.
fn arb_linear_retry_policy() -> impl Strategy<Value = RetryPolicy> {
    (any::<u32>(), 1u64..=60_000)
        .prop_flat_map(|(max_retries, retry_delay_ms)| {
            (
                Just(max_retries),
                Just(retry_delay_ms),
                retry_delay_ms..=3_600_000u64,
            )
        })
        .prop_map(
            |(max_retries, retry_delay_ms, max_retry_delay_ms)| RetryPolicy {
                max_retries,
                retry_delay_ms,
                exponential_backoff: false,
                max_retry_delay_ms,
            },
        )
}

// ── CachePolicy::NoCache invariants ─────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// NoCache: is_fresh() is ALWAYS false regardless of age.
    #[test]
    fn nocache_is_fresh_always_false(age_ms in any::<u128>()) {
        let policy = CachePolicy::NoCache;
        prop_assert!(!policy.is_fresh(age_ms));
    }

    /// NoCache: is_expired() is ALWAYS true regardless of age.
    #[test]
    fn nocache_is_expired_always_true(age_ms in any::<u128>()) {
        let policy = CachePolicy::NoCache;
        prop_assert!(policy.is_expired(age_ms));
    }
}

#[test]
fn nocache_can_short_circuit_always_false() {
    assert!(!CachePolicy::NoCache.can_short_circuit());
}

#[test]
fn nocache_can_serve_stale_always_false() {
    assert!(!CachePolicy::NoCache.can_serve_stale());
}

#[test]
fn nocache_ttl_ms_is_none() {
    assert_eq!(CachePolicy::NoCache.ttl_ms(), None);
}

// ── CachePolicy::Ttl invariants ─────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Ttl: is_fresh(age) == (age <= ttl_ms) for ALL u64/u128 value pairs.
    #[test]
    fn ttl_is_fresh_matches_comparison(ttl_ms in any::<u64>(), age_ms in any::<u128>()) {
        let policy = CachePolicy::Ttl { ttl_ms };
        let expected = age_ms <= ttl_ms as u128;
        prop_assert_eq!(policy.is_fresh(age_ms), expected);
    }

    /// Ttl: can_short_circuit() is ALWAYS true.
    #[test]
    fn ttl_can_short_circuit_always_true(ttl_ms in any::<u64>()) {
        let policy = CachePolicy::Ttl { ttl_ms };
        prop_assert!(policy.can_short_circuit());
    }

    /// Ttl: can_serve_stale() is ALWAYS false (no stale window in pure TTL).
    #[test]
    fn ttl_can_serve_stale_always_false(ttl_ms in any::<u64>()) {
        let policy = CachePolicy::Ttl { ttl_ms };
        prop_assert!(!policy.can_serve_stale());
    }

    /// Ttl: is_stale_but_serveable() is ALWAYS false (no stale window).
    #[test]
    fn ttl_is_stale_but_serveable_always_false(ttl_ms in any::<u64>(), age_ms in any::<u128>()) {
        let policy = CachePolicy::Ttl { ttl_ms };
        prop_assert!(!policy.is_stale_but_serveable(age_ms));
    }

    /// Ttl: total_valid_ms() == Some(ttl_ms).
    #[test]
    fn ttl_total_valid_ms_equals_ttl(ttl_ms in any::<u64>()) {
        let policy = CachePolicy::Ttl { ttl_ms };
        prop_assert_eq!(policy.total_valid_ms(), Some(ttl_ms));
    }
}

// ── CachePolicy::StaleWhileRevalidate invariants ────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// SWR: is_fresh(age) == (age <= ttl_ms).
    #[test]
    fn swr_is_fresh_uses_ttl_only(
        ttl_ms in any::<u64>(),
        stale_ms in any::<u64>(),
        age_ms in any::<u128>(),
    ) {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms };
        let expected = age_ms <= ttl_ms as u128;
        prop_assert_eq!(policy.is_fresh(age_ms), expected);
    }

    /// SWR: can_short_circuit() is ALWAYS true.
    #[test]
    fn swr_can_short_circuit_always_true(ttl_ms in any::<u64>(), stale_ms in any::<u64>()) {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms };
        prop_assert!(policy.can_short_circuit());
    }

    /// SWR: can_serve_stale() is ALWAYS true.
    #[test]
    fn swr_can_serve_stale_always_true(ttl_ms in any::<u64>(), stale_ms in any::<u64>()) {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms };
        prop_assert!(policy.can_serve_stale());
    }

    /// SWR: is_stale_but_serveable(age) is true exactly when
    /// age > ttl_ms AND age <= ttl_ms + stale_ms.
    #[test]
    fn swr_stale_serveable_window(
        ttl_ms in any::<u64>(),
        stale_ms in any::<u64>(),
        age_ms in any::<u128>(),
    ) {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms };
        let ttl_128 = ttl_ms as u128;
        let stale_128 = stale_ms as u128;
        let total = ttl_128 + stale_128;
        let expected = age_ms > ttl_128 && age_ms <= total;
        prop_assert_eq!(policy.is_stale_but_serveable(age_ms), expected);
    }

    /// SWR: total_valid_ms() saturates at u64::MAX on overflow.
    #[test]
    fn swr_total_valid_ms_saturates(ttl_ms in any::<u64>(), stale_ms in any::<u64>()) {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms, stale_ms };
        let expected = ttl_ms.saturating_add(stale_ms);
        prop_assert_eq!(policy.total_valid_ms(), Some(expected));
    }
}

// ── CachePolicy cross-variant invariants ────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// For any CachePolicy, data that is fresh is never expired.
    #[test]
    fn fresh_implies_not_expired(policy in arb_cache_policy(), age_ms in any::<u128>()) {
        if policy.is_fresh(age_ms) {
            prop_assert!(!policy.is_expired(age_ms));
        }
    }

    /// For any CachePolicy, data that is stale-but-serveable is never expired.
    #[test]
    fn stale_serveable_implies_not_expired(policy in arb_cache_policy(), age_ms in any::<u128>()) {
        if policy.is_stale_but_serveable(age_ms) {
            prop_assert!(!policy.is_expired(age_ms));
        }
    }

    /// For any CachePolicy, data is either fresh, stale-but-serveable, or expired.
    #[test]
    fn fresh_stale_expired_covers_all_states(
        policy in arb_cache_policy(),
        age_ms in any::<u128>(),
    ) {
        let fresh = policy.is_fresh(age_ms);
        let stale = policy.is_stale_but_serveable(age_ms);
        let expired = policy.is_expired(age_ms);
        prop_assert!(fresh || stale || expired);
        prop_assert!(!(fresh && stale));
        prop_assert!(!(fresh && expired));
        prop_assert!(!(stale && expired));
    }
}

// ── RetryPolicy invariants ─────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// should_retry(n) == (n < max_retries) for all valid n.
    #[test]
    fn should_retry_matches_count(
        policy in arb_retry_policy(),
        current_retries in any::<u32>(),
    ) {
        let expected = current_retries < policy.max_retries;
        prop_assert_eq!(policy.should_retry(current_retries), expected);
    }

    /// delay_for_attempt(0) always equals retry_delay_ms.
    #[test]
    fn delay_for_attempt_zero_is_base_delay(policy in arb_retry_policy()) {
        prop_assert_eq!(policy.delay_for_attempt(0), policy.retry_delay_ms);
    }

    /// delay_for_attempt never exceeds max_retry_delay_ms.
    #[test]
    fn delay_never_exceeds_max(policy in arb_retry_policy(), attempt in any::<u32>()) {
        let delay = policy.delay_for_attempt(attempt);
        prop_assert!(
            delay <= policy.max_retry_delay_ms,
            "delay {} exceeded max_retry_delay_ms {} at attempt {}",
            delay,
            policy.max_retry_delay_ms,
            attempt,
        );
    }

    /// delay_for_attempt never exceeds the absolute ceiling of 1 hour.
    #[test]
    fn delay_never_exceeds_absolute_ceiling(
        policy in arb_retry_policy(),
        attempt in any::<u32>(),
    ) {
        let delay = policy.delay_for_attempt(attempt);
        prop_assert!(
            delay <= 3_600_000,
            "delay {} exceeded 1-hour absolute ceiling at attempt {}",
            delay,
            attempt,
        );
    }

    /// With exponential backoff, delays are monotonically non-decreasing.
    #[test]
    fn exponential_delays_monotonically_non_decreasing(policy in arb_exponential_retry_policy()) {
        let mut prev = policy.delay_for_attempt(0);
        for attempt in 1..=62 {
            let cur = policy.delay_for_attempt(attempt);
            prop_assert!(
                cur >= prev,
                "delay decreased at attempt {}: prev={}, cur={}",
                attempt,
                prev,
                cur,
            );
            prev = cur;
        }
    }

    /// Without exponential backoff, delay_for_attempt returns the same value for all attempts.
    #[test]
    fn linear_delays_are_constant(policy in arb_linear_retry_policy(), attempt in any::<u32>()) {
        prop_assert_eq!(policy.delay_for_attempt(attempt), policy.retry_delay_ms);
    }

    /// delay_for_attempt never panics, even at u32::MAX attempt values.
    #[test]
    fn delay_does_not_panic_on_large_attempt(policy in arb_retry_policy()) {
        let _ = policy.delay_for_attempt(u32::MAX);
    }
}

#[test]
fn extreme_exponential_saturates() {
    let policy = RetryPolicy {
        max_retries: 100,
        retry_delay_ms: u64::MAX,
        exponential_backoff: true,
        max_retry_delay_ms: 30_000,
    };
    let delay = policy.delay_for_attempt(10);
    assert_eq!(delay, 30_000, "should saturate at max_retry_delay_ms");
}
