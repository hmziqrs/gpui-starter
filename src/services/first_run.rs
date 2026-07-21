use gpui::App;

pub fn is_pending(cx: &App) -> bool {
    !crate::app_state::with_config(cx, |c| c.first_run_completed)
}

pub fn complete(cx: &mut App) {
    crate::app_state::update_config(cx, |config| {
        config.first_run_completed = true;
    });
    tracing::info!(target: "gpui_starter::first_run", "first-run marked complete");
}

pub fn reset(cx: &mut App) {
    crate::app_state::update_config(cx, |config| {
        config.first_run_completed = false;
    });
    tracing::info!(target: "gpui_starter::first_run", "first-run reset");
}
