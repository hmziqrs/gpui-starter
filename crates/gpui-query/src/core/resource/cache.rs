//! Query resource cache logic.

use crate::core::{QueryStatus, QueryTimestamp};

use super::QueryResource;

impl<T, E> QueryResource<T, E> {
    /// Cache age in milliseconds.
    pub fn cache_age_ms(&self, now_ms: u128) -> Option<u128> {
        QueryTimestamp::from(now_ms).elapsed_since(self.last_updated_at?)
    }

    /// Whether the cache is fresh (within TTL).
    ///
    /// For all policies with a TTL, this checks that data exists and the age
    /// is within the TTL window. The stale-while-revalidate window is NOT
    /// considered fresh — it is stale-but-serveable (see [`is_stale_but_serveable`]).
    ///
    /// **Boundary behavior**: data at exactly TTL milliseconds old is considered
    /// fresh (`age <= ttl_ms`). Data older than TTL is stale (`age > ttl_ms`).
    /// This differs from HTTP `Cache-Control: max-age` where the boundary is
    /// exclusive. The inclusive boundary is chosen so that the fresh/stale
    /// partition is total: every age is either fresh or stale, with no gap.
    pub fn is_cache_fresh(&self, now_ms: u128) -> bool {
        self.has_data()
            && self
                .cache_policy
                .ttl_ms()
                .zip(self.cache_age_ms(now_ms))
                .map(|(ttl_ms, age_ms)| age_ms <= ttl_ms as u128)
                .unwrap_or(false)
    }

    /// Whether the cache is stale but still within the stale-while-revalidate window.
    ///
    /// Returns `true` when:
    /// - The policy is `StaleWhileRevalidate`
    /// - Data exists
    /// - Data age is past TTL but within `ttl_ms + stale_ms`
    pub fn is_stale_but_serveable(&self, now_ms: u128) -> bool {
        self.has_data()
            && self
                .cache_age_ms(now_ms)
                .map(|age_ms| self.cache_policy.is_stale_but_serveable(age_ms))
                .unwrap_or(false)
    }

    /// Whether the cache is fully expired (past the total valid window).
    ///
    /// For `StaleWhileRevalidate`, this means past `ttl_ms + stale_ms`.
    /// For `Ttl`, this means past `ttl_ms`.
    /// For `NoCache`, always returns `true` (no data is ever valid).
    pub fn is_cache_expired(&self, now_ms: u128) -> bool {
        if !self.has_data() {
            return true;
        }
        self.cache_age_ms(now_ms)
            .map(|age_ms| self.cache_policy.is_expired(age_ms))
            .unwrap_or(true)
    }

    /// Whether the cache can short-circuit (fresh data, no fetch needed).
    ///
    /// Only returns `true` when the policy supports short-circuiting AND the
    /// data is within the TTL window (fresh, not stale).
    pub fn should_short_circuit_cache(&self, now_ms: u128) -> bool {
        self.cache_policy.can_short_circuit() && self.is_cache_fresh(now_ms)
    }

    /// Whether the resource should serve stale data while triggering a background refetch.
    ///
    /// This is the core stale-while-revalidate check: data is past its TTL but
    /// still within the stale window. The caller should:
    /// 1. Return existing data to the consumer immediately.
    /// 2. Start a background fetch to revalidate.
    pub fn should_serve_stale_and_revalidate(&self, now_ms: u128) -> bool {
        self.cache_policy.can_serve_stale() && self.is_stale_but_serveable(now_ms)
    }

    /// Record a cache hit.
    ///
    /// Increments the hit counter and transitions status to [`Success`](QueryStatus::Success)
    /// **only if the resource is not in a terminal failure state** (`Failure` or `Cancelled`).
    /// This prevents a surprising `Failure -> Success` transition without a new fetch
    /// having occurred. The error is only cleared when transitioning to `Success`.
    ///
    /// A cache hit on data that was previously fetched successfully will still set
    /// `Success` as expected.
    pub(crate) fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
        // Only transition to Success from non-terminal states.
        // Failure/Cancelled are terminal — a cache hit on old data should not
        // silently clear the error a consumer is already handling.
        if !matches!(self.status, QueryStatus::Failure | QueryStatus::Cancelled) {
            self.status = QueryStatus::Success;
            self.error = None;
        }
    }

    /// Record a stale cache hit (data served from stale window).
    ///
    /// Increments cache hit counter and keeps the data as `Success` status.
    /// The caller is expected to also trigger a background revalidation.
    pub(crate) fn record_stale_cache_hit(&mut self) {
        self.cache_hits += 1;
        // Keep status as Success — we are still serving valid data to the consumer.
        self.error = None;
    }

    /// Invalidate the cache (clear last-updated timestamp).
    ///
    /// Data is retained but the resource is considered stale.
    pub fn invalidate(&mut self) {
        self.last_updated_at = None;
    }
}
