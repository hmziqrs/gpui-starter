#![allow(dead_code)]

use gpui::{App, BorrowAppContext, Global};
use serde::{Deserialize, Serialize};

use crate::{ids::EventId, time::AppTimestamp};

// ---------------------------------------------------------------------------
// Error categories
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCategory {
    Network,
    Storage,
    Rendering,
    Config,
    System,
}

impl ErrorCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Storage => "storage",
            Self::Rendering => "rendering",
            Self::Config => "config",
            Self::System => "system",
        }
    }
}

// ---------------------------------------------------------------------------
// Error actions and records
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorAction {
    Retry,
    OpenSettings,
    CopyDetails,
    Dismiss,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub id: EventId,
    pub occurred_at: AppTimestamp,
    pub severity: crate::errors::AppErrorSeverity,
    pub category: ErrorCategory,
    pub message: String,
    pub actions: Vec<ErrorAction>,
}

// ---------------------------------------------------------------------------
// In-memory error surface state (primary store, capped at 200)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ErrorSurfaceState {
    pub records: Vec<ErrorRecord>,
}

impl Global for ErrorSurfaceState {}

pub fn initialize(cx: &mut App) {
    cx.set_global(ErrorSurfaceState::default());
}

pub fn report(
    message: impl Into<String>,
    severity: crate::errors::AppErrorSeverity,
    category: ErrorCategory,
    actions: Vec<ErrorAction>,
    cx: &mut App,
) -> EventId {
    let record = ErrorRecord {
        id: EventId::new(),
        occurred_at: AppTimestamp::now(),
        severity,
        category,
        message: message.into(),
        actions,
    };
    let id = record.id;
    let record_clone = record.clone();
    let message_str = record.message.clone();

    // Track the error message so the panic handler can attach it to crash reports.
    crate::lifecycle::track_recent_error(message_str);

    cx.update_global::<ErrorSurfaceState, _>(|state, cx| {
        state.records.insert(0, record);
        if state.records.len() > 200 {
            state.records.truncate(200);
        }

        // Persist to SQLite as secondary store (best-effort, non-blocking).
        if let Some(runtime) = cx.try_global::<crate::storage::StorageRuntime>()
            && let Err(err) = persist_error(&*runtime.backend, &record_clone)
        {
            tracing::warn!(
                target: "gpui_starter::error_surface",
                error = %err,
                "failed to persist error to sqlite"
            );
        }
    });

    id
}

pub fn snapshot(cx: &App) -> Vec<ErrorRecord> {
    cx.global::<ErrorSurfaceState>().records.clone()
}

pub fn latest(cx: &App) -> Option<ErrorRecord> {
    cx.try_global::<ErrorSurfaceState>()
        .and_then(|state| state.records.first().cloned())
}

/// Returns the message of the latest error record, without cloning the full record.
pub fn latest_message(cx: &App) -> Option<String> {
    cx.try_global::<ErrorSurfaceState>()
        .and_then(|state| state.records.first().map(|r| r.message.clone()))
}

pub fn dismiss(id: EventId, cx: &mut App) {
    cx.update_global::<ErrorSurfaceState, _>(|state, _cx| {
        state.records.retain(|r| r.id != id);
    });
}

// ---------------------------------------------------------------------------
// SQLite persistence (secondary store)
// ---------------------------------------------------------------------------

/// Persist a single error record to the `error_log` table.
///
/// The `error_log` table migration is defined in `db_migrations` (version 2).
/// If the table does not exist yet this call will fail and the in-memory store
/// remains authoritative.
pub fn persist_error(
    db: &dyn crate::storage::StorageBackend,
    error: &ErrorRecord,
) -> rusqlite::Result<()> {
    db.persist_error_record(error)
}

/// Load the most recent `limit` error records from SQLite, ordered by
/// occurrence time descending (newest first).
pub fn load_error_history(
    db: &dyn crate::storage::StorageBackend,
    limit: usize,
) -> rusqlite::Result<Vec<ErrorRecord>> {
    db.load_error_history(limit)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "error_surface.test.rs"]
mod error_surface_test;
