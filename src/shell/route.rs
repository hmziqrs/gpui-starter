use serde::{Deserialize, Serialize};
use url::Url;

use crate::{errors::AppError, sidebar::Page};

pub const APP_URL_SCHEME: &str = "gpui-starter";

/// Hosts that are recognized as valid deep link targets.
///
/// Derived entirely from [`Page::host`](crate::sidebar::Page::host) — the
/// `Page` enum is the single source of truth for the route set, so this list
/// cannot drift from `to_url` / `parse_deep_link`. `validate_deep_link_url`
/// in `foundation::validation` reuses this list so deep-link validation and
/// route parsing also stay in sync.
pub(crate) const VALID_HOSTS: &[&str] = &[
    Page::Home.host(),
    Page::Form.host(),
    Page::Settings.host(),
    Page::Notifications.host(),
    Page::Diagnostics.host(),
    Page::ErrorPlayground.host(),
    Page::QueryPlayground.host(),
    Page::QueryDevToolsV2.host(),
    Page::About.host(),
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
        return Err(AppError::invalid_deep_link(
            segment,
            "path traversal detected in URL segment",
        ));
    }

    for &forbidden in INVALID_SEGMENT_CHARS {
        if segment.contains(forbidden) {
            return Err(AppError::invalid_deep_link(
                segment,
                format!("path segment contains forbidden character `{:?}`", forbidden),
            ));
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
            // The host segment comes from `Page::host` — the enum is the
            // single source of truth for the route set.
            Self::Page(page) => format!("{}://{}", APP_URL_SCHEME, page.host()),
            Self::SettingsNotifications => {
                format!("{}://{}/notifications", APP_URL_SCHEME, Page::Settings.host())
            }
        }
    }

    pub fn parse_deep_link(input: &str) -> Result<Self, AppError> {
        // --- 1. Parse the raw URL -------------------------------------------
        let url = Url::parse(input).map_err(|err| {
            AppError::invalid_deep_link(input, err.to_string())
        })?;

        // --- 2. Scheme validation -------------------------------------------
        if url.scheme() != APP_URL_SCHEME {
            return Err(AppError::invalid_deep_link(
                input,
                format!(
                    "unsupported scheme `{}`, expected `{}`",
                    url.scheme(),
                    APP_URL_SCHEME
                ),
            ));
        }

        // --- 3. Host validation ---------------------------------------------
        let host = url.host_str().unwrap_or_default();
        if !VALID_HOSTS.contains(&host) {
            return Err(AppError::invalid_deep_link(
                input,
                format!("unexpected host `{}`", host),
            ));
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

        // --- 6. Route matching ----------------------------------------------
        // The host→Page mapping is delegated to `Page::from_host` so the
        // parser, `to_url`, and `VALID_HOSTS` all share one source of truth.
        // Only the sub-route `settings/notifications` needs a literal arm;
        // its host is compared via `Page::Settings.host()` to avoid drift.
        match (host, segments.as_slice()) {
            (host, []) => match Page::from_host(host) {
                Some(page) => Ok(Self::Page(page)),
                None => Err(AppError::invalid_deep_link(input, "unknown route")),
            },
            (h, ["notifications"]) if h == Page::Settings.host() => {
                Ok(Self::SettingsNotifications)
            }
            _ => Err(AppError::invalid_deep_link(input, "unknown route")),
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
