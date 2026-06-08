use gpui::App;
use gpui_component::ActiveTheme as _;

pub fn set_theme_mode(mode: gpui_component::ThemeMode, cx: &mut App) {
    set_theme_mode_with_record(mode, true, cx);
}

pub fn set_theme_mode_with_record(
    mode: gpui_component::ThemeMode,
    record: bool,
    cx: &mut App,
) {
    let before = cx.theme().mode;
    gpui_component::Theme::change(mode, None, cx);
    if record {
        crate::undo_stack::record_theme_mode_change(before, mode, cx);
    }
    cx.refresh_windows();
}
