use crate::core::{QueryStatus, QueryTimestamp, RequestGuard, RequestId};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    pub fn complete_current_success(
        &mut self,
        request_id: RequestId,
        data: T,
        now_ms: u128,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_success(&guard, data, now_ms);
        true
    }

    pub fn complete_current_failure(&mut self, request_id: RequestId, error: impl Into<E>) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_failure(&guard, error);
        true
    }

    pub fn complete_current_optional_success(
        &mut self,
        request_id: RequestId,
        data: Option<T>,
        now_ms: u128,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_success_optional(&guard, data, now_ms);
        true
    }

    pub fn complete_current_failure_with_data(
        &mut self,
        request_id: RequestId,
        data: T,
        error: impl Into<E>,
    ) -> bool {
        let Some(guard) = self.accept_current_request(request_id) else {
            return false;
        };
        self.complete_failure_with_data(&guard, data, error);
        true
    }

    pub fn complete_success(&mut self, _guard: &RequestGuard, data: T, now_ms: u128) {
        self.apply_success(data, now_ms);
    }

    pub fn complete_failure(&mut self, _guard: &RequestGuard, error: impl Into<E>) {
        self.apply_failure(error);
    }

    pub fn complete_success_optional(
        &mut self,
        _guard: &RequestGuard,
        data: Option<T>,
        now_ms: u128,
    ) {
        self.apply_success_optional(data, now_ms);
    }

    pub fn complete_failure_with_data(
        &mut self,
        _guard: &RequestGuard,
        data: T,
        error: impl Into<E>,
    ) {
        self.apply_failure_with_data(data, error);
    }

    pub(crate) fn apply_success(&mut self, data: T, now_ms: u128) {
        self.previous_data = self.data.take();
        self.status = QueryStatus::Success;
        self.data = Some(data);
        self.error = None;
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));

        // Memory optimization: initial_data is only useful before the first
        // successful fetch. Once real data has arrived the extra copy is
        // redundant, so drop it to halve the initial-data memory footprint.
        drop(self.initial_data.take());
    }

    pub(crate) fn apply_failure(&mut self, error: impl Into<E>) {
        self.status = QueryStatus::Failure;
        self.error = Some(error.into());
        self.active_request_id = None;
    }

    pub(crate) fn apply_success_optional(&mut self, data: Option<T>, now_ms: u128) {
        self.previous_data = self.data.take();
        self.status = QueryStatus::Success;
        self.data = data;
        self.error = None;
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));

        // Memory optimization: same rationale as apply_success — the
        // initial_data copy is redundant once a real fetch has resolved.
        drop(self.initial_data.take());
    }

    pub(crate) fn apply_failure_with_data(&mut self, data: T, error: impl Into<E>) {
        self.status = QueryStatus::Failure;
        self.data = Some(data);
        self.error = Some(error.into());
        self.active_request_id = None;
    }
}
