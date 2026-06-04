use gpui::{App, InteractiveElement as _, ParentElement as _, Styled as _, div};
use gpui_component::ActiveTheme as _;

use crate::{connectivity, notifications, routes::AppRoute, session, tasks};

pub fn render(route: &AppRoute, cx: &App) -> impl gpui::IntoElement {
    let render_started = std::time::Instant::now();
    let tasks_active = tasks::active_count(cx);
    let unread = notifications::inbox::unread_count(cx);

    // Borrow globals directly instead of cloning snapshot structs.
    let connectivity_state = cx
        .try_global::<connectivity::ConnectivitySnapshot>()
        .map(|s| &s.state);
    let degraded = cx
        .try_global::<notifications::NativeNotificationState>()
        .map(|s| s.snapshot.degraded_reason.as_deref())
        .flatten()
        .unwrap_or("No");
    let active_backend = cx
        .try_global::<notifications::NativeNotificationState>()
        .map(|s| s.snapshot.active_backend)
        .unwrap_or(notifications::NotificationBackendKind::UiOnly);
    let session_state = cx
        .try_global::<session::SessionSnapshot>()
        .map(|s| &s.state);
    let latest_error = crate::error_surface::latest_message(cx)
        .unwrap_or_else(|| "None".to_string());

    let session_label = match session_state {
        Some(session::SessionState::SignedOut) => "SignedOut".to_string(),
        Some(session::SessionState::SigningIn) => "SigningIn".to_string(),
        Some(session::SessionState::SignedIn { account_label }) => {
            format!("SignedIn({account_label})")
        }
        Some(session::SessionState::Error(error)) => format!("Error({error})"),
        None => "Unknown".to_string(),
    };

    tracing::debug!(
        target: "gpui_starter::status_bar::render",
        route = %route.title(),
        tasks_active,
        unread,
        connectivity = ?connectivity_state,
        elapsed_us = render_started.elapsed().as_micros() as u64,
        "status bar render prepared"
    );

    div()
        .id("status-bar")
        .w_full()
        .px_3()
        .py_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.35))
        .text_xs()
        .child(div().flex().gap_4().items_center().children([
            div().child(format!("Route: {}", route.title())),
            div().child(format!("Tasks: {tasks_active}")),
            div().child(format!("Unread: {unread}")),
            div().child(format!(
                "Connectivity: {:?}",
                connectivity_state.unwrap_or(&connectivity::ConnectivityState::Unknown)
            )),
            div().child(format!("Session: {session_label}")),
            div().child(format!("Notifications: {active_backend}")),
            div().child(format!("Degraded: {degraded}")),
            div().child(format!("LastError: {latest_error}")),
        ]))
}
