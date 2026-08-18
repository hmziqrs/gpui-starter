//! Retry behaviour, retry count tracking, prepare_retry, increment/reset retry helpers.

use crate::core::*;

// -- Retry from Failure transitions to Loading --

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

// -- Retry count is respected (max reached) --

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

// -- Retry only works from Failure status --

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

    // After retry, back to Loading -- cannot retry again
    assert!(m.is_loading());
    assert!(!m.retry());
}

// -- Retry creates a fresh (non-cancelled) signal --

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
    // old_signal is independent -- it may or may not be cancelled depending on impl,
    // but new_signal must definitely be fresh and uncancelled
    assert_ne!(old_signal, *new_signal, "signals must be different objects");
}

// -- prepare_retry stays in Loading with fresh signal --

#[test]
fn prepare_retry_stays_in_loading_with_fresh_signal() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));
    m.begin("vars".to_string());

    let old_signal = m.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    m.prepare_retry();

    // Must still be Loading -- no transient Failure flash
    assert!(m.is_loading(), "prepare_retry must not leave Loading");
    assert!(m.error().is_none());
    assert!(old_signal.is_cancelled(), "old signal must be cancelled by prepare_retry");

    let new_signal = m.signal().unwrap();
    assert!(!new_signal.is_cancelled(), "new signal must be fresh");
}

// -- prepare_retry is no-op when not in Loading --

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

// -- increment_retry bumps counter without state transition --

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

// -- reset_retry_count zeroes the counter --

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

// -- Saturating retry_count via repeated complete_failure+retry cycles --

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

// -- Retry is allowed after cancel sets Failure state --

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
