//! Diagnostic types for query and mutation DevTools.
//!
//! **v2 improvements**:
//! - `QueryDiagnostic.key` uses `to_path()` for full key display (not just first segment)
//! - `MutationDiagnostic` added for mutation devtools support
//!
//! **Audit 3 additions**:
//! - `DehydratedState` and `DehydratedEntry` for state serialization

use std::any::TypeId;

use crate::core::{MutationStatus, QueryStatus};

/// Diagnostic information about a single query resource.
#[derive(Clone, Debug)]
pub struct QueryDiagnostic {
    /// Full key path (e.g., "users::42::posts").
    pub key: String,
    /// Current status.
    pub status: QueryStatus,
    /// Cache policy label.
    pub cache_policy: String,
    /// Cache age in milliseconds, if available.
    pub cache_age_ms: Option<u128>,
    /// Number of cache hits.
    pub cache_hits: u64,
    /// Number of retries.
    pub retry_count: u32,
}

/// Diagnostic information about a single mutation resource.
///
/// **v2 new**: v1 had no mutation diagnostics.
#[derive(Clone, Debug)]
pub struct MutationDiagnostic {
    /// Optional key associated with this mutation.
    pub key: Option<String>,
    /// Current status.
    pub status: MutationStatus,
    /// Number of retries.
    pub retry_count: u32,
}

/// Aggregate diagnostic for the entire QueryClient.
#[derive(Clone, Debug, Default)]
pub struct ClientDiagnostic {
    /// Total number of tracked query resources.
    pub query_count: usize,
    /// Total number of tracked mutation resources.
    pub mutation_count: usize,
    /// Per-query diagnostics.
    pub queries: Vec<QueryDiagnostic>,
    /// Per-mutation diagnostics.
    pub mutations: Vec<MutationDiagnostic>,
}

// ── Dehydration / Hydration types (Audit 3, Finding 8) ────────────────

/// A single entry in a dehydrated query cache snapshot.
///
/// Each entry represents one cached query resource, identified by its key
/// and the `TypeId` of its `(T, E)` type pair. The `data_json` field holds
/// the serialized data (if the resource had data at dehydration time).
///
/// The `kind` field distinguishes regular queries from infinite queries,
/// allowing consumers to deserialize appropriately.
#[derive(Clone, Debug)]
pub struct DehydratedEntry {
    /// Full key path (e.g., "users::42::posts").
    pub key: String,
    /// `TypeId` of the `(T, E)` (query) or `(T, E)` (infinite query) type pair.
    /// Used to match entries to concrete types during hydration.
    pub type_id: TypeId,
    /// Whether this entry is a regular query or an infinite query.
    pub kind: &'static str,
    /// Serialized data as a JSON string, if the resource had data at
    /// dehydration time. `None` if the resource was Idle, Loading, or
    /// had no serializable data.
    ///
    /// Full serialization requires type-specific code. This field is a
    /// placeholder for future typed serialization support. Callers can
    /// use `get_query_data` to extract typed data and serialize it
    /// externally if needed.
    pub data_json: Option<String>,
}

/// A portable snapshot of all cached query state.
///
/// Produced by [`QueryClient::dehydrate`](super::QueryClient::dehydrate) and
/// consumed by [`QueryClient::hydrate`](super::QueryClient::hydrate). Can be
/// persisted to disk or sent over a network for state restoration.
///
/// # Type erasure
///
/// Because `QueryClient` uses type-erased buckets, `DehydratedState` stores
/// `TypeId` values but cannot perform typed deserialization internally.
/// Callers that know the concrete `T` and `E` types should iterate `entries`
/// and use `QueryClient::set_query_data` for each matching entry.
///
/// # Example
///
/// ```
/// use gpui_query::client::{DehydratedState, DehydratedEntry};
/// use std::any::TypeId;
///
/// // DehydratedState can be constructed directly
/// let state = DehydratedState::default();
/// assert!(state.entries.is_empty());
///
/// // Entries can be created and added
/// let entries = vec![DehydratedEntry {
///     key: "users".to_string(),
///     type_id: TypeId::of::<(String, String)>(),
///     kind: "query",
///     data_json: None,
/// }];
/// let state = DehydratedState { entries };
/// assert_eq!(state.entries.len(), 1);
/// ```
#[derive(Clone, Debug, Default)]
pub struct DehydratedState {
    /// All dehydrated cache entries.
    pub entries: Vec<DehydratedEntry>,
}
