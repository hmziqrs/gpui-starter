use crate::core::{
    CachePolicy, QueryKey, QuerySignal, QueryStatus, QueryTimestamp, RequestId, RequestPolicy,
    RetryPolicy,
};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    pub fn is_loading(&self) -> bool {
        self.status.is_loading()
    }

    pub fn key(&self) -> &QueryKey {
        &self.key
    }

    pub fn status(&self) -> QueryStatus {
        self.status
    }

    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    pub fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    pub fn active_request_id(&self) -> Option<RequestId> {
        self.active_request_id
    }

    pub fn cache_policy(&self) -> CachePolicy {
        self.cache_policy
    }

    pub fn request_policy(&self) -> RequestPolicy {
        self.request_policy
    }

    pub fn started_at_ms(&self) -> Option<u128> {
        self.started_at.map(QueryTimestamp::as_millis)
    }

    pub fn last_updated_at_ms(&self) -> Option<u128> {
        self.last_updated_at.map(QueryTimestamp::as_millis)
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    pub fn ignored_results(&self) -> u64 {
        self.ignored_results
    }

    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// Returns a reference to the cancellation signal for the current request,
    /// if one exists. The signal is created when a request begins and cleared on reset.
    pub fn signal(&self) -> Option<&QuerySignal> {
        self.signal.as_ref()
    }

    /// Returns a mutable reference to the cancellation signal for the current request,
    /// if one exists.
    pub fn signal_mut(&mut self) -> Option<&mut QuerySignal> {
        self.signal.as_mut()
    }

    /// Returns the placeholder data, if set.
    ///
    /// Placeholder data is shown while loading before the first fetch completes,
    /// similar to TanStack Query's `placeholderData`.
    pub fn placeholder_data(&self) -> Option<&T> {
        self.placeholder_data.as_ref()
    }

    /// Returns the previous data, if set.
    ///
    /// Previous data is stored automatically when `apply_success` or
    /// `apply_success_optional` overwrites existing data. It can be
    /// restored via [`rollback_to_previous`](super::QueryResource::rollback_to_previous).
    pub fn previous_data(&self) -> Option<&T> {
        self.previous_data.as_ref()
    }

    /// Returns the initial data, if set.
    ///
    /// Initial data is seeded via [`set_initial_data`](super::QueryResource::set_initial_data)
    /// and serves as the default value before any fetch completes.
    pub fn initial_data(&self) -> Option<&T> {
        self.initial_data.as_ref()
    }

    /// Returns the data for display, falling back to placeholder data.
    ///
    /// If actual data is present, it is returned. Otherwise, placeholder
    /// data is returned (if set). This is the recommended accessor for
    /// UI rendering.
    pub fn display_data(&self) -> Option<&T> {
        self.data.as_ref().or(self.placeholder_data.as_ref())
    }

    /// How many retries have been attempted for the current request cycle.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// A reference to the retry policy for this resource.
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    /// Increment the retry counter, typically called after a failed request.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    /// Replace the retry policy for this resource.
    pub fn set_retry_policy(&mut self, policy: RetryPolicy) {
        self.retry_policy = policy;
    }

    /// Reset the retry counter to zero.
    pub fn reset_retry_count(&mut self) {
        self.retry_count = 0;
    }
}
