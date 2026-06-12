//! Mutation resource for tracking async write operations.
//!
//! [`MutationResource`] tracks the lifecycle of a single mutation — from idle,
//! through loading, to success or failure. It supports retry via a configurable
//! [`RetryPolicy`] and cooperative cancellation through [`QuerySignal`].
//!
//! This module depends only on `serde` — zero framework coupling.

use serde::{Deserialize, Serialize};

use super::{QueryError, QueryKey, QuerySignal, RetryPolicy};

/// Status of a mutation operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationStatus {
    /// No mutation has been started yet.
    Idle,
    /// Mutation is in progress.
    Loading,
    /// Mutation completed successfully.
    Success,
    /// Mutation failed.
    Failure,
}

impl MutationStatus {
    /// Human-readable label for the status.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Loading => "Loading",
            Self::Success => "Success",
            Self::Failure => "Failure",
        }
    }
}

/// A mutation resource that tracks the state of a single mutation.
///
/// `V` is the variables (input) type, `T` is the success output type,
/// and `E` is the error type.
///
/// # Lifecycle
///
/// 1. **Idle** — initial state, no mutation in progress.
/// 2. **Loading** — mutation started via [`begin`](MutationResource::begin),
///    variables stored, signal created.
/// 3. **Success** — mutation completed via
///    [`complete_success`](MutationResource::complete_success).
/// 4. **Failure** — mutation failed via
///    [`complete_failure`](MutationResource::complete_failure).
///    If retries remain, [`retry`](MutationResource::retry) transitions back
///    to Loading.
///
/// # Retry
///
/// Each failure increments an internal retry counter. Call
/// [`should_retry`](MutationResource::should_retry) to check whether another
/// attempt is allowed, then [`retry`](MutationResource::retry) to re-enter
/// the Loading state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationResource<V, T, E = QueryError> {
    key: Option<QueryKey>,
    status: MutationStatus,
    data: Option<T>,
    error: Option<E>,
    variables: Option<V>,
    retry_count: u32,
    retry_policy: RetryPolicy,
    /// Timestamp (ms) of the most recent [`begin`](MutationResource::begin) call.
    /// Used by [`MutationBucket::gc`](crate::client::mutation_bucket::MutationBucket::gc)
    /// to decide whether the resource is old enough to evict.
    created_at: u64,
    #[serde(skip)]
    signal: Option<QuerySignal>,
}

impl<V, T, E> MutationResource<V, T, E> {
    /// Create a new mutation resource with the given retry policy.
    pub fn new(retry_policy: RetryPolicy) -> Self {
        Self {
            key: None,
            status: MutationStatus::Idle,
            data: None,
            error: None,
            variables: None,
            retry_count: 0,
            retry_policy,
            created_at: 0,
            signal: None,
        }
    }

    /// The current status of this mutation.
    pub fn status(&self) -> MutationStatus {
        self.status
    }

    /// The most recent successful data, if any.
    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    /// The most recent error, if any.
    pub fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    /// The variables (input) for the current or most recent mutation.
    pub fn variables(&self) -> Option<&V> {
        self.variables.as_ref()
    }

    /// How many retries have been attempted so far.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// A reference to the retry policy.
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    /// Whether the mutation is currently loading.
    pub fn is_loading(&self) -> bool {
        self.status == MutationStatus::Loading
    }

    /// Whether the mutation is idle (not started).
    pub fn is_idle(&self) -> bool {
        self.status == MutationStatus::Idle
    }

    /// Whether the mutation completed successfully.
    pub fn is_success(&self) -> bool {
        self.status == MutationStatus::Success
    }

    /// Whether the mutation failed.
    pub fn is_failure(&self) -> bool {
        self.status == MutationStatus::Failure
    }

    /// The optional query key associated with this mutation.
    ///
    /// When set, the key can be used to correlate mutations with specific
    /// query resources for cache invalidation or optimistic updates.
    pub fn key(&self) -> Option<&QueryKey> {
        self.key.as_ref()
    }

    /// Associate a query key with this mutation.
    ///
    /// Returns `self` for builder-style chaining.
    pub fn with_key(mut self, key: QueryKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Start a mutation with the given variables.
    ///
    /// Transitions to [`Loading`](MutationStatus::Loading), stores the
    /// variables, clears any previous error, records `now_ms` as the
    /// creation timestamp, and creates a fresh cancellation signal.
    ///
    /// `now_ms` is a monotonically-nondecreasing timestamp in milliseconds,
    /// typically sourced from [`QueryClient`](crate::client::QueryClient)'s
    /// clock. It is used by garbage collection to determine resource age.
    pub fn begin(&mut self, variables: V, now_ms: u64) {
        self.status = MutationStatus::Loading;
        self.variables = Some(variables);
        self.error = None;
        self.created_at = now_ms;
        self.signal = Some(QuerySignal::new());
    }

    /// The timestamp (ms) when [`begin`](MutationResource::begin) was last
    /// called, or `0` if the mutation has never been started.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Complete the mutation successfully.
    ///
    /// Transitions to [`Success`](MutationStatus::Success) and stores the
    /// result data.
    pub fn complete_success(&mut self, data: T) {
        self.status = MutationStatus::Success;
        self.data = Some(data);
        self.error = None;
        self.signal = None;
    }

    /// Complete the mutation with a failure.
    ///
    /// Transitions to [`Failure`](MutationStatus::Failure), stores the error,
    /// and increments the retry counter.
    pub fn complete_failure(&mut self, error: E) {
        self.status = MutationStatus::Failure;
        self.error = Some(error);
        self.retry_count += 1;
        self.signal = None;
    }

    /// Whether another retry is allowed given the current retry count.
    pub fn should_retry(&self) -> bool {
        self.retry_policy.should_retry(self.retry_count)
    }

    /// Retry the mutation by transitioning back to Loading.
    ///
    /// Only valid when [`should_retry`](MutationResource::should_retry)
    /// returns `true` and the current status is
    /// [`Failure`](MutationStatus::Failure). Returns `true` if the retry
    /// was initiated.
    ///
    /// Variables from the original [`begin`](MutationResource::begin) call
    /// are preserved. A fresh cancellation signal is created.
    pub fn retry(&mut self) -> bool {
        if self.status != MutationStatus::Failure || !self.should_retry() {
            return false;
        }
        self.status = MutationStatus::Loading;
        self.error = None;
        self.signal = Some(QuerySignal::new());
        true
    }

    /// Reset the mutation back to idle.
    ///
    /// Clears all data, error, variables, retry count, signal, and creation
    /// timestamp.
    pub fn reset(&mut self) {
        self.status = MutationStatus::Idle;
        self.data = None;
        self.error = None;
        self.variables = None;
        self.retry_count = 0;
        self.created_at = 0;
        self.signal = None;
    }

    /// Returns a reference to the cancellation signal, if one exists.
    pub fn signal(&self) -> Option<&QuerySignal> {
        self.signal.as_ref()
    }

    /// Cancel the mutation with the given error.
    ///
    /// Sets the error, transitions to [`Failure`](MutationStatus::Failure),
    /// and cancels the signal so any in-flight work can observe it.
    pub fn cancel(&mut self, error: E) {
        self.status = MutationStatus::Failure;
        self.error = Some(error);
        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }
        self.signal = None;
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_mutation_is_idle() {
        let m: MutationResource<String, String> = MutationResource::new(RetryPolicy::no_retries());
        assert!(m.is_idle());
        assert!(!m.is_loading());
        assert!(!m.is_success());
        assert!(!m.is_failure());
        assert_eq!(m.status(), MutationStatus::Idle);
        assert!(m.data().is_none());
        assert!(m.error().is_none());
        assert!(m.variables().is_none());
        assert_eq!(m.retry_count(), 0);
    }

    #[test]
    fn begin_transitions_to_loading() {
        let mut m: MutationResource<String, String> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("my-vars".to_string(), 1_000);

        assert!(m.is_loading());
        assert_eq!(m.variables(), Some(&"my-vars".to_string()));
        assert!(m.error().is_none());
        assert!(m.signal().is_some());
        assert_eq!(m.created_at(), 1_000);
    }

    #[test]
    fn complete_success() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string(), 0);
        m.complete_success(42);

        assert!(m.is_success());
        assert_eq!(m.data(), Some(&42));
        assert!(m.error().is_none());
        assert!(m.signal().is_none());
        // Variables are preserved after success
        assert_eq!(m.variables(), Some(&"vars".to_string()));
    }

    #[test]
    fn complete_failure() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string(), 0);
        m.complete_failure(QueryError::response("bad"));

        assert!(m.is_failure());
        assert!(m.data().is_none());
        assert_eq!(m.error().unwrap().message(), "bad");
        assert_eq!(m.retry_count(), 1);
    }

    #[test]
    fn retry_resets_to_loading() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));

        m.begin("vars".to_string(), 0);
        m.complete_failure(QueryError::response("fail 1"));

        assert!(m.is_failure());
        assert_eq!(m.retry_count(), 1);
        assert!(m.should_retry());

        let retried = m.retry();
        assert!(retried);
        assert!(m.is_loading());
        assert!(m.error().is_none());
        assert!(m.signal().is_some());
        // Variables preserved
        assert_eq!(m.variables(), Some(&"vars".to_string()));
    }

    #[test]
    fn retry_respects_max_retries() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(1));

        m.begin("vars".to_string(), 0);

        // First failure, retry_count = 1
        m.complete_failure(QueryError::response("fail 1"));
        assert_eq!(m.retry_count(), 1);
        assert!(!m.should_retry()); // max_retries=1, so 1 is not < 1

        let retried = m.retry();
        assert!(!retried);
        assert!(m.is_failure());
    }

    #[test]
    fn reset_clears_everything() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

        m.begin("vars".to_string(), 0);
        m.complete_success(99);
        m.reset();

        assert!(m.is_idle());
        assert!(m.data().is_none());
        assert!(m.error().is_none());
        assert!(m.variables().is_none());
        assert_eq!(m.retry_count(), 0);
        assert!(m.signal().is_none());
    }

    #[test]
    fn cancel_sets_error_and_cancels_signal() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string(), 0);

        let signal = m.signal().unwrap().clone();
        assert!(!signal.is_cancelled());

        m.cancel(QueryError::cancelled("aborted"));

        assert!(m.is_failure());
        assert_eq!(m.error().unwrap().message(), "aborted");
        assert!(signal.is_cancelled());
        assert!(m.signal().is_none());
    }

    #[test]
    fn status_labels() {
        assert_eq!(MutationStatus::Idle.label(), "Idle");
        assert_eq!(MutationStatus::Loading.label(), "Loading");
        assert_eq!(MutationStatus::Success.label(), "Success");
        assert_eq!(MutationStatus::Failure.label(), "Failure");
    }

    #[test]
    fn retry_delay_calculation() {
        let mut m: MutationResource<String, i32> = MutationResource::new(
            RetryPolicy::new(5)
                .with_delay(500)
                .with_exponential_backoff()
                .with_max_delay(5000),
        );

        m.begin("vars".to_string(), 0);
        m.complete_failure(QueryError::response("fail")); // retry_count = 1
        assert_eq!(m.retry_policy().delay_for_attempt(0), 500);
        assert_eq!(m.retry_policy().delay_for_attempt(1), 1000);
        assert_eq!(m.retry_policy().delay_for_attempt(4), 5000); // capped
    }

    #[test]
    fn retry_only_from_failure_status() {
        let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

        // Cannot retry from Idle
        assert!(!m.retry());

        // Cannot retry from Loading
        m.begin("vars".to_string(), 0);
        assert!(!m.retry());

        // Can retry from Failure
        m.complete_failure(QueryError::response("fail"));
        assert!(m.retry());

        // After retry, back to Loading — cannot retry again without failing first
        assert!(!m.retry());
    }
}
