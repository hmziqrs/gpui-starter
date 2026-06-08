mod render;
mod state;

pub use state::AppRoot;

use gpui::*;

impl Focusable for AppRoot {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Flush any pending window bounds to disk immediately.
///
/// Reads the current platform window bounds and persists them, bypassing the
/// debounce timer. Safe to call even when no window is open. Called from the
/// `Quit` action handler so that the final window position is persisted even
/// when the debounce timer has not yet fired.
pub fn flush_window_bounds(cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    window_handle
        .update(cx, |_, window, cx| {
            let bounds = window.window_bounds().get_bounds();
            let persisted = crate::app_state::PersistedWindowBounds {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: bounds.size.width.into(),
                height: bounds.size.height.into(),
            };
            crate::app_state::update_config(cx, |config| {
                config.window_bounds = Some(persisted);
            });
        })
        .ok();
}

#[cfg(test)]
#[path = "../../root.test.rs"]
mod root_test;
