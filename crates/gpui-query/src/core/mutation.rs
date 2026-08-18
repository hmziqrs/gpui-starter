//! Mutation resource for tracking async write operations.

use serde::{Deserialize, Serialize};

use super::{QueryError, QueryKey, QuerySignal, RetryPolicy};

/// Status of a mutation operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationStatus {
    /// No mutation has been started yet.
    #[default]
    Idle,
    /// Mutation is in progress.
    Loading,
    /// Mutation completed successfully.
    Success,
    /// Mutation failed.
    Failure,
}

impl MutationStatus {
    /// Human-readable label.
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationResource<V, T, E = QueryError> {
    key: Option<QueryKey>,
    status: MutationStatus,
    data: Option<T>,
    error: Option<E>,
    variables: Option<V>,
    retry_count: u32,
    cancelled_count: u64,
    retry_policy: RetryPolicy,
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
            cancelled_count: 0,
            retry_policy,
            signal: None,
        }
    }

    /// Current status.
    pub fn status(&self) -> MutationStatus {
        self.status
    }

    /// Most recent successful data.
    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    /// Most recent error.
    pub fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    /// Variables for the current or most recent mutation.
    pub fn variables(&self) -> Option<&V> {
        self.variables.as_ref()
    }

    /// Current retry count.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// Number of times this mutation has been cancelled.
    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    /// The retry policy.
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    /// Whether the mutation is currently loading.
    pub fn is_loading(&self) -> bool {
        self.status == MutationStatus::Loading
    }

    /// Whether the mutation is idle.
    pub fn is_idle(&self) -> bool {
        self.status == MutationStatus::Idle
    }

    /// Whether the mutation succeeded.
    pub fn is_success(&self) -> bool {
        self.status == MutationStatus::Success
    }

    /// Whether the mutation failed.
    pub fn is_failure(&self) -> bool {
        self.status == MutationStatus::Failure
    }

    /// Optional query key for this mutation.
    pub fn key(&self) -> Option<&QueryKey> {
        self.key.as_ref()
    }

    /// Associate a query key with this mutation.
    pub fn with_key(mut self, key: QueryKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Start a mutation with the given variables.
    ///
    /// Transitions to `Loading`, stores variables, clears error, creates signal.
    /// Cancels any previous in-flight signal so a prior fetcher observes cancellation.
    /// Resets `retry_count` so each mutation invocation starts fresh, matching
    /// `QueryResource`'s behavior where the hook layer resets retries on success.
    pub fn begin(&mut self, variables: V) {
        // Cancel old signal before replacing, matching QueryResource/InfiniteQueryResource pattern.
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }
        self.status = MutationStatus::Loading;
        self.variables = Some(variables);
        self.error = None;
        self.retry_count = 0;
        self.signal = Some(QuerySignal::new());
    }

    /// Complete successfully.
    pub fn complete_success(&mut self, data: T) {
        self.status = MutationStatus::Success;
        self.data = Some(data);
        self.error = None;
        self.signal = None;
    }

    /// Complete with failure.
    ///
    /// Clears `data` so consumers do not see stale success data alongside
    /// a `Failure` status. Increments `retry_count` with saturating add to
    /// prevent wraparound.
    pub fn complete_failure(&mut self, error: E) {
        self.status = MutationStatus::Failure;
        self.data = None;
        self.error = Some(error);
        self.retry_count = self.retry_count.saturating_add(1);
        self.signal = None;
    }

    /// Whether another retry is allowed.
    pub fn should_retry(&self) -> bool {
        self.retry_policy.should_retry(self.retry_count)
    }

    /// Retry by transitioning back to Loading.
    ///
    /// Only valid from `Failure` when retries remain.
    /// A fresh cancellation signal is created.
    pub fn retry(&mut self) -> bool {
        if self.status != MutationStatus::Failure || !self.should_retry() {
            return false;
        }
        self.status = MutationStatus::Loading;
        self.error = None;
        self.signal = Some(QuerySignal::new());
        true
    }

    /// Reset to idle, clearing everything.
    pub fn reset(&mut self) {
        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }
        self.status = MutationStatus::Idle;
        self.data = None;
        self.error = None;
        self.variables = None;
        self.retry_count = 0;
        self.cancelled_count = 0;
        self.signal = None;
    }

    /// The cancellation signal.
    pub fn signal(&self) -> Option<&QuerySignal> {
        self.signal.as_ref()
    }

    /// Increment the retry counter.
    ///
    /// Used by the mutation retry loop to track how many attempts have been made
    /// without transitioning through a terminal `Failure` state.
    pub fn increment_retry(&mut self) {
        self.retry_count = self.retry_count.saturating_add(1);
    }

    /// Prepare for a retry by refreshing the signal without transitioning
    /// through `Failure`.
    ///
    /// This is the fix for audit finding #19: avoids a transient `Failure`
    /// status that would cause observers to see a brief Failure flash between
    /// retry attempts. The mutation stays in `Loading` state, the old signal
    /// is cancelled, and a fresh signal is created for the next attempt.
    pub fn prepare_retry(&mut self) {
        if self.status != MutationStatus::Loading {
            return;
        }
        // Cancel the old signal before creating a new one.
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }
        self.error = None;
        self.signal = Some(QuerySignal::new());
    }

    /// Reset the retry counter to zero.
    ///
    /// Called on terminal failure so that `retry_count` is clean for the
    /// next mutation invocation (audit finding #4).
    pub fn reset_retry_count(&mut self) {
        self.retry_count = 0;
    }

    /// Cancel the mutation.
    ///
    /// Only has effect when the mutation is in `Loading` state. Returns without
    /// side effects if the mutation is already `Idle`, `Success`, or `Failure`,
    /// matching the `QueryResource::cancel` behavior where a no-op cancel is silent.
    ///
    /// When effective, increments `cancelled_count` for diagnostics and sets
    /// status to `Failure`.
    pub fn cancel(&mut self, error: E) {
        if self.status != MutationStatus::Loading {
            return;
        }
        self.cancelled_count += 1;
        self.status = MutationStatus::Failure;
        self.error = Some(error);
        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }
        self.signal = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_mutation_is_idle() {
        let m: MutationResource<String, String> =
            MutationResource::new(RetryPolicy::no_retries());
        assert!(m.is_idle());
        assert_eq!(m.status(), MutationStatus::Idle);
    }

    #[test]
    fn begin_transitions_to_loading() {
        let mut m: MutationResource<String, String> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        assert!(m.is_loading());
        assert_eq!(m.variables(), Some(&"vars".to_string()));
    }

    #[test]
    fn complete_success_stores_data() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        m.complete_success(42);
        assert!(m.is_success());
        assert_eq!(m.data(), Some(&42));
    }

    #[test]
    fn complete_failure_stores_error() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        m.complete_failure(QueryError::response("bad"));
        assert!(m.is_failure());
        assert_eq!(m.retry_count(), 1);
        assert!(m.data().is_none(), "data should be cleared on failure");
    }

    #[test]
    fn retry_from_failure() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::new(2));
        m.begin("vars".to_string());
        m.complete_failure(QueryError::response("fail"));
        assert!(m.retry());
        assert!(m.is_loading());
    }

    #[test]
    fn retry_respects_max() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::new(1));
        m.begin("vars".to_string());
        m.complete_failure(QueryError::response("fail"));
        assert!(!m.should_retry()); // retry_count=1, max=1
        assert!(!m.retry());
    }

    #[test]
    fn reset_clears_everything() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::new(3));
        m.begin("vars".to_string());
        m.complete_success(99);
        m.reset();
        assert!(m.is_idle());
        assert!(m.data().is_none());
        assert_eq!(m.retry_count(), 0);
    }

    #[test]
    fn cancel_cancels_signal() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        let signal = m.signal().unwrap().clone();
        assert!(!signal.is_cancelled());
        m.cancel(QueryError::cancelled("aborted"));
        assert!(signal.is_cancelled());
    }

    #[test]
    fn begin_cancels_old_signal() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("first".to_string());
        let old_signal = m.signal().unwrap().clone();
        assert!(!old_signal.is_cancelled());
        // Starting a new mutation should cancel the old signal.
        m.begin("second".to_string());
        assert!(old_signal.is_cancelled());
        // New signal should not be cancelled.
        assert!(!m.signal().unwrap().is_cancelled());
    }

    #[test]
    fn complete_failure_clears_previous_data() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::new(2));
        m.begin("vars".to_string());
        m.complete_success(42);
        assert_eq!(m.data(), Some(&42));
        // Succeed then fail: data should be cleared.
        m.begin("vars2".to_string());
        m.complete_failure(QueryError::response("fail"));
        assert!(m.is_failure());
        assert!(m.data().is_none(), "data from previous success must be cleared on failure");
    }

    #[test]
    fn cancel_increments_cancelled_count() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        assert_eq!(m.cancelled_count(), 0);
        m.begin("vars".to_string());
        m.cancel(QueryError::cancelled("aborted"));
        assert_eq!(m.cancelled_count(), 1);
        m.begin("vars2".to_string());
        m.cancel(QueryError::cancelled("aborted2"));
        assert_eq!(m.cancelled_count(), 2);
    }

    #[test]
    fn reset_clears_cancelled_count() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        m.cancel(QueryError::cancelled("aborted"));
        assert_eq!(m.cancelled_count(), 1);
        m.reset();
        assert_eq!(m.cancelled_count(), 0);
    }

    #[test]
    fn cancel_on_idle_is_noop() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        assert!(m.is_idle());
        m.cancel(QueryError::cancelled("aborted"));
        assert!(m.is_idle(), "cancel on Idle should be a no-op");
        assert_eq!(m.cancelled_count(), 0);
        assert!(m.error().is_none());
    }

    #[test]
    fn cancel_on_success_is_noop() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        m.complete_success(42);
        m.cancel(QueryError::cancelled("aborted"));
        assert!(m.is_success(), "cancel on Success should be a no-op");
        assert_eq!(m.cancelled_count(), 0);
        assert_eq!(m.data(), Some(&42));
    }

    #[test]
    fn cancel_on_failure_is_noop() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::no_retries());
        m.begin("vars".to_string());
        m.complete_failure(QueryError::response("fail"));
        m.cancel(QueryError::cancelled("aborted"));
        assert!(m.is_failure(), "cancel on Failure should be a no-op");
        assert_eq!(m.cancelled_count(), 0);
    }

    #[test]
    fn begin_resets_retry_count() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::new(2));
        m.begin("vars".to_string());
        m.complete_failure(QueryError::response("fail"));
        assert_eq!(m.retry_count(), 1);
        // Starting a new invocation resets retry_count.
        m.begin("vars2".to_string());
        assert_eq!(m.retry_count(), 0, "begin() should reset retry_count");
    }

    #[test]
    fn begin_resets_retry_count_allows_fresh_retries() {
        let mut m: MutationResource<String, i32> =
            MutationResource::new(RetryPolicy::new(1));
        // First invocation: fail, exhaust retries.
        m.begin("vars".to_string());
        m.complete_failure(QueryError::response("fail"));
        assert_eq!(m.retry_count(), 1);
        assert!(!m.should_retry(), "retries exhausted after first invocation");
        // Second invocation: begin resets retry_count, so retries are fresh.
        m.begin("vars2".to_string());
        assert_eq!(m.retry_count(), 0);
        assert!(m.should_retry(), "should_retry should be true after begin resets retry_count");
        m.complete_failure(QueryError::response("fail again"));
        assert_eq!(m.retry_count(), 1);
        assert!(!m.should_retry(), "retries exhausted after second invocation");
    }
}
