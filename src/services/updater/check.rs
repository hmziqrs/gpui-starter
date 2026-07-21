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

    let (rt, client) = crate::services::tokio_runtime::runtime_and_client(cx)
        .expect("tokio runtime global must be installed before checking for updates");
    let current_version = current.current_version.clone();

    // P8: If we already hold a manifest cached from a recent check, reuse it
    // instead of hitting the network again. "Recent" = within one periodic
    // check interval (PERIODIC_CHECK_INTERVAL_SECS), matching the staleness
    // horizon used by the periodic re-check loop in `mod.rs`.
    let cached_manifest = current.cached_manifest.clone();
    let cache_fresh = cached_manifest.is_some()
        && current
            .last_check
            .as_ref()
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|checked_at| {
                let age = chrono::Utc::now()
                    .signed_duration_since(checked_at.with_timezone(&chrono::Utc));
                age.num_seconds() < PERIODIC_CHECK_INTERVAL_SECS as i64
            })
            .unwrap_or(false);

    cx.spawn(async move |cx| {
        let manifest_result: Result<UpdateManifest, String> = if cache_fresh {
            // cache_fresh is only true when cached_manifest is Some.
            Ok(cached_manifest.expect("cache_fresh implies cached_manifest is Some"))
        } else {
            fetch_manifest(rt.clone(), client.clone()).await
        };

        cx.update(move |cx| match manifest_result {
            Ok(manifest) => {
                tracing::info!(
                    target: "gpui_starter::updater",
                    manifest_version = %manifest.version,
                    current_version = %current_version,
                    cache_hit = cache_fresh,
                    "fetched update manifest"
                );

                // P8: Cache the manifest and the resolved platform asset so
                // `download_update` can skip re-fetching. Re-storing on a cache
                // hit is idempotent and harmless.
                let asset_for_cache = {
                    let key = platform_key();
                    manifest.platforms.get(&key).cloned()
                };
                UpdateSnapshot::update_global(cx, |snap, _cx| {
                    snap.cached_manifest = Some(manifest.clone());
                    snap.cached_asset = asset_for_cache;
                });

                // Only stamp `last_check` when we actually talked to the
                // network — a cache hit did not re-verify the manifest.
                if !cache_fresh {
                    let now = chrono::Utc::now().to_rfc3339();
                    UpdateSnapshot::update_global(cx, |snap, _cx| {
                        snap.last_check = Some(now.clone());
                    });
                    crate::app_state::update_config(cx, |config| {
                        config.last_update_check = Some(now);
                    });
                }

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
// Internal helpers — retry/backoff (shared by check and download)
// ---------------------------------------------------------------------------

/// Field accessor used by [`schedule_retry`] to read/write the check retry
/// counter on [`UpdateSnapshot`].
pub(super) fn check_retry_field(snap: &mut UpdateSnapshot) -> &mut u32 {
    &mut snap.check_retry_count
}

/// Field accessor used by [`schedule_retry`] to read/write the download retry
/// counter on [`UpdateSnapshot`].
pub(super) fn download_retry_field(snap: &mut UpdateSnapshot) -> &mut u32 {
    &mut snap.download_retry_count
}

/// Shared retry/backoff scheduler used by both `handle_check_failure` and
/// `handle_download_failure`.
///
/// Reads the retry counter via `retry_field`, increments it, and (when still
/// under [`MAX_UPDATE_RETRIES`]) waits out the exponential backoff on the
/// background executor and then invokes `rearm` to kick off the next attempt.
/// Returns `true` when a retry was scheduled, or `false` when retries are
/// exhausted — the caller owns the terminal error path in that case.
///
/// The caller is responsible for setting the interim status (`Error` or
/// `Available`); this helper only touches the retry counter and the rearm.
///
/// Uses the GPUI background-executor timer instead of spawning a fresh tokio
/// task purely to sleep, so the recheck is cooperatively scheduled.
pub(super) fn schedule_retry<F, R>(
    cx: &mut App,
    retry_field: F,
    retry_kind: &'static str,
    rearm: R,
) -> bool
where
    F: Fn(&mut UpdateSnapshot) -> &mut u32,
    R: FnOnce(&mut App) + Send + 'static,
{
    let retry_count = *retry_field(cx.global_mut::<UpdateSnapshot>());
    if retry_count >= MAX_UPDATE_RETRIES {
        return false;
    }
    let new_count = retry_count + 1;
    let delay_secs = RETRY_BASE_DELAY_SECS * 2u64.pow(new_count - 1);
    tracing::warn!(
        target: "gpui_starter::updater",
        kind = retry_kind,
        retry = new_count,
        max = MAX_UPDATE_RETRIES,
        delay_secs,
        "update operation failed — scheduling retry"
    );
    UpdateSnapshot::update_global(cx, |snap, _cx| {
        *retry_field(snap) = new_count;
    });

    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(delay_secs))
            .await;
        cx.update(|cx| {
            tracing::info!(
                target: "gpui_starter::updater",
                kind = retry_kind,
                retry = new_count,
                "retrying update operation"
            );
            rearm(cx);
        });
    })
    .detach();
    true
}

// ---------------------------------------------------------------------------
// Internal helpers — retry/backoff for checks
// ---------------------------------------------------------------------------

/// Handle a failed check with retry/backoff logic.
fn handle_check_failure(error: String, cx: &mut App) {
    let scheduled = schedule_retry(cx, check_retry_field, "check", |cx| check_for_updates(cx));
    super::set_status(UpdateStatus::Error(error.clone()), cx);
    if !scheduled {
        tracing::error!(
            target: "gpui_starter::updater",
            error = %error,
            "update check failed — retries exhausted, setting permanent error"
        );
        // Notify the user about permanent failure.
        super::notify_update_error(cx);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers — networking
// ---------------------------------------------------------------------------

/// Fetch the update manifest from the configured URL.
///
/// Single source of truth for the 5-step reqwest pipeline
/// (GET → timeout → status check → bytes → `serde_json`). Callers that also
/// need a platform asset should use [`fetch_platform_asset`], which delegates
/// here and then resolves the platform key.
pub(crate) async fn fetch_manifest(
    rt: Arc<tokio::runtime::Runtime>,
    client: reqwest::Client,
) -> Result<UpdateManifest, String> {
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
}

pub(crate) async fn fetch_platform_asset(
    rt: Arc<tokio::runtime::Runtime>,
    client: reqwest::Client,
) -> Result<PlatformAsset, String> {
    let manifest = fetch_manifest(rt, client).await?;
    let key = platform_key();
    manifest
        .platforms
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("no asset for platform '{key}' in manifest"))
}
