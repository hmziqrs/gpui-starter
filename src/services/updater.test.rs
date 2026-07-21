use super::types::{DEFAULT_MANIFEST_URL, current_app_version, pending_swap_path, platform_key};
use super::*;

// ---------------------------------------------------------------------------
// platform_key
// ---------------------------------------------------------------------------

#[test]
fn platform_key_format() {
    let key = platform_key();
    // Must contain os and arch separated by a dash.
    let parts: Vec<&str> = key.split('-').collect();
    assert!(
        parts.len() >= 2,
        "platform_key should contain at least os and arch separated by dash, got: {key}"
    );
    // Verify the arch portion is a known value.
    let arch = std::env::consts::ARCH;
    assert!(
        key.contains(arch),
        "platform_key should contain arch '{arch}', got: {key}"
    );
}

// ---------------------------------------------------------------------------
// current_app_version
// ---------------------------------------------------------------------------

#[test]
fn current_app_version_matches_cargo_pkg() {
    let version = current_app_version();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}

// ---------------------------------------------------------------------------
// UpdateManifest JSON parsing
// ---------------------------------------------------------------------------

#[test]
fn manifest_parse_valid_full() {
    let json = r#"{
        "version": "2.0.0",
        "release_notes": "Big update",
        "platforms": {
            "macos-aarch64": {
                "url": "https://example.com/app.tar.gz",
                "signature": "abc123",
                "size": 12345678
            }
        }
    }"#;
    let manifest: UpdateManifest = serde_json::from_str(json).expect("parse full manifest");
    assert_eq!(manifest.version, "2.0.0");
    assert_eq!(manifest.release_notes, "Big update");
    assert_eq!(manifest.platforms.len(), 1);
    let asset = manifest
        .platforms
        .get("macos-aarch64")
        .expect("platform key");
    assert_eq!(asset.url, "https://example.com/app.tar.gz");
    assert_eq!(asset.signature, "abc123");
    assert_eq!(asset.size, 12345678);
}

#[test]
fn manifest_parse_minimal_version_only() {
    let json = r#"{ "version": "1.0.0" }"#;
    let manifest: UpdateManifest = serde_json::from_str(json).expect("parse minimal manifest");
    assert_eq!(manifest.version, "1.0.0");
    assert!(manifest.release_notes.is_empty());
    assert!(manifest.platforms.is_empty());
}

#[test]
fn manifest_parse_invalid_json() {
    let json = r#"not valid json"#;
    assert!(serde_json::from_str::<UpdateManifest>(json).is_err());
}

// ---------------------------------------------------------------------------
// Semver comparison (via the same logic the updater uses)
// ---------------------------------------------------------------------------

#[test]
fn semver_newer_available() {
    let manifest_ver = semver::Version::parse("2.0.0").unwrap();
    let cur_ver = semver::Version::parse("1.0.0").unwrap();
    assert!(manifest_ver > cur_ver);
}

#[test]
fn semver_equal() {
    let manifest_ver = semver::Version::parse("1.0.0").unwrap();
    let cur_ver = semver::Version::parse("1.0.0").unwrap();
    assert!(manifest_ver == cur_ver);
    assert!(manifest_ver <= cur_ver);
}

#[test]
fn semver_older_manifest() {
    let manifest_ver = semver::Version::parse("0.9.0").unwrap();
    let cur_ver = semver::Version::parse("1.0.0").unwrap();
    assert!(manifest_ver < cur_ver);
}

#[test]
fn semver_invalid_version() {
    assert!(semver::Version::parse("not-a-version").is_err());
}

// ---------------------------------------------------------------------------
// UpdateStatus::default
// ---------------------------------------------------------------------------

#[test]
fn update_status_default_is_idle() {
    assert_eq!(UpdateStatus::default(), UpdateStatus::Idle);
}

// ---------------------------------------------------------------------------
// pending_swap_path consistency
// ---------------------------------------------------------------------------

#[test]
fn pending_swap_path_is_consistent() {
    let a = pending_swap_path();
    let b = pending_swap_path();
    assert_eq!(
        a, b,
        "pending_swap_path should return the same path on every call"
    );
}

// ---------------------------------------------------------------------------
// DEFAULT_MANIFEST_URL is not empty
// ---------------------------------------------------------------------------

#[test]
fn default_manifest_url_not_empty() {
    assert!(!DEFAULT_MANIFEST_URL.is_empty());
}
