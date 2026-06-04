//! Snapshot tests using [`insta`] for core serialisable structures.
//!
//! Each test is independent. Run with:
//!
//!     cargo test --test snapshot_tests
//!
//! Review / accept snapshots with `cargo insta review`.

use std::collections::HashSet;

use gpui_starter::app_state::{AppConfig, PersistedWindowBounds};
use gpui_starter::routes::AppRoute;
use gpui_starter::services::updater::{
    PlatformAsset, UpdateManifest, UpdateSnapshot, UpdateStatus,
};
use gpui_starter::sidebar::Page;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn theme_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("themes")
}

// ---------------------------------------------------------------------------
// 1. Default AppConfig serialisation
// ---------------------------------------------------------------------------

#[test]
fn test_config_default_serialization() {
    let config = AppConfig::default();
    insta::assert_yaml_snapshot!("config_default", &config);
}

// ---------------------------------------------------------------------------
// 2. AppConfig round-trip (serialize -> deserialize -> compare)
// ---------------------------------------------------------------------------

#[test]
fn test_config_roundtrip() {
    let original = AppConfig {
        version: 1,
        theme: "Gruvbox Dark".to_string(),
        scrollbar_show: None,
        locale: "en".to_string(),
        active_route: AppRoute::page(Page::Settings),
        sidebar_collapsed: true,
        native_notifications_enabled: false,
        global_shortcut_enabled: true,
        first_run_completed: true,
        notification_inbox: Vec::new(),
        window_bounds: Some(PersistedWindowBounds {
            x: 100.0,
            y: 200.0,
            width: 800.0,
            height: 600.0,
        }),
        granted_permissions: HashSet::from(["camera".to_string(), "microphone".to_string()]),
        denied_permissions: HashSet::new(),
        update_channel: "stable".to_string(),
        last_update_check: None,
        show_frame_time: false,
    };

    let json = serde_json::to_string(&original).expect("serialize config");
    let restored: AppConfig = serde_json::from_str(&json).expect("deserialize config");
    assert_eq!(original, restored, "round-tripped config must be identical");

    let mut snapshot = serde_json::to_value(&original).expect("config to value");
    for key in ["granted_permissions", "denied_permissions"] {
        if let Some(values) = snapshot.get_mut(key).and_then(|value| value.as_array_mut()) {
            values.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        }
    }
    insta::assert_yaml_snapshot!("config_roundtrip", &snapshot);
}

// ---------------------------------------------------------------------------
// 3. Parse valid deep links
// ---------------------------------------------------------------------------

#[test]
fn test_route_parse_valid() {
    let valid_links = &[
        ("gpui-starter://home", "home"),
        ("gpui-starter://form", "form"),
        ("gpui-starter://settings", "settings"),
        ("gpui-starter://notifications", "notifications"),
        ("gpui-starter://diagnostics", "diagnostics"),
        ("gpui-starter://about", "about"),
        (
            "gpui-starter://settings/notifications",
            "settings_notifications",
        ),
        (
            "gpui-starter://home?ref=test&source=menu",
            "home_with_query",
        ),
    ];

    let results: Vec<(&str, String)> = valid_links
        .iter()
        .map(|(url, _label)| {
            let route = AppRoute::parse_deep_link(url)
                .unwrap_or_else(|e| panic!("expected `{url}` to parse successfully: {e}"));
            (*url, format!("{route:?}"))
        })
        .collect();

    insta::assert_yaml_snapshot!("route_parse_valid", &results);
}

// ---------------------------------------------------------------------------
// 4. Parse invalid deep links
// ---------------------------------------------------------------------------

#[test]
fn test_route_parse_invalid() {
    let invalid_links = &[
        ("https://example.com", "wrong_scheme"),
        ("gpui-starter://unknown-host", "unknown_host"),
        ("gpui-starter://home/extra", "extra_segment"),
        ("not-a-url-at-all", "garbage"),
        ("gpui-starter://settings/..%2Fetc", "path_traversal"),
        ("", "empty_string"),
    ];

    let results: Vec<(&str, String)> = invalid_links
        .iter()
        .map(|(url, _label)| {
            let err = AppRoute::parse_deep_link(url).unwrap_err();
            (*url, err.to_string())
        })
        .collect();

    insta::assert_yaml_snapshot!("route_parse_invalid", &results);
}

// ---------------------------------------------------------------------------
// 5. Theme file structure (keys / values)
// ---------------------------------------------------------------------------

#[test]
fn test_theme_structure() {
    let theme_path = theme_dir().join("gruvbox.json");
    let raw = std::fs::read_to_string(&theme_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", theme_path.display()));
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("theme file must be valid JSON");

    // Extract top-level keys and the name of each sub-theme for a stable snapshot.
    #[derive(serde::Serialize)]
    struct ThemeSummary {
        name: String,
        author: String,
        theme_count: usize,
        theme_names: Vec<String>,
        theme_modes: Vec<String>,
        color_key_count_per_theme: Vec<usize>,
        highlight_key_count_per_theme: Vec<usize>,
    }

    let themes = value.get("themes").and_then(|t| t.as_array()).unwrap();
    let summary = ThemeSummary {
        name: value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string(),
        author: value
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string(),
        theme_count: themes.len(),
        theme_names: themes
            .iter()
            .map(|t| t.get("name").and_then(|v| v.as_str()).unwrap().to_string())
            .collect(),
        theme_modes: themes
            .iter()
            .map(|t| t.get("mode").and_then(|v| v.as_str()).unwrap().to_string())
            .collect(),
        color_key_count_per_theme: themes
            .iter()
            .map(|t| {
                t.get("colors")
                    .and_then(|c| c.as_object())
                    .map(|o| o.len())
                    .unwrap_or(0)
            })
            .collect(),
        highlight_key_count_per_theme: themes
            .iter()
            .map(|t| {
                t.get("highlight")
                    .and_then(|h| h.as_object())
                    .map(|o| o.len())
                    .unwrap_or(0)
            })
            .collect(),
    };

    insta::assert_yaml_snapshot!("theme_gruvbox_structure", &summary);
}

// ---------------------------------------------------------------------------
// 6. UpdateManifest serialization
// ---------------------------------------------------------------------------

#[test]
fn test_update_manifest_serialization() {
    let mut platforms = std::collections::HashMap::new();
    platforms.insert(
        "macos-aarch64".to_string(),
        PlatformAsset {
            url: "https://releases.example.com/app-2.0.0.tar.gz".to_string(),
            signature: "deadbeef".to_string(),
            size: 42_000_000,
        },
    );
    let manifest = UpdateManifest {
        version: "2.0.0".to_string(),
        release_notes: "Major release with new features.".to_string(),
        platforms,
    };

    insta::assert_yaml_snapshot!("update_manifest", &manifest);
}

// ---------------------------------------------------------------------------
// 7. UpdateStatus serialization
// ---------------------------------------------------------------------------

#[test]
fn test_update_status_serialization() {
    #[derive(serde::Serialize)]
    struct StatusVariants {
        idle: UpdateStatus,
        checking: UpdateStatus,
        up_to_date: UpdateStatus,
        available: UpdateStatus,
        downloading: UpdateStatus,
        downloaded: UpdateStatus,
        ready_to_install: UpdateStatus,
        error: UpdateStatus,
    }

    let variants = StatusVariants {
        idle: UpdateStatus::Idle,
        checking: UpdateStatus::Checking,
        up_to_date: UpdateStatus::UpToDate,
        available: UpdateStatus::Available {
            version: "2.0.0".to_string(),
            notes: "Bug fixes".to_string(),
        },
        downloading: UpdateStatus::Downloading { progress: 42 },
        downloaded: UpdateStatus::Downloaded {
            version: "2.0.0".to_string(),
            path: "/tmp/update.tar.gz".to_string(),
        },
        ready_to_install: UpdateStatus::ReadyToInstall,
        error: UpdateStatus::Error("network timeout".to_string()),
    };

    insta::assert_yaml_snapshot!("update_status_variants", &variants);
}

// ---------------------------------------------------------------------------
// 8. UpdateSnapshot serialization
// ---------------------------------------------------------------------------

#[test]
fn test_update_snapshot_serialization() {
    let snapshot = UpdateSnapshot {
        status: UpdateStatus::Available {
            version: "2.1.0".to_string(),
            notes: "New features".to_string(),
        },
        current_version: "2.0.0".to_string(),
        last_check: Some("2026-06-04T12:00:00Z".to_string()),
        update_channel: "stable".to_string(),
        check_retry_count: 0,
        download_retry_count: 0,
    };

    insta::assert_yaml_snapshot!("update_snapshot", &snapshot);
}
