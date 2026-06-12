//! Query resource completion methods.

use crate::core::{CachePolicy, QueryStatus, QueryTimestamp, RequestGuard, RequestId};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    /// Complete the current request with success by request id.
    ///
    /// Convenience method that accepts + completes in one call.
    /// Returns `true` if the request was accepted.
    pub fn complete_current_success(
        &mut self,
        request_id: RequestId,
        data: T,
        now_ms: u128,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_success(guard, data, now_ms);
        true
    }

    /// Complete the current request with failure by request id.
    pub fn complete_current_failure(
        &mut self,
        request_id: RequestId,
        error: impl Into<E>,
        now_ms: u128,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_failure(guard, error, now_ms);
        true
    }

    /// Complete the current request with optional success by request id.
    pub fn complete_current_optional_success(
        &mut self,
        request_id: RequestId,
        data: Option<T>,
        now_ms: u128,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_success_optional(guard, data, now_ms);
        true
    }

    /// Complete the current request with failure but retain data by request id.
    pub fn complete_current_failure_with_data(
        &mut self,
        request_id: RequestId,
        data: T,
        error: impl Into<E>,
        now_ms: u128,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_failure_with_data(guard, data, error, now_ms);
        true
    }

    /// Complete with success, consuming the guard (two-phase protocol).
    ///
    /// The guard is moved, preventing double-completion at the type level.
    /// Validates that no new request was started after the guard was issued.
    pub fn complete_success(&mut self, guard: RequestGuard, data: T, now_ms: u128) {
        self.validate_guard(&guard);
        self.apply_success(data, now_ms);
    }

    /// Complete with failure, consuming the guard (two-phase protocol).
    ///
    /// The guard is moved, preventing double-completion at the type level.
    /// Validates that no new request was started after the guard was issued.
    pub fn complete_failure(&mut self, guard: RequestGuard, error: impl Into<E>, now_ms: u128) {
        self.validate_guard(&guard);
        self.apply_failure(error, now_ms);
    }

    /// Complete with optional success, consuming the guard.
    ///
    /// If `data` is `None`, the status is set to [`QueryStatus::Idle`] rather than
    /// [`QueryStatus::Success`] to maintain the invariant that Success implies data exists.
    pub fn complete_success_optional(
        &mut self,
        guard: RequestGuard,
        data: Option<T>,
        now_ms: u128,
    ) {
        self.validate_guard(&guard);
        self.apply_success_optional(data, now_ms);
    }

    /// Complete with failure but retain data, consuming the guard.
    ///
    /// Validates that no new request was started after the guard was issued.
    pub fn complete_failure_with_data(
        &mut self,
        guard: RequestGuard,
        data: T,
        error: impl Into<E>,
        now_ms: u128,
    ) {
        self.validate_guard(&guard);
        self.apply_failure_with_data(data, error, now_ms);
    }

    pub(crate) fn apply_success(&mut self, data: T, now_ms: u128) {
        self.previous_data = self.data.take();
        self.status = QueryStatus::Success;
        self.data = Some(data);
        self.error = None;
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
    }

    pub(crate) fn apply_failure(&mut self, error: impl Into<E>, now_ms: u128) {
        self.status = QueryStatus::Failure;
        self.error = Some(error.into());
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
    }

    pub(crate) fn apply_success_optional(&mut self, data: Option<T>, now_ms: u128) {
        self.previous_data = self.data.take();
        // When data is None, use Idle instead of Success to maintain the
        // invariant that Success always implies data is available. Callers
        // that check status() == Success and then unwrap data() will not panic.
        if data.is_some() {
            self.status = QueryStatus::Success;
        } else {
            self.status = QueryStatus::Idle;
        }
        self.data = data;
        self.error = None;
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
    }

    pub(crate) fn apply_failure_with_data(&mut self, data: T, error: impl Into<E>, now_ms: u128) {
        self.status = QueryStatus::Failure;
        self.data = Some(data);
        self.error = Some(error.into());
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
    }

    /// Validate that the guard is still valid for the current resource state.
    ///
    /// `accept_current_request` clears `active_request_id`, so we cannot compare
    /// directly. However, if `active_request_id` is `Some`, it means a *new*
    /// request was started after the guard was issued but before completion.
    /// This indicates the caller interleaved `begin_request` between accept and
    /// complete, which would overwrite the newer request's state with stale data.
    fn validate_guard(&self, guard: &RequestGuard) {
        debug_assert!(
            self.active_request_id.is_none(),
            "complete_* called with stale guard (id={}) but a new request is active ({:?}). \
             The caller must not interleave begin_request between accept and complete.",
            guard.request_id().label(),
            self.active_request_id,
        );
    }
}

impl<T, E> QueryResource<T, E> {
    /// Whether data should be evicted after observers consume it.
    ///
    /// Returns `true` when `CachePolicy::NoCache` is set, meaning stored data
    /// will never be used for cache hits and should be cleared after delivery
    /// to avoid holding it in memory indefinitely.
    pub fn should_clear_data_on_complete(&self) -> bool {
        matches!(self.cache_policy, CachePolicy::NoCache)
    }
}
