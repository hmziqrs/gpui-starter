use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::{
    errors::AppError, ids::EventId, ipc::command::ForwardedCommand, routes::AppRoute,
    time::AppTimestamp,
};

// Re-export the forwarded-command type so consumers of the event queue can
// pattern-match on `RemoteCommand(cmd)` without reaching into the ipc module.
pub use crate::ipc::command::ForwardedCommand as RemoteCommandKind;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppEventKind {
    Navigate(AppRoute),
    DeepLinkReceived(String),
    /// A typed command forwarded from a second instance over IPC. The dispatch
    /// layer translates this into the appropriate app action.
    RemoteCommand(ForwardedCommand),
    AppError {
        message: String,
        severity: crate::errors::AppErrorSeverity,
    },
    DiagnosticsChanged,
    Test {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppEvent {
    pub id: EventId,
    pub emitted_at: AppTimestamp,
    pub kind: AppEventKind,
}

impl AppEvent {
    pub fn new(kind: AppEventKind) -> Self {
        Self {
            id: EventId::new(),
            emitted_at: AppTimestamp::now(),
            kind,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AppEventQueue(pub Vec<AppEvent>);

impl Global for AppEventQueue {}

pub fn emit(kind: AppEventKind, cx: &mut App) {
    let event = AppEvent::new(kind);
    tracing::debug!(
        target: "gpui_starter::events",
        event_id = %event.id,
        kind = ?event.kind,
        "emitting app event"
    );
    cx.default_global::<AppEventQueue>().0.push(event);
}

pub fn emit_error(error: AppError, cx: &mut App) {
    let severity = error.severity();
    let message = error.to_string();
    tracing::warn!(
        target: "gpui_starter::events",
        severity = ?severity,
        error = %message,
        "emitting app error"
    );
    emit(AppEventKind::AppError { message, severity }, cx);
}

pub fn drain(cx: &mut App) -> Vec<AppEvent> {
    // Read-only check first: if the queue is empty, return early without
    // touching a mutable reference.  Without this guard, default_global()
    // mutates the global on every call, which re-triggers observe_global
    // callbacks, which call drain() again → infinite loop / hang.
    if cx
        .try_global::<AppEventQueue>()
        .map_or(true, |q| q.0.is_empty())
    {
        return Vec::new();
    }
    std::mem::take(&mut cx.default_global::<AppEventQueue>().0)
}

#[cfg(test)]
#[path = "events.test.rs"]
mod events_test;
