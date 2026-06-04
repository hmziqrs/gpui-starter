use std::path::PathBuf;

use gpui::{App, BorrowAppContext as _, Global};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default)]
pub struct UpdateSnapshot {
    pub status: UpdateStatus,
    pub current_version: String,
    pub last_check: Option<String>,
    pub update_channel: String,
}

impl Global for UpdateSnapshot {}

// ---------------------------------------------------------------------------
// Manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    release_notes: String,
    #[serde(default)]
    platforms: std::collections::HashMap<String, PlatformAsset>,
}

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
struct PlatformAsset {
    url: String,
    #[serde(default)]
    signature: String,
    #[serde(default)]
    size: u64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_MANIFEST_URL: &str = "https://releases.example.com/manifest.json";
const MACOS_PLATFORM_KEY: &str = "macos-aarch64";

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
    });
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
        set_status(UpdateStatus::Error(format!(
            "no network connectivity ({:?})",
            connectivity.state
        )), cx);
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

                if manifest.version == current_version {
                    set_status(UpdateStatus::UpToDate, cx);
                } else {
                    set_status(
                        UpdateStatus::Available {
                            version: manifest.version,
                            notes: manifest.release_notes,
                        },
                        cx,
                    );
                }
            }
            Err(err) => {
                tracing::error!(
                    target: "gpui_starter::updater",
                    error = %err,
                    "update check failed"
                );
                set_status(UpdateStatus::Error(err), cx);
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
                cx.update(|cx| set_status(UpdateStatus::Error(err), cx));
                return;
            }
        };

        // Step 2: Download the asset to a temp directory.
        let download_result: Result<PathBuf, String> = (|| async {
            let tmp_dir = std::env::temp_dir().join("gpui-starter-updates");
            std::fs::create_dir_all(&tmp_dir)
                .map_err(|e| format!("failed to create temp dir: {e}"))?;

            let file_name = asset
                .url
                .rsplit('/')
                .next()
                .unwrap_or("update.bin")
                .to_string();
            let dest_path = tmp_dir.join(&file_name);

            tracing::info!(
                target: "gpui_starter::updater",
                url = %asset.url,
                dest = %dest_path.display(),
                size = asset.size,
                "starting download"
            );

            let response = rt
                .spawn(async move {
                    client
                        .get(&asset.url)
                        .timeout(std::time::Duration::from_secs(300))
                        .send()
                        .await
                })
                .await
                .map_err(|e| format!("download request panicked: {e}"))?
                .map_err(|e| format!("download request failed: {e}"))?;

            if !response.status().is_success() {
                return Err(format!("download returned status {}", response.status()));
            }

            let bytes = response
                .bytes()
                .await
                .map_err(|e| format!("failed to read download body: {e}"))?;

            std::fs::write(&dest_path, &bytes)
                .map_err(|e| format!("failed to write update file: {e}"))?;

            Ok(dest_path)
        })()
        .await;

        cx.update(move |cx| match download_result {
            Ok(path) => {
                let path_str = path.to_string_lossy().to_string();
                tracing::info!(
                    target: "gpui_starter::updater",
                    version = %version,
                    path = %path_str,
                    "download complete"
                );

                // Step 3: Verify codesign on macOS.
                #[cfg(target_os = "macos")]
                {
                    match verify_codesign(&path) {
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
                            set_status(UpdateStatus::Error(err), cx);
                            return;
                        }
                    }
                }

                set_status(
                    UpdateStatus::Downloaded {
                        version: version.clone(),
                        path: path_str,
                    },
                    cx,
                );
            }
            Err(err) => {
                tracing::error!(
                    target: "gpui_starter::updater",
                    error = %err,
                    "download failed"
                );
                set_status(UpdateStatus::Error(err), cx);
            }
        });
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
    let marker_dir = std::env::temp_dir().join("gpui-starter-updates");
    let _ = std::fs::create_dir_all(&marker_dir);
    let marker_path = marker_dir.join("pending-swap.json");
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
// Internal helpers
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

async fn fetch_platform_asset(
    rt: std::sync::Arc<tokio::runtime::Runtime>,
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

    manifest
        .platforms
        .get(MACOS_PLATFORM_KEY)
        .cloned()
        .ok_or_else(|| format!("no asset for platform '{MACOS_PLATFORM_KEY}' in manifest"))
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
