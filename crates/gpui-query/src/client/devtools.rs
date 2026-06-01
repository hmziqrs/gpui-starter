//! DevTools primitives — diagnostic types and methods for inspecting
//! query and mutation state at runtime.
//!
//! These types are designed for developer tooling, debugging, and monitoring.
//! They provide serializable snapshots of resource state that can be rendered
//! in a devtools panel or logged for debugging.

use serde::Serialize;

use gpui::App;

use super::QueryClient;

// ── Diagnostic types ─────────────────────────────────────────────────────

/// Diagnostic information about a single query resource.
#[derive(Clone, Debug, Serialize)]
pub struct QueryDiagnostic {
    /// The cache key as a string.
    pub key: String,
    /// Current status label.
    pub status: String,
    /// Whether the resource has data.
    pub has_data: bool,
    /// Whether the resource has an error.
    pub has_error: bool,
    /// Human-readable cache policy.
    pub cache_policy: String,
    /// Human-readable request policy.
    pub request_policy: String,
    /// Total cache hits.
    pub cache_hits: u64,
    /// Total cancelled requests.
    pub cancelled_count: u64,
    /// Total ignored (stale) results.
    pub ignored_results: u64,
    /// Last updated at (ms since UNIX epoch), if any.
    pub last_updated_at_ms: Option<u128>,
    /// Started at (ms since UNIX epoch), if any.
    pub started_at_ms: Option<u128>,
}

/// Diagnostic information about the entire query client.
#[derive(Clone, Debug, Serialize)]
pub struct ClientDiagnostic {
    /// Total number of query resources across all type buckets.
    pub total_resources: usize,
    /// Number of type-partitioned buckets.
    pub bucket_count: usize,
    /// Total number of mutation resources across all type buckets.
    pub mutation_count: usize,
    /// Per-query diagnostics.
    pub queries: Vec<QueryDiagnostic>,
}

// ── QueryClient methods ──────────────────────────────────────────────────

impl QueryClient {
    /// Get diagnostic information about all queries and mutations.
    ///
    /// Returns a [`ClientDiagnostic`] snapshot with per-query details.
    pub fn diagnostics(&self, cx: &App) -> ClientDiagnostic {
        let mut queries = Vec::new();
        for erased in self.buckets.values() {
            erased.bucket.collect_diagnostics(cx, &mut queries);
        }

        ClientDiagnostic {
            total_resources: self.total_count(),
            bucket_count: self.bucket_count(),
            mutation_count: self.mutation_count(),
            queries,
        }
    }

    /// Get diagnostics for a specific `(T, E)` type bucket.
    ///
    /// Returns an empty vec if no bucket exists for this type pair.
    pub fn query_diagnostics<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &self,
        cx: &App,
    ) -> Vec<QueryDiagnostic> {
        use std::any::TypeId;

        let type_id = TypeId::of::<(T, E)>();
        let Some(erased) = self.buckets.get(&type_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        erased.bucket.collect_diagnostics(cx, &mut out);
        out
    }
}
