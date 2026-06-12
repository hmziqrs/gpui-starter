//! Tests for RequestGuard, two-phase completion, and begin_request_with_id.
//!
//! Covers:
//! - RequestGuard: proof-of-ownership, scope-based rejection, into_request_id
//! - Two-phase completion via guard: complete_success, complete_failure, stale rejection
//! - begin_request_with_id variant

use crate::core::{
    CachePolicy, QueryBeginResult, QueryFetchMode, QueryResource, QueryStatus, RequestId,
    RequestPolicy,
};
use crate::tests::test_support::{
    TEST_NOW_MS, assert_status, test_resource_with_policies, test_sequencer,
};

// ═══════════════════════════════════════════════════════════════════════════
// RequestGuard: proof-of-ownership (obtained via accept_current_request)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn guard_holds_the_correct_request_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    assert_eq!(guard.request_id(), rid);
}

#[test]
fn guard_into_request_id_consumes_guard() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    let extracted = guard.into_request_id();
    assert_eq!(extracted, rid);
    // guard is consumed — calling guard.request_id() here would not compile.
}

#[test]
fn accept_current_request_returns_guard_for_active_request() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let request_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource
        .accept_current_request(request_id)
        .expect("should accept the active request");
    assert_eq!(guard.request_id(), request_id);
}

#[test]
fn accept_current_request_rejects_stale_request_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Start request 1
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let old_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    // Start request 2 — replaces request 1 under LatestWins
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let _new_id = match result {
        QueryBeginResult::Started {
            request_id,
            replaced_request_id,
            ..
        } => {
            assert_eq!(replaced_request_id, Some(old_id));
            request_id
        }
        other => panic!("expected Started, got {:?}", other),
    };

    // Trying to accept the old request should fail
    let result = resource.accept_current_request(old_id);
    assert!(
        result.is_none(),
        "stale request id should be rejected, but got a guard"
    );
    assert_eq!(resource.ignored_results(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Two-phase completion via guard
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complete_success_consumes_guard_and_sets_data() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    resource.complete_success(guard, "result", TEST_NOW_MS);

    assert_eq!(resource.data(), Some(&"result"));
    assert_status(&resource, QueryStatus::Success);
    assert_eq!(resource.active_request_id(), None);
}

#[test]
fn complete_failure_consumes_guard_and_sets_error() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    resource.complete_failure(guard, "network error", TEST_NOW_MS);

    assert!(resource.data().is_none());
    assert_status(&resource, QueryStatus::Failure);
    assert!(resource.error().is_some());
}

#[test]
fn complete_convenience_method_rejects_stale_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Request 1
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let old_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    // Request 2 replaces request 1
    resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);

    // Trying to complete old request should fail
    let completed = resource.complete_current_success(old_id, "stale data", TEST_NOW_MS);
    assert!(!completed, "stale request should not be completed");
    assert!(
        resource.data().is_none(),
        "data should remain unset after stale completion attempt"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// begin_request_with_id variant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_with_id_uses_provided_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let custom_id = RequestId::scoped(99, 7);

    let result =
        resource.begin_request_with_id(Some(custom_id), TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    assert_eq!(rid, custom_id);
}

#[test]
fn begin_request_with_id_none_falls_back_to_transient_sequencer() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);

    let result = resource.begin_request_with_id(None, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    // Transient sequencer starts at scope 1, seq 1
    assert_eq!(rid.scope_id(), 1);
    assert_eq!(rid.value(), 1);
}
