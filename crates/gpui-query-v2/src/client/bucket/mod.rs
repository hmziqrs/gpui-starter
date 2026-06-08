//! Type-partitioned bucket for query resources.
//!
//! **v2 improvements**:
//! - Uses `AHashMap` instead of `std::collections::HashMap`
//! - Co-locates `RequestSequencer` with entity in `BucketEntry`
//! - Collect-then-update pattern avoids nested entity borrows
//!
//! **Audit fixes (findings 1-5)**:
//! - Uses `WeakEntity` to avoid preventing GC of unused resources (finding 3)
//! - Tracks observer count in `BucketEntry` to protect in-use resources (finding 4)
//! - Enforces minimum GC time of 1000ms to avoid aggressive eviction (finding 1)
//! - Implements actual policy updates and calls them from `get_or_create` (findings 2, 5)
//!
//! **Audit 3 fixes**:
//! - Caches `status_snapshot` and `last_updated_ms` in `BucketEntry` so GC can
//!   filter without acquiring entity read locks (finding 1)
//! - Uses `HashMap::retain()` instead of collect-then-remove to avoid
//!   intermediate `Vec<QueryKey>` allocation (finding 2)
//! - `invalidate_matching`/`reset_matching` collect cheap keys then upgrade
//!   individually, deferring the `upgrade()` cost (finding 3)
//! - Configurable max entry count per bucket to prevent unbounded growth (finding 4)
//! - GC protects `StaleWhileRevalidate` resources within the stale window
//!   from premature eviction (finding 5)
//! - GC evicts `Success` resources whose age exceeds `2 * gc_time_ms` (finding 6)
//! - `retain()`/`release()` doc comments note wiring requirement (finding 7)

mod erased_ops;
mod ops;
mod types;

pub use ops::QueryBucket;
pub use types::StatusSnapshot;
