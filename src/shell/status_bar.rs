use gpui::{App, InteractiveElement as _, ParentElement as _, Styled as _, div};
use gpui_component::ActiveTheme as _;

use crate::{connectivity, notifications, routes::AppRoute, services::updater, session, tasks};

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
    let latest_error =
        crate::error_surface::latest_message(cx).unwrap_or_else(|| "None".to_string());

    let updater_status = cx
        .try_global::<updater::UpdateSnapshot>()
        .map(|s| &s.status);
    let updater_label = match updater_status {
        Some(updater::UpdateStatus::Available { version, .. }) => {
            Some(format!("Update: {version} available"))
        }
        Some(updater::UpdateStatus::Downloading { progress }) => {
            Some(format!("Update: downloading {progress}%"))
        }
        Some(updater::UpdateStatus::Downloaded { version, .. }) => {
            Some(format!("Update: {version} ready"))
        }
        Some(updater::UpdateStatus::ReadyToInstall) => {
            Some("Update: restart to install".to_string())
        }
        Some(updater::UpdateStatus::Error(err)) => {
            Some(format!("Update: error ({})", truncate_error(err, 30)))
        }
        Some(updater::UpdateStatus::Checking) => Some("Update: checking...".to_string()),
        _ => None,
    };

    let session_label = match session_state {
        Some(session::SessionState::SignedOut) => "SignedOut".to_string(),
        Some(session::SessionState::SigningIn) => "SigningIn".to_string(),
        Some(session::SessionState::SignedIn { account_label }) => {
            format!("SignedIn({account_label})")
        }
        Some(session::SessionState::Error(error)) => format!("Error({error})"),
        None => "Unknown".to_string(),
    };

    // Dev-only frame-time readout.
    let frame_time_el = render_frame_time(cx);

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
        .child({
            let mut children: Vec<gpui::Div> = vec![
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
            ];
            if let Some(label) = updater_label {
                children.push(div().child(label));
            }
            div()
                .flex()
                .gap_4()
                .items_center()
                .children(frame_time_el)
                .children(children)
        })
}

fn truncate_error(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        match s.char_indices().nth(max_len) {
            Some((idx, _)) => &s[..idx],
            None => s,
        }
    }
}

// ---------------------------------------------------------------------------
// Dev-only frame-time readout
// ---------------------------------------------------------------------------

/// Renders a small frame-time label in the status bar. Only compiled into debug
/// builds. Further gated by the `show_frame_time` setting so devs can toggle it
/// at runtime from the Settings page.
#[cfg(debug_assertions)]
fn render_frame_time(cx: &App) -> Option<gpui::Div> {
    let config = crate::app_state::config(cx);
    if !config.show_frame_time {
        return None;
    }

    let us = crate::root::last_frame_time_us();
    let threshold = crate::root::slow_frame_threshold_us();

    // Format as milliseconds (e.g. "Frame: 2.13ms").
    let ms = us as f64 / 1000.0;
    let label = format!("Frame: {ms:.2}ms");

    // Colour-code: green < 50% threshold, yellow < threshold, red >= threshold.
    let color = if us < threshold / 2 {
        gpui::rgb(0x22c55e) // green
    } else if us < threshold {
        gpui::rgb(0xeab308) // yellow
    } else {
        gpui::rgb(0xef4444) // red
    };

    Some(div().text_color(color).child(label))
}

#[cfg(not(debug_assertions))]
fn render_frame_time(_cx: &App) -> Option<gpui::Div> {
    None
}
