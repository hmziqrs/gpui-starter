use gpui::{
    App, AppContext as _, Bounds, Focusable as _, SharedString, WindowBounds, WindowKind,
    WindowOptions, px, size,
};
use gpui_component::{Root, TitleBar};

// ---------------------------------------------------------------------------
// Window creation
// ---------------------------------------------------------------------------

pub fn create_new_window(title: &str, cx: &mut App) {
    let mut window_size = size(px(1400.0), px(900.0));
    if let Some(display) = cx.primary_display() {
        let display_size = display.bounds().size;
        window_size.width = window_size.width.min(display_size.width * 0.85);
        window_size.height = window_size.height.min(display_size.height * 0.85);
    }
    let persisted_bounds = crate::app_state::config(cx).window_bounds;
    let window_bounds = if let Some(bounds) = persisted_bounds {
        Bounds {
            origin: gpui::point(px(bounds.x), px(bounds.y)),
            size: gpui::size(px(bounds.width), px(bounds.height)),
        }
    } else {
        Bounds::centered(None, window_size, cx)
    };
    let title: SharedString = title.into();

    cx.spawn(async move |cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(window_bounds)),
            titlebar: Some(TitleBar::title_bar_options()),
            window_min_size: Some(gpui::Size {
                width: px(480.),
                height: px(320.),
            }),
            kind: WindowKind::Normal,
            // Set the app id so the desktop can group/identify us: it becomes the
            // Wayland app_id and the X11 WM_CLASS (== "gpui-starter"), which the
            // shipped .desktop's StartupWMClass matches. Without this, no
            // WM_CLASS is set and launcher/notification grouping + icon break.
            app_id: Some("gpui-starter".to_string()),
            #[cfg(target_os = "linux")]
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            #[cfg(target_os = "linux")]
            window_decorations: Some(gpui::WindowDecorations::Client),
            ..Default::default()
        };

        let Some(window) = cx
            .open_window(options, |window, cx| {
                let root_view = cx.new(|cx| crate::root::AppRoot::new(title.clone(), window, cx));

                let focus_handle = root_view.focus_handle(cx);
                window.defer(cx, move |window, cx| {
                    focus_handle.focus(window, cx);
                });

                cx.new(|cx| Root::new(root_view, window, cx))
            })
            .ok()
        else {
            tracing::error!("failed to open window");
            return Ok::<_, anyhow::Error>(());
        };

        window.update(cx, |_, window, _| {
            window.activate_window();
            window.set_window_title(&title);
        })?;

        Ok::<_, anyhow::Error>(())
    })
    .detach();
}
