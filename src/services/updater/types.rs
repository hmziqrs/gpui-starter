use std::path::PathBuf;

use gpui::actions;
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

impl gpui::Global for UpdateSnapshot {}

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

pub(crate) const DEFAULT_MANIFEST_URL: &str = match option_env!("GPUI_UPDATE_MANIFEST_URL") {
    Some(url) => url,
    None => "https://releases.example.com/manifest.json",
};

/// Hardcoded Ed25519 public key for update manifest signature verification.
/// The corresponding private key is stored in CI secrets (UPDATE_SIGNING_KEY).
/// Replace this with your actual public key when deploying.
pub(crate) const UPDATER_PUBLIC_KEY: &[u8; 32] = include_bytes!("../updater_public_key.bin");

pub(crate) const MAX_UPDATE_RETRIES: u32 = 3;
pub(crate) const RETRY_BASE_DELAY_SECS: u64 = 30;
pub(crate) const STARTUP_CHECK_DELAY_SECS: u64 = 5;
pub(crate) const PERIODIC_CHECK_INTERVAL_SECS: u64 = 4 * 60 * 60; // 4 hours

pub(crate) fn platform_key() -> String {
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

pub(crate) fn pending_swap_path() -> PathBuf {
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

pub(crate) fn current_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
