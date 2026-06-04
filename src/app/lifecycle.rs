#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::time::AppTimestamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleStage {
    Starting,
    Running,
    ShuttingDown,
    Crashed,
}

#[derive(Clone, Debug)]
pub struct LifecycleState {
    pub stage: LifecycleStage,
    pub updated_at: AppTimestamp,
    pub startup_step: Option<String>,
    pub shutdown_step: Option<String>,
    pub last_startup_error: Option<String>,
    pub last_shutdown_error: Option<String>,
    pub last_error: Option<String>,
}

impl Global for LifecycleState {}

impl Default for LifecycleState {
    fn default() -> Self {
        Self::starting()
    }
}

impl LifecycleState {
    pub fn starting() -> Self {
        Self {
            stage: LifecycleStage::Starting,
            updated_at: AppTimestamp::now(),
            startup_step: None,
            shutdown_step: None,
            last_startup_error: None,
            last_shutdown_error: None,
            last_error: None,
        }
    }
}

pub fn set_stage(stage: LifecycleStage, cx: &mut App) {
    let state = cx.default_global::<LifecycleState>();
    state.stage = stage;
    state.updated_at = AppTimestamp::now();
}

pub fn set_startup_step(step: impl Into<String>, cx: &mut App) {
    let state = cx.default_global::<LifecycleState>();
    state.startup_step = Some(step.into());
    state.updated_at = AppTimestamp::now();
}

pub fn set_shutdown_step(step: impl Into<String>, cx: &mut App) {
    let state = cx.default_global::<LifecycleState>();
    state.shutdown_step = Some(step.into());
    state.updated_at = AppTimestamp::now();
}

pub fn set_startup_error(error: impl Into<String>, cx: &mut App) {
    let error = error.into();
    let state = cx.default_global::<LifecycleState>();
    state.last_startup_error = Some(error.clone());
    state.last_error = Some(error);
    state.updated_at = AppTimestamp::now();
}

pub fn set_shutdown_error(error: impl Into<String>, cx: &mut App) {
    let error = error.into();
    let state = cx.default_global::<LifecycleState>();
    state.last_shutdown_error = Some(error.clone());
    state.last_error = Some(error);
    state.updated_at = AppTimestamp::now();
}

static LAST_PANIC_SUMMARY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Application data directory, set once during startup so the panic hook can
/// write crash report files without needing GPUI context.
static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Ring buffer of recent error messages, populated by
/// [`track_recent_error`] so the panic handler can attach them to the
/// crash report.
static RECENT_ERRORS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// Set the application data directory for use by the panic hook.
///
/// Must be called once during startup, after `app_state` is initialized.
pub fn set_app_data_dir(path: PathBuf) {
    let _ = APP_DATA_DIR.set(path);
}

/// Record a recent error message so it can be attached to the next crash
/// report. Keeps at most 20 entries (oldest are dropped).
pub fn track_recent_error(msg: String) {
    let slot = RECENT_ERRORS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut guard) = slot.lock() {
        guard.push(msg);
        if guard.len() > 20 {
            guard.remove(0);
        }
    }
}

// ---------------------------------------------------------------------------
// Render-path tracking (thread-local guard)
// ---------------------------------------------------------------------------

std::thread_local! {
    static IN_RENDER_PATH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII guard returned by [`enter_render_path`]. Resets the thread-local flag
/// when dropped.
struct RenderPathGuard;

impl Drop for RenderPathGuard {
    fn drop(&mut self) {
        IN_RENDER_PATH.with(|c| c.set(false));
    }
}

/// Mark the current thread as being inside the render path.
///
/// Returns a guard that clears the flag on drop. Intended to wrap only the
/// `active_page_view` call so that only render-originating panics trigger the
/// error boundary.
pub fn enter_render_path() -> impl Drop {
    IN_RENDER_PATH.with(|c| c.set(true));
    RenderPathGuard
}

/// Returns `true` if the current thread is inside the render path.
pub fn in_render_path() -> bool {
    IN_RENDER_PATH.with(|c| c.get())
}

/// Set to `true` inside the panic hook so the next render pass can detect
/// that a panic occurred and swap in the error boundary view instead of the
/// crashing page.
static RENDER_PANIC_OCCURRED: AtomicBool = AtomicBool::new(false);

pub fn last_panic_summary() -> Option<String> {
    LAST_PANIC_SUMMARY
        .get()
        .and_then(|slot| slot.lock().ok().and_then(|value| value.clone()))
}

/// Atomically read and reset the render-panic flag.
///
/// Returns `true` if a panic was captured since the last call, `false`
/// otherwise.
pub fn take_render_panic() -> bool {
    RENDER_PANIC_OCCURRED.swap(false, Ordering::SeqCst)
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let summary = info.to_string();
        let slot = LAST_PANIC_SUMMARY.get_or_init(|| Mutex::new(None));
        if let Ok(mut value) = slot.lock() {
            *value = Some(summary.clone());
        }

        let is_render = in_render_path();

        // Mark that a render panic occurred so the render loop can show the
        // error boundary view on the next frame instead of re-trying the
        // crashing page.  Only set the flag when the panic originates inside
        // the render path to avoid false error-boundary activation from
        // background tasks, init, etc.
        if is_render {
            RENDER_PANIC_OCCURRED.store(true, Ordering::SeqCst);
        }

        // Write a crash report file if the data directory is configured.
        if let Some(data_dir) = APP_DATA_DIR.get() {
            let backtrace = std::backtrace::Backtrace::capture();
            let bt_string = match backtrace.status() {
                std::backtrace::BacktraceStatus::Captured => backtrace.to_string(),
                _ => String::new(),
            };

            let recent_errors = RECENT_ERRORS
                .get()
                .and_then(|slot| slot.lock().ok())
                .map(|guard| guard.clone())
                .unwrap_or_default();

            let report = crate::services::crash_report::CrashReport::new(
                summary.clone(),
                bt_string,
                is_render,
                recent_errors,
            );

            if let Err(err) =
                crate::services::crash_report::write_crash_report(&report, data_dir)
            {
                // We are inside the panic handler -- best-effort logging only.
                eprintln!(
                    "[gpui_starter::lifecycle] failed to write crash report: {err}"
                );
            }
        }

        tracing::error!(
            target: "gpui_starter::lifecycle",
            panic = %summary,
            "application panic captured"
        );
        previous(info);
    }));
}

// ---------------------------------------------------------------------------
// Crash marker (file-based crash detection)
// ---------------------------------------------------------------------------

fn crash_marker_path() -> PathBuf {
    std::env::temp_dir().join("gpui-starter.crash-marker")
}

/// Write a crash marker file at startup. If the process crashes, this file
/// will remain on disk so the next launch can detect it.
pub fn write_crash_marker() {
    let path = crash_marker_path();
    let pid = std::process::id();
    let timestamp = chrono::Utc::now().to_rfc3339();
    if let Err(err) = fs::write(&path, format!("pid={pid}\nstarted_at={timestamp}\n")) {
        tracing::warn!(
            target: "gpui_starter::lifecycle",
            path = %path.display(),
            error = %err,
            "failed to write crash marker"
        );
    }
}

/// Check whether a crash marker from a previous run exists. Returns `Some`
/// with the marker contents if found, `None` otherwise.
pub fn check_previous_crash() -> Option<String> {
    let path = crash_marker_path();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(err) => {
                tracing::warn!(
                    target: "gpui_starter::lifecycle",
                    path = %path.display(),
                    error = %err,
                    "crash marker exists but could not be read"
                );
                Some("<unreadable>".to_string())
            }
        }
    } else {
        None
    }
}

/// Remove the crash marker on a clean shutdown.
pub fn remove_crash_marker() {
    let path = crash_marker_path();
    if path.exists()
        && let Err(err) = fs::remove_file(&path)
    {
        tracing::warn!(
            target: "gpui_starter::lifecycle",
            path = %path.display(),
            error = %err,
            "failed to remove crash marker"
        );
    }
}

#[cfg(test)]
#[path = "lifecycle.test.rs"]
mod lifecycle_test;
