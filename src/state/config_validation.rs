//! Non-fatal configuration validation tier.
//!
//! Runs a set of *lints* against a loaded [`AppConfig`] and returns a list of
//! [`ConfigLint`] warnings. This tier is deliberately non-fatal: every check
//! produces warnings (a [`Vec<ConfigLint>`]), never a [`Result`], so a config
//! with cosmetic issues still loads. The caller is free to log the lints,
//! surface them in a diagnostics page, or ignore them entirely.
//!
//! Adapted from the reference launcher (`config/validation.rs`) but rebound to
//! the gpui-starter [`AppConfig`] / [`PersistedWindowBounds`] types. No
//! launcher-specific names (no `LauncherTheme`, no search providers) leak
//! through: this is generic boilerplate that checks the real gpui-starter
//! config fields.

use std::collections::HashSet;

use crate::app::{LOCALE_EN, LOCALE_ZH_CN};
use crate::state::config_store::{AppConfig, PersistedWindowBounds};

/// Logical log target for this module, matching the `gpui_starter::*` idiom.
const LOG_TARGET: &str = "gpui_starter::config_validation";

/// How serious a single [`ConfigLint`] is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LintSeverity {
    /// Informational note; no action required (e.g. an unusual-but-legal value).
    Info,
    /// Something looks off and probably warrants attention, but the app will
    /// still run correctly (e.g. a window position that may be off-screen).
    Warning,
    /// A value the app cannot interpret; it will fall back to a default
    /// (e.g. an unknown locale or update channel).
    Error,
}

impl LintSeverity {
    /// Lowercase label suitable for embedding in log lines or diagnostics rows.
    pub fn as_str(self) -> &'static str {
        match self {
            LintSeverity::Info => "info",
            LintSeverity::Warning => "warning",
            LintSeverity::Error => "error",
        }
    }
}

impl std::fmt::Display for LintSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single non-fatal configuration warning.
///
/// `field` uses dotted-path notation (e.g. `"window_bounds.width"`,
/// `"granted_permissions.<name>"`) so a UI can group lints by their target.
#[derive(Clone, Debug)]
pub struct ConfigLint {
    /// Dotted path to the offending field (e.g. `theme`, `window_bounds.x`).
    pub field: String,
    /// Human-readable description of the issue and the likely fallback.
    pub message: String,
    /// How serious this lint is.
    pub severity: LintSeverity,
}

impl ConfigLint {
    /// Convenience constructor mirroring the struct's natural ordering.
    fn new(field: impl Into<String>, severity: LintSeverity, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            severity,
            message: message.into(),
        }
    }
}

/// Update channels the updater recognises. Anything else is treated as unknown
/// and the updater falls back to `stable`. Kept as a `const` slice so the lint
/// stays self-contained (no import from the updater service required).
const KNOWN_UPDATE_CHANNELS: &[&str] = &["stable", "beta", "nightly"];

/// Locales the app ships translations for. Mirrors the two constants the rest
/// of the crate treats as authoritative (`LOCALE_EN`, `LOCALE_ZH_CN`).
const KNOWN_LOCALES: &[&str] = &[LOCALE_EN, LOCALE_ZH_CN];

/// Permission keys the app actually understands. The config stores these as
/// opaque `String`s, so anything outside this set is almost certainly stale
/// (left over from a renamed/removed feature) or foreign data. This list is the
/// single source of truth for "what may legitimately appear in
/// `granted_permissions`"; extend it when a new permission is introduced.
const KNOWN_PERMISSIONS: &[&str] = &["notifications"];

/// Below this the window is unusably small; above this it likely exceeds a
/// typical display and will be clamped by the platform.
const MIN_WINDOW_DIM: f32 = 100.0;
/// A generous upper bound for a single window dimension on a consumer display.
/// Values beyond this are "on-screen-ish" failures (e.g. a corrupted value of
/// `1e30`) rather than deliberate large windows.
const MAX_WINDOW_DIM: f32 = 8192.0;

/// Validate an [`AppConfig`], returning a list of non-fatal [`ConfigLint`]
/// warnings.
///
/// This never fails: an empty `Vec` means "no issues found". The config is
/// still usable regardless of what this returns. Checks performed:
///
/// * `window_bounds` sanity (non-negative, within a believable on-screen range)
/// * `update_channel` is one of the recognised channels
/// * `locale` is one of the shipped locales
/// * `theme` is non-empty (a missing/blank theme would fall back to the default)
/// * `granted_permissions` contains only known permission keys
///
/// Shortcut-conflict detection is intentionally omitted at this layer: the
/// gpui-starter config only stores a single global shortcut enable flag plus an
/// accelerator string (`"Alt+Space"`), so there is no user-editable set of
/// bindings to conflict with. If multiple user-bindable shortcuts are added
/// later, a `shortcut_conflicts` check can be appended here.
pub fn validate_config(cfg: &AppConfig) -> Vec<ConfigLint> {
    let mut lints = Vec::new();

    lints.extend(validate_window_bounds(cfg.window_bounds.as_ref()));
    lints.extend(validate_update_channel(&cfg.update_channel));
    lints.extend(validate_locale(&cfg.locale));
    lints.extend(validate_theme(&cfg.theme));
    lints.extend(validate_granted_permissions(&cfg.granted_permissions));

    if !lints.is_empty() {
        tracing::warn!(
            target: LOG_TARGET,
            count = lints.len(),
            "non-fatal config validation produced warnings; config will still load"
        );
    }

    lints
}

/// Window-bounds sanity: non-negative and within a believable on-screen range.
fn validate_window_bounds(bounds: Option<&PersistedWindowBounds>) -> Vec<ConfigLint> {
    let Some(b) = bounds else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let dims = [
        ("window_bounds.x", b.x),
        ("window_bounds.y", b.y),
        ("window_bounds.width", b.width),
        ("window_bounds.height", b.height),
    ];

    for (field, value) in dims {
        if value.is_nan() {
            out.push(ConfigLint::new(
                field,
                LintSeverity::Error,
                format!("{field} is NaN; falling back to default window placement"),
            ));
        } else if value.is_infinite() {
            out.push(ConfigLint::new(
                field,
                LintSeverity::Error,
                format!("{field} is infinite; falling back to default window placement"),
            ));
        } else if value < 0.0 {
            // A negative x/y is legal on some multi-monitor setups, but a
            // negative width/height is never meaningful.
            let severity = if field.ends_with("width") || field.ends_with("height") {
                LintSeverity::Error
            } else {
                LintSeverity::Warning
            };
            out.push(ConfigLint::new(
                field,
                severity,
                format!("{field} = {value} is negative; the window may not be visible"),
            ));
        }
    }

    // Width/height below the usable floor or above the generous ceiling.
    if b.width.is_finite() && (b.width < MIN_WINDOW_DIM || b.width > MAX_WINDOW_DIM) {
        out.push(ConfigLint::new(
            "window_bounds.width",
            LintSeverity::Warning,
            format!(
                "window_bounds.width = {} is outside the expected range [{}, {}]; \
                 it may be clamped by the platform",
                b.width, MIN_WINDOW_DIM, MAX_WINDOW_DIM
            ),
        ));
    }
    if b.height.is_finite() && (b.height < MIN_WINDOW_DIM || b.height > MAX_WINDOW_DIM) {
        out.push(ConfigLint::new(
            "window_bounds.height",
            LintSeverity::Warning,
            format!(
                "window_bounds.height = {} is outside the expected range [{}, {}]; \
                 it may be clamped by the platform",
                b.height, MIN_WINDOW_DIM, MAX_WINDOW_DIM
            ),
        ));
    }

    // "On-screen-ish" heuristic: if both the width and height are finite and
    // positive but absurdly large (e.g. a serialization bug produced 1e9), flag
    // it. This complements the per-dimension ceiling check above for the case
    // where a single dimension is just under the ceiling but the window is
    // still clearly bogus.
    if b.width.is_finite()
        && b.height.is_finite()
        && b.width > 0.0
        && b.height > 0.0
        && (b.width * b.height) > (MAX_WINDOW_DIM * MAX_WINDOW_DIM)
    {
        out.push(ConfigLint::new(
            "window_bounds",
            LintSeverity::Warning,
            format!(
                "window area {}x{} is implausibly large; likely a corrupted value",
                b.width, b.height
            ),
        ));
    }

    out
}

/// `update_channel` must be one of the recognised channels.
fn validate_update_channel(channel: &str) -> Vec<ConfigLint> {
    if channel.is_empty() {
        return vec![ConfigLint::new(
            "update_channel",
            LintSeverity::Warning,
            "update_channel is empty; the updater will default to \"stable\"",
        )];
    }
    if !KNOWN_UPDATE_CHANNELS.contains(&channel) {
        return vec![ConfigLint::new(
            "update_channel",
            LintSeverity::Warning,
            format!(
                "update_channel = {channel:?} is not recognised (expected one of {:?}); \
                 the updater will fall back to \"stable\"",
                KNOWN_UPDATE_CHANNELS
            ),
        )];
    }
    Vec::new()
}

/// `locale` must be one of the shipped locales.
fn validate_locale(locale: &str) -> Vec<ConfigLint> {
    if locale.is_empty() {
        return vec![ConfigLint::new(
            "locale",
            LintSeverity::Warning,
            format!("locale is empty; falling back to {LOCALE_EN:?}"),
        )];
    }
    if !KNOWN_LOCALES.contains(&locale) {
        return vec![ConfigLint::new(
            "locale",
            LintSeverity::Warning,
            format!(
                "locale = {locale:?} has no bundled translations (expected one of {:?}); \
                 falling back to {LOCALE_EN:?}",
                KNOWN_LOCALES
            ),
        )];
    }
    Vec::new()
}

/// `theme` must be non-empty. A blank theme would leave the UI unstyled until
/// the runtime substitutes its default, so this is surfaced as a warning.
///
/// Note: we do *not* try to verify the theme exists in the active theme
/// registry here, because that requires GPUI `App` context which this
/// pure-data tier deliberately avoids taking. The runtime already handles an
/// unknown theme name gracefully.
fn validate_theme(theme: &str) -> Vec<ConfigLint> {
    if theme.trim().is_empty() {
        return vec![ConfigLint::new(
            "theme",
            LintSeverity::Warning,
            "theme is empty or blank; the default theme will be used",
        )];
    }
    Vec::new()
}

/// `granted_permissions` should only contain keys the app understands.
fn validate_granted_permissions(granted: &HashSet<String>) -> Vec<ConfigLint> {
    let known: HashSet<&str> = KNOWN_PERMISSIONS.iter().copied().collect();
    let mut out = Vec::new();
    // Sort for deterministic lint ordering (HashSet iteration is randomized).
    let mut unknown: Vec<&String> = granted
        .iter()
        .filter(|p| !known.contains(p.as_str()))
        .collect();
    unknown.sort();
    for perm in unknown {
        out.push(ConfigLint::new(
            format!("granted_permissions.{perm}"),
            LintSeverity::Info,
            format!(
                "granted permission {perm:?} is not recognised by this build \
                 (known: {:?}); it will be ignored",
                KNOWN_PERMISSIONS
            ),
        ));
    }
    out
}

#[cfg(test)]
#[path = "config_validation.tests.rs"]
mod config_validation_tests;
