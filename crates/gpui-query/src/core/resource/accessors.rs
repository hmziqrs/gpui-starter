//! Query resource read-only accessors.

use crate::core::{
    CachePolicy, QueryKey, QuerySignal, QueryStatus, QueryTimestamp, RequestId, RequestPolicy,
    RetryPolicy,
};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    /// Whether the resource is currently loading.
    pub fn is_loading(&self) -> bool {
        self.status.is_loading()
    }

    /// Whether the resource is pending (no data yet).
    pub fn is_pending(&self) -> bool {
        self.status.is_pending()
    }

    /// The cache key.
    pub fn key(&self) -> &QueryKey {
        &self.key
    }

    /// Current status.
    pub fn status(&self) -> QueryStatus {
        self.status
    }

    /// Current data, if loaded.
    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    /// Current error, if any.
    pub fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    /// Active request id, if a request is in flight.
    pub fn active_request_id(&self) -> Option<RequestId> {
        self.active_request_id
    }

    /// The cache policy.
    pub fn cache_policy(&self) -> CachePolicy {
        self.cache_policy
    }

    /// The request policy.
    pub fn request_policy(&self) -> RequestPolicy {
        self.request_policy
    }

    /// When the current request started (ms since UNIX epoch).
    pub fn started_at_ms(&self) -> Option<u128> {
        self.started_at.map(QueryTimestamp::as_millis)
    }

    /// When data was last updated (ms since UNIX epoch).
    pub fn last_updated_at_ms(&self) -> Option<u128> {
        self.last_updated_at.map(QueryTimestamp::as_millis)
    }

    /// Total cache hits.
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    /// Total cancelled requests.
    pub fn cancelled_count(&self) -> u64 {
        self.cancelled_count
    }

    /// Total ignored (stale) results.
    pub fn ignored_results(&self) -> u64 {
        self.ignored_results
    }

    /// Whether data exists.
    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// The cancellation signal, if a request is in flight.
    pub fn signal(&self) -> Option<&QuerySignal> {
        self.signal.as_ref()
    }

    /// Mutable reference to the cancellation signal.
    pub fn signal_mut(&mut self) -> Option<&mut QuerySignal> {
        self.signal.as_mut()
    }

    /// Placeholder data (shown while loading before first fetch).
    pub fn placeholder_data(&self) -> Option<&T> {
        self.placeholder_data.as_ref()
    }

    /// Previous data (saved during optimistic updates for rollback).
    pub fn previous_data(&self) -> Option<&T> {
        self.previous_data.as_ref()
    }

    /// Initial data (seeded before first fetch).
    pub fn initial_data(&self) -> Option<&T> {
        self.initial_data.as_ref()
    }

    /// Data for display, falling back to placeholder.
    pub fn display_data(&self) -> Option<&T> {
        self.data.as_ref().or(self.placeholder_data.as_ref())
    }

    /// Current retry count.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// The retry policy.
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    /// Increment the retry counter.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    /// Set the retry policy.
    pub fn set_retry_policy(&mut self, policy: RetryPolicy) {
        self.retry_policy = policy;
    }

    /// Reset the retry counter to zero.
    pub fn reset_retry_count(&mut self) {
        self.retry_count = 0;
    }

    /// Set the cache policy.
    ///
    /// This allows policy updates on existing resources when `use_query` is
    /// called with the same key but different policies (e.g., a different TTL).
    pub fn set_cache_policy(&mut self, policy: CachePolicy) {
        self.cache_policy = policy;
    }

    /// Set the request policy.
    ///
    /// This allows policy updates on existing resources when `use_query` is
    /// called with the same key but different request behavior.
    pub fn set_request_policy(&mut self, policy: RequestPolicy) {
        self.request_policy = policy;
    }
}
