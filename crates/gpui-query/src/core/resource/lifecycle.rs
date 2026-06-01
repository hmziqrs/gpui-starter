use crate::core::{
    QueryBeginResult, QueryFetchMode, QuerySignal, QueryStatus, QueryTimestamp, RequestGuard,
    RequestId, RequestPolicy, RequestSequencer,
};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    pub fn begin_request(
        &mut self,
        sequencer: &mut RequestSequencer,
        now_ms: u128,
        fetch_mode: QueryFetchMode,
    ) -> QueryBeginResult {
        if fetch_mode == QueryFetchMode::Normal && self.should_short_circuit_cache(now_ms) {
            self.record_cache_hit();
            return QueryBeginResult::CacheHit;
        }

        if self.request_policy == RequestPolicy::IgnoreWhileLoading
            && let Some(active_request_id) = self.active_request_id
        {
            return QueryBeginResult::IgnoredWhileLoading { active_request_id };
        }

        let replaced_request_id = self.active_request_id;
        if replaced_request_id.is_some() {
            self.cancelled_count += 1;
        }

        let request_id = sequencer.next_request();
        let status = self.begin_loading(request_id, now_ms);
        QueryBeginResult::Started {
            request_id,
            status,
            replaced_request_id,
        }
    }

    pub(crate) fn begin_loading(&mut self, request_id: RequestId, now_ms: u128) -> QueryStatus {
        let status = if self.has_data() {
            QueryStatus::LoadingWithData
        } else {
            QueryStatus::LoadingEmpty
        };
        self.status = status;
        self.active_request_id = Some(request_id);
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(QuerySignal::new());
        status
    }

    pub fn is_current_request(&self, request_id: RequestId) -> bool {
        self.active_request_id == Some(request_id)
    }

    pub fn accept_current_request(&mut self, request_id: RequestId) -> Option<RequestGuard> {
        if self.is_current_request(request_id) {
            self.active_request_id = None;
            Some(RequestGuard::new(request_id))
        } else {
            self.mark_ignored_result();
            None
        }
    }

    /// Cancel the active request. Returns `false` if there is no active request.
    ///
    /// The `error` is stored and can be retrieved via [`error()`](super::QueryResource::error).
    /// If a signal exists, it is also cancelled so the in-flight fetcher can observe it.
    pub fn cancel(&mut self, error: E) -> bool {
        if self.active_request_id.is_none() {
            return false;
        }

        self.active_request_id = None;
        self.status = QueryStatus::Cancelled;
        self.error = Some(error);
        self.cancelled_count += 1;

        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }

        true
    }

    pub fn mark_ignored_result(&mut self) {
        self.ignored_results += 1;
    }

    pub fn reset(&mut self) {
        self.status = QueryStatus::Idle;
        self.data = None;
        self.error = None;
        self.active_request_id = None;
        self.started_at = None;
        self.last_updated_at = None;
        self.cache_hits = 0;
        self.cancelled_count = 0;
        self.ignored_results = 0;
        self.retry_count = 0;
        self.placeholder_data = None;
        self.previous_data = None;
        self.initial_data = None;
        self.signal = None;
    }

    /// Set (or clear) placeholder data for this resource.
    ///
    /// Placeholder data is returned by [`display_data`](super::QueryResource::display_data)
    /// when no actual data is available yet. This is useful for showing a
    /// previously-known value while a new key is loading, similar to
    /// TanStack Query's `placeholderData`.
    pub fn set_placeholder_data(&mut self, data: Option<T>) {
        self.placeholder_data = data;
    }

    /// Roll back to the previous data, if available.
    ///
    /// On success, the resource transitions to `Success` status with the
    /// previous data restored. Returns `true` if rollback succeeded.
    /// Returns `false` if there is no previous data to restore.
    ///
    /// This is the core mechanism for optimistic update rollback.
    pub fn rollback_to_previous(&mut self) -> bool {
        if let Some(prev) = self.previous_data.take() {
            self.data = Some(prev);
            self.status = QueryStatus::Success;
            return true;
        }
        false
    }

    /// Apply an optimistic update to the resource's data.
    ///
    /// Stores the current data in [`previous_data`](super::QueryResource::previous_data)
    /// for rollback via [`rollback_to_previous`](super::QueryResource::rollback_to_previous),
    /// then sets the new data. Does **not** change the request status or active request.
    ///
    /// Typical usage:
    /// 1. Call `set_data(optimistic_value)` before starting a mutation.
    /// 2. If the mutation succeeds, complete the request normally — the real
    ///    data overwrites the optimistic value.
    /// 3. If the mutation fails, call `rollback_to_previous()` to restore the
    ///    original data.
    pub fn set_data(&mut self, data: T) {
        self.previous_data = self.data.take();
        self.data = Some(data);
    }

    /// Clear the resource's data optimistically.
    ///
    /// Stores the current data in [`previous_data`](super::QueryResource::previous_data)
    /// for rollback via [`rollback_to_previous`](super::QueryResource::rollback_to_previous),
    /// then sets data to `None`. Does **not** change the request status or active request.
    pub fn clear_data(&mut self) {
        self.previous_data = self.data.take();
    }

    /// Seed initial data into the resource.
    ///
    /// Only takes effect when the status is [`Idle`](QueryStatus::Idle) and
    /// [`data()`](super::QueryResource::data) is `None`. Sets both
    /// `initial_data` and `data` to `Some(data)`, and records `now_ms` as
    /// `last_updated_at`.
    pub fn set_initial_data(&mut self, data: T, now_ms: u128)
    where
        T: Clone,
    {
        if self.status == QueryStatus::Idle && self.data.is_none() {
            self.initial_data = Some(data);
            self.data = self.initial_data.clone();
            self.last_updated_at = Some(QueryTimestamp::from(now_ms));
        }
    }

    /// Clear the stored initial data.
    ///
    /// Does **not** touch `data` — only removes the retained `initial_data`
    /// reference so it will not survive a [`reset`](super::QueryResource::reset).
    pub fn clear_initial_data(&mut self) {
        self.initial_data = None;
    }
}
