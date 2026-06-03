//! Type-partitioned bucket for mutation resources.
//!
//! **v2 fix**: Implements actual GC instead of the v1 no-op.
//!
//! Each entry tracks its own `updated_at` timestamp (set on insertion and
//! whenever the hook layer calls `touch`). The GC removes entries whose
//! `updated_at` is older than `gc_time_ms` **and** that are not currently
//! loading. Because the erased `gc` signature has no `cx` (and therefore
//! cannot read entity state), the loading check is handled by a secondary
//! `retain_with_cx` pass — but the primary age-based filtering works purely
//! from the timestamp stored in the entry.
//!
//! **Audit 3 fixes**:
//! - Uses `WeakEntity` instead of `Entity` to avoid preventing GPUI GC of
//!   mutation resources (finding 5). Matches `QueryBucket`/`InfiniteQueryBucket`.
//! - GC reads entity state via `cx` to skip loading mutations (finding 4).
//! - `loading` flag on entry prevents mid-flight eviction even if weak ref
//!   upgrade fails during GC (finding 2).
//! - `checked_add` on `next_id` prevents wraparound (finding 3).
//! - `all_entities()` documented as allocating (finding 1).

use ahash::AHashMap;
use gpui::{App, WeakEntity};

use crate::core::{MutationResource, MutationStatus};

use super::ErasedMutationBucket;
use super::devtools::MutationDiagnostic;

/// Default garbage-collection time for idle mutations (5 minutes).
#[allow(dead_code)]
pub const DEFAULT_MUTATION_GC_TIME_MS: u64 = 300_000;

/// Minimum GC time in milliseconds (mirrors `bucket::MIN_GC_TIME_MS`).
///
/// A `gc_time_ms` of 0 would cause every non-loading resource to be evicted
/// immediately. Enforcing a 1-second minimum prevents this footgun.
const MIN_GC_TIME_MS: u64 = 1_000;

/// Per-entry metadata stored alongside the entity.
///
/// Uses `WeakEntity` instead of `Entity` so that the bucket does not prevent
/// GPUI from garbage-collecting mutation resources when all component-held
/// strong references are dropped. The weak reference is upgraded on access;
/// if the entity was already collected, the entry is treated as dead and
/// cleaned up by GC.
///
/// The `loading` flag is maintained by the hook layer via `set_loading()`
/// and `set_not_loading()`. It provides a `cx`-free check so that the GC
/// can avoid evicting mid-flight mutations without needing to upgrade the
/// weak reference and read entity state.
struct MutationEntry<V, T, E> {
    entity: WeakEntity<MutationResource<V, T, E>>,
    /// Monotonic millisecond timestamp of the last state transition
    /// (insertion, completion, or explicit `touch`). Used for GC age checks.
    updated_at: u128,
    /// Whether the mutation is currently in-flight (Loading state).
    /// Set to `true` on `begin()`, `false` on completion or reset.
    /// This allows the GC to protect mid-flight mutations without
    /// needing to read entity state via `cx`.
    loading: bool,
    /// Number of active component subscriptions holding a strong reference
    /// to this mutation entity. Entries with `observer_count > 0` are never
    /// evicted by GC, regardless of age.
    observer_count: usize,
}

/// Type-partitioned storage for mutation resources.
pub struct MutationBucket<V, T, E> {
    resources: AHashMap<u64, MutationEntry<V, T, E>>,
    next_id: u64,
}

/// Returns the current time as milliseconds since UNIX epoch.
fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

impl<
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
> MutationBucket<V, T, E>
{
    pub(crate) fn new() -> Self {
        Self {
            resources: AHashMap::new(),
            next_id: 0,
        }
    }

    /// Insert a mutation entity, recording the current time as `updated_at`.
    ///
    /// Returns the generated numeric id for the entry.
    ///
    /// **Audit 3 fix (finding 3)**: Uses `checked_add` on `next_id` to
    /// prevent wraparound after `u64::MAX` insertions. If the counter
    /// overflows, the method panics — consistent with the principle that
    /// IDs must be unique and wraparound would cause data loss.
    pub(crate) fn insert(&mut self, entity: &gpui::Entity<MutationResource<V, T, E>>, _cx: &App) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1)
            .expect("MutationBucket::insert: next_id overflow after u64::MAX insertions");
        self.resources.insert(
            id,
            MutationEntry {
                entity: entity.downgrade(),
                updated_at: now_ms(),
                loading: false,
                observer_count: 0,
            },
        );
        id
    }

    /// Refresh the `updated_at` timestamp for an entry.
    ///
    /// The hook layer should call this whenever the mutation completes
    /// (transitions to `Success` or `Failure`) so that the GC timer
    /// restarts from the completion moment rather than from insertion.
    #[allow(dead_code)]
    pub(crate) fn touch(&mut self, id: u64) {
        if let Some(entry) = self.resources.get_mut(&id) {
            entry.updated_at = now_ms();
        }
    }

    /// Mark a mutation entry as currently loading (in-flight).
    ///
    /// **Audit 3 fix (finding 2)**: The hook layer should call this when
    /// `begin()` is called on the mutation resource. This sets a `loading`
    /// flag on the entry that the GC checks without needing `cx` to read
    /// entity state, preventing mid-flight eviction of long-running mutations.
    #[allow(dead_code)]
    pub(crate) fn set_loading(&mut self, id: u64) {
        if let Some(entry) = self.resources.get_mut(&id) {
            entry.loading = true;
        }
    }

    /// Mark a mutation entry as no longer loading (completed or reset).
    ///
    /// The hook layer should call this when the mutation reaches a terminal
    /// state (`Success`, `Failure`, or `Idle` via `reset()`).
    #[allow(dead_code)]
    pub(crate) fn set_not_loading(&mut self, id: u64) {
        if let Some(entry) = self.resources.get_mut(&id) {
            entry.loading = false;
        }
    }

    /// Increment the observer count for an entry.
    ///
    /// Call this when a component mounts and holds a strong reference to
    /// the mutation entity. Prevents GC from evicting the entry while
    /// components are actively using it.
    #[allow(dead_code)]
    pub(crate) fn retain(&mut self, id: u64) {
        if let Some(entry) = self.resources.get_mut(&id) {
            entry.observer_count = entry.observer_count.saturating_add(1);
        }
    }

    /// Decrement the observer count for an entry.
    ///
    /// Call this when the component unmounts and releases its strong
    /// reference to the mutation entity.
    #[allow(dead_code)]
    pub(crate) fn release(&mut self, id: u64) {
        if let Some(entry) = self.resources.get_mut(&id) {
            entry.observer_count = entry.observer_count.saturating_sub(1);
        }
    }

    /// All entities in this bucket that are still alive.
    ///
    /// **Audit 3 fix (finding 1)**: This method allocates a `Vec` of all
    /// mutation entities by upgrading weak references. Callers that invoke
    /// this on every render (e.g., `use_mutation_state()`) will allocate a
    /// new `Vec` each time. If this becomes a performance concern, consider
    /// caching the result or calling this less frequently.
    pub(crate) fn all_entities(&self) -> Vec<gpui::Entity<MutationResource<V, T, E>>> {
        self.resources
            .values()
            .filter_map(|e| e.entity.upgrade())
            .collect()
    }
}

impl<
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
> ErasedMutationBucket for MutationBucket<V, T, E>
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Garbage-collect stale mutation resources.
    ///
    /// **Audit 3 fixes**:
    /// - **Finding 4**: Uses `cx` to read entity state and check
    ///   `is_loading()`, matching the pattern used in `QueryBucket::gc`
    ///   and `InfiniteQueryBucket::gc`. This prevents eviction of
    ///   long-running mutations that exceed `gc_time_ms` mid-flight.
    /// - **Finding 2**: Also checks the entry-level `loading` flag as a
    ///   secondary guard. This protects mutations even when the weak
    ///   reference cannot be upgraded (e.g., during a brief window where
    ///   the entity is still alive but the weak ref is temporarily
    ///   non-upgradeable).
    /// - **Finding 5**: Removes dead entries whose `WeakEntity` can no
    ///   longer be upgraded (all strong references dropped).
    ///
    /// Evicts entries where:
    /// 1. The entity has been collected (weak ref dead), OR
    /// 2. The entry is not loading, has no active observers, AND the
    ///    age exceeds `gc_time_ms` (minimum 1 second).
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &App) {
        // Enforce minimum GC time to prevent aggressive eviction.
        let gc_time_ms = gc_time_ms.max(MIN_GC_TIME_MS);
        let gc_threshold = gc_time_ms as u128;

        // Phase 1: Collect ids to evict (no HashMap mutation during iteration).
        let to_remove: Vec<u64> = self
            .resources
            .iter()
            .filter_map(|(id, entry)| {
                // Clean up entries whose entity has already been collected.
                let entity = match entry.entity.upgrade() {
                    Some(e) => e,
                    None => {
                        // Only remove dead entries that are not loading.
                        // A dead entry with loading=true means the mutation
                        // is in-flight but the weak ref couldn't upgrade —
                        // keep it for one more GC cycle as a safety measure.
                        if entry.loading {
                            return None;
                        }
                        return Some(*id);
                    }
                };

                // Audit fix (finding 2): Never evict entries flagged as loading.
                // This is a `cx`-free check that works even if the entity
                // state read below somehow disagrees.
                if entry.loading {
                    return None;
                }

                // Never evict entries with active observers.
                if entry.observer_count > 0 {
                    return None;
                }

                // Audit fix (finding 4): Read entity state via cx to check
                // if the mutation is actually loading, matching QueryBucket.
                let resource = entity.read(cx);

                // Never evict resources that are actively loading.
                if resource.is_loading() {
                    return None;
                }

                // Only evict if in a terminal state (Idle or Failure).
                // Success entries are kept until they age out.
                let evictable = matches!(
                    resource.status(),
                    MutationStatus::Idle | MutationStatus::Failure
                );
                if !evictable {
                    return None;
                }

                // Check age: evict if older than gc_time_ms.
                let age = now_ms.saturating_sub(entry.updated_at);
                if age >= gc_threshold {
                    return Some(*id);
                }

                None
            })
            .collect();

        // Phase 2: Remove collected ids.
        for id in to_remove {
            self.resources.remove(&id);
        }
    }

    fn count(&self) -> usize {
        self.resources.len()
    }

    /// Collect per-resource diagnostic details for all live mutation entries.
    ///
    /// Iterates all entries, upgrades weak references, reads entity state,
    /// and constructs a `MutationDiagnostic` for each live resource.
    /// Dead entries (collected entities) are skipped.
    fn collect_diagnostics(&self, cx: &App) -> Vec<MutationDiagnostic> {
        self.resources
            .values()
            .filter_map(|entry| {
                let entity = entry.entity.upgrade()?;
                let resource = entity.read(cx);
                Some(MutationDiagnostic {
                    key: resource.key().map(|k| k.to_path()),
                    status: resource.status(),
                    retry_count: resource.retry_count(),
                })
            })
            .collect()
    }
}
