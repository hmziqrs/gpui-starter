use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use arboard::Clipboard;
use gpui::{App, BorrowAppContext as _, Global};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, thiserror::Error)]
pub enum DesktopActionError {
    #[error("clipboard operation failed: {0}")]
    Clipboard(#[from] arboard::Error),
    #[error("failed to open path '{path}'")]
    OpenPathFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open url '{url}'")]
    OpenUrlFailed {
        url: String,
        #[source]
        source: std::io::Error,
    },
    #[error("file dialog failed: {0}")]
    DialogFailed(String),
    #[error("watcher error: {0}")]
    Watcher(#[from] notify::Error),
    #[error("desktop actions unavailable")]
    Unavailable,
    #[error("watcher lock poisoned")]
    LockPoisoned,
}

#[derive(Clone, Debug)]
pub struct DesktopActionsSnapshot {
    pub clipboard_available: bool,
    pub picker_available: bool,
    pub opener_available: bool,
    pub active_watchers: usize,
    pub last_error: Option<String>,
}

impl Default for DesktopActionsSnapshot {
    fn default() -> Self {
        Self {
            clipboard_available: false,
            picker_available: true,
            opener_available: true,
            active_watchers: 0,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct DesktopActionsState {
    snapshot: DesktopActionsSnapshot,
    inner: Arc<Mutex<DesktopActionsInner>>,
}

struct DesktopActionsInner {
    next_watcher_id: u64,
    watchers: BTreeMap<u64, RecommendedWatcher>,
}

impl Global for DesktopActionsState {}

pub fn initialize(cx: &mut App) {
    let mut snapshot = DesktopActionsSnapshot {
        clipboard_available: Clipboard::new().is_ok(),
        ..DesktopActionsSnapshot::default()
    };
    if !snapshot.clipboard_available {
        snapshot.last_error = Some("clipboard backend unavailable".to_string());
    }
    let status = match snapshot.last_error.as_deref() {
        Some(err) => crate::capabilities::CapabilityStatus::degraded(err),
        None => crate::capabilities::CapabilityStatus::supported_enabled(),
    };
    crate::capabilities::set("desktop_actions", status, cx);
    cx.set_global(DesktopActionsState {
        snapshot,
        inner: Arc::new(Mutex::new(DesktopActionsInner {
            next_watcher_id: 1,
            watchers: BTreeMap::new(),
        })),
    });
}

pub fn snapshot(cx: &App) -> DesktopActionsSnapshot {
    cx.try_global::<DesktopActionsState>()
        .map(|s| s.snapshot.clone())
        .unwrap_or_default()
}

pub fn copy_text(text: &str, cx: &mut App) -> Result<(), DesktopActionError> {
    let result = Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_string()))
        .map_err(DesktopActionError::from);
    update_result(
        "clipboard_copy",
        result.as_ref().err().map(|e| e.to_string()),
        cx,
    );
    result
}

pub fn copy_diagnostics(cx: &mut App) -> Result<(), DesktopActionError> {
    let diagnostics = build_diagnostics_text(cx);
    copy_text(&diagnostics, cx)
}

pub fn open_logs_folder(cx: &mut App) -> Result<(), DesktopActionError> {
    let path = crate::app_state::paths(cx).log_dir;
    open_path(path, cx)
}

pub fn open_config_folder(cx: &mut App) -> Result<(), DesktopActionError> {
    let path = crate::app_state::paths(cx).config_dir;
    open_path(path, cx)
}

pub fn open_url(url: &str, cx: &mut App) -> Result<(), DesktopActionError> {
    let result = open::that_detached(url).map_err(|source| DesktopActionError::OpenUrlFailed {
        url: url.to_string(),
        source,
    });
    update_result("open_url", result.as_ref().err().map(|e| e.to_string()), cx);
    result
}

pub fn pick_file(cx: &mut App) -> Option<PathBuf> {
    let file = rfd::FileDialog::new().pick_file();
    tracing::info!(target: "gpui_starter::desktop_actions", file = ?file, "file picker result");
    update_result("pick_file", None, cx);
    file
}

pub fn pick_folder(cx: &mut App) -> Option<PathBuf> {
    let folder = rfd::FileDialog::new().pick_folder();
    tracing::info!(target: "gpui_starter::desktop_actions", folder = ?folder, "folder picker result");
    update_result("pick_folder", None, cx);
    folder
}

pub fn save_file(cx: &mut App) -> Option<PathBuf> {
    let file = rfd::FileDialog::new().save_file();
    tracing::info!(target: "gpui_starter::desktop_actions", file = ?file, "save file picker result");
    update_result("save_file", None, cx);
    file
}

pub fn watch_path(path: PathBuf, cx: &mut App) -> Result<u64, DesktopActionError> {
    let state = cx
        .try_global::<DesktopActionsState>()
        .ok_or(DesktopActionError::Unavailable)?;
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| DesktopActionError::LockPoisoned)?;
    let watcher_id = inner.next_watcher_id;
    inner.next_watcher_id += 1;

    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) => tracing::debug!(
                target: "gpui_starter::desktop_actions",
                watcher_id,
                kind = ?event.kind,
                paths = ?event.paths,
                "watch event"
            ),
            Err(err) => tracing::warn!(
                target: "gpui_starter::desktop_actions",
                watcher_id,
                error = %err,
                "watch event error"
            ),
        })
        .map_err(DesktopActionError::from)?;
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(DesktopActionError::from)?;
    inner.watchers.insert(watcher_id, watcher);
    let active_watchers = inner.watchers.len();
    drop(inner);
    cx.update_global::<DesktopActionsState, _>(|state, _cx| {
        state.snapshot.active_watchers = active_watchers;
        state.snapshot.last_error = None;
    });
    tracing::info!(target: "gpui_starter::desktop_actions", watcher_id, path = %path.display(), "watcher registered");
    Ok(watcher_id)
}

pub fn shutdown(cx: &mut App) {
    let Some(state) = cx.try_global::<DesktopActionsState>() else {
        return;
    };
    if let Ok(mut inner) = state.inner.lock() {
        inner.watchers.clear();
    }
    cx.update_global::<DesktopActionsState, _>(|state, _cx| {
        state.snapshot.active_watchers = 0;
    });
}

pub fn watch_log_dir(cx: &mut App) -> Result<u64, DesktopActionError> {
    let path = crate::app_state::paths(cx).log_dir;
    watch_path(path, cx)
}

pub fn watch_config_dir(cx: &mut App) -> Result<u64, DesktopActionError> {
    let path = crate::app_state::paths(cx).config_dir;
    watch_path(path, cx)
}

/// Watch the config directory for changes and coalesce bursts into a single
/// config-refresh. Activates the previously inert `notify` scaffolding:
/// instead of merely logging events, the watcher callback pushes into a flume
/// channel; a gpui task `recv`s the first event, sleeps 500ms to let an
/// editor's write burst settle, drains any remaining events, then triggers a
/// window refresh so the UI picks up the new config.
pub fn watch_config_dir_reactive(cx: &mut App) -> Result<u64, DesktopActionError> {
    let state = cx
        .try_global::<DesktopActionsState>()
        .ok_or(DesktopActionError::Unavailable)?;
    let path = crate::app_state::paths(cx).config_dir.clone();
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| DesktopActionError::LockPoisoned)?;
    let watcher_id = inner.next_watcher_id;
    inner.next_watcher_id += 1;

    let (tx, rx) = flume::bounded::<()>(64);
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) => {
                tracing::debug!(
                    target: "gpui_starter::desktop_actions",
                    watcher_id,
                    kind = ?event.kind,
                    paths = ?event.paths,
                    "config watch event"
                );
                // Best-effort enqueue; a full channel just means a refresh is
                // already pending — the burst will be coalesced anyway.
                let _ = tx.try_send(());
            }
            Err(err) => tracing::warn!(
                target: "gpui_starter::desktop_actions",
                watcher_id,
                error = %err,
                "config watch event error"
            ),
        })
        .map_err(DesktopActionError::from)?;
    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(DesktopActionError::from)?;
    inner.watchers.insert(watcher_id, watcher);
    let active_watchers = inner.watchers.len();
    drop(inner);
    cx.update_global::<DesktopActionsState, _>(|state, _cx| {
        state.snapshot.active_watchers = active_watchers;
        state.snapshot.last_error = None;
    });

    // Coalescing task: wait for the first event, settle, drain, refresh.
    let bg = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            // Block until the first event of a new burst arrives.
            if rx.recv_async().await.is_err() {
                // Sender half dropped (watcher unregistered): stop.
                break;
            }
            // Let an editor's multi-write burst settle, then drain the rest.
            bg.timer(std::time::Duration::from_millis(500)).await;
            while rx.try_recv().is_ok() {}
            tracing::info!(
                target: "gpui_starter::desktop_actions",
                "config change burst settled; refreshing windows"
            );
            cx.update(|cx| {
                cx.refresh_windows();
            });
        }
    })
    .detach();

    tracing::info!(
        target: "gpui_starter::desktop_actions",
        watcher_id,
        path = %path.display(),
        "reactive config watcher registered"
    );
    Ok(watcher_id)
}

pub fn unwatch_path(id: u64, cx: &mut App) -> bool {
    let Some(state) = cx.try_global::<DesktopActionsState>() else {
        return false;
    };
    let (removed, active_watchers) = if let Ok(mut inner) = state.inner.lock() {
        let removed = inner.watchers.remove(&id).is_some();
        (removed, inner.watchers.len())
    } else {
        return false;
    };
    cx.update_global::<DesktopActionsState, _>(|state, _cx| {
        state.snapshot.active_watchers = active_watchers;
    });
    removed
}

pub fn unwatch_all(cx: &mut App) -> usize {
    let state = match cx.try_global::<DesktopActionsState>().cloned() {
        Some(state) => state,
        None => return 0,
    };
    let watcher_ids = state
        .inner
        .lock()
        .ok()
        .map(|inner| inner.watchers.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    let count = watcher_ids.len();
    for watcher_id in watcher_ids {
        let _ = unwatch_path(watcher_id, cx);
    }
    count
}

fn build_diagnostics_text(cx: &App) -> String {
    let app_state = crate::app_state::config(cx);
    let stage = cx
        .try_global::<crate::lifecycle::LifecycleState>()
        .map(|s| s.stage)
        .unwrap_or(crate::lifecycle::LifecycleStage::Starting);
    let connectivity = crate::connectivity::snapshot(cx);
    format!(
        "app={} version={} route={} lifecycle={:?} connectivity={:?}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        app_state.active_route.to_url(),
        stage,
        connectivity.state
    )
}

pub fn open_path(path: PathBuf, cx: &mut App) -> Result<(), DesktopActionError> {
    let result =
        open::that_detached(path.as_path()).map_err(|source| DesktopActionError::OpenPathFailed {
            path: path.display().to_string(),
            source,
        });
    update_result(
        "open_path",
        result.as_ref().err().map(|e| e.to_string()),
        cx,
    );
    result
}

fn update_result(action: &str, error: Option<String>, cx: &mut App) {
    let Some(state) = cx.try_global::<DesktopActionsState>() else {
        return;
    };
    let active_watchers = state
        .inner
        .lock()
        .ok()
        .map(|inner| inner.watchers.len())
        .unwrap_or(0);
    cx.update_global::<DesktopActionsState, _>(|state, _cx| {
        state.snapshot.last_error = error.clone();
        state.snapshot.active_watchers = active_watchers;
    });
    if let Some(error) = error {
        tracing::warn!(
            target: "gpui_starter::desktop_actions",
            action,
            error = %error,
            "desktop action failed"
        );
        crate::error_surface::report(
            format!("Desktop action `{action}` failed: {error}"),
            crate::errors::AppErrorSeverity::Warning,
            crate::error_surface::ErrorCategory::System,
            vec![
                crate::error_surface::ErrorAction::Retry,
                crate::error_surface::ErrorAction::Dismiss,
            ],
            cx,
        );
    } else {
        tracing::debug!(
            target: "gpui_starter::desktop_actions",
            action,
            "desktop action succeeded"
        );
    }
}
