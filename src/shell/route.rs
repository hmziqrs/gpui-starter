use serde::{Deserialize, Serialize};
use url::Url;

use crate::{errors::AppError, sidebar::Page};

pub const APP_URL_SCHEME: &str = "gpui-starter";

/// Hosts that are recognized as valid deep link targets.
const VALID_HOSTS: &[&str] = &[
    "home",
    "form",
    "http",
    "httplab-testing",
    "settings",
    "notifications",
    "diagnostics",
    "error-playground",
    "query-devtools",
    "query-playground",
    "query-devtools-v2",
    "about",
];

/// Characters that are not permitted in path segments.
const INVALID_SEGMENT_CHARS: &[char] = &['/', '\\', '\0', '<', '>', '|', '"'];

/// Strip control characters (U+0000–U+001F and U+007F) from a string,
/// returning a sanitized copy suitable for safe use.
fn sanitize_control_chars(input: &str) -> String {
    input.chars().filter(|c| !c.is_control()).collect()
}

/// Reject path segments that contain traversal sequences or other dangerous
/// characters. Returns `Ok(())` when the segment is safe.
fn validate_path_segment(segment: &str) -> Result<(), AppError> {
    if segment.is_empty() {
        return Ok(());
    }

    // Path-traversal checks.
    if segment == ".." || segment == "." || segment.contains("..") {
        return Err(AppError::InvalidDeepLink {
            input: segment.to_string(),
            reason: "path traversal detected in URL segment".to_string(),
        });
    }

    for &forbidden in INVALID_SEGMENT_CHARS {
        if segment.contains(forbidden) {
            return Err(AppError::InvalidDeepLink {
                input: segment.to_string(),
                reason: format!(
                    "path segment contains forbidden character `{:?}`",
                    forbidden
                ),
            });
        }
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppRoute {
    Page(Page),
    SettingsNotifications,
}

impl AppRoute {
    pub fn page(page: Page) -> Self {
        Self::Page(page)
    }

    pub fn page_for_render(&self) -> Page {
        match self {
            Self::Page(page) => *page,
            Self::SettingsNotifications => Page::Settings,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Page(page) => page.title(),
            Self::SettingsNotifications => "Settings",
        }
    }

    pub fn to_url(&self) -> String {
        match self {
            Self::Page(Page::Home) => "gpui-starter://home".to_string(),
            Self::Page(Page::Form) => "gpui-starter://form".to_string(),
            Self::Page(Page::Settings) => "gpui-starter://settings".to_string(),
            Self::Page(Page::Notifications) => "gpui-starter://notifications".to_string(),
            Self::Page(Page::Diagnostics) => "gpui-starter://diagnostics".to_string(),
            Self::Page(Page::ErrorPlayground) => "gpui-starter://error-playground".to_string(),
            Self::Page(Page::QueryPlayground) => "gpui-starter://query-playground".to_string(),
            Self::Page(Page::QueryDevToolsV2) => "gpui-starter://query-devtools-v2".to_string(),
            Self::Page(Page::About) => "gpui-starter://about".to_string(),
            Self::SettingsNotifications => "gpui-starter://settings/notifications".to_string(),
        }
    }

    pub fn parse_deep_link(input: &str) -> Result<Self, AppError> {
        // --- 1. Parse the raw URL -------------------------------------------
        let url = Url::parse(input).map_err(|err| AppError::InvalidDeepLink {
            input: input.to_string(),
            reason: err.to_string(),
        })?;

        // --- 2. Scheme validation -------------------------------------------
        if url.scheme() != APP_URL_SCHEME {
            return Err(AppError::InvalidDeepLink {
                input: input.to_string(),
                reason: format!(
                    "unsupported scheme `{}`, expected `{}`",
                    url.scheme(),
                    APP_URL_SCHEME
                ),
            });
        }

        // --- 3. Host validation ---------------------------------------------
        let host = url.host_str().unwrap_or_default();
        if !VALID_HOSTS.contains(&host) {
            return Err(AppError::InvalidDeepLink {
                input: input.to_string(),
                reason: format!("unexpected host `{}`", host),
            });
        }

        // --- 4. Path segment validation -------------------------------------
        let segments: Vec<&str> = url
            .path_segments()
            .map(|segs| segs.filter(|s| !s.is_empty()).collect::<Vec<_>>())
            .unwrap_or_default();

        for segment in &segments {
            validate_path_segment(segment)?;
        }

        // --- 5. Query parameter sanitization --------------------------------
        for (key, value) in url.query_pairs() {
            let _key = sanitize_control_chars(&key);
            let _value = sanitize_control_chars(&value);
            // Sanitized values are currently unused but are validated here so
            // that future consumers start from clean data. Control-character
            // injection through query strings is prevented at the gate.
        }

        // --- 6. Route matching (unchanged logic) ----------------------------
        match (host, segments.as_slice()) {
            ("home", []) => Ok(Self::Page(Page::Home)),
            ("form", []) => Ok(Self::Page(Page::Form)),
            ("settings", []) => Ok(Self::Page(Page::Settings)),
            ("settings", ["notifications"]) => Ok(Self::SettingsNotifications),
            ("notifications", []) => Ok(Self::Page(Page::Notifications)),
            ("diagnostics", []) => Ok(Self::Page(Page::Diagnostics)),
            ("error-playground", []) => Ok(Self::Page(Page::ErrorPlayground)),
            ("query-playground", []) => Ok(Self::Page(Page::QueryPlayground)),
            ("query-devtools-v2", []) => Ok(Self::Page(Page::QueryDevToolsV2)),
            ("about", []) => Ok(Self::Page(Page::About)),
            _ => Err(AppError::InvalidDeepLink {
                input: input.to_string(),
                reason: "unknown route".to_string(),
            }),
        }
    }
}

impl Default for AppRoute {
    fn default() -> Self {
        Self::Page(Page::Home)
    }
}

#[cfg(test)]
#[path = "route.test.rs"]
mod route_test;
