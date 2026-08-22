//! Core types and constants for the query bucket.

use gpui::WeakEntity;

use crate::core::{CachePolicy, QueryResource, QueryStatus, RequestSequencer};

/// Minimum GC time in milliseconds.
///
/// A `gc_time_ms` of 0 would cause every `Idle` and `Failure` resource to be
/// evicted on every GC pass (since `age_ms >= 0` is always true for unsigned
/// values). This effectively disables caching for non-active resources.
/// Enforcing a 1-second minimum prevents this footgun.
pub(crate) const MIN_GC_TIME_MS: u64 = 1_000;

/// Default maximum number of entries per bucket.
///
/// Prevents unbounded memory growth from malicious or buggy components that
/// register unlimited unique query keys. When the limit is reached, the
/// oldest (least-recently-updated) entry is evicted to make room.
pub(crate) const DEFAULT_MAX_ENTRIES: usize = 10_000;

/// Multiplier applied to `gc_time_ms` to determine the maximum age for
/// `Success` resources before they become eligible for eviction.
///
/// A `Success` resource with zero observers whose data age exceeds
/// `SUCCESS_GC_MULTIPLIER * gc_time_ms` will be evicted even though it
/// holds valuable data. This prevents memory leaks from queries that
/// succeeded once but were never observed again.
pub(crate) const SUCCESS_GC_MULTIPLIER: u32 = 2;

/// Cached status snapshot for GC decisions, updated on each request completion.
///
/// This allows the GC to filter entries without acquiring entity read locks
/// (finding 1 fix). The trade-off is a small per-update cost: when a request
/// completes (success or failure), the hook layer calls
/// `update_status_snapshot` to sync this cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusSnapshot {
    /// The `QueryStatus` at the time of the last snapshot.
    pub status: QueryStatus,
    /// `last_updated_at` in milliseconds since UNIX epoch, or `None` if the
    /// resource has never completed a fetch.
    pub last_updated_ms: Option<u128>,
    /// The `CachePolicy` at the time of the last snapshot.
    pub cache_policy: CachePolicy,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            status: QueryStatus::Idle,
            last_updated_ms: None,
            cache_policy: CachePolicy::default(),
        }
    }
}

/// Entry co-locating weak entity reference, sequencer, observer tracking,
/// and cached status for GC.
///
/// Uses `WeakEntity` instead of `Entity` so that the bucket does not prevent
/// GPUI from garbage-collecting unused query resources. The weak reference is
/// upgraded on access; if the entity was already collected, the entry is
/// treated as missing and re-created on next `get_or_create`.
///
/// The `observer_count` field tracks the number of active `QueryObserver`
/// subscriptions held by mounted components. GC will refuse to evict entries
/// with `observer_count > 0`, preserving cache deduplication for in-use
/// resources regardless of their state (including `Success`).
///
/// The `status_snapshot` field caches the resource's status and
/// `last_updated_at` so GC can make eviction decisions without acquiring
/// entity read locks (finding 1 fix). It is updated via
/// `update_status_snapshot` after each request completion.
pub(crate) struct BucketEntry<T, E> {
    pub entity: WeakEntity<QueryResource<T, E>>,
    pub sequencer: RequestSequencer,
    /// Number of active observer subscriptions for this resource.
    /// Incremented when an observer is attached, decremented when dropped.
    ///
    /// **Note (finding 7)**: The hook layer (`use_query`, `use_query_manual`)
    /// must call `bucket.retain()` after creating an observer and
    /// `bucket.release()` when the subscription is dropped. Without these
    /// calls, `observer_count` remains 0 and GC protection for observed
    /// resources is ineffective.
    pub observer_count: usize,
    /// Cached status for GC decisions, updated on each request completion.
    /// Allows GC to filter entries without acquiring entity read locks.
    pub status_snapshot: StatusSnapshot,
}
