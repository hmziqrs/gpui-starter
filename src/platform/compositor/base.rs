//! Common helpers shared across compositor backends.

#![cfg(target_os = "linux")]

use super::WindowInfo;

/// Describes the capabilities a compositor implementation advertises.
#[derive(Debug, Clone, Default)]
pub struct CompositorCapabilities {
    /// Whether the compositor supports blur effects via layer rules.
    pub blur_support: bool,
    /// Whether the compositor supports the layer-shell protocol.
    pub layer_shell: bool,
    /// Whether window switching is functional.
    pub window_switching: bool,
    /// Whether accurate workspace information is available.
    pub workspace_info: bool,
    /// Whether focus-state tracking is accurate.
    pub focus_tracking: bool,
}

impl CompositorCapabilities {
    /// Capabilities for a fully-featured compositor (Hyprland, Niri).
    pub fn full() -> Self {
        Self {
            blur_support: true,
            layer_shell: true,
            window_switching: true,
            workspace_info: true,
            focus_tracking: true,
        }
    }

    /// Capabilities for a compositor with limited features.
    pub fn limited() -> Self {
        Self {
            blur_support: false,
            layer_shell: true,
            window_switching: true,
            workspace_info: false,
            focus_tracking: false,
        }
    }

    /// Capabilities for the no-op fallback.
    pub fn none() -> Self {
        Self::default()
    }
}

/// Build a display title for a window, falling back to `class` when the
/// title is empty. Both Hyprland and Niri follow this pattern.
pub fn get_display_title(title: &str, class: &str) -> String {
    if title.is_empty() {
        class.to_string()
    } else {
        title.to_string()
    }
}

/// Check whether a window class/app-id belongs to the host application
/// itself.
///
/// `app_id` is generalized so callers pass their own application id
/// (e.g. `"gpui-starter"`) rather than relying on a hard-coded value.
pub fn is_app_window(class: &str, app_id: &str) -> bool {
    !app_id.is_empty() && class.to_lowercase() == app_id.to_lowercase()
}

/// Filter a list of windows to exclude the host application's own window.
///
/// `app_id` is matched case-insensitively against each window's class.
pub fn filter_app_windows(windows: Vec<WindowInfo>, app_id: &str) -> Vec<WindowInfo> {
    windows
        .into_iter()
        .filter(|w| !is_app_window(&w.class, app_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_display_title() {
        assert_eq!(get_display_title("Firefox", "firefox"), "Firefox");
        assert_eq!(get_display_title("", "firefox"), "firefox");
        assert_eq!(get_display_title("", ""), "");
    }

    #[test]
    fn test_is_app_window() {
        assert!(is_app_window("gpui-starter", "gpui-starter"));
        assert!(is_app_window("GPUI-Starter", "gpui-starter"));
        assert!(!is_app_window("firefox", "gpui-starter"));
        // An empty app_id never matches, even against an empty class.
        assert!(!is_app_window("", ""));
    }

    #[test]
    fn test_filter_app_windows() {
        let windows = vec![
            WindowInfo {
                address: "1".to_string(),
                title: "Firefox".to_string(),
                class: "firefox".to_string(),
                workspace: 1,
                focused: false,
            },
            WindowInfo {
                address: "2".to_string(),
                title: "Starter".to_string(),
                class: "gpui-starter".to_string(),
                workspace: 1,
                focused: true,
            },
        ];

        let filtered = filter_app_windows(windows, "gpui-starter");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].class, "firefox");
    }
}
