//! Tests for QueryStatus, QueryTimestamp, RequestId, MutationStatus,
//! CachePolicy, RequestPolicy, and QueryFetchMode.

use crate::core::*;

// ═══════════════════════════════════════════════════════════════════════════
// QueryStatus
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_status_default_is_idle() {
    assert_eq!(QueryStatus::default(), QueryStatus::Idle);
}

#[test]
fn query_status_labels() {
    assert_eq!(QueryStatus::Idle.label(), "Idle");
    assert_eq!(QueryStatus::LoadingEmpty.label(), "Loading empty");
    assert_eq!(QueryStatus::LoadingWithData.label(), "Loading with data");
    assert_eq!(QueryStatus::Success.label(), "Success");
    assert_eq!(QueryStatus::Failure.label(), "Failure");
    assert_eq!(QueryStatus::Cancelled.label(), "Cancelled");
}

#[test]
fn query_status_is_loading() {
    assert!(QueryStatus::LoadingEmpty.is_loading());
    assert!(QueryStatus::LoadingWithData.is_loading());
    assert!(!QueryStatus::Idle.is_loading());
    assert!(!QueryStatus::Success.is_loading());
    assert!(!QueryStatus::Failure.is_loading());
    assert!(!QueryStatus::Cancelled.is_loading());
}

#[test]
fn query_status_is_pending() {
    assert!(QueryStatus::Idle.is_pending());
    assert!(QueryStatus::LoadingEmpty.is_pending());
    assert!(!QueryStatus::LoadingWithData.is_pending());
    assert!(!QueryStatus::Success.is_pending());
    assert!(!QueryStatus::Failure.is_pending());
    assert!(!QueryStatus::Cancelled.is_pending());
}

#[test]
fn query_status_serde_roundtrip() {
    for status in [
        QueryStatus::Idle,
        QueryStatus::LoadingEmpty,
        QueryStatus::LoadingWithData,
        QueryStatus::Success,
        QueryStatus::Failure,
        QueryStatus::Cancelled,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let back: QueryStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, status);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// QueryTimestamp
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn query_timestamp_from_millis() {
    let ts = QueryTimestamp::from_millis(1_000);
    assert_eq!(ts.as_millis(), 1_000);
}

#[test]
fn query_timestamp_from_u128() {
    let ts: QueryTimestamp = 5_000u128.into();
    assert_eq!(ts.as_millis(), 5_000);
}

#[test]
fn query_timestamp_zero() {
    let ts = QueryTimestamp::from_millis(0);
    assert_eq!(ts.as_millis(), 0);
}

#[test]
fn query_timestamp_large_value() {
    let ts = QueryTimestamp::from_millis(u128::MAX);
    assert_eq!(ts.as_millis(), u128::MAX);
}

#[test]
fn query_timestamp_ordering() {
    let earlier = QueryTimestamp::from_millis(100);
    let later = QueryTimestamp::from_millis(200);
    assert!(earlier < later);
    assert!(later > earlier);
    assert!(earlier <= later);
    assert!(later >= earlier);
}

#[test]
fn query_timestamp_equality() {
    let a = QueryTimestamp::from_millis(1_000);
    let b = QueryTimestamp::from_millis(1_000);
    let c = QueryTimestamp::from_millis(2_000);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// ═══════════════════════════════════════════════════════════════════════════
// RequestId
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_id_hash_consistency() {
    use std::collections::HashSet;
    let a = RequestId::scoped(1, 10);
    let b = RequestId::scoped(1, 10);
    let c = RequestId::scoped(2, 10);
    let mut set = HashSet::new();
    set.insert(a);
    assert!(set.contains(&b), "equal ids should have equal hashes");
    assert!(!set.contains(&c));
}

#[test]
fn request_id_copy_semantics() {
    let a = RequestId::scoped(5, 10);
    let b = a; // Copy
    assert_eq!(a, b);
}

#[test]
fn request_id_serde_roundtrip() {
    let id = RequestId::scoped(42, 99);
    let json = serde_json::to_string(&id).unwrap();
    let back: RequestId = serde_json::from_str(&json).unwrap();
    assert_eq!(back, id);
}

// ═══════════════════════════════════════════════════════════════════════════
// MutationStatus
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mutation_status_default_is_idle() {
    assert_eq!(MutationStatus::default(), MutationStatus::Idle);
}

#[test]
fn mutation_status_labels() {
    assert_eq!(MutationStatus::Idle.label(), "Idle");
    assert_eq!(MutationStatus::Loading.label(), "Loading");
    assert_eq!(MutationStatus::Success.label(), "Success");
    assert_eq!(MutationStatus::Failure.label(), "Failure");
}

#[test]
fn mutation_status_serde_roundtrip() {
    for status in [
        MutationStatus::Idle,
        MutationStatus::Loading,
        MutationStatus::Success,
        MutationStatus::Failure,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let back: MutationStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, status);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CachePolicy edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cache_policy_is_fresh_at_zero_age() {
    let policy = CachePolicy::Ttl { ttl_ms: 100 };
    assert!(policy.is_fresh(0));
}

#[test]
fn cache_policy_is_fresh_at_exact_ttl() {
    let policy = CachePolicy::Ttl { ttl_ms: 500 };
    assert!(policy.is_fresh(500), "age == ttl_ms should be fresh (inclusive)");
}

#[test]
fn cache_policy_is_fresh_one_past_ttl() {
    let policy = CachePolicy::Ttl { ttl_ms: 500 };
    assert!(!policy.is_fresh(501));
}

#[test]
fn cache_policy_nocache_is_never_fresh() {
    assert!(!CachePolicy::NoCache.is_fresh(0));
    assert!(!CachePolicy::NoCache.is_fresh(1));
}

#[test]
fn cache_policy_swr_is_stale_between_ttl_and_total() {
    let policy = CachePolicy::StaleWhileRevalidate { ttl_ms: 100, stale_ms: 200 };
    // Within TTL: not stale
    assert!(!policy.is_stale_but_serveable(50));
    assert!(!policy.is_stale_but_serveable(100));
    // Between TTL and total (100 < age <= 300): stale
    assert!(policy.is_stale_but_serveable(101));
    assert!(policy.is_stale_but_serveable(300));
    // Past total: not stale (expired)
    assert!(!policy.is_stale_but_serveable(301));
}

#[test]
fn cache_policy_swr_is_expired_past_total() {
    let policy = CachePolicy::StaleWhileRevalidate { ttl_ms: 100, stale_ms: 200 };
    assert!(!policy.is_expired(100));
    assert!(!policy.is_expired(300));
    assert!(policy.is_expired(301));
}

#[test]
fn cache_policy_nocache_is_always_expired() {
    assert!(CachePolicy::NoCache.is_expired(0));
    assert!(CachePolicy::NoCache.is_expired(1));
}

#[test]
fn cache_policy_ttl_is_expired_past_ttl() {
    let policy = CachePolicy::Ttl { ttl_ms: 100 };
    assert!(!policy.is_expired(100));
    assert!(policy.is_expired(101));
}

#[test]
fn cache_policy_serde_roundtrip() {
    for policy in [
        CachePolicy::NoCache,
        CachePolicy::Ttl { ttl_ms: 5_000 },
        CachePolicy::StaleWhileRevalidate { ttl_ms: 1_000, stale_ms: 2_000 },
    ] {
        let json = serde_json::to_string(&policy).unwrap();
        let back: CachePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}

#[test]
fn cache_policy_label_subsecond() {
    assert_eq!(CachePolicy::Ttl { ttl_ms: 500 }.label(), "Cache TTL 500ms");
    assert_eq!(CachePolicy::Ttl { ttl_ms: 0 }.label(), "Cache TTL 0ms");
}

#[test]
fn cache_policy_label_seconds() {
    assert_eq!(CachePolicy::Ttl { ttl_ms: 1_000 }.label(), "Cache TTL 1s");
    assert_eq!(CachePolicy::Ttl { ttl_ms: 60_000 }.label(), "Cache TTL 60s");
}

#[test]
fn cache_policy_label_nocache() {
    assert_eq!(CachePolicy::NoCache.label(), "No cache");
}

#[test]
fn cache_policy_label_swr() {
    let policy = CachePolicy::StaleWhileRevalidate { ttl_ms: 30_000, stale_ms: 500 };
    assert_eq!(policy.label(), "Stale-while-revalidate TTL 30s stale 500ms");
}

#[test]
fn request_policy_labels() {
    assert_eq!(RequestPolicy::LatestWins.label(), "Latest wins");
    assert_eq!(RequestPolicy::IgnoreWhileLoading.label(), "Ignore while loading");
}

#[test]
fn request_policy_serde_roundtrip() {
    for policy in [RequestPolicy::LatestWins, RequestPolicy::IgnoreWhileLoading] {
        let json = serde_json::to_string(&policy).unwrap();
        let back: RequestPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}

#[test]
fn request_policy_default() {
    assert_eq!(RequestPolicy::default(), RequestPolicy::LatestWins);
}

#[test]
fn query_fetch_mode_default() {
    assert_eq!(QueryFetchMode::default(), QueryFetchMode::Normal);
}
