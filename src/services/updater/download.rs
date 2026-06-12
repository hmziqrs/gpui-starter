use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::types::*;
use gpui::{App, UpdateGlobal as _};

// ---------------------------------------------------------------------------
// Download update
// ---------------------------------------------------------------------------

pub fn download_update(cx: &mut App) {
    let current = super::snapshot(cx);
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

    super::set_status(UpdateStatus::Downloading { progress: 0 }, cx);

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
        let asset = match super::check::fetch_platform_asset(rt.clone(), client.clone()).await {
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
                    super::set_status(UpdateStatus::Downloading { progress: cur }, cx);
                });
            }

            // Check if the download finished — try a non-blocking poll.
            if download_handle.is_finished() {
                break;
            }

            // Sleep briefly on the tokio runtime to avoid busy-waiting.
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            })
            .await
            .ok();
        }

        // Read final progress.
        let cur = progress.load(Ordering::Relaxed);
        if cur != last_progress {
            let _ = cx.update(|cx| {
                super::set_status(UpdateStatus::Downloading { progress: cur }, cx);
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
                        cx.update(|cx| super::set_status(UpdateStatus::Error(err), cx));
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
                        cx.update(|cx| super::set_status(UpdateStatus::Error(err), cx));
                        return;
                    }
                }
            }

            // Reset download retry count on success.
            cx.update(|cx| {
                super::reset_download_retry(cx);
                let status = UpdateStatus::Downloaded {
                    version: version.clone(),
                    path: path_str,
                };
                super::set_status(status, cx);
                super::notify_update_downloaded(&version, cx);
            });
        }
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Internal helpers — retry/backoff for downloads
// ---------------------------------------------------------------------------

/// Handle a failed download with retry/backoff logic.
fn handle_download_failure(error: String, cx: &mut App) {
    let retry_count = cx.global::<UpdateSnapshot>().download_retry_count;
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
        UpdateSnapshot::update_global(cx, |snap, _cx| {
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
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            })
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
        super::set_status(UpdateStatus::Error(error), cx);
        super::notify_update_error(cx);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers — crypto
// ---------------------------------------------------------------------------

/// Verify the Ed25519 signature of the downloaded file.
///
/// The signature covers the SHA-256 hash of the file contents.
/// The signature is base64-encoded in the manifest.
fn verify_ed25519_signature(
    file_path: &std::path::Path,
    signature_b64: &str,
) -> Result<(), String> {
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
