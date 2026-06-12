use std::sync::Arc;

use super::types::*;
use gpui::{App, UpdateGlobal as _};

// ---------------------------------------------------------------------------
// Check for updates
// ---------------------------------------------------------------------------

pub fn check_for_updates(cx: &mut App) {
    // Bail if already checking or downloading.
    let current = super::snapshot(cx);
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

    super::set_status(UpdateStatus::Checking, cx);

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
                UpdateSnapshot::update_global(cx, |snap, _cx| {
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
                            super::reset_check_retry(cx);
                            let status = UpdateStatus::Available {
                                version: manifest.version.clone(),
                                notes: manifest.release_notes.clone(),
                            };
                            super::set_status(status, cx);
                            super::notify_update_available(&manifest.version, cx);
                        } else {
                            // Equal or older manifest version — we are up to date (or ahead).
                            super::reset_check_retry(cx);
                            super::set_status(UpdateStatus::UpToDate, cx);
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        tracing::error!(
                            target: "gpui_starter::updater",
                            error = %e,
                            "failed to parse version for comparison"
                        );
                        super::set_status(
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
// Internal helpers — retry/backoff for checks
// ---------------------------------------------------------------------------

/// Handle a failed check with retry/backoff logic.
fn handle_check_failure(error: String, cx: &mut App) {
    let retry_count = cx.global::<UpdateSnapshot>().check_retry_count;
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
        UpdateSnapshot::update_global(cx, |snap, _cx| {
            snap.check_retry_count = new_count;
        });
        super::set_status(UpdateStatus::Error(error), cx);

        // Schedule a retry.
        let rt = cx
            .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
            .0
            .runtime
            .clone();
        cx.spawn(async move |cx| {
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            })
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
        super::set_status(UpdateStatus::Error(error), cx);
        // Notify the user about permanent failure.
        super::notify_update_error(cx);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers — networking
// ---------------------------------------------------------------------------

pub(crate) async fn fetch_platform_asset(
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

    let manifest: UpdateManifest =
        serde_json::from_slice(&body).map_err(|e| format!("failed to parse manifest: {e}"))?;

    let key = platform_key();
    manifest
        .platforms
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("no asset for platform '{key}' in manifest"))
}
