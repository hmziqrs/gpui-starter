use gpui_component::IconName;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    Home,
    Form,
    HttpLab,
    HttpLabTesting,
    Settings,
    Notifications,
    Diagnostics,
    QueryDevTools,
    About,
}

impl Page {
    pub fn title(&self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Form => "Form",
            Page::HttpLab => "HTTP Lab",
            Page::HttpLabTesting => "HTTP Lab Testing",
            Page::Settings => "Settings",
            Page::Notifications => "Notifications",
            Page::Diagnostics => "Diagnostics",
            Page::QueryDevTools => "Query DevTools",
            Page::About => "About",
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Page::Home => IconName::Inbox,
            Page::Form => IconName::File,
            Page::HttpLab => IconName::Globe,
            Page::HttpLabTesting => IconName::Globe,
            Page::Settings => IconName::Settings2,
            Page::Notifications => IconName::Bell,
            Page::Diagnostics => IconName::Info,
            Page::QueryDevTools => IconName::Search,
            Page::About => IconName::Info,
        }
    }

    pub fn all() -> &'static [Page] {
        &[
            Page::Home,
            Page::Form,
            Page::HttpLab,
            Page::HttpLabTesting,
            Page::Settings,
            Page::Notifications,
            Page::Diagnostics,
            Page::QueryDevTools,
            Page::About,
        ]
    }
}
