use gpui_component::IconName;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    Home,
    Form,
    Settings,
    Notifications,
    Diagnostics,
    QueryPlayground,
    QueryDevToolsV2,
    ErrorPlayground,
    About,
}

impl Page {
    pub fn title(&self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Form => "Form",
            Page::Settings => "Settings",
            Page::Notifications => "Notifications",
            Page::Diagnostics => "Diagnostics",
            Page::QueryPlayground => "Query Playground",
            Page::QueryDevToolsV2 => "Query DevTools V2",
            Page::ErrorPlayground => "Error Playground",
            Page::About => "About",
        }
    }

    /// The deep-link host segment for this page (e.g. `"home"`, `"error-playground"`).
    ///
    /// This is the **single source of truth** for the `Page` ↔ host mapping.
    /// `VALID_HOSTS`, `AppRoute::to_url`, and `AppRoute::parse_deep_link` all
    /// consult this method (and its inverse [`from_host`](Self::from_host)),
    /// so the route set cannot drift across them. Adding or renaming a variant
    /// only requires updating this match — the match is non-exhaustive until
    /// you do, so the compiler enforces it.
    pub const fn host(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Form => "form",
            Page::Settings => "settings",
            Page::Notifications => "notifications",
            Page::Diagnostics => "diagnostics",
            Page::ErrorPlayground => "error-playground",
            Page::QueryPlayground => "query-playground",
            Page::QueryDevToolsV2 => "query-devtools-v2",
            Page::About => "about",
        }
    }

    /// Inverse of [`host`](Self::host): look up the page for a host segment.
    /// Returns `None` for an unrecognized host.
    pub fn from_host(host: &str) -> Option<Self> {
        Self::all().iter().copied().find(|p| p.host() == host)
    }

    pub fn icon(&self) -> IconName {
        match self {
            Page::Home => IconName::Inbox,
            Page::Form => IconName::File,
            Page::Settings => IconName::Settings2,
            Page::Notifications => IconName::Bell,
            Page::Diagnostics => IconName::Info,
            Page::QueryPlayground => IconName::Play,
            Page::QueryDevToolsV2 => IconName::LayoutDashboard,
            Page::ErrorPlayground => IconName::TriangleAlert,
            Page::About => IconName::Info,
        }
    }

    pub fn all() -> &'static [Page] {
        &[
            Page::Home,
            Page::Form,
            Page::Settings,
            Page::Notifications,
            Page::Diagnostics,
            Page::QueryPlayground,
            Page::QueryDevToolsV2,
            Page::ErrorPlayground,
            Page::About,
        ]
    }
}
