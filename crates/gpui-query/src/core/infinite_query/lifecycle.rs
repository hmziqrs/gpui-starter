use super::{
    QueryStatus, QueryTimestamp, RequestId, RequestSequencer,
};
use crate::core::InfiniteQueryResource;

// ── Lifecycle ───────────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// Begin fetching the next page.
    ///
    /// Returns `None` (and does nothing) if `has_next_page` is `false`.
    /// If a previous-page fetch is in progress, it is cancelled and replaced.
    /// If a next-page fetch is already in progress with `LatestWins` policy,
    /// the old request is cancelled and a new one starts. With
    /// `IgnoreWhileLoading`, returns `None` instead.
    pub fn begin_fetch_next(
        &mut self,
        sequencer: &mut RequestSequencer,
        now_ms: u128,
    ) -> Option<RequestId> {
        if !self.has_next_page {
            return None;
        }

        if self.is_fetching_next_page {
            match self.request_policy {
                super::RequestPolicy::IgnoreWhileLoading => return None,
                super::RequestPolicy::LatestWins => {
                    // Cancel the existing next-page fetch and start fresh
                }
            }
        }

        // If any request is active, count it as cancelled.
        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
        }

        self.is_fetching_next_page = true;
        self.is_fetching_previous_page = false;

        let request_id = sequencer.next_request();
        self.active_request_id = Some(request_id);
        self.status = if self.pages.is_empty() {
            QueryStatus::LoadingEmpty
        } else {
            QueryStatus::LoadingWithData
        };
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(super::QuerySignal::new());

        Some(request_id)
    }

    /// Begin fetching the previous page.
    ///
    /// Returns `None` (and does nothing) if `has_previous_page` is `false`.
    /// Respects the same `RequestPolicy` as [`begin_fetch_next`](Self::begin_fetch_next)
    /// when a previous-page fetch is already in progress.
    pub fn begin_fetch_previous(
        &mut self,
        sequencer: &mut RequestSequencer,
        now_ms: u128,
    ) -> Option<RequestId> {
        if !self.has_previous_page {
            return None;
        }

        if self.is_fetching_previous_page {
            match self.request_policy {
                super::RequestPolicy::IgnoreWhileLoading => return None,
                super::RequestPolicy::LatestWins => {
                    // Cancel the existing previous-page fetch and start fresh
                }
            }
        }

        // Cancel any in-flight request.
        if self.active_request_id.is_some() {
            self.cancelled_count += 1;
        }

        self.is_fetching_previous_page = true;
        self.is_fetching_next_page = false;

        let request_id = sequencer.next_request();
        self.active_request_id = Some(request_id);
        self.status = if self.pages.is_empty() {
            QueryStatus::LoadingEmpty
        } else {
            QueryStatus::LoadingWithData
        };
        self.started_at = Some(QueryTimestamp::from(now_ms));
        self.error = None;
        self.signal = Some(super::QuerySignal::new());

        Some(request_id)
    }

    /// Convenience wrapper around [`begin_fetch_next`](Self::begin_fetch_next) that
    /// uses the resource's own persistent [`RequestSequencer`].
    ///
    /// This is the preferred entry point for the hook layer — callers don't need
    /// to manage a separate sequencer instance.
    pub fn begin_fetch_next_auto(&mut self, now_ms: u128) -> Option<RequestId> {
        // Take the sequencer out to avoid borrowing `self` and `self.sequencer`
        // mutably at the same time.
        let mut seq = std::mem::take(&mut self.sequencer);
        let result = self.begin_fetch_next(&mut seq, now_ms);
        self.sequencer = seq;
        result
    }

    /// Convenience wrapper around [`begin_fetch_previous`](Self::begin_fetch_previous)
    /// that uses the resource's own persistent [`RequestSequencer`].
    pub fn begin_fetch_previous_auto(&mut self, now_ms: u128) -> Option<RequestId> {
        let mut seq = std::mem::take(&mut self.sequencer);
        let result = self.begin_fetch_previous(&mut seq, now_ms);
        self.sequencer = seq;
        result
    }

    /// Complete a page fetch with success.
    ///
    /// If `request_id` matches the active request:
    /// - Appends the page (if `is_next`) or prepends it (if previous).
    /// - Sets `has_next_page` / `has_previous_page` from the result.
    /// - Transitions to `Success` status.
    ///
    /// Returns `true` if the request was accepted, `false` if it was stale.
    pub fn complete_page_success(
        &mut self,
        request_id: RequestId,
        page: T,
        has_more: bool,
        is_next: bool,
        now_ms: u128,
    ) -> bool {
        if self.active_request_id != Some(request_id) {
            return false;
        }

        if is_next {
            self.pages.push(page);
            self.has_next_page = has_more;
            self.enforce_max_pages_remove_front();
        } else {
            self.pages.insert(0, page);
            self.has_previous_page = has_more;
            self.enforce_max_pages_remove_back();
        }

        self.status = QueryStatus::Success;
        self.error = None;
        self.active_request_id = None;
        self.last_updated_at = Some(QueryTimestamp::from(now_ms));
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;

        true
    }

    /// Complete a page fetch with failure.
    ///
    /// If `request_id` matches the active request, stores the error and
    /// transitions to `Failure` status. Returns `true` if accepted.
    pub fn complete_page_failure(&mut self, request_id: RequestId, error: E) -> bool {
        if self.active_request_id != Some(request_id) {
            return false;
        }

        self.status = QueryStatus::Failure;
        self.error = Some(error);
        self.active_request_id = None;
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;

        true
    }

    /// Check whether the given request id is the current active request.
    pub fn is_current_request(&self, request_id: RequestId) -> bool {
        self.active_request_id == Some(request_id)
    }

    /// Reset the resource back to idle, clearing all pages and state.
    pub fn reset(&mut self) {
        self.pages.clear();
        self.status = QueryStatus::Idle;
        self.error = None;
        self.active_request_id = None;
        self.started_at = None;
        self.last_updated_at = None;
        self.cache_hits = 0;
        self.cancelled_count = 0;
        self.has_next_page = true;
        self.has_previous_page = false;
        self.is_fetching_next_page = false;
        self.is_fetching_previous_page = false;
        self.signal = None;
    }

    /// Invalidate the resource by clearing the last-updated timestamp.
    ///
    /// Pages are retained but the resource is considered stale, so the next
    /// access will trigger a refetch.
    pub fn invalidate(&mut self) {
        self.last_updated_at = None;
    }
}
