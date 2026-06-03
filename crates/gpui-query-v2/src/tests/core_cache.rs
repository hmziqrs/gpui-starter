//! Core cache layer tests for gpui-query-v2.
//!
//! Covers:
//! - TTL cache policy: freshness, boundary, expiry, renewal
//! - StaleWhileRevalidate: serving stale data, background refetch, full expiry
//! - NoCache: always stale/expired, no short-circuit
//! - Cache invalidation
//! - Cache reset
//! - Data retention: placeholder_data, previous_data, display_data, rollback
//! - Cache interactions with different request policies

use crate::core::*;
use crate::tests::test_support::*;

// ── Named time constants ────────────────────────────────────────────────
//
// All TTL/SWR tests seed data at STORED_AT_MS and reason about boundary
// offsets from there.  Naming these values makes the age arithmetic
// self-documenting rather than forcing the reader to reverse-engineer
// magic numbers.

/// The `stored_at` timestamp used by every seeded cache entry (ms).
const STORED_AT_MS: u128 = 1_000;

/// TTL duration shared by both the default TTL resource and the SWR resource.
const TTL_MS: u64 = 1_000;

/// Stale-window duration used by the SWR resource.
const STALE_MS: u64 = 2_000;

/// Total validity window for the SWR resource (TTL + stale).
const SWR_TOTAL_MS: u64 = TTL_MS + STALE_MS; // 3_000

// Derived boundary offsets from STORED_AT_MS:
const AT_TTL_BOUNDARY: u128 = STORED_AT_MS + TTL_MS as u128; // 2_000 — exactly at TTL edge
const ONE_MS_PAST_TTL: u128 = AT_TTL_BOUNDARY + 1; // 2_001 — just past TTL
const AT_SWR_BOUNDARY: u128 = STORED_AT_MS + SWR_TOTAL_MS as u128; // 4_000 — exactly at total edge
const ONE_MS_PAST_SWR: u128 = AT_SWR_BOUNDARY + 1; // 4_001 — fully expired

// ── Helpers ──────────────────────────────────────────────────────────────

fn ttl_resource() -> QueryResource<&'static str> {
    test_resource()
}

fn swr_resource() -> QueryResource<&'static str> {
    QueryResource::new(
        "swr-test",
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: TTL_MS,
            stale_ms: STALE_MS,
        },
        RequestPolicy::LatestWins,
    )
}

fn nocache_resource() -> QueryResource<&'static str> {
    QueryResource::new("nocache-test", CachePolicy::NoCache, RequestPolicy::LatestWins)
}

fn seed_data(resource: &mut QueryResource<&'static str>, data: &'static str, stored_at_ms: u128) {
    resource.apply_success(data, stored_at_ms);
}

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

// ══════════════════════════════════════════════════════════════════════════
// CACHE INVALIDATION
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn invalidate_clears_last_updated_but_retains_data() {
    let mut r = ttl_resource();
    seed_data(&mut r, "cached", STORED_AT_MS);
    r.invalidate();
    assert_eq!(r.data(), Some(&"cached"), "data is retained after invalidate");
    assert_eq!(r.last_updated_at_ms(), None, "last_updated_at cleared by invalidate");
    assert!(!r.is_cache_fresh(STORED_AT_MS + 1), "after invalidate, data is not fresh even within TTL");
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

// ══════════════════════════════════════════════════════════════════════════
// DATA RETENTION: placeholder_data, previous_data, display_data, rollback
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn placeholder_data_set_and_clear() {
    let mut r = ttl_resource();
    assert_eq!(r.placeholder_data(), None);
    r.set_placeholder_data(Some("loading..."));
    assert_eq!(r.placeholder_data(), Some(&"loading..."));
    r.set_placeholder_data(None);
    assert_eq!(r.placeholder_data(), None);
}

#[test]
fn display_data_prefers_real_data_over_placeholder() {
    let mut r = ttl_resource();
    r.set_placeholder_data(Some("placeholder"));
    seed_data(&mut r, "real", STORED_AT_MS);
    assert_eq!(r.display_data(), Some(&"real"));
}

#[test]
fn display_data_falls_back_to_placeholder_when_no_data() {
    let mut r = ttl_resource();
    r.set_placeholder_data(Some("placeholder"));
    assert_eq!(r.data(), None);
    assert_eq!(r.display_data(), Some(&"placeholder"));
}

#[test]
fn display_data_none_when_neither_set() {
    let r = ttl_resource();
    assert_eq!(r.display_data(), None);
}

#[test]
fn previous_data_tracked_across_successive_successes() {
    let mut r = ttl_resource();
    seed_data(&mut r, "first", 100);
    assert_eq!(r.previous_data(), None, "no previous on first success");
    seed_data(&mut r, "second", 200);
    assert_eq!(r.data(), Some(&"second"));
    assert_eq!(r.previous_data(), Some(&"first"));
}

#[test]
fn previous_data_preserved_across_failure() {
    let mut r = ttl_resource();
    seed_data(&mut r, "v1", 100);
    seed_data(&mut r, "v2", 200);
    r.apply_failure("error", 300);
    assert_eq!(r.data(), Some(&"v2"), "failure preserves current data");
    assert_eq!(r.previous_data(), Some(&"v1"), "failure does not touch previous_data");
}

#[test]
fn rollback_restores_previous_data() {
    let mut r = ttl_resource();
    seed_data(&mut r, "original", 100);
    seed_data(&mut r, "updated", 200);
    let rolled_back = r.rollback_to_previous();
    assert!(rolled_back);
    assert_eq!(r.data(), Some(&"original"));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.previous_data(), None, "previous_data cleared after rollback");
}

#[test]
fn rollback_returns_false_when_no_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "only", 100);
    assert!(!r.rollback_to_previous());
    assert_eq!(r.data(), Some(&"only"));
}

#[test]
fn set_data_optimistic_update_saves_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "original", 100);
    r.set_data("optimistic");
    assert_eq!(r.data(), Some(&"optimistic"));
    assert_eq!(r.previous_data(), Some(&"original"));
}

#[test]
fn clear_data_saves_to_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "existing", 100);
    r.clear_data();
    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"existing"));
}

#[test]
fn rollback_after_optimistic_update() {
    let mut r = ttl_resource();
    seed_data(&mut r, "original", 100);
    r.set_data("optimistic");
    let rolled_back = r.rollback_to_previous();
    assert!(rolled_back);
    assert_eq!(r.data(), Some(&"original"));
    assert_eq!(r.status(), QueryStatus::Success);
}

#[test]
fn reset_clears_placeholder_and_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "first", 100);
    seed_data(&mut r, "second", 200);
    r.set_placeholder_data(Some("placeholder"));
    assert_eq!(r.previous_data(), Some(&"first"));
    assert_eq!(r.placeholder_data(), Some(&"placeholder"));
    r.reset();
    assert_eq!(r.placeholder_data(), None);
    assert_eq!(r.previous_data(), None);
    assert_eq!(r.data(), None);
}

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
