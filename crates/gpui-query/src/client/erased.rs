//! Type-erased bucket traits and persistence adapter.
//!
//! These traits allow `QueryClient` to store heterogeneous query and mutation
//! buckets in a single `AHashMap<TypeId, Box<dyn Erased*>>` map, dispatching
//! to concrete types only when the caller provides generic parameters.

use crate::core::QueryKeyFilter;
use crate::client::devtools::{MutationDiagnostic, QueryDiagnostic};

// ── Helper: current time in milliseconds since UNIX epoch ─────────────

/// Returns the current time as milliseconds since the UNIX epoch.
///
/// Used internally by `gc()` and other time-sensitive operations.
/// Exposed so callers can cache the value and pass it to `gc_with_time()`
/// to avoid repeated syscalls.
pub fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Type-erased bucket trait for storage in a homogeneous map.
pub(crate) trait ErasedBucket {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &gpui::App);
    fn count(&self) -> usize;
    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut gpui::App);
    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut gpui::App);
    fn remove_matching(&mut self, filter: &QueryKeyFilter);
    fn cancel_matching(&mut self, filter: &QueryKeyFilter, cx: &mut gpui::App);
    fn collect_diagnostics(&self, now_ms: u128, cx: &gpui::App) -> Vec<QueryDiagnostic>;
}

/// Type-erased infinite query bucket trait.
pub(crate) trait ErasedInfiniteBucket {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &gpui::App);
    fn count(&self) -> usize;
    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut gpui::App);
    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut gpui::App);
    fn remove_matching(&mut self, filter: &QueryKeyFilter);
    fn cancel_matching(&mut self, filter: &QueryKeyFilter, cx: &mut gpui::App);
    fn collect_diagnostics(&self, now_ms: u128, cx: &gpui::App) -> Vec<QueryDiagnostic>;
}

/// Type-erased mutation bucket trait.
pub(crate) trait ErasedMutationBucket {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &gpui::App);
    fn count(&self) -> usize;
    fn collect_diagnostics(&self, cx: &gpui::App) -> Vec<MutationDiagnostic>;
}

/// Persistence adapter trait for query cache persistence across app restarts.
///
/// Implementations can store cached data in any backend (filesystem, database, etc.).
/// Entries are serialized as JSON strings to avoid generic bounds on the persister.
///
/// # Example
///
/// ```
/// use std::path::PathBuf;
/// use gpui_query::client::{QueryPersister, DehydratedEntry};
///
/// struct FilePersister { path: PathBuf }
///
/// impl QueryPersister for FilePersister {
///     fn load(&self) -> Vec<DehydratedEntry> { Vec::new() }
///     fn save(&self, _entries: Vec<DehydratedEntry>) {}
/// }
/// ```
pub trait QueryPersister: Send + Sync {
    /// Load persisted entries from storage.
    fn load(&self) -> Vec<crate::client::devtools::DehydratedEntry>;

    /// Save entries to storage, replacing any previously stored data.
    fn save(&self, entries: Vec<crate::client::devtools::DehydratedEntry>);
}
