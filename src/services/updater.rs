use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use gpui::{App, BorrowAppContext as _, Global, actions};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

actions!(updater, [CheckForUpdates]);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
        notes: String,
    },
    Downloading {
        progress: u32, // 0–100
    },
    Downloaded {
        version: String,
        path: String,
    },
    ReadyToInstall,
    Error(String),
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UpdateSnapshot {
    pub status: UpdateStatus,
    pub current_version: String,
    pub last_check: Option<String>,
    pub update_channel: String,
    pub check_retry_count: u32,
    pub download_retry_count: u32,
}

impl Global for UpdateSnapshot {}

// ---------------------------------------------------------------------------
// Manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub release_notes: String,
    #[serde(default)]
    pub platforms: std::collections::HashMap<String, PlatformAsset>,
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
#[allow(dead_code)]
pub struct PlatformAsset {
    pub url: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub size: u64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_MANIFEST_URL: &str = match option_env!("GPUI_UPDATE_MANIFEST_URL") {
    Some(url) => url,
    None => "https://releases.example.com/manifest.json",
};

/// Hardcoded Ed25519 public key for update manifest signature verification.
/// The corresponding private key is stored in CI secrets (UPDATE_SIGNING_KEY).
/// Replace this with your actual public key when deploying.
const UPDATER_PUBLIC_KEY: &[u8; 32] = include_bytes!("updater_public_key.bin");

const MAX_UPDATE_RETRIES: u32 = 3;
const RETRY_BASE_DELAY_SECS: u64 = 30;
const STARTUP_CHECK_DELAY_SECS: u64 = 5;
const PERIODIC_CHECK_INTERVAL_SECS: u64 = 4 * 60 * 60; // 4 hours

fn platform_key() -> String {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };
    let arch = std::env::consts::ARCH; // "aarch64", "x86_64", etc.
    format!("{os}-{arch}")
}

fn pending_swap_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "gpui-starter")
        .map(|pd| {
            let dir = pd.data_dir().join("updates");
            let _ = std::fs::create_dir_all(&dir);
            dir.join("pending-swap.json")
        })
        .unwrap_or_else(|| {
            let dir = std::env::temp_dir().join("gpui-starter-updates");
            let _ = std::fs::create_dir_all(&dir);
            dir.join("pending-swap.json")
        })
}

fn current_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------------------------------------------------------------------------
// Initialize
// ---------------------------------------------------------------------------

pub fn initialize(cx: &mut App) {
    let channel = crate::app_state::config(cx).update_channel.clone();
    cx.set_global(UpdateSnapshot {
        status: UpdateStatus::Idle,
        current_version: current_app_version(),
        last_check: None,
        update_channel: if channel.is_empty() {
            "stable".to_string()
        } else {
            channel
        },
        check_retry_count: 0,
        download_retry_count: 0,
    });

    // Register the CheckForUpdates action handler.
    cx.on_action(|_: &CheckForUpdates, cx| {
        check_for_updates(cx);
    });

    // Schedule a delayed startup check (5 seconds after launch).
    let startup_rt = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .runtime
        .clone();
    cx.spawn(async move |cx| {
        startup_rt
            .spawn(tokio::time::sleep(std::time::Duration::from_secs(
                STARTUP_CHECK_DELAY_SECS,
            )))
            .await
            .ok();
        cx.update(|cx| {
            tracing::info!(
                target: "gpui_starter::updater",
                "running startup update check"
            );
            check_for_updates(cx);
        });
    })
    .detach();

    // Schedule periodic re-check every 4 hours.
    let periodic_rt = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .runtime
        .clone();
    cx.spawn(async move |cx| {
        loop {
            periodic_rt
                .spawn(tokio::time::sleep(std::time::Duration::from_secs(
                    PERIODIC_CHECK_INTERVAL_SECS,
                )))
                .await
                .ok();

            let should_check: bool = cx.update(|cx| {
                let snap = snapshot(cx);
                matches!(
                    snap.status,
                    UpdateStatus::Idle
                        | UpdateStatus::UpToDate
                        | UpdateStatus::Error(_)
                )
            });
            if should_check {
                cx.update(|cx| {
                    tracing::info!(
                        target: "gpui_starter::updater",
                        "running periodic update check"
                    );
                    check_for_updates(cx);
                });
            }
        }
    })
    .detach();

    tracing::info!(
        target: "gpui_starter::updater",
        version = %env!("CARGO_PKG_VERSION"),
        "updater service initialized"
    );
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

pub fn snapshot(cx: &App) -> UpdateSnapshot {
    cx.try_global::<UpdateSnapshot>()
        .cloned()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Check for updates
// ---------------------------------------------------------------------------

pub fn check_for_updates(cx: &mut App) {
    // Bail if already checking or downloading.
    let current = snapshot(cx);
    match &current.status {
        UpdateStatus::Checking | UpdateStatus::Downloading { .. } => {
            tracing::debug!(
                target: "gpui_starter::updater",
                status = ?current.status,
                "skipping check — update operation already in progress"
            );
            return;
        }
        _ => {}
    }

    // Check connectivity first.
    let connectivity = crate::connectivity::snapshot(cx);
    if connectivity.state != crate::connectivity::ConnectivityState::Online {
        tracing::warn!(
            target: "gpui_starter::updater",
            connectivity = ?connectivity.state,
            "skipping update check — not online"
        );
        handle_check_failure("no network connectivity".to_string(), cx);
        return;
    }

    set_status(UpdateStatus::Checking, cx);

    let rt = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .runtime
        .clone();
    let client = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .http_client
        .clone();
    let current_version = current.current_version.clone();

    cx.spawn(async move |cx| {
        let manifest_result: Result<UpdateManifest, String> = (|| async {
            let response = rt
                .spawn(async move {
                    client
                        .get(DEFAULT_MANIFEST_URL)
                        .timeout(std::time::Duration::from_secs(30))
                        .send()
                        .await
                })
                .await
                .map_err(|e| format!("manifest request panicked: {e}"))?
                .map_err(|e| format!("manifest request failed: {e}"))?;

            if !response.status().is_success() {
                return Err(format!("manifest returned status {}", response.status()));
            }

            let body = response
                .bytes()
                .await
                .map_err(|e| format!("failed to read manifest body: {e}"))?;

            serde_json::from_slice::<UpdateManifest>(&body)
                .map_err(|e| format!("failed to parse manifest: {e}"))
        })()
        .await;

        cx.update(move |cx| match manifest_result {
            Ok(manifest) => {
                tracing::info!(
                    target: "gpui_starter::updater",
                    manifest_version = %manifest.version,
                    current_version = %current_version,
                    "fetched update manifest"
                );

                let now = chrono::Utc::now().to_rfc3339();
                cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
                    snap.last_check = Some(now.clone());
                });
                crate::app_state::update_config(cx, |config| {
                    config.last_update_check = Some(now);
                });

                match (
                    semver::Version::parse(&manifest.version),
                    semver::Version::parse(&current_version),
                ) {
                    (Ok(manifest_ver), Ok(cur_ver)) => {
                        if manifest_ver > cur_ver {
                            // Reset retry count on successful check.
                            reset_check_retry(cx);
                            let status = UpdateStatus::Available {
                                version: manifest.version.clone(),
                                notes: manifest.release_notes.clone(),
                            };
                            set_status(status, cx);
                            notify_update_available(&manifest.version, cx);
                        } else {
                            // Equal or older manifest version — we are up to date (or ahead).
                            reset_check_retry(cx);
                            set_status(UpdateStatus::UpToDate, cx);
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        tracing::error!(
                            target: "gpui_starter::updater",
                            error = %e,
                            "failed to parse version for comparison"
                        );
                        set_status(
                            UpdateStatus::Error(format!("version parse error: {e}")),
                            cx,
                        );
                    }
                }
            }
            Err(err) => {
                tracing::error!(
                    target: "gpui_starter::updater",
                    error = %err,
                    "update check failed"
                );
                handle_check_failure(err, cx);
            }
        });
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Download update
// ---------------------------------------------------------------------------

pub fn download_update(cx: &mut App) {
    let current = snapshot(cx);
    let (version, _asset_url, _expected_size) = match &current.status {
        UpdateStatus::Available { version, .. } => {
            // We need to fetch the manifest again to get the asset URL.
            // Store the version; we'll resolve the URL from the manifest.
            (version.clone(), None::<String>, None::<u64>)
        }
        _ => {
            tracing::warn!(
                target: "gpui_starter::updater",
                status = ?current.status,
                "download requested but no update available"
            );
            return;
        }
    };

    set_status(UpdateStatus::Downloading { progress: 0 }, cx);

    let rt = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .runtime
        .clone();
    let client = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .http_client
        .clone();

    cx.spawn(async move |cx| {
        // Step 1: Re-fetch manifest to get the asset URL for the current platform.
        let asset = match fetch_platform_asset(rt.clone(), client.clone()).await {
            Ok(a) => a,
            Err(err) => {
                tracing::error!(
                    target: "gpui_starter::updater",
                    error = %err,
                    "failed to resolve platform asset for download"
                );
                cx.update(|cx| handle_download_failure(err, cx));
                return;
            }
        };

        // Step 2: Download the asset to a temp directory with streaming progress.
        let tmp_dir = std::env::temp_dir().join("gpui-starter-updates");
        if let Err(err) = std::fs::create_dir_all(&tmp_dir) {
            let msg = format!("failed to create temp dir: {err}");
            tracing::error!(target: "gpui_starter::updater", error = %msg);
            cx.update(|cx| handle_download_failure(msg, cx));
            return;
        };

        let file_name = asset
            .url
            .rsplit('/')
            .next()
            .unwrap_or("update.bin")
            .to_string();
        let dest_path = tmp_dir.join(&file_name);
        let signature = asset.signature.clone();

        tracing::info!(
            target: "gpui_starter::updater",
            url = %asset.url,
            dest = %dest_path.display(),
            size = asset.size,
            "starting download"
        );

        let response = match rt
            .spawn(async move {
                client
                    .get(&asset.url)
                    .timeout(std::time::Duration::from_secs(300))
                    .send()
                    .await
            })
            .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                let msg = format!("download request failed: {e}");
                tracing::error!(target: "gpui_starter::updater", error = %msg);
                cx.update(|cx| handle_download_failure(msg, cx));
                return;
            }
            Err(e) => {
                let msg = format!("download request panicked: {e}");
                tracing::error!(target: "gpui_starter::updater", error = %msg);
                cx.update(|cx| handle_download_failure(msg, cx));
                return;
            }
        };

        if !response.status().is_success() {
            let msg = format!("download returned status {}", response.status());
            tracing::error!(target: "gpui_starter::updater", error = %msg);
            cx.update(|cx| handle_download_failure(msg, cx));
            return;
        }

        let total: u64 = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let file = match std::fs::File::create(&dest_path) {
            Ok(f) => f,
            Err(err) => {
                let msg = format!("failed to create download file: {err}");
                tracing::error!(target: "gpui_starter::updater", error = %msg);
                cx.update(|cx| handle_download_failure(msg, cx));
                return;
            }
        };

        // Shared progress value updated by the download task.
        let progress = Arc::new(AtomicU32::new(0));

        let total_for_task = total;
        let progress_clone = progress.clone();

        // Spawn the streaming download as a self-contained 'static task.
        let download_handle = rt.spawn(async move {
            use futures_util::StreamExt as _;
            use std::io::Write as _;
            let mut stream = response.bytes_stream();
            let mut downloaded: u64 = 0;
            let mut last_reported: u32 = 0;
            let mut file = file;
            let mut last_err: Option<String> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        downloaded += chunk.len() as u64;
                        if let Err(err) = file.write_all(&chunk) {
                            last_err = Some(format!("failed to write download chunk: {err}"));
                            break;
                        }
                        if total_for_task > 0 {
                            let pct = (downloaded as f32 / total_for_task as f32 * 100.0) as u32;
                            if pct / 10 > last_reported / 10 {
                                last_reported = pct;
                                progress_clone.store(pct, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(e) => {
                        last_err = Some(format!("failed to read download chunk: {e}"));
                        break;
                    }
                }
            }

            let _ = file.flush();
            if let Some(err) = last_err {
                Err(err)
            } else {
                Ok(downloaded)
            }
        });

        // Poll the shared progress and push updates into GPUI state.
        // We use a loop with short tokio sleeps to periodically check.
        let mut last_progress: u32 = 0;
        loop {
            let cur = progress.load(Ordering::Relaxed);
            if cur != last_progress {
                last_progress = cur;
                let _ = cx.update(|cx| {
                    set_status(UpdateStatus::Downloading { progress: cur }, cx);
                });
            }

            // Check if the download finished — try a non-blocking poll.
            if download_handle.is_finished() {
                break;
            }

            // Sleep briefly on the tokio runtime to avoid busy-waiting.
            rt.spawn(tokio::time::sleep(std::time::Duration::from_millis(200)))
                .await
                .ok();
        }

        // Read final progress.
        let cur = progress.load(Ordering::Relaxed);
        if cur != last_progress {
            let _ = cx.update(|cx| {
                set_status(UpdateStatus::Downloading { progress: cur }, cx);
            });
        }

        let downloaded = match download_handle.await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(err)) => {
                tracing::error!(target: "gpui_starter::updater", error = %err, "download failed");
                cx.update(|cx| handle_download_failure(err, cx));
                return;
            }
            Err(e) => {
                let msg = format!("download task panicked: {e}");
                tracing::error!(target: "gpui_starter::updater", error = %msg);
                cx.update(|cx| handle_download_failure(msg, cx));
                return;
            }
        };

        {

        let path_str = dest_path.to_string_lossy().to_string();
        tracing::info!(
            target: "gpui_starter::updater",
            version = %version,
            path = %path_str,
            downloaded_bytes = downloaded,
            "download complete"
        );

        // Step 3: Verify Ed25519 signature (if present in manifest).
        if !signature.is_empty() {
            match verify_ed25519_signature(&dest_path, &signature) {
                Ok(()) => {
                    tracing::info!(
                        target: "gpui_starter::updater",
                        path = %path_str,
                        "Ed25519 signature verification passed"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        target: "gpui_starter::updater",
                        path = %path_str,
                        error = %err,
                        "Ed25519 signature verification failed — deleting download"
                    );
                    let _ = std::fs::remove_file(&dest_path);
                    cx.update(|cx| set_status(UpdateStatus::Error(err), cx));
                    return;
                }
            }
        } else {
            tracing::warn!(
                target: "gpui_starter::updater",
                path = %path_str,
                "no signature in manifest — skipping Ed25519 verification (backward compat)"
            );
        }

        // Step 4: Verify codesign on macOS.
        #[cfg(target_os = "macos")]
        {
            match verify_codesign(&dest_path) {
                Ok(()) => {
                    tracing::info!(
                        target: "gpui_starter::updater",
                        path = %path_str,
                        "codesign verification passed"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        target: "gpui_starter::updater",
                        path = %path_str,
                        error = %err,
                        "codesign verification failed"
                    );
                    cx.update(|cx| set_status(UpdateStatus::Error(err), cx));
                    return;
                }
            }
        }

        // Reset download retry count on success.
        cx.update(|cx| {
            reset_download_retry(cx);
            let status = UpdateStatus::Downloaded {
                version: version.clone(),
                path: path_str,
            };
            set_status(status, cx);
            notify_update_downloaded(&version, cx);
        });
        }
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Apply update
// ---------------------------------------------------------------------------

pub fn apply_update(cx: &mut App) {
    let current = snapshot(cx);
    let (version, path) = match &current.status {
        UpdateStatus::Downloaded { version, path } => (version.clone(), path.clone()),
        _ => {
            tracing::warn!(
                target: "gpui_starter::updater",
                status = ?current.status,
                "apply requested but no downloaded update"
            );
            return;
        }
    };

    tracing::info!(
        target: "gpui_starter::updater",
        version = %version,
        path = %path,
        "scheduling update swap on next launch"
    );

    set_status(UpdateStatus::ReadyToInstall, cx);

    // Write a marker file so the app launcher can perform the swap on next boot.
    let marker_path = pending_swap_path();
    let pending = serde_json::json!({
        "version": version,
        "source_path": path,
        "scheduled_at": chrono::Utc::now().to_rfc3339(),
    });
    if let Err(err) = std::fs::write(&marker_path, pending.to_string()) {
        tracing::error!(
            target: "gpui_starter::updater",
            path = %marker_path.display(),
            error = %err,
            "failed to write pending swap marker"
        );
        set_status(
            UpdateStatus::Error(format!("failed to schedule swap: {err}")),
            cx,
        );
    }
}

// ---------------------------------------------------------------------------
// Check and apply pending swap on startup
// ---------------------------------------------------------------------------

pub fn check_pending_swap(cx: &mut App) {
    let marker_path = pending_swap_path();
    if !marker_path.exists() {
        return;
    }

    tracing::info!(
        target: "gpui_starter::updater",
        path = %marker_path.display(),
        "pending swap marker found, attempting binary swap"
    );

    let data = match std::fs::read_to_string(&marker_path) {
        Ok(d) => d,
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "failed to read pending swap marker"
            );
            return;
        }
    };

    let pending: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "failed to parse pending swap marker"
            );
            // Remove corrupt marker so we don't retry indefinitely.
            let _ = std::fs::remove_file(&marker_path);
            return;
        }
    };

    let source_path = match pending.get("source_path").and_then(|v| v.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            tracing::error!(
                target: "gpui_starter::updater",
                "pending swap marker missing source_path"
            );
            let _ = std::fs::remove_file(&marker_path);
            return;
        }
    };

    let version = pending
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    if !source_path.exists() {
        tracing::error!(
            target: "gpui_starter::updater",
            path = %source_path.display(),
            "pending swap source path does not exist"
        );
        let _ = std::fs::remove_file(&marker_path);
        return;
    }

    let current_exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "failed to determine current executable path"
            );
            return;
        }
    };

    // Detect whether we are inside a .app bundle on macOS.
    let swap_result: Result<(), String> = (|| {
        #[cfg(target_os = "macos")]
        {
            // If the current exe is inside a .app bundle, swap the entire bundle.
            if let Some(bundle_path) = current_exe
                .ancestors()
                .find(|a| a.extension().is_some_and(|ext| ext == "app"))
            {
                // The downloaded source might also be a .app bundle or a directory
                // that should replace the bundle.
                let source_bundle = if source_path.extension().is_some_and(|ext| ext == "app") {
                    source_path.clone()
                } else if source_path.is_dir() {
                    source_path.clone()
                } else {
                    // Standalone binary inside a bundle — replace the binary directly.
                    let dest_binary = current_exe.clone();
                    std::process::Command::new("mv")
                        .arg("-f")
                        .arg(&source_path)
                        .arg(&dest_binary)
                        .status()
                        .map_err(|e| format!("failed to mv binary: {e}"))?;
                    return Ok(());
                };

                tracing::info!(
                    target: "gpui_starter::updater",
                    source = %source_bundle.display(),
                    dest = %bundle_path.display(),
                    "swapping .app bundle"
                );
                // Remove old bundle and move new one into place.
                if bundle_path.exists() {
                    std::fs::remove_dir_all(bundle_path)
                        .map_err(|e| format!("failed to remove old bundle: {e}"))?;
                }
                std::process::Command::new("mv")
                    .arg(&source_bundle)
                    .arg(bundle_path)
                    .status()
                    .map_err(|e| format!("failed to mv bundle: {e}"))?;
                return Ok(());
            }
        }

        // Standalone binary fallback: rename the new binary over the current exe.
        let dest = current_exe.clone();
        std::fs::rename(&source_path, &dest)
            .map_err(|e| format!("failed to rename binary: {e}"))?;

        Ok(())
    })();

    match swap_result {
        Ok(()) => {
            tracing::info!(
                target: "gpui_starter::updater",
                version = %version,
                "pending swap applied successfully"
            );
            let _ = std::fs::remove_file(&marker_path);
            set_status(UpdateStatus::Idle, cx);
        }
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "pending swap failed"
            );
            // Leave the marker so the user/admin can investigate, but set error status.
            set_status(UpdateStatus::Error(format!("swap failed: {err}")), cx);
        }
    }
}

// ---------------------------------------------------------------------------
// Set channel
// ---------------------------------------------------------------------------

pub fn set_channel(channel: &str, cx: &mut App) {
    let ch = if channel.is_empty() {
        "stable".to_string()
    } else {
        channel.to_string()
    };
    cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
        snap.update_channel = ch.clone();
    });
    crate::app_state::update_config(cx, |config| {
        config.update_channel = ch;
    });
    tracing::info!(
        target: "gpui_starter::updater",
        channel = %channel,
        "update channel set"
    );
}

// ---------------------------------------------------------------------------
// Internal helpers — status management
// ---------------------------------------------------------------------------

fn set_status(status: UpdateStatus, cx: &mut App) {
    tracing::debug!(
        target: "gpui_starter::updater",
        status = ?status,
        "update status changed"
    );
    cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
        snap.status = status;
    });
}

// ---------------------------------------------------------------------------
// Internal helpers — retry/backoff
// ---------------------------------------------------------------------------

/// Handle a failed check with retry/backoff logic.
fn handle_check_failure(error: String, cx: &mut App) {
    let retry_count = cx
        .global::<UpdateSnapshot>()
        .check_retry_count;
    if retry_count < MAX_UPDATE_RETRIES {
        let new_count = retry_count + 1;
        let delay_secs = RETRY_BASE_DELAY_SECS * 2u64.pow(new_count - 1);
        tracing::warn!(
            target: "gpui_starter::updater",
            error = %error,
            retry = new_count,
            max = MAX_UPDATE_RETRIES,
            delay_secs,
            "update check failed — scheduling retry"
        );
        cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
            snap.check_retry_count = new_count;
        });
        set_status(UpdateStatus::Error(error), cx);

        // Schedule a retry.
        let rt = cx
            .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
            .0
            .runtime
            .clone();
        cx.spawn(async move |cx| {
            rt.spawn(tokio::time::sleep(std::time::Duration::from_secs(delay_secs)))
                .await
                .ok();
            cx.update(|cx| {
                tracing::info!(
                    target: "gpui_starter::updater",
                    retry = new_count,
                    "retrying update check"
                );
                check_for_updates(cx);
            });
        })
        .detach();
    } else {
        tracing::error!(
            target: "gpui_starter::updater",
            error = %error,
            retries = retry_count,
            "update check failed — retries exhausted, setting permanent error"
        );
        set_status(UpdateStatus::Error(error), cx);
        // Notify the user about permanent failure.
        notify_update_error(cx);
    }
}

/// Handle a failed download with retry/backoff logic.
fn handle_download_failure(error: String, cx: &mut App) {
    let retry_count = cx
        .global::<UpdateSnapshot>()
        .download_retry_count;
    if retry_count < MAX_UPDATE_RETRIES {
        let new_count = retry_count + 1;
        let delay_secs = RETRY_BASE_DELAY_SECS * 2u64.pow(new_count - 1);
        tracing::warn!(
            target: "gpui_starter::updater",
            error = %error,
            retry = new_count,
            max = MAX_UPDATE_RETRIES,
            delay_secs,
            "download failed — scheduling retry"
        );
        cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
            snap.download_retry_count = new_count;
            // Revert status back to Available so we can re-attempt.
            snap.status = UpdateStatus::Available {
                version: String::new(),
                notes: String::new(),
            };
        });

        // Schedule a download retry.
        let rt = cx
            .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
            .0
            .runtime
            .clone();
        cx.spawn(async move |cx| {
            rt.spawn(tokio::time::sleep(std::time::Duration::from_secs(delay_secs)))
                .await
                .ok();
            cx.update(|cx| {
                tracing::info!(
                    target: "gpui_starter::updater",
                    retry = new_count,
                    "retrying update download"
                );
                download_update(cx);
            });
        })
        .detach();
    } else {
        tracing::error!(
            target: "gpui_starter::updater",
            error = %error,
            retries = retry_count,
            "download failed — retries exhausted"
        );
        set_status(UpdateStatus::Error(error), cx);
        notify_update_error(cx);
    }
}

fn reset_check_retry(cx: &mut App) {
    cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
        snap.check_retry_count = 0;
    });
}

fn reset_download_retry(cx: &mut App) {
    cx.update_global::<UpdateSnapshot, _>(|snap, _cx| {
        snap.download_retry_count = 0;
    });
}

// ---------------------------------------------------------------------------
// Internal helpers — notifications
// ---------------------------------------------------------------------------

/// Dispatch a native notification for "update available".
fn notify_update_available(version: &str, cx: &mut App) {
    dispatch_background_notification(
        &format!("Update v{version} available"),
        "A new version is available. Open settings to download and install.",
        cx,
    );
}

/// Dispatch a native notification for "update downloaded".
fn notify_update_downloaded(version: &str, cx: &mut App) {
    dispatch_background_notification(
        &format!("Update v{version} ready"),
        "Update downloaded — restart to apply.",
        cx,
    );
}

/// Dispatch a native notification for permanent update errors (retries exhausted).
fn notify_update_error(cx: &mut App) {
    dispatch_background_notification(
        "Update check failed",
        "Could not check or download updates. Please try again later.",
        cx,
    );
}

/// Send a notification via the notification service without requiring a window handle.
fn dispatch_background_notification(title: &str, body: &str, cx: &mut App) {
    // Record in the notification inbox.
    crate::notifications::inbox::record_attempt(
        crate::notifications::inbox::NotificationAttemptRecord {
            title: title.to_string(),
            body: body.to_string(),
            backend: crate::notifications::NotificationBackendKind::UiOnly,
            delivered_natively: false,
            degraded: false,
            error_summary: None,
            kind: crate::notifications::inbox::NotificationInboxKind::SettingsUpdate,
        },
        cx,
    );

    // Also attempt a native notification via the service if available.
    let state = match cx.try_global::<crate::notifications::NativeNotificationState>() {
        Some(s) => s.clone(),
        None => return,
    };
    let enabled_by_user = state.snapshot.enabled_by_user
        && state.snapshot.permission
            != crate::notifications::NotificationPermissionState::Denied;
    if !enabled_by_user {
        return;
    }

    let request = crate::notifications::NotificationRequest::background_worthy(title, body);
    let service = state.service.clone();

    cx.spawn(async move |cx| {
        let result = service.send(request, enabled_by_user).await;
        tracing::info!(
            target: "gpui_starter::updater",
            backend = %result.backend_used,
            delivered = result.delivered_natively,
            "update notification send completed"
        );
        cx.update(|cx| {
            crate::notifications::apply_send_result_static(&result, cx);
        });
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Internal helpers — crypto
// ---------------------------------------------------------------------------

/// Verify the Ed25519 signature of the downloaded file.
///
/// The signature covers the SHA-256 hash of the file contents.
/// The signature is base64-encoded in the manifest.
fn verify_ed25519_signature(file_path: &std::path::Path, signature_b64: &str) -> Result<(), String> {
    use base64::Engine as _;
    use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
    use sha2::{Digest, Sha256};

    // Decode the base64 signature.
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|e| format!("failed to decode base64 signature: {e}"))?;

    if sig_bytes.len() != 64 {
        return Err(format!(
            "invalid signature length: expected 64 bytes, got {}",
            sig_bytes.len()
        ));
    }

    let sig_len = sig_bytes.len();
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| format!("invalid signature length: expected 64 bytes, got {sig_len}"))?;
    let signature = Signature::from_bytes(&sig_array);

    // Build the verifying key from the hardcoded public key.
    let pubkey_bytes: [u8; 32] = *UPDATER_PUBLIC_KEY;
    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|e| format!("invalid updater public key: {e}"))?;

    // Read the file and compute SHA-256.
    let file_data = std::fs::read(file_path)
        .map_err(|e| format!("failed to read downloaded file for signature check: {e}"))?;

    let hash = Sha256::digest(&file_data);

    // Verify the signature against the hash.
    verifying_key
        .verify(&hash, &signature)
        .map_err(|e| format!("Ed25519 signature verification failed: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers — networking
// ---------------------------------------------------------------------------

async fn fetch_platform_asset(
    rt: Arc<tokio::runtime::Runtime>,
    client: reqwest::Client,
) -> Result<PlatformAsset, String> {
    let response = rt
        .spawn(async move {
            client
                .get(DEFAULT_MANIFEST_URL)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
        })
        .await
        .map_err(|e| format!("manifest request panicked: {e}"))?
        .map_err(|e| format!("manifest request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("manifest returned status {}", response.status()));
    }

    let body = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read manifest body: {e}"))?;

    let manifest: UpdateManifest = serde_json::from_slice(&body)
        .map_err(|e| format!("failed to parse manifest: {e}"))?;

    let key = platform_key();
    manifest
        .platforms
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("no asset for platform '{key}' in manifest"))
}

#[cfg(target_os = "macos")]
fn verify_codesign(path: &std::path::Path) -> Result<(), String> {
    let output = std::process::Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run codesign: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("codesign verification failed: {stderr}"))
    }
}

#[cfg(test)]
#[path = "updater.test.rs"]
mod updater_test;
