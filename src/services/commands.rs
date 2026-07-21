use gpui::{App, SharedString};
use gpui_component::{IconName, ThemeMode};
use serde::{Deserialize, Serialize};

use crate::{
    events::{self, AppEventKind},
    routes::AppRoute,
    sidebar::Page,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandId {
    OpenHome,
    OpenForm,
    OpenSettings,
    OpenNotifications,
    OpenDiagnostics,
    OpenAbout,
    ThemeLight,
    ThemeDark,
    StartDemoTask,
    CheckConnectivity,
    CopyDiagnostics,
    OpenLogsFolder,
    OpenConfigFolder,
    Undo,
    Redo,
    Restart,
}

#[derive(Clone, Debug)]
pub struct CommandAvailability {
    pub enabled: bool,
    pub disabled_reason: Option<SharedString>,
}

#[derive(Clone)]
pub struct CommandSpec {
    pub id: CommandId,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub icon: IconName,
}

pub fn availability(id: CommandId, cx: &App) -> CommandAvailability {
    let desktop = crate::desktop_actions::snapshot(cx);
    match id {
        CommandId::OpenHome
        | CommandId::OpenForm
        | CommandId::OpenSettings
        | CommandId::OpenNotifications
        | CommandId::OpenDiagnostics
        | CommandId::OpenAbout
        | CommandId::ThemeLight
        | CommandId::ThemeDark
        | CommandId::StartDemoTask => CommandAvailability {
            enabled: true,
            disabled_reason: None,
        },
        CommandId::CheckConnectivity => CommandAvailability {
            enabled: true,
            disabled_reason: None,
        },
        CommandId::CopyDiagnostics => CommandAvailability {
            enabled: desktop.clipboard_available,
            disabled_reason: (!desktop.clipboard_available)
                .then_some("Clipboard backend unavailable".into()),
        },
        CommandId::OpenLogsFolder | CommandId::OpenConfigFolder => CommandAvailability {
            enabled: desktop.opener_available,
            disabled_reason: (!desktop.opener_available)
                .then_some("System opener backend unavailable".into()),
        },
        CommandId::Undo => CommandAvailability {
            enabled: crate::undo_stack::can_undo(cx).is_some(),
            disabled_reason: crate::undo_stack::can_undo(cx)
                .is_none()
                .then_some("No undo available".into()),
        },
        CommandId::Redo => CommandAvailability {
            enabled: crate::undo_stack::can_redo(cx).is_some(),
            disabled_reason: crate::undo_stack::can_redo(cx)
                .is_none()
                .then_some("No redo available".into()),
        },
        CommandId::Restart => CommandAvailability {
            enabled: cfg!(unix),
            disabled_reason: (!cfg!(unix)).then_some("Restart requires exec-reload (unix)".into()),
        },
    }
}

pub fn registry() -> Vec<CommandSpec> {
    vec![
        command(
            CommandId::OpenHome,
            "Home",
            "Open the Home page",
            IconName::Inbox,
        ),
        command(
            CommandId::OpenForm,
            "Form",
            "Open the Form page",
            IconName::File,
        ),
        command(
            CommandId::OpenSettings,
            "Settings",
            "Open the Settings page",
            IconName::Settings2,
        ),
        command(
            CommandId::OpenNotifications,
            "Notifications",
            "Open the Notifications page",
            IconName::Bell,
        ),
        command(
            CommandId::OpenDiagnostics,
            "Diagnostics",
            "Open diagnostics page",
            IconName::Info,
        ),
        command(
            CommandId::OpenAbout,
            "About",
            "Open the About page",
            IconName::Info,
        ),
        command(
            CommandId::ThemeLight,
            "Light Mode",
            "Switch to light theme",
            IconName::Sun,
        ),
        command(
            CommandId::ThemeDark,
            "Dark Mode",
            "Switch to dark theme",
            IconName::Moon,
        ),
        command(
            CommandId::StartDemoTask,
            "Start Demo Task",
            "Start an example background task",
            IconName::Play,
        ),
        command(
            CommandId::CheckConnectivity,
            "Check Connectivity",
            "Run network connectivity probe",
            IconName::Globe,
        ),
        command(
            CommandId::CopyDiagnostics,
            "Copy Diagnostics",
            "Copy a diagnostics summary to clipboard",
            IconName::Info,
        ),
        command(
            CommandId::OpenLogsFolder,
            "Open Logs Folder",
            "Open logs folder in system file manager",
            IconName::Info,
        ),
        command(
            CommandId::OpenConfigFolder,
            "Open Config Folder",
            "Open config folder in system file manager",
            IconName::Info,
        ),
        command(
            CommandId::Undo,
            "Undo",
            "Undo last reversible command",
            IconName::Info,
        ),
        command(
            CommandId::Redo,
            "Redo",
            "Redo last reversed command",
            IconName::Info,
        ),
        command(
            CommandId::Restart,
            "Restart",
            "Restart the application",
            IconName::Info,
        ),
    ]
}

pub fn execute(id: CommandId, cx: &mut App) {
    match id {
        CommandId::OpenHome => navigate(Page::Home, cx),
        CommandId::OpenForm => navigate(Page::Form, cx),
        CommandId::OpenSettings => navigate(Page::Settings, cx),
        CommandId::OpenNotifications => navigate(Page::Notifications, cx),
        CommandId::OpenDiagnostics => navigate(Page::Diagnostics, cx),
        CommandId::OpenAbout => navigate(Page::About, cx),
        CommandId::ThemeLight => crate::app::set_theme_mode(ThemeMode::Light, cx),
        CommandId::ThemeDark => crate::app::set_theme_mode(ThemeMode::Dark, cx),
        CommandId::StartDemoTask => crate::tasks::start_demo_task(cx),
        CommandId::CheckConnectivity => crate::connectivity::check_now(cx),
        CommandId::CopyDiagnostics => {
            if let Err(error) = crate::desktop_actions::copy_diagnostics(cx) {
                tracing::warn!(target: "gpui_starter::commands", %error, "copy diagnostics failed");
                report_error(
                    format!("Copy diagnostics failed: {error}"),
                    crate::error_surface::ErrorCategory::System,
                    vec![
                        crate::error_surface::ErrorAction::Retry,
                        crate::error_surface::ErrorAction::Dismiss,
                    ],
                    cx,
                );
            }
        }
        CommandId::OpenLogsFolder => {
            if let Err(error) = crate::desktop_actions::open_logs_folder(cx) {
                tracing::warn!(target: "gpui_starter::commands", %error, "open logs folder failed");
                report_error(
                    format!("Open logs folder failed: {error}"),
                    crate::error_surface::ErrorCategory::Storage,
                    vec![
                        crate::error_surface::ErrorAction::OpenSettings,
                        crate::error_surface::ErrorAction::Dismiss,
                    ],
                    cx,
                );
            }
        }
        CommandId::OpenConfigFolder => {
            if let Err(error) = crate::desktop_actions::open_config_folder(cx) {
                tracing::warn!(target: "gpui_starter::commands", %error, "open config folder failed");
                report_error(
                    format!("Open config folder failed: {error}"),
                    crate::error_surface::ErrorCategory::Config,
                    vec![
                        crate::error_surface::ErrorAction::OpenSettings,
                        crate::error_surface::ErrorAction::Dismiss,
                    ],
                    cx,
                );
            }
        }
        CommandId::Undo => {
            let _ = crate::undo_stack::undo(cx);
        }
        CommandId::Redo => {
            let _ = crate::undo_stack::redo(cx);
        }
        CommandId::Restart => {
            #[cfg(unix)]
            cx.dispatch_action(&crate::app::Restart);
            #[cfg(not(unix))]
            {
                let _ = cx;
                tracing::warn!(
                    target: "gpui_starter::commands",
                    "restart command unavailable on this platform"
                );
            }
        }
    }
}

fn navigate(page: Page, cx: &mut App) {
    events::emit(AppEventKind::Navigate(AppRoute::page(page)), cx);
}

fn report_error(
    message: impl Into<String>,
    category: crate::error_surface::ErrorCategory,
    actions: Vec<crate::error_surface::ErrorAction>,
    cx: &mut App,
) {
    crate::error_surface::report(
        message,
        crate::errors::AppErrorSeverity::Error,
        category,
        actions,
        cx,
    );
}

fn command(id: CommandId, title: &str, subtitle: &str, icon: IconName) -> CommandSpec {
    CommandSpec {
        id,
        title: title.into(),
        subtitle: subtitle.into(),
        icon,
    }
}
