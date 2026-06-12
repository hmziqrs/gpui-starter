//! Basic state transition tests (sections 1-6).
//!
//! Covers: Idle -> LoadingEmpty, LoadingEmpty -> Success/Failure,
//! refetch with cached data, LoadingWithData -> Success/Failure.

use crate::core::*;
use crate::tests::test_support::*;

// ── Helpers ────────────────────────────────────────────────────────────

/// Create a default test resource with LatestWins policy.
pub fn resource() -> QueryResource<&'static str> {
    test_resource()
}

/// Create a fresh sequencer.
pub fn seq() -> RequestSequencer {
    test_sequencer()
}

/// Extract the error display string from a resource.
pub fn err_str(r: &QueryResource<&'static str>) -> Option<String> {
    r.error().map(|e| e.to_string())
}

/// Begin a request, returning (request_id, status).
///
/// Panics with a descriptive message if the result is not `Started` or
/// `StaleCacheHit`. This includes `CacheHit` (which means the cache was
/// still fresh at `now_ms` -- likely a TTL miscalculation in the test)
/// and `IgnoredWhileLoading` (which means a request was already active).
pub fn begin(
    r: &mut QueryResource<&'static str>,
    seq: &mut RequestSequencer,
    now_ms: u128,
) -> (RequestId, QueryStatus) {
    let result = r.begin_request(seq, now_ms, QueryFetchMode::Normal);
    match &result {
        QueryBeginResult::Started {
            request_id, status, ..
        } => (*request_id, *status),
        QueryBeginResult::StaleCacheHit {
            request_id, status, ..
        } => (*request_id, *status),
        QueryBeginResult::CacheHit => {
            panic!(
                "begin() got CacheHit at now_ms={} -- the cache is still fresh. \
                 Adjust now_ms to be past the TTL (or use a longer gap between \
                 completion and the next begin_request call). \
                 Resource state: status={:?}, data={:?}, last_updated_at_ms={:?}",
                now_ms,
                r.status(),
                r.data(),
                r.last_updated_at_ms(),
            );
        }
        QueryBeginResult::IgnoredWhileLoading { .. } => {
            panic!(
                "begin() got IgnoredWhileLoading at now_ms={} -- there is already \
                 an active request (active_request_id={:?}). \
                 This helper is intended for LatestWins resources where begin \
                 should always succeed. Check that the resource uses the \
                 correct RequestPolicy.",
                now_ms,
                r.active_request_id(),
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Idle -> LoadingEmpty
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn idle_to_loading_empty_transitions_correctly() {
    let mut r = resource();
    let mut s = seq();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert!(r.active_request_id().is_none());

    let (rid, status) = begin(&mut r, &mut s, 100);

    assert_eq!(status, QueryStatus::LoadingEmpty);
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.is_loading());
    assert!(r.is_pending());
    assert_eq!(r.active_request_id(), Some(rid));
    assert_eq!(r.started_at_ms(), Some(100));
    assert_eq!(r.error(), None, "error should be cleared on begin_loading");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. LoadingEmpty -> Success
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_empty_to_success_with_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);

    assert!(r.complete_current_success(rid, "hello", 200));

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"hello"));
    assert!(r.active_request_id().is_none());
    assert_eq!(r.last_updated_at_ms(), Some(200));
    assert!(r.error().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. LoadingEmpty -> Failure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_empty_to_failure() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);

    assert!(r.complete_current_failure(rid, QueryError::response("server error"), 200));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), None, "no prior data, so data should remain None");
    assert_eq!(
        err_str(&r),
        Some("response error: server error".to_string())
    );
    assert!(r.active_request_id().is_none());
    assert_eq!(r.last_updated_at_ms(), Some(200));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Idle -> LoadingWithData (refetch with existing cached data)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn success_to_loading_with_data_on_refetch() {
    let mut r = resource();
    let mut s = seq();

    // Seed data via a successful fetch
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "cached", 200));

    // Refetch (beyond TTL so cache doesn't short-circuit)
    let (rid2, status) = begin(&mut r, &mut s, 1_500);

    assert_eq!(status, QueryStatus::LoadingWithData);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert_eq!(
        r.data(),
        Some(&"cached"),
        "prior data is preserved during refetch"
    );
    assert!(r.is_loading());
    assert!(!r.is_pending());
    assert_eq!(r.active_request_id(), Some(rid2));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. LoadingWithData -> Success (data updated)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_with_data_to_success_updates_data() {
    let mut r = resource();
    let mut s = seq();

    // First fetch
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "old", 200));

    // Refetch
    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_success(rid2, "new", 1_600));

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"new"));
    assert_eq!(
        r.previous_data(),
        Some(&"old"),
        "previous_data holds old value"
    );
    assert_eq!(r.last_updated_at_ms(), Some(1_600));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. LoadingWithData -> Failure (data retained; only cancel clears data)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_with_data_to_failure_retains_data() {
    let mut r = resource();
    let mut s = seq();

    // First fetch succeeds
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "cached", 200));

    // Refetch fails
    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_failure(rid2, QueryError::transport("timeout"), 1_600));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(
        r.data(),
        Some(&"cached"),
        "apply_failure retains existing data (only cancel() clears it)"
    );
    assert_eq!(err_str(&r), Some("transport error: timeout".to_string()));
    assert_eq!(
        r.last_updated_at_ms(),
        Some(1_600),
        "failure updates last_updated_at"
    );
    // Cached data is still within TTL window relative to its original timestamp,
    // but the status is Failure, not Success.
    assert!(r.is_data_stale(), "data with Failure status is stale");
}

#[test]
fn failure_with_data_retains_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "old", 200));

    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_failure_with_data(
        rid2,
        "stale-fallback",
        QueryError::response("err"),
        1_600
    ));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"stale-fallback"));
    assert_eq!(err_str(&r), Some("response error: err".to_string()));
}
