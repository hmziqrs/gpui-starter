#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{io::Write, path::{Path, PathBuf}};

use atomic_write_file::AtomicWriteFile;
use gpui::{App, BorrowAppContext, Global};
use gpui_component::scroll::ScrollbarShow;
use serde::{Deserialize, Serialize};

use crate::{
    app::{LOCALE_EN, LOCALE_ZH_CN},
    errors::AppError,
    notifications::inbox::NotificationInboxItem,
    paths::{AppPaths, ensure_parent_dir},
    routes::AppRoute,
};

/// Duration (in milliseconds) to wait after the last config mutation before
/// flushing to disk. Rapid successive calls to [`update_config`] are coalesced
/// into a single write.
const DEBOUNCE_MS: u64 = 300;

pub const APP_STATE_VERSION: u32 = 1;

/// Shared flag that coordinates the debounce timer. When a save is already
/// scheduled, the flag is `true` and the timer loop simply resets its wait.
/// This avoids spawning multiple concurrent debounce tasks.
static SAVE_SCHEDULED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct AppState {
    pub paths: AppPaths,
    pub config: AppConfig,
    pub last_load_error: Option<String>,
    pub last_save_error: Option<String>,
    /// Tracks whether the in-memory config has changed since the last disk flush.
    dirty: bool,
    /// The serialized bytes of the last successfully persisted config. Used for
    /// dirty-checking: if a new serialization matches, we skip the write entirely.
    last_flushed_bytes: Vec<u8>,
}

impl Global for AppState {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    pub theme: String,
    pub scrollbar_show: Option<ScrollbarShow>,
    pub locale: String,
    pub active_route: AppRoute,
    pub sidebar_collapsed: bool,
    pub native_notifications_enabled: bool,
    #[serde(default = "default_true")]
    pub global_shortcut_enabled: bool,
    pub first_run_completed: bool,
    pub notification_inbox: Vec<NotificationInboxItem>,
    pub window_bounds: Option<PersistedWindowBounds>,
    #[serde(default)]
    pub granted_permissions: HashSet<String>,
    #[serde(default)]
    pub denied_permissions: HashSet<String>,
    #[serde(default = "default_stable")]
    pub update_channel: String,
    #[serde(default)]
    pub last_update_check: Option<String>,
    /// Show the dev-only frame-time readout in the status bar.
    /// Defaults to `true` in debug builds, `false` in release builds.
    #[serde(default = "default_show_frame_time")]
    pub show_frame_time: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersistedWindowBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: APP_STATE_VERSION,
            theme: "Default Light".to_string(),
            scrollbar_show: None,
            locale: LOCALE_EN.to_string(),
            active_route: AppRoute::default(),
            sidebar_collapsed: false,
            native_notifications_enabled: true,
            global_shortcut_enabled: true,
            first_run_completed: false,
            notification_inbox: Vec::new(),
            window_bounds: None,
            granted_permissions: HashSet::new(),
            denied_permissions: HashSet::new(),
            update_channel: default_stable(),
            last_update_check: None,
            show_frame_time: default_show_frame_time(),
        }
    }
}

impl AppConfig {
    pub fn normalized(mut self) -> Self {
        if self.version == 0 {
            self.version = APP_STATE_VERSION;
        }
        if self.locale != LOCALE_EN && self.locale != LOCALE_ZH_CN {
            self.locale = LOCALE_EN.to_string();
        }
        self
    }
}

fn default_true() -> bool {
    true
}

fn default_stable() -> String {
    "stable".to_string()
}

/// Default for `show_frame_time`: enabled in debug builds, disabled in release.
fn default_show_frame_time() -> bool {
    cfg!(debug_assertions)
}

pub fn initialize(cx: &mut App) {
    let paths = match AppPaths::new() {
        Ok(paths) => paths,
        Err(err) => {
            tracing::error!(target: "gpui_starter::app_state", error = %err, "failed to initialize app paths");
            return;
        }
    };

    let (config, last_load_error) = load_config(&paths.state_file);
    tracing::info!(
        target: "gpui_starter::app_state",
        state_file = %paths.state_file.display(),
        config_dir = %paths.config_dir.display(),
        data_dir = %paths.data_dir.display(),
        log_dir = %paths.log_dir.display(),
        last_load_error = ?last_load_error,
        "loaded app state"
    );

    // Pre-compute the initial flushed bytes so that a no-op update_config
    // immediately after startup will skip the write.
    let initial_bytes = serde_json::to_vec(&config).unwrap_or_default();

    cx.set_global(AppState {
        paths,
        config,
        last_load_error,
        last_save_error: None,
        dirty: false,
        last_flushed_bytes: initial_bytes,
    });
}

pub fn config(cx: &App) -> AppConfig {
    cx.try_global::<AppState>()
        .map(|s| s.config.clone())
        .unwrap_or_default()
}

/// Borrow the active [`AppConfig`] without cloning the whole struct.
///
/// Returns `None` only if [`initialize`] has not yet run. Prefer this — or
/// [`with_config`] — over [`config`] in render paths that only need to read a
/// field or two (e.g. the per-frame status-bar readout), so the full `AppConfig`
/// (now carrying `notification_inbox`, two permission `HashSet`s, …) is not
/// deep-cloned every frame.
pub fn config_handle(cx: &App) -> Option<&AppConfig> {
    cx.try_global::<AppState>().map(|s| &s.config)
}

/// Run a closure with borrowed access to the active [`AppConfig`].
///
/// Falls back to [`AppConfig::default`] when the global is not yet installed,
/// so callers never need to handle the absent case themselves.
pub fn with_config<R>(cx: &App, f: impl FnOnce(&AppConfig) -> R) -> R {
    match cx.try_global::<AppState>() {
        Some(state) => f(&state.config),
        None => f(&AppConfig::default()),
    }
}

/// Convenience field getter: returns just the configured update channel,
/// cloning the cheap `String` rather than the whole `AppConfig`.
pub fn update_channel(cx: &App) -> String {
    with_config(cx, |c| c.update_channel.clone())
}

pub fn paths(cx: &App) -> AppPaths {
    cx.try_global::<AppState>()
        .map(|s| s.paths.clone())
        .unwrap_or_else(|| {
            tracing::error!(target: "gpui_starter::app_state", "AppState not initialized, using fallback paths");
            AppPaths::new().expect("failed to initialize fallback app paths")
        })
}

/// Mutates the application config and schedules a debounced save.
///
/// The mutation closure runs synchronously. Instead of immediately writing to
/// disk, the config is marked dirty and a delayed save is scheduled. If another
/// call arrives before the timer fires, the flag is already set and the two
/// updates are coalesced into a single I/O operation.
///
/// For immediate persistence (e.g. during shutdown), use [`force_save`].
pub fn update_config(cx: &mut App, update: impl FnOnce(&mut AppConfig)) {
    if cx.try_global::<AppState>().is_none() {
        tracing::warn!(target: "gpui_starter::app_state", "attempted to update app state before initialization");
        return;
    }

    cx.update_global::<AppState, _>(|state, _cx| {
        update(&mut state.config);
        state.config = state.config.clone().normalized();
        state.dirty = true;
    });

    // Only spawn a new debounce task if one is not already scheduled.
    if SAVE_SCHEDULED
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        let bg = cx.background_executor().clone();
        cx.spawn(async move |cx| {
            bg.timer(std::time::Duration::from_millis(DEBOUNCE_MS))
                .await;

            // Clear the flag so the next update_config can schedule a fresh timer.
            SAVE_SCHEDULED.store(false, Ordering::Relaxed);

            // Step 1 (UI thread): serialize + dirty-check. No I/O here.
            let request = cx.update(|cx| {
                cx.update_global::<AppState, _>(|state, _cx| {
                    if !state.dirty {
                        return None;
                    }
                    prepare_flush(state)
                })
            });

            let Some((path, bytes)) = request else { return; };

            // Step 2 (background thread): atomic write + fsync. The UI thread
            // never blocks on disk I/O.
            let write_bytes = bytes.clone();
            let result = bg
                .spawn(async move { save_config(&path, &write_bytes) })
                .await;

            // Step 3 (UI thread): apply the result to in-memory state.
            cx.update(|cx| {
                cx.update_global::<AppState, _>(|state, _cx| {
                    commit_flush(state, bytes, result);
                });
            });
        })
        .detach();
    }
}

/// Flushes any pending config changes to disk immediately.
///
/// Call this during shutdown to ensure no configuration is lost. If the config
/// is not dirty, this is a no-op.
pub fn force_save(cx: &mut App) {
    if cx.try_global::<AppState>().is_none() {
        return;
    }

    // Clear any pending debounce timer.
    SAVE_SCHEDULED.store(false, Ordering::Relaxed);

    // Shutdown path: write synchronously. We cannot yield from here and risk
    // the process exiting before the write lands, so the atomic write + fsync
    // stays inline. (The debounced hot path in `update_config` is the one that
    // moves fsync off the UI thread.)
    cx.update_global::<AppState, _>(|state, _cx| {
        if !state.dirty {
            return;
        }
        if let Some((path, bytes)) = prepare_flush(state) {
            let result = save_config(&path, &bytes);
            commit_flush(state, bytes, result);
        }
    });
}

/// Serializes the config and runs the dirty-check.
///
/// Uses compact JSON (`serde_json::to_vec`) instead of pretty-printed JSON to
/// reduce I/O volume (~30% fewer bytes). Returns `Some((path, bytes))` when a
/// write is actually needed, or `None` if the config is clean / byte-identical
/// to the last successful flush. Serialization failures are recorded on
/// `state.last_save_error` and yield `None`.
///
/// This runs on the UI thread — it does no I/O, only serialization.
fn prepare_flush(state: &mut AppState) -> Option<(PathBuf, Vec<u8>)> {
    let new_bytes = match serde_json::to_vec(&state.config) {
        Ok(bytes) => bytes,
        Err(err) => {
            let error = err.to_string();
            tracing::error!(
                target: "gpui_starter::app_state",
                error = %error,
                "failed to serialize app state"
            );
            state.last_save_error = Some(error);
            return None;
        }
    };

    // Dirty-check: if the serialized form is identical to what is already on
    // disk, skip the atomic write entirely.
    if new_bytes == state.last_flushed_bytes {
        state.dirty = false;
        tracing::debug!(
            target: "gpui_starter::app_state",
            "config unchanged after normalization; skipping write"
        );
        return None;
    }

    Some((state.paths.state_file.clone(), new_bytes))
}

/// Applies the outcome of a [`save_config`] call to the in-memory state.
///
/// On success, marks the state clean and records the flushed bytes — but only
/// if the in-memory config still serializes to the same bytes we just wrote.
/// That guard handles the (small) race where a `update_config` mutation lands
/// while the write is in flight on the background thread: instead of falsely
/// marking the state clean, we leave `dirty = true` so the next debounce
/// re-flushes the newer bytes. On failure, `last_save_error` is set and the
/// state stays dirty so the next debounce retries.
fn commit_flush(state: &mut AppState, written_bytes: Vec<u8>, result: Result<(), AppError>) {
    match result {
        Ok(()) => {
            state.last_save_error = None;
            let current = serde_json::to_vec(&state.config).unwrap_or_default();
            if current == written_bytes {
                state.dirty = false;
                state.last_flushed_bytes = written_bytes;
                tracing::debug!(
                    target: "gpui_starter::app_state",
                    state_file = %state.paths.state_file.display(),
                    "persisted app state"
                );
            } else {
                tracing::debug!(
                    target: "gpui_starter::app_state",
                    "config changed during flush; will re-flush on next debounce"
                );
            }
        }
        Err(err) => {
            let error = err.to_string();
            tracing::error!(
                target: "gpui_starter::app_state",
                error = %error,
                "failed to persist app state"
            );
            state.last_save_error = Some(error);
        }
    }
}

fn load_config(path: &Path) -> (AppConfig, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(json) => match serde_json::from_str::<AppConfig>(&json) {
            Ok(config) => (crate::config_migrations::migrate(config).normalized(), None),
            Err(err) => {
                quarantine_bad_config(path);
                (
                    AppConfig::default(),
                    Some(
                        AppError::StateParse {
                            path: path.to_path_buf(),
                            details: err.to_string(),
                        }
                        .to_string(),
                    ),
                )
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (AppConfig::default(), None),
        Err(err) => (
            AppConfig::default(),
            Some(
                AppError::StateRead {
                    path: path.to_path_buf(),
                    details: err.to_string(),
                }
                .to_string(),
            ),
        ),
    }
}

/// Writes pre-serialized bytes to the config file using an atomic write.
fn save_config(path: &Path, json_bytes: &[u8]) -> Result<(), AppError> {
    ensure_parent_dir(path)?;
    let mut file = AtomicWriteFile::options()
        .open(path)
        .map_err(|err| AppError::StateWrite {
            path: path.to_path_buf(),
            details: err.to_string(),
        })?;
    file.write_all(json_bytes)
        .map_err(|err| AppError::StateWrite {
            path: path.to_path_buf(),
            details: err.to_string(),
        })?;
    file.write_all(b"\n").map_err(|err| AppError::StateWrite {
        path: path.to_path_buf(),
        details: err.to_string(),
    })?;
    file.commit().map_err(|err| AppError::StateWrite {
        path: path.to_path_buf(),
        details: err.to_string(),
    })?;
    Ok(())
}

fn quarantine_bad_config(path: &Path) {
    if !path.exists() {
        return;
    }
    let quarantine_path = path.with_extension("json.bad");
    if let Err(err) = std::fs::rename(path, &quarantine_path) {
        tracing::warn!(
            target: "gpui_starter::app_state",
            source = %path.display(),
            target_path = %quarantine_path.display(),
            error = %err,
            "failed to quarantine corrupt app state"
        );
    }
}

#[cfg(test)]
#[path = "config_store.test.rs"]
mod config_store_test;
