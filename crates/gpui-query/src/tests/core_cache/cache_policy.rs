//! TTL cache policy, StaleWhileRevalidate, and NoCache tests.

use crate::core::*;
use crate::tests::core_cache::*;
use crate::tests::test_support::test_sequencer;

// ══════════════════════════════════════════════════════════════════════════
// TTL CACHE POLICY
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn ttl_cache_is_fresh_at_exact_boundary() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(r.is_cache_fresh(AT_TTL_BOUNDARY));
}

#[test]
fn ttl_cache_is_stale_one_ms_past_boundary() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(!r.is_cache_fresh(ONE_MS_PAST_TTL));
}

#[test]
fn ttl_cache_is_fresh_well_within_window() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(r.is_cache_fresh(STORED_AT_MS + 500));
    assert!(r.is_cache_fresh(AT_TTL_BOUNDARY - 1));
}

#[test]
fn ttl_cache_is_not_fresh_for_future_timestamp() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", 10_000);
    assert!(!r.is_cache_fresh(9_999));
}

#[test]
fn ttl_expired_check_after_ttl_window() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(!r.is_cache_expired(STORED_AT_MS + 500), "within TTL, not expired");
    assert!(!r.is_cache_expired(AT_TTL_BOUNDARY), "at TTL boundary, not expired");
    assert!(r.is_cache_expired(ONE_MS_PAST_TTL), "past TTL, expired");
}

#[test]
fn ttl_renewal_resets_freshness() {
    let mut r = ttl_resource();
    seed_data(&mut r, "v1", STORED_AT_MS);
    assert!(!r.is_cache_fresh(STORED_AT_MS + 1_500));
    let renewed_at: u128 = AT_TTL_BOUNDARY;
    seed_data(&mut r, "v2", renewed_at);
    assert!(r.is_cache_fresh(renewed_at + 500));
    assert_eq!(r.data(), Some(&"v2"));
    assert_eq!(r.previous_data(), Some(&"v1"));
}

#[test]
fn ttl_short_circuit_when_fresh() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(r.should_short_circuit_cache(STORED_AT_MS + 500));
}

#[test]
fn ttl_no_short_circuit_when_stale() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    assert!(!r.should_short_circuit_cache(ONE_MS_PAST_TTL));
}

#[test]
fn ttl_no_short_circuit_without_data() {
    let r = ttl_resource();
    assert!(!r.should_short_circuit_cache(STORED_AT_MS));
}

// ══════════════════════════════════════════════════════════════════════════
// STALE-WHILE-REVALIDATE
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn swr_fresh_within_ttl() {
    let mut r = swr_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    // stored_at=1000, TTL=1000. Fresh while age < TTL.
    assert!(r.is_cache_fresh(STORED_AT_MS + 500));
    assert!(r.is_cache_fresh(AT_TTL_BOUNDARY));
}

#[test]
fn swr_stale_but_serveable_in_stale_window() {
    let mut r = swr_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    // stored_at=1000, TTL=1000, stale=2000. Past TTL but within stale window.
    assert!(!r.is_cache_fresh(ONE_MS_PAST_TTL), "past TTL, not fresh");
    assert!(r.is_stale_but_serveable(ONE_MS_PAST_TTL));
    assert!(r.is_stale_but_serveable(STORED_AT_MS + 2_000), "mid stale window, serveable");
}

#[test]
fn swr_fully_expired_past_stale_window() {
    let mut r = swr_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    // stored_at=1000, total=3000. age at ONE_MS_PAST_SWR = 3001 > total => expired
    assert!(r.is_cache_expired(ONE_MS_PAST_SWR));
    assert!(!r.is_cache_fresh(ONE_MS_PAST_SWR));
    assert!(!r.is_stale_but_serveable(ONE_MS_PAST_SWR));
}

#[test]
fn swr_should_serve_stale_and_revalidate_in_stale_window() {
    let mut r = swr_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    // Between TTL boundary and stale boundary => should revalidate.
    assert!(r.should_serve_stale_and_revalidate(ONE_MS_PAST_TTL));
    assert!(r.should_serve_stale_and_revalidate(STORED_AT_MS + 2_000));
}

#[test]
fn swr_should_not_serve_stale_within_ttl() {
    let mut r = swr_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    // Still fresh => no need for stale-serve.
    assert!(!r.should_serve_stale_and_revalidate(STORED_AT_MS + 500));
}

#[test]
fn swr_begin_request_stale_cache_hit_triggers_background_refetch() {
    let mut r = swr_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "cached", STORED_AT_MS);

    // One ms past TTL => stale but serveable, triggers background refetch.
    let result = r.begin_request(&mut seq, ONE_MS_PAST_TTL, QueryFetchMode::Normal);
    assert!(matches!(result, QueryBeginResult::StaleCacheHit { .. }));
    assert_eq!(r.cache_hits(), 1, "stale cache hit should increment cache_hits");
    assert_eq!(r.data(), Some(&"cached"), "stale data still accessible");
}

#[test]
fn swr_begin_request_expired_starts_normal_fetch() {
    let mut r = swr_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "cached", STORED_AT_MS);

    // stored_at=1000, total=3000. age at now=5000 is 4000 > total => expired
    let result = r.begin_request(&mut seq, STORED_AT_MS + 4_000, QueryFetchMode::Normal);
    assert!(matches!(result, QueryBeginResult::Started { .. }));
    assert_eq!(r.cache_hits(), 0, "no cache hit when expired");
}

#[test]
fn swr_stale_boundary_exact() {
    let mut r = swr_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    // stored_at=1000, total=3000. age at AT_SWR_BOUNDARY = 3000 == total => serveable (inclusive)
    assert!(r.is_stale_but_serveable(AT_SWR_BOUNDARY));
    // age at ONE_MS_PAST_SWR = 3001 > total => expired
    assert!(!r.is_stale_but_serveable(ONE_MS_PAST_SWR));
    assert!(r.is_cache_expired(ONE_MS_PAST_SWR));
}

// ══════════════════════════════════════════════════════════════════════════
// NO-CACHE POLICY
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn nocache_never_fresh() {
    let mut r = nocache_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    assert!(!r.is_cache_fresh(STORED_AT_MS));
    assert!(!r.is_cache_fresh(STORED_AT_MS + 1));
}

#[test]
fn nocache_always_expired() {
    let mut r = nocache_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    assert!(r.is_cache_expired(STORED_AT_MS));
    assert!(r.is_cache_expired(500));
}

#[test]
fn nocache_no_short_circuit() {
    let mut r = nocache_resource();
    seed_data(&mut r, "data", STORED_AT_MS);
    assert!(!r.should_short_circuit_cache(STORED_AT_MS));
}

#[test]
fn nocache_cannot_short_circuit_policy() {
    assert!(!CachePolicy::NoCache.can_short_circuit());
}

#[test]
fn nocache_begin_request_always_starts() {
    let mut r = nocache_resource();
    let mut seq = test_sequencer();
    seed_data(&mut r, "data", STORED_AT_MS);
    let result = r.begin_request(&mut seq, STORED_AT_MS, QueryFetchMode::Normal);
    assert!(matches!(result, QueryBeginResult::Started { .. }));
    assert_eq!(r.cache_hits(), 0);
}

#[test]
fn nocache_should_clear_data_on_complete() {
    let r = nocache_resource();
    assert!(r.should_clear_data_on_complete());
}
