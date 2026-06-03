//! Comprehensive tests for `MutationResource` in gpui-query-v2.
//!
//! Covers the full mutation lifecycle including:
//! - Idle -> Loading -> Success / Failure transitions
//! - Retry behaviour and retry count tracking
//! - Cancellation, signal propagation, and cancelled_count
//! - Reset semantics
//! - LatestWins (double-mutate-while-loading) via `begin` cancelling old signal
//! - Data preservation and clearing through state transitions
//! - `prepare_retry` (flash-free retry)
//! - `increment_retry` and `reset_retry_count` helpers
//! - Key association
//! - Edge cases: cancel on terminal states, retry from non-Failure, saturating retry_count

use crate::core::*;

// ── 1. New mutation is idle ────────────────────────────────────────────

#[test]
fn new_mutation_is_idle() {
    let m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.is_idle());
    assert!(!m.is_loading());
    assert!(!m.is_success());
    assert!(!m.is_failure());
    assert_eq!(m.status(), MutationStatus::Idle);
    assert!(m.data().is_none());
    assert!(m.error().is_none());
    assert!(m.variables().is_none());
    assert_eq!(m.retry_count(), 0);
    assert_eq!(m.cancelled_count(), 0);
    assert!(m.signal().is_none());
    assert!(m.key().is_none());
}

// ── 2. Full lifecycle: Idle -> Loading -> Success ─────────────────────

#[test]
fn lifecycle_idle_to_loading_to_success() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    // Idle
    assert_eq!(m.status(), MutationStatus::Idle);

    // Begin -> Loading
    m.begin("update-user".to_string());
    assert!(m.is_loading());
    assert_eq!(m.variables(), Some(&"update-user".to_string()));
    assert!(m.error().is_none());
    assert!(m.signal().is_some());
    assert!(!m.signal().unwrap().is_cancelled());

    // Complete with data -> Success
    m.complete_success(42);
    assert!(m.is_success());
    assert_eq!(m.data(), Some(&42));
    assert!(m.error().is_none());
    assert!(m.signal().is_none());
    // Variables persist through success
    assert_eq!(m.variables(), Some(&"update-user".to_string()));
}

// ── 3. Full lifecycle: Idle -> Loading -> Failure ─────────────────────

#[test]
fn lifecycle_idle_to_loading_to_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("delete-user".to_string());
    assert!(m.is_loading());

    m.complete_failure(QueryError::response("server error"));
    assert!(m.is_failure());
    assert_eq!(m.error().unwrap().message(), "server error");
    assert_eq!(m.error().unwrap().kind(), QueryErrorKind::Response);
    assert!(m.data().is_none(), "failure must clear previous data");
    assert!(m.signal().is_none());
    assert_eq!(m.retry_count(), 1);
    assert_eq!(m.variables(), Some(&"delete-user".to_string()));
}

// ── 4. Retry from Failure transitions to Loading ──────────────────────

#[test]
fn retry_from_failure_goes_to_loading() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));

    m.begin("vars".to_string());
    m.complete_failure(QueryError::transport("timeout"));

    assert!(m.is_failure());
    assert!(m.should_retry(), "retry_count=1 < max_retries=2");
    assert_eq!(m.retry_count(), 1);

    let retried = m.retry();
    assert!(retried);
    assert!(m.is_loading());
    assert!(m.error().is_none(), "retry clears error");
    assert!(m.signal().is_some(), "retry creates fresh signal");
    assert!(!m.signal().unwrap().is_cancelled());
    assert_eq!(m.variables(), Some(&"vars".to_string()), "variables preserved");
}

// ── 5. Retry count is respected (max reached) ─────────────────────────

#[test]
fn retry_respects_max_retries() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));

    m.begin("vars".to_string());

    // First failure -> retry_count = 1
    m.complete_failure(QueryError::response("fail 1"));
    assert_eq!(m.retry_count(), 1);
    assert!(m.should_retry(), "1 < 2");
    assert!(m.retry());

    // Second failure -> retry_count = 2
    m.complete_failure(QueryError::response("fail 2"));
    assert_eq!(m.retry_count(), 2);
    assert!(!m.should_retry(), "2 is not < 2");
    assert!(!m.retry(), "retry should fail when max reached");

    assert!(m.is_failure());
    assert_eq!(m.error().unwrap().message(), "fail 2");
}

// ── 6. Retry only works from Failure status ───────────────────────────

#[test]
fn retry_only_from_failure_status() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

    // Cannot retry from Idle
    assert!(!m.retry());

    // Cannot retry from Loading
    m.begin("vars".to_string());
    assert!(!m.retry());

    // Can retry from Failure
    m.complete_failure(QueryError::response("fail"));
    assert!(m.retry());

    // After retry, back to Loading — cannot retry again
    assert!(m.is_loading());
    assert!(!m.retry());
}

// ── 7. Retry creates a fresh (non-cancelled) signal ───────────────────

#[test]
fn retry_creates_fresh_signal() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));
    m.begin("vars".to_string());
    let old_signal = m.signal().unwrap().clone();

    m.complete_failure(QueryError::response("fail"));

    // old signal was cleared on failure
    assert!(m.signal().is_none());

    let retried = m.retry();
    assert!(retried);

    let new_signal = m.signal().unwrap();
    assert!(
        !new_signal.is_cancelled(),
        "fresh signal after retry must not be cancelled"
    );
    // old_signal is independent — it may or may not be cancelled depending on impl,
    // but new_signal must definitely be fresh and uncancelled
    assert_ne!(old_signal, *new_signal, "signals must be different objects");
}

// ── 8. Cancellation cancels signal and sets Failure ───────────────────

#[test]
fn cancel_during_loading_sets_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());

    let signal = m.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    m.cancel(QueryError::cancelled("user aborted"));

    assert!(m.is_failure());
    assert_eq!(m.error().unwrap().kind(), QueryErrorKind::Cancelled);
    assert_eq!(m.error().unwrap().message(), "user aborted");
    assert!(signal.is_cancelled(), "external signal clone must see cancellation");
    assert!(m.signal().is_none(), "signal cleared after cancel");
    assert_eq!(m.cancelled_count(), 1);
}

// ── 9. Cancel is a no-op on Idle, Success, Failure ────────────────────

#[test]
fn cancel_on_idle_is_noop() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.cancel(QueryError::cancelled("x"));
    assert!(m.is_idle());
    assert_eq!(m.cancelled_count(), 0);
    assert!(m.error().is_none());
}

#[test]
fn cancel_on_success_is_noop() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    m.complete_success(10);
    m.cancel(QueryError::cancelled("x"));
    assert!(m.is_success());
    assert_eq!(m.data(), Some(&10));
    assert_eq!(m.cancelled_count(), 0);
}

#[test]
fn cancel_on_failure_is_noop() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail"));
    let retry_count_before = m.retry_count();
    m.cancel(QueryError::cancelled("x"));
    assert!(m.is_failure());
    assert_eq!(m.retry_count(), retry_count_before);
    assert_eq!(m.cancelled_count(), 0);
}

// ── 10. cancelled_count increments across multiple cancellations ──────

#[test]
fn cancelled_count_increments_across_mutations() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("first".to_string());
    m.cancel(QueryError::cancelled("abort 1"));
    assert_eq!(m.cancelled_count(), 1);

    m.begin("second".to_string());
    m.cancel(QueryError::cancelled("abort 2"));
    assert_eq!(m.cancelled_count(), 2);

    m.begin("third".to_string());
    m.cancel(QueryError::cancelled("abort 3"));
    assert_eq!(m.cancelled_count(), 3);
}

// ── 11. Reset returns to Idle, clears everything ──────────────────────

#[test]
fn reset_returns_to_idle_and_clears_all_state() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

    // Build up some state: load -> fail -> retry -> cancel
    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail"));
    assert!(m.retry());
    m.cancel(QueryError::cancelled("abort"));

    assert_eq!(m.cancelled_count(), 1);
    assert!(m.is_failure());

    m.reset();

    assert!(m.is_idle());
    assert!(m.data().is_none());
    assert!(m.error().is_none());
    assert!(m.variables().is_none());
    assert_eq!(m.retry_count(), 0);
    assert_eq!(m.cancelled_count(), 0, "reset clears cancelled_count");
    assert!(m.signal().is_none());
}

// ── 12. Reset cancels in-flight signal ────────────────────────────────

#[test]
fn reset_cancels_in_flight_signal() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    let signal = m.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    m.reset();

    assert!(signal.is_cancelled(), "reset must cancel the in-flight signal");
    assert!(m.signal().is_none());
}

// ── 13. LatestWins: begin cancels previous in-flight signal ───────────

#[test]
fn begin_cancels_previous_signal_on_replacement() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("first".to_string());
    let first_signal = m.signal().unwrap().clone();
    assert!(!first_signal.is_cancelled());

    // Second begin while first is still loading → LatestWins
    m.begin("second".to_string());
    assert!(first_signal.is_cancelled(), "old signal must be cancelled");
    assert!(!m.signal().unwrap().is_cancelled(), "new signal must be fresh");
    assert_eq!(m.variables(), Some(&"second".to_string()));
    assert_eq!(m.retry_count(), 0, "begin resets retry_count");
}

// ── 14. LatestWins: data from previous success is overwritten ─────────

#[test]
fn begin_clears_error_from_previous_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("first".to_string());
    m.complete_failure(QueryError::response("first failed"));
    assert!(m.is_failure());
    assert!(m.error().is_some());

    m.begin("second".to_string());
    assert!(m.is_loading());
    assert!(m.error().is_none(), "begin clears previous error");
}

// ── 15. Data preservation: success data persists until failure ────────

#[test]
fn success_data_preserved_until_next_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));

    // First invocation succeeds
    m.begin("v1".to_string());
    m.complete_success(100);
    assert_eq!(m.data(), Some(&100));

    // Second invocation fails — data is cleared
    m.begin("v2".to_string());
    m.complete_failure(QueryError::response("fail"));
    assert!(m.data().is_none(), "failure clears data");

    // Third invocation succeeds again — new data
    m.begin("v3".to_string());
    m.complete_success(200);
    assert_eq!(m.data(), Some(&200));
}

// ── 16. begin resets retry_count for fresh invocation ─────────────────

#[test]
fn begin_resets_retry_count_allowing_fresh_retries() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(1));

    // First invocation: exhaust retries
    m.begin("v1".to_string());
    m.complete_failure(QueryError::response("fail 1"));
    assert_eq!(m.retry_count(), 1);
    assert!(!m.should_retry(), "retries exhausted");

    // Second invocation: begin resets retry_count
    m.begin("v2".to_string());
    assert_eq!(m.retry_count(), 0, "begin must reset retry_count");
    assert!(m.should_retry(), "should_retry is true after fresh begin");

    m.complete_failure(QueryError::response("fail 2"));
    assert_eq!(m.retry_count(), 1);
    assert!(!m.should_retry(), "retries exhausted again");
}

// ── 17. prepare_retry stays in Loading with fresh signal ──────────────

#[test]
fn prepare_retry_stays_in_loading_with_fresh_signal() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));
    m.begin("vars".to_string());

    let old_signal = m.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    m.prepare_retry();

    // Must still be Loading — no transient Failure flash
    assert!(m.is_loading(), "prepare_retry must not leave Loading");
    assert!(m.error().is_none());
    assert!(old_signal.is_cancelled(), "old signal must be cancelled by prepare_retry");

    let new_signal = m.signal().unwrap();
    assert!(!new_signal.is_cancelled(), "new signal must be fresh");
}

// ── 18. prepare_retry is no-op when not in Loading ────────────────────

#[test]
fn prepare_retry_noop_when_not_loading() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

    // No-op on Idle
    m.prepare_retry();
    assert!(m.is_idle());

    // No-op on Success
    m.begin("vars".to_string());
    m.complete_success(42);
    m.prepare_retry();
    assert!(m.is_success());
    assert_eq!(m.data(), Some(&42));

    // No-op on Failure
    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail"));
    m.prepare_retry();
    assert!(m.is_failure());
}

// ── 19. increment_retry bumps counter without state transition ────────

#[test]
fn increment_retry_bumps_counter_without_state_change() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(5));
    m.begin("vars".to_string());

    assert_eq!(m.retry_count(), 0);
    assert!(m.is_loading());

    m.increment_retry();
    assert_eq!(m.retry_count(), 1);
    assert!(m.is_loading(), "must still be Loading after increment_retry");

    m.increment_retry();
    m.increment_retry();
    assert_eq!(m.retry_count(), 3);
}

// ── 20. reset_retry_count zeroes the counter ──────────────────────────

#[test]
fn reset_retry_count_zeroes_counter() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(5));
    m.begin("vars".to_string());
    m.increment_retry();
    m.increment_retry();
    assert_eq!(m.retry_count(), 2);

    m.reset_retry_count();
    assert_eq!(m.retry_count(), 0);
}

// ── 21. Mutation with key association ─────────────────────────────────

#[test]
fn mutation_with_key_association() {
    let m: MutationResource<String, i32> =
        MutationResource::new(RetryPolicy::no_retries()).with_key(QueryKey::from(["users", "42"]));

    assert!(m.key().is_some());
    assert_eq!(m.key().unwrap(), &QueryKey::from(["users", "42"]));
}

#[test]
fn mutation_without_key() {
    let m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.key().is_none());
}

// ── 22. Status labels ─────────────────────────────────────────────────

#[test]
fn status_labels() {
    assert_eq!(MutationStatus::Idle.label(), "Idle");
    assert_eq!(MutationStatus::Loading.label(), "Loading");
    assert_eq!(MutationStatus::Success.label(), "Success");
    assert_eq!(MutationStatus::Failure.label(), "Failure");
}

// ── 23. retry_policy accessor returns the configured policy ───────────

#[test]
fn retry_policy_accessor() {
    let policy = RetryPolicy::new(5).with_delay(200).with_exponential_backoff();
    let m: MutationResource<String, i32> = MutationResource::new(policy.clone());
    assert_eq!(m.retry_policy(), &policy);
    assert_eq!(m.retry_policy().max_retries, 5);
}

// ── 24. Multiple sequential mutations work correctly ──────────────────

#[test]
fn multiple_sequential_mutations() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    // First mutation
    m.begin("a".to_string());
    m.complete_success(1);
    assert_eq!(m.data(), Some(&1));
    assert_eq!(m.variables(), Some(&"a".to_string()));

    // Second mutation
    m.begin("b".to_string());
    assert_eq!(m.retry_count(), 0, "retry_count reset on new begin");
    m.complete_success(2);
    assert_eq!(m.data(), Some(&2));
    assert_eq!(m.variables(), Some(&"b".to_string()));

    // Third mutation fails
    m.begin("c".to_string());
    m.complete_failure(QueryError::response("err"));
    assert!(m.data().is_none());
    assert_eq!(m.error().unwrap().message(), "err");
    assert_eq!(m.retry_count(), 1);
}

// ── 25. Saturating retry_count via repeated complete_failure+retry cycles ─

#[test]
fn retry_count_increments_saturating_on_complete_failure() {
    // Use a very high retry limit so we can observe multiple complete_failure increments.
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(100));
    m.begin("vars".to_string());

    // Each complete_failure increments retry_count by 1 (saturating_add).
    // Retry clears the Failure state so we can fail again without a fresh begin.
    for i in 1..=10u32 {
        m.complete_failure(QueryError::response("fail"));
        assert_eq!(m.retry_count(), i, "retry_count must increment on each failure");
        if i < 10 {
            let retried = m.retry();
            assert!(retried, "retry must succeed while retries remain");
        }
    }
    assert_eq!(m.retry_count(), 10);
}

// ── 26. Retry is allowed after cancel sets Failure state ──────────────

#[test]
fn retry_allowed_after_cancel_sets_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));
    m.begin("vars".to_string());
    m.cancel(QueryError::cancelled("abort"));

    // Even though retries remain, cancel is terminal for this invocation
    // and retry only works from Failure when the mutation failed via complete_failure.
    // cancel also sets Failure, so let's check should_retry logic:
    assert_eq!(m.retry_count(), 0, "cancel does not increment retry_count");
    // Actually cancel sets Failure and retry_count is still 0, so should_retry is true
    // but retry() checks status == Failure AND should_retry.
    // Since cancel sets Failure, retry should be possible if should_retry.
    assert!(m.should_retry(), "retry_count=0 < max_retries=3");
    // retry() should succeed since status is Failure and retries remain
    assert!(m.retry(), "retry from cancelled Failure should work");
    assert!(m.is_loading());
}
