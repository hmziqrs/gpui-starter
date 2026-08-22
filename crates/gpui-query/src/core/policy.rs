//! Cache and request policies for query resources.

use serde::{Deserialize, Serialize};

use super::{QueryStatus, RequestId};

/// How cached data is treated when a query is accessed.
///
/// - [`NoCache`](CachePolicy::NoCache): Always fetch fresh data.
/// - [`Ttl`](CachePolicy::Ttl): Use cached data if fresh (within TTL).
/// - [`StaleWhileRevalidate`](CachePolicy::StaleWhileRevalidate): Return stale
///   data immediately while fetching fresh data in the background.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CachePolicy {
    /// Never cache — always fetch fresh data.
    NoCache,
    /// Cache with a time-to-live. Data is considered fresh within the TTL.
    ///
    /// **Note:** `ttl_ms` should be greater than zero. A `ttl_ms` of `0` is
    /// equivalent to [`NoCache`](Self::NoCache) for all practical purposes —
    /// data is only "fresh" at the exact instant it is stored, so every
    /// `begin_request` call triggers a new fetch. This is not validated at
    /// runtime in release builds, but a `debug_assert` will fire in debug
    /// builds if `ttl_ms` is zero.
    Ttl { ttl_ms: u64 },
    /// Return stale data immediately while revalidating in the background.
    ///
    /// Data within `ttl_ms` is served as a fresh cache hit (no refetch).
    /// Data between `ttl_ms` and `ttl_ms + stale_ms` is served as stale data
    /// **and** a background revalidation is triggered.
    /// After `ttl_ms + stale_ms`, data is considered expired and a normal
    /// fetch is performed (no stale data served).
    ///
    /// **Note:** `stale_ms` should be greater than zero. Setting `stale_ms`
    /// to `0` effectively disables the stale-while-revalidate feature,
    /// degenerating to pure TTL behavior (the stale window is an empty set).
    /// Both `ttl_ms` and `stale_ms` should be greater than zero. This is not
    /// validated at runtime in release builds, but `debug_assert`s will fire
    /// in debug builds if either value is zero.
    StaleWhileRevalidate { ttl_ms: u64, stale_ms: u64 },
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self::Ttl { ttl_ms: 60_000 } // 1 minute default
    }
}

impl CachePolicy {
    /// Human-readable label.
    ///
    /// Sub-second values are shown with millisecond precision (e.g. "500ms")
    /// rather than truncating to "0s" via integer division.
    pub fn label(self) -> String {
        match self {
            Self::NoCache => "No cache".to_string(),
            Self::Ttl { ttl_ms } => format!("Cache TTL {}", format_duration(ttl_ms)),
            Self::StaleWhileRevalidate { ttl_ms, stale_ms } => {
                format!(
                    "Stale-while-revalidate TTL {} stale {}",
                    format_duration(ttl_ms),
                    format_duration(stale_ms)
                )
            }
        }
    }

    /// Whether this policy can short-circuit (return cached data without fetching).
    ///
    /// Returns `true` for `Ttl` and `StaleWhileRevalidate` since both can serve
    /// cached data when it is fresh (within the TTL window). The actual freshness
    /// check is done separately in [`is_fresh`](Self::is_fresh).
    pub fn can_short_circuit(self) -> bool {
        matches!(self, Self::Ttl { .. } | Self::StaleWhileRevalidate { .. })
    }

    /// Whether this policy allows serving stale data while revalidating.
    pub fn can_serve_stale(self) -> bool {
        matches!(self, Self::StaleWhileRevalidate { .. })
    }

    /// The TTL in milliseconds, if applicable.
    pub fn ttl_ms(self) -> Option<u64> {
        match self {
            Self::NoCache => None,
            Self::Ttl { ttl_ms } | Self::StaleWhileRevalidate { ttl_ms, .. } => Some(ttl_ms),
        }
    }

    /// The stale-while-revalidate window in milliseconds beyond TTL.
    ///
    /// Returns `None` for policies that are not `StaleWhileRevalidate`.
    pub fn stale_ms(self) -> Option<u64> {
        match self {
            Self::StaleWhileRevalidate { stale_ms, .. } => Some(stale_ms),
            _ => None,
        }
    }

    /// Total valid window (TTL + stale) in milliseconds.
    ///
    /// This is the maximum age at which data can still be served under this policy.
    /// For `Ttl`, this equals `ttl_ms`. For `StaleWhileRevalidate`, it equals
    /// `ttl_ms + stale_ms`. Returns `None` for `NoCache`.
    ///
    /// On overflow (extremely large `ttl_ms + stale_ms`), saturates to `u64::MAX`,
    /// effectively treating the data as indefinitely valid.
    pub fn total_valid_ms(self) -> Option<u64> {
        match self {
            Self::NoCache => None,
            Self::Ttl { ttl_ms } => {
                debug_assert!(ttl_ms > 0, "CachePolicy::Ttl with ttl_ms=0 behaves like NoCache");
                Some(ttl_ms)
            }
            Self::StaleWhileRevalidate { ttl_ms, stale_ms } => {
                debug_assert!(ttl_ms > 0, "CachePolicy::StaleWhileRevalidate with ttl_ms=0 behaves like NoCache");
                debug_assert!(stale_ms > 0, "CachePolicy::StaleWhileRevalidate with stale_ms=0 degenerates to Ttl-only behavior");
                Some(ttl_ms.saturating_add(stale_ms))
            }
        }
    }

    /// Whether the data is fresh (within the TTL window).
    ///
    /// Returns `false` if the policy has no TTL or the data age exceeds TTL.
    pub fn is_fresh(self, age_ms: u128) -> bool {
        self.ttl_ms()
            .map(|ttl| age_ms <= ttl as u128)
            .unwrap_or(false)
    }

    /// Whether the data is stale but still within the stale-while-revalidate window.
    ///
    /// Data is "stale-but-serveable" when:
    /// - The policy is `StaleWhileRevalidate`
    /// - Data age is past TTL but within `ttl_ms + stale_ms`
    pub fn is_stale_but_serveable(self, age_ms: u128) -> bool {
        match self {
            Self::StaleWhileRevalidate { ttl_ms, stale_ms } => {
                let total = (ttl_ms as u128) + (stale_ms as u128);
                age_ms > (ttl_ms as u128) && age_ms <= total
            }
            _ => false,
        }
    }

    /// Whether the data is expired (past the total valid window).
    ///
    /// Returns `true` if the data age exceeds the total valid window for this policy.
    pub fn is_expired(self, age_ms: u128) -> bool {
        self.total_valid_ms()
            .map(|total| age_ms > total as u128)
            .unwrap_or(true) // NoCache always considers data expired
    }
}

/// How concurrent requests are handled.
///
/// - [`LatestWins`](RequestPolicy::LatestWins): New requests cancel in-flight ones.
/// - [`IgnoreWhileLoading`](RequestPolicy::IgnoreWhileLoading): New requests are
///   ignored if one is already in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestPolicy {
    /// New requests replace in-flight ones (default).
    #[default]
    LatestWins,
    /// Ignore new requests while one is already loading.
    IgnoreWhileLoading,
}

impl RequestPolicy {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::LatestWins => "Latest wins",
            Self::IgnoreWhileLoading => "Ignore while loading",
        }
    }
}

/// Whether the fetch is a normal request or forced (ignoring cache).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QueryFetchMode {
    /// Normal fetch — respects cache policy.
    #[default]
    Normal,
    /// Force fetch — ignores cache freshness.
    Force,
}

/// The result of calling `begin_request` on a query resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryBeginResult {
    /// A new request was started.
    Started {
        request_id: RequestId,
        status: QueryStatus,
        replaced_request_id: Option<RequestId>,
    },
    /// Cache is fresh — no fetch needed.
    CacheHit,
    /// Stale data was served and a background revalidation was started.
    ///
    /// The caller should:
    /// 1. Return the existing stale data to the consumer immediately.
    /// 2. Use the `request_id` to perform a background fetch.
    /// 3. Complete the request normally via `complete_success`/`complete_failure`.
    StaleCacheHit {
        request_id: RequestId,
        status: QueryStatus,
        replaced_request_id: Option<RequestId>,
    },
    /// A request is already loading and the policy is `IgnoreWhileLoading`.
    IgnoredWhileLoading {
        active_request_id: RequestId,
    },
}

/// Format a duration in milliseconds as a human-readable string.
///
/// Shows seconds for values >= 1000ms, milliseconds otherwise.
/// This avoids the misleading "0s" label that integer division produces
/// for sub-second values.
fn format_duration(ms: u64) -> String {
    if ms >= 1_000 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{}ms", ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_shows_millis_for_subsecond() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(999), "999ms");
    }

    #[test]
    fn format_duration_shows_seconds_for_one_second_and_above() {
        assert_eq!(format_duration(1_000), "1s");
        assert_eq!(format_duration(60_000), "60s");
    }

    #[test]
    fn ttl_zero_label_uses_ms() {
        let policy = CachePolicy::Ttl { ttl_ms: 0 };
        assert_eq!(policy.label(), "Cache TTL 0ms");
    }

    #[test]
    fn swr_zero_values_label_uses_ms() {
        let policy = CachePolicy::StaleWhileRevalidate { ttl_ms: 0, stale_ms: 0 };
        assert_eq!(policy.label(), "Stale-while-revalidate TTL 0ms stale 0ms");
    }

    #[test]
    fn total_valid_ms_saturates_on_overflow() {
        let policy = CachePolicy::StaleWhileRevalidate {
            ttl_ms: u64::MAX,
            stale_ms: u64::MAX,
        };
        assert_eq!(policy.total_valid_ms(), Some(u64::MAX));
    }
}
