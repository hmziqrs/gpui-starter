//! Query resource lifecycle: begin, cancel, reset, optimistic updates.

use crate::core::{
    QueryBeginResult, QueryFetchMode, QuerySignal, QueryStatus, QueryTimestamp, RequestGuard,
    RequestId, RequestPolicy, RequestSequencer,
};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    /// Begin a new request on this resource.
    ///
    /// Respects the cache policy (may return `CacheHit`) and request policy
    /// (`IgnoreWhileLoading` or `LatestWins`). When replacing an existing
    /// request, the old signal is **cancelled** so the in-flight fetcher
    /// can observe it and abort early.
    pub fn begin_request(
        &mut self,
        sequencer: &mut RequestSequencer,
        now_ms: u128,
        fetch_mode: QueryFetchMode,
    ) -> QueryBeginResult {
        // 1. Fresh cache hit — no fetch needed at all.
        if fetch_mode == QueryFetchMode::Normal && self.should_short_circuit_cache(now_ms) {
            self.record_cache_hit();
            return QueryBeginResult::CacheHit;
        }

        // 2. Stale-while-revalidate: serve stale data immediately, trigger
        //    background refetch. This is checked before the IgnoreWhileLoading
        //    guard so we always revalidate stale data even if another request
        //    is in flight (the new request replaces it via LatestWins below).
        if fetch_mode == QueryFetchMode::Normal && self.should_serve_stale_and_revalidate(now_ms) {
            self.record_stale_cache_hit();

            // If IgnoreWhileLoading and a request is already active, skip the
            // background refetch — an in-flight request will refresh the data.
            if self.request_policy == RequestPolicy::IgnoreWhileLoading
                && let Some(active_request_id) = self.active_request_id
            {
                return QueryBeginResult::StaleCacheHit {
                    request_id: active_request_id,
                    status: self.status,
                    replaced_request_id: None,
                };
            }

            let replaced_request_id = self.active_request_id;
            if replaced_request_id.is_some() {
                self.cancelled_count += 1;
            }

            let request_id = sequencer.next_request();
            let status = self.begin_loading(request_id, now_ms);
            return QueryBeginResult::StaleCacheHit {
                request_id,
                status,
                replaced_request_id,
            };
        }

        // 3. IgnoreWhileLoading guard for normal (non-stale) requests.
        if self.request_policy == RequestPolicy::IgnoreWhileLoading
            && let Some(active_request_id) = self.active_request_id
        {
            return QueryBeginResult::IgnoredWhileLoading { active_request_id };
        }

        // 4. Normal fetch — start a new request.
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

    /// Like [`begin_request`] but accepts an optional pre-generated `RequestId`
    /// instead of using a `RequestSequencer`.
    ///
    /// When `maybe_request_id` is `Some`, uses that ID directly (useful when
    /// the bucket's co-located sequencer has already generated the ID).
    /// When `None`, falls back to a transient `RequestSequencer::new()` for
    /// compatibility.
    ///
    /// This is the preferred entry point for the hook layer (audit fixes
    /// #1/#5/#15/#18): it allows the bucket's persistent sequencer to provide
    /// globally unique, monotonically increasing RequestIds.
    pub fn begin_request_with_id(
        &mut self,
        maybe_request_id: Option<RequestId>,
        now_ms: u128,
        fetch_mode: QueryFetchMode,
    ) -> QueryBeginResult {
        // 1. Fresh cache hit — no fetch needed at all.
        if fetch_mode == QueryFetchMode::Normal && self.should_short_circuit_cache(now_ms) {
            self.record_cache_hit();
            return QueryBeginResult::CacheHit;
        }

        // 2. Stale-while-revalidate
        if fetch_mode == QueryFetchMode::Normal && self.should_serve_stale_and_revalidate(now_ms) {
            self.record_stale_cache_hit();

            if self.request_policy == RequestPolicy::IgnoreWhileLoading
                && let Some(active_request_id) = self.active_request_id
            {
                return QueryBeginResult::StaleCacheHit {
                    request_id: active_request_id,
                    status: self.status,
                    replaced_request_id: None,
                };
            }

            let replaced_request_id = self.active_request_id;
            if replaced_request_id.is_some() {
                self.cancelled_count += 1;
            }

            let request_id =
                maybe_request_id.unwrap_or_else(|| RequestSequencer::new().next_request());
            let status = self.begin_loading(request_id, now_ms);
            return QueryBeginResult::StaleCacheHit {
                request_id,
                status,
                replaced_request_id,
            };
        }

        // 3. IgnoreWhileLoading guard
        if self.request_policy == RequestPolicy::IgnoreWhileLoading
            && let Some(active_request_id) = self.active_request_id
        {
            return QueryBeginResult::IgnoredWhileLoading { active_request_id };
        }

        // 4. Normal fetch — start a new request.
        let replaced_request_id = self.active_request_id;
        if replaced_request_id.is_some() {
            self.cancelled_count += 1;
        }

        let request_id = maybe_request_id.unwrap_or_else(|| RequestSequencer::new().next_request());
        let status = self.begin_loading(request_id, now_ms);
        QueryBeginResult::Started {
            request_id,
            status,
            replaced_request_id,
        }
    }

    /// Internal: transition to a loading state.
    ///
    /// **v2 fix**: Cancels the old signal before creating a new one,
    /// so in-flight fetchers for replaced requests can abort early.
    ///
    /// Note: This method performs no guard against the current status. Under
    /// `LatestWins` policy, a second call while already `LoadingEmpty` is
    /// intentional — it cancels the old request and starts a new one. The old
    /// request's async task holds a stale `RequestId` and will be rejected by
    /// `accept_current_request()`.
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

        // v2 fix: Cancel the OLD signal before replacing it.
        if let Some(old_signal) = self.signal.as_ref() {
            old_signal.cancel();
        }
        self.signal = Some(QuerySignal::new());

        status
    }

    /// Whether the given request id is the current active request.
    pub fn is_current_request(&self, request_id: RequestId) -> bool {
        self.active_request_id == Some(request_id)
    }

    /// Accept a request for completion.
    ///
    /// Returns a [`RequestGuard`] if the request is still active, or `None`
    /// if it was replaced or cancelled. The guard is a capability token for
    /// the two-phase protocol (validate → complete).
    pub fn accept_current_request(&mut self, request_id: RequestId) -> Option<RequestGuard> {
        if self.is_current_request(request_id) {
            self.active_request_id = None;
            Some(RequestGuard::new(request_id))
        } else {
            self.mark_ignored_result();
            None
        }
    }

    /// Cancel the active request.
    ///
    /// Returns `false` if there is no active request.
    /// The signal is cancelled so the in-flight fetcher can observe it.
    ///
    /// Data is preserved across cancellations. Current data (if any) is saved
    /// to `previous_data` before being cleared, allowing recovery via
    /// `rollback_to_previous()`. This matches TanStack Query behavior where
    /// cancelling a refetch does not destroy existing data.
    ///
    /// When the resource was in `LoadingEmpty` status (no prior data existed),
    /// both `data` and `previous_data` remain `None`. When the resource was in
    /// `LoadingWithData` status (a refetch with existing data), the prior data
    /// is saved to `previous_data` and `data` is set to `None`. Callers can use
    /// `rollback_to_previous()` to recover the data if needed.
    pub fn cancel(&mut self, error: E) -> bool {
        if self.active_request_id.is_none() {
            return false;
        }

        self.active_request_id = None;
        self.status = QueryStatus::Cancelled;
        self.error = Some(error);
        self.cancelled_count += 1;

        // Save current data to previous_data before clearing so
        // rollback_to_previous() can recover it.
        if self.data.is_some() {
            self.previous_data = self.data.take();
        }
        self.data = None;

        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }

        true
    }

    pub fn mark_ignored_result(&mut self) {
        self.ignored_results += 1;
    }

    /// Whether the current data was served from stale cache (i.e., a
    /// stale-while-revalidate background refetch is in progress or failed).
    ///
    /// Returns `true` when the resource has data but the status indicates
    /// the most recent fetch attempt failed or was cancelled. Consumers can
    /// use this to distinguish "fresh success" from "stale data still being
    /// displayed after a background refetch failure".
    ///
    /// Note: This is a heuristic check. A `true` result means data exists but
    /// the last fetch did not succeed — the data may still be perfectly valid.
    pub fn is_data_stale(&self) -> bool {
        self.data.is_some()
            && matches!(
                self.status,
                QueryStatus::LoadingWithData | QueryStatus::Failure | QueryStatus::Cancelled
            )
    }

    /// Reset the resource back to idle, clearing state and diagnostic counters.
    ///
    /// **v2 fix**: Cancels the signal before clearing it.
    ///
    /// **Preserves**: `cache_policy`, `request_policy`, `retry_policy`, and `key`.
    /// These are considered configuration, not runtime state, and persist across
    /// resets. Use `QueryResource::new()` to create a fully fresh resource with
    /// default policies.
    ///
    /// **`initial_data`** is transient — it is marked `#[serde(skip)]` and is not
    /// persisted across serialization/deserialization. After deserialization,
    /// `initial_data` is `None` regardless of its prior value. `reset()` also
    /// clears `initial_data`.
    ///
    /// Calling `reset()` on an already-Idle resource resets diagnostic counters
    /// (`cache_hits`, `cancelled_count`, `ignored_results`, `retry_count`) to zero.
    /// This is intentional — `reset()` always resets counters regardless of current
    /// state. If counter preservation is needed, read them before calling `reset()`.
    pub fn reset(&mut self) {
        // Cancel signal before dropping
        if let Some(signal) = self.signal.as_ref() {
            signal.cancel();
        }
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
        // Note: initial_data is transient (#[serde(skip)]) and is cleared on reset.
        // It will not survive serialization/deserialization either.
        self.initial_data = None;
        self.signal = None;
    }

    /// Set placeholder data (shown while loading before first fetch).
    pub fn set_placeholder_data(&mut self, data: Option<T>) {
        self.placeholder_data = data;
    }

    /// Roll back to the previous data (optimistic update undo).
    pub fn rollback_to_previous(&mut self) -> bool {
        if let Some(prev) = self.previous_data.take() {
            self.data = Some(prev);
            self.status = QueryStatus::Success;
            return true;
        }
        false
    }

    /// Apply an optimistic update. Current data is saved for rollback.
    pub fn set_data(&mut self, data: T) {
        self.previous_data = self.data.take();
        self.data = Some(data);
    }

    /// Clear data optimistically. Current data is saved for rollback.
    pub fn clear_data(&mut self) {
        self.previous_data = self.data.take();
    }

    /// Seed initial data (only when Idle with no data).
    ///
    /// Note: `initial_data` is transient — it is marked `#[serde(skip)]` and
    /// will be lost if the resource is serialized and deserialized.
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

    /// Clear the stored initial data reference.
    pub fn clear_initial_data(&mut self) {
        self.initial_data = None;
    }
}
