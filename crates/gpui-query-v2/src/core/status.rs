//! Query status enum representing the lifecycle states of a query resource.

use serde::{Deserialize, Serialize};

/// The status of a query resource.
///
/// A query transitions through these states:
/// `Idle` → `LoadingEmpty` → `Success` / `Failure`
/// `Success` → `LoadingWithData` → `Success` / `Failure` (refetch)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryStatus {
    /// No data has been fetched yet. Initial state.
    #[default]
    Idle,
    /// Loading for the first time (no data available).
    LoadingEmpty,
    /// Refetching with existing data available.
    LoadingWithData,
    /// Data loaded successfully.
    Success,
    /// The last fetch failed.
    Failure,
    /// The request was cancelled.
    Cancelled,
}

impl QueryStatus {
    /// Human-readable label for the status.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::LoadingEmpty => "Loading empty",
            Self::LoadingWithData => "Loading with data",
            Self::Success => "Success",
            Self::Failure => "Failure",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Whether the resource is currently loading (first time or refetch).
    pub fn is_loading(self) -> bool {
        matches!(self, Self::LoadingEmpty | Self::LoadingWithData)
    }

    /// Whether the resource is pending (no data yet and currently loading).
    ///
    /// Equivalent to TanStack Query's `isPending`.
    pub fn is_pending(self) -> bool {
        matches!(self, Self::Idle | Self::LoadingEmpty)
    }
}
