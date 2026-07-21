//! Unit tests for [`super::validate_config`].
//!
//! Style note: each test mutates exactly one field of an otherwise-default
//! [`AppConfig`] and asserts the matching lint fires, mirroring the
//! `config_store.test.rs` convention of `..AppConfig::default()` overrides.

use std::collections::HashSet;

use super::*;
use crate::state::config_store::{AppConfig, PersistedWindowBounds};

/// Helper: does the lint list contain an entry for the given dotted `field`?
fn has_lint_for(lints: &[ConfigLint], field: &str) -> bool {
    lints.iter().any(|l| l.field == field)
}

#[test]
fn default_config_produces_no_lints() {
    let cfg = AppConfig::default();
    let lints = validate_config(&cfg);
    assert!(
        lints.is_empty(),
        "default config should be clean, got: {lints:?}"
    );
}

#[test]
fn empty_theme_fires_a_theme_lint() {
    let cfg = AppConfig {
        theme: String::new(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "theme"));
    assert!(lints.iter().any(|l| l.field == "theme"
        && l.severity == LintSeverity::Warning
        && l.message.contains("empty")));
}

#[test]
fn blank_theme_fires_a_theme_lint() {
    let cfg = AppConfig {
        theme: "   ".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "theme"));
}

#[test]
fn unknown_locale_fires_a_locale_lint() {
    let cfg = AppConfig {
        locale: "klingon".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "locale"));
    assert!(
        lints
            .iter()
            .any(|l| l.field == "locale" && l.message.contains("klingon"))
    );
}

#[test]
fn empty_locale_fires_a_locale_lint() {
    let cfg = AppConfig {
        locale: String::new(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "locale"));
}

#[test]
fn known_locale_en_is_clean() {
    let cfg = AppConfig {
        locale: "en".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(!has_lint_for(&lints, "locale"));
}

#[test]
fn known_locale_zh_cn_is_clean() {
    let cfg = AppConfig {
        locale: "zh-CN".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(!has_lint_for(&lints, "locale"));
}

#[test]
fn unknown_update_channel_fires_a_channel_lint() {
    let cfg = AppConfig {
        update_channel: "canary-experimental".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "update_channel"));
    assert!(
        lints
            .iter()
            .any(|l| l.field == "update_channel" && l.message.contains("canary-experimental"))
    );
}

#[test]
fn empty_update_channel_fires_a_channel_lint() {
    let cfg = AppConfig {
        update_channel: String::new(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "update_channel"));
}

#[test]
fn stable_update_channel_is_clean() {
    let cfg = AppConfig {
        update_channel: "stable".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(!has_lint_for(&lints, "update_channel"));
}

#[test]
fn beta_update_channel_is_clean() {
    let cfg = AppConfig {
        update_channel: "beta".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(!has_lint_for(&lints, "update_channel"));
}

#[test]
fn negative_width_fires_an_error_lint() {
    let cfg = AppConfig {
        window_bounds: Some(PersistedWindowBounds {
            x: 10.0,
            y: 10.0,
            width: -50.0,
            height: 400.0,
        }),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(
        lints
            .iter()
            .any(|l| l.field == "window_bounds.width" && l.severity == LintSeverity::Error)
    );
}

#[test]
fn negative_height_fires_an_error_lint() {
    let cfg = AppConfig {
        window_bounds: Some(PersistedWindowBounds {
            x: 10.0,
            y: 10.0,
            width: 400.0,
            height: -1.0,
        }),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(
        lints
            .iter()
            .any(|l| l.field == "window_bounds.height" && l.severity == LintSeverity::Error)
    );
}

#[test]
fn negative_origin_is_a_warning_not_error() {
    let cfg = AppConfig {
        window_bounds: Some(PersistedWindowBounds {
            x: -100.0,
            y: 200.0,
            width: 800.0,
            height: 600.0,
        }),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    // Negative x is legal on multi-monitor setups but worth a warning.
    assert!(
        lints
            .iter()
            .any(|l| l.field == "window_bounds.x" && l.severity == LintSeverity::Warning)
    );
    // y is positive so should not be flagged.
    assert!(!has_lint_for(&lints, "window_bounds.y"));
}

#[test]
fn tiny_window_fires_a_width_and_height_lint() {
    let cfg = AppConfig {
        window_bounds: Some(PersistedWindowBounds {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        }),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "window_bounds.width"));
    assert!(has_lint_for(&lints, "window_bounds.height"));
}

#[test]
fn huge_window_fires_a_lint() {
    let cfg = AppConfig {
        window_bounds: Some(PersistedWindowBounds {
            x: 0.0,
            y: 0.0,
            width: 50_000.0,
            height: 600.0,
        }),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "window_bounds.width"));
}

#[test]
fn nan_dimension_fires_an_error_lint() {
    let cfg = AppConfig {
        window_bounds: Some(PersistedWindowBounds {
            x: 0.0,
            y: 0.0,
            width: f32::NAN,
            height: 600.0,
        }),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(lints.iter().any(|l| l.field == "window_bounds.width"
        && l.severity == LintSeverity::Error
        && l.message.contains("NaN")));
}

#[test]
fn none_window_bounds_is_clean() {
    let cfg = AppConfig {
        window_bounds: None,
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(lints.iter().all(|l| !l.field.starts_with("window_bounds")));
}

#[test]
fn unknown_granted_permission_fires_an_info_lint() {
    let mut perms = HashSet::new();
    perms.insert("notifications".to_string()); // known
    perms.insert("mind-reading".to_string()); // unknown
    let cfg = AppConfig {
        granted_permissions: perms,
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(
        lints
            .iter()
            .any(|l| l.field == "granted_permissions.mind-reading"
                && l.severity == LintSeverity::Info)
    );
    // The known permission must NOT be flagged.
    assert!(!has_lint_for(&lints, "granted_permissions.notifications"));
}

#[test]
fn known_granted_permissions_are_clean() {
    let mut perms = HashSet::new();
    perms.insert("notifications".to_string());
    let cfg = AppConfig {
        granted_permissions: perms,
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(
        lints
            .iter()
            .all(|l| !l.field.starts_with("granted_permissions"))
    );
}

#[test]
fn multiple_bad_fields_stack_into_one_vec() {
    // Several independent problems; all should appear, and the function still
    // returns a Vec (never panics / never returns an error).
    let cfg = AppConfig {
        theme: String::new(),
        locale: "klingon".to_string(),
        update_channel: "weird".to_string(),
        ..AppConfig::default()
    };
    let lints = validate_config(&cfg);
    assert!(has_lint_for(&lints, "theme"));
    assert!(has_lint_for(&lints, "locale"));
    assert!(has_lint_for(&lints, "update_channel"));
    assert!(lints.len() >= 3);
}

#[test]
fn lint_severity_labels_are_stable() {
    assert_eq!(LintSeverity::Info.as_str(), "info");
    assert_eq!(LintSeverity::Warning.as_str(), "warning");
    assert_eq!(LintSeverity::Error.as_str(), "error");
    assert_eq!(format!("{}", LintSeverity::Error), "error");
}
