use gpui::{prelude::*, *};

use crate::sidebar::Page;
use crate::title_bar::AppTitleBar;
use crate::views::{
    AboutPage, DiagnosticsPage, ErrorPlaygroundPage, FormPage, HomePage, HttpLabPage,
    HttpLabTestingPage, NotificationsPage, QueryDevToolsPage, QueryDevToolsV2Page,
    QueryPlaygroundPage, RenderErrorPage, SettingsPage,
};
use crate::{
    events::{self, AppEventKind},
    routes::AppRoute,
};

use super::super::actions::NavigateToPage;

pub struct AppRoot {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) title_bar: Entity<AppTitleBar>,
    pub(crate) active_route: AppRoute,
    pub(crate) collapsed: bool,
    pub(crate) home_page: Entity<HomePage>,
    pub(crate) form_page: Entity<FormPage>,
    pub(crate) http_lab_page: Entity<HttpLabPage>,
    pub(crate) http_lab_testing_page: Entity<HttpLabTestingPage>,
    pub(crate) settings_page: Entity<SettingsPage>,
    pub(crate) notifications_page: Entity<NotificationsPage>,
    pub(crate) diagnostics_page: Entity<DiagnosticsPage>,
    pub(crate) error_playground_page: Entity<ErrorPlaygroundPage>,
    pub(crate) query_devtools_page: Entity<QueryDevToolsPage>,
    pub(crate) query_playground_page: Entity<QueryPlaygroundPage>,
    pub(crate) query_devtools_v2_page: Entity<QueryDevToolsV2Page>,
    pub(crate) about_page: Entity<AboutPage>,

    /// Error boundary: when `true`, the error fallback view is shown instead
    /// of the active page. Set when a render panic is detected; cleared when
    /// the user clicks "Reload Page".
    pub(crate) render_error: bool,
    /// Cached error-boundary entity so we don't re-create it every frame.
    pub(crate) error_page: Option<Entity<RenderErrorPage>>,

    /// Debounced window-bounds persistence.
    ///
    /// On macOS, `observe_window_bounds` fires ~60 times/sec during a drag. Each
    /// `update_config` call clones the full `AppConfig`, normalises it, pretty-prints
    /// JSON, and performs an atomic file write. Debouncing collapses that burst into
    /// a single write once the user stops moving/resizing the window.
    pub(crate) pending_bounds: Option<crate::app_state::PersistedWindowBounds>,
    pub(crate) _pending_bounds_flush: Option<Task<()>>,
}

impl AppRoot {
    pub fn new(
        title: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title_bar = cx.new(|cx| AppTitleBar::new(title, window, cx));
        let home_page = cx.new(|_| HomePage::new());
        let form_page = cx.new(|cx| FormPage::new(window, cx));
        let http_lab_page = cx.new(|cx| HttpLabPage::new(window, cx));
        let http_lab_testing_page = cx.new(|_| HttpLabTestingPage::new());
        let settings_page = cx.new(|cx| SettingsPage::new(window, cx));
        let notifications_page = cx.new(|cx| NotificationsPage::new(window, cx));
        let diagnostics_page = cx.new(|cx| DiagnosticsPage::new(window, cx));
        let error_playground_page = cx.new(|_| ErrorPlaygroundPage::new());
        let query_devtools_page = cx.new(|cx| QueryDevToolsPage::new(window, cx));
        let query_playground_page = cx.new(|cx| QueryPlaygroundPage::new(window, cx));
        let query_devtools_v2_page = cx.new(|cx| QueryDevToolsV2Page::new(window, cx));
        let about_page = cx.new(|_| AboutPage::new());

        // Eagerly register QueryClient global so DevTools page can observe it.
        if !cx.has_global::<gpui_query_legacy::client::QueryClient>() {
            cx.set_global(gpui_query_legacy::client::QueryClient::new(
                gpui_query_legacy::CachePolicy::default(),
                gpui_query_legacy::RequestPolicy::default(),
            ));
        }

        // Register v2 QueryClient global.
        if !cx.has_global::<gpui_query::client::QueryClient>() {
            cx.set_global(gpui_query::client::QueryClient::new());
        }

        // React to app-wide events coming from launcher/deep links.
        cx.observe_global::<events::AppEventQueue>(|this, cx| {
            for event in events::drain(cx) {
                match event.kind {
                    AppEventKind::Navigate(route) => this.set_route(route, cx),
                    AppEventKind::DeepLinkReceived(link) => match AppRoute::parse_deep_link(&link) {
                        Ok(route) => this.set_route(route, cx),
                        Err(err) => events::emit_error(err, cx),
                    },
                    AppEventKind::AppError { message, severity } => {
                        tracing::warn!(target: "gpui_starter::root", error = %message, ?severity, "app error event received");
                        crate::error_surface::report(
                            message,
                            severity,
                            crate::error_surface::ErrorCategory::System,
                            vec![crate::error_surface::ErrorAction::Dismiss],
                            cx,
                        );
                        cx.notify();
                    }
                    AppEventKind::DiagnosticsChanged => {}
                    AppEventKind::Test { message } => {
                        tracing::info!(target: "gpui_starter::root", message, "test event received");
                    }
                }
            }
        })
        .detach();
        cx.observe_global::<crate::tasks::TaskRegistry>(|_, cx| {
            tracing::debug!(
                target: "gpui_starter::root::render",
                active_tasks = crate::tasks::active_count(cx),
                "TaskRegistry changed; notifying root"
            );
            cx.notify();
        })
        .detach();
        cx.observe_global::<crate::notifications::NativeNotificationState>(|_, cx| {
            cx.notify();
        })
        .detach();
        cx.observe_global::<crate::notifications::inbox::NotificationInboxState>(|_, cx| {
            cx.notify();
        })
        .detach();
        cx.observe_global::<crate::connectivity::ConnectivitySnapshot>(|_, cx| {
            cx.notify();
        })
        .detach();
        cx.observe_global::<crate::session::SessionSnapshot>(|_, cx| {
            cx.notify();
        })
        .detach();
        // Debounce window-bounds persistence to avoid hammering disk at ~60 Hz
        // during a drag/resize. We stash the latest bounds and flush them once
        // the user stops moving the window for 500 ms.
        cx.observe_window_bounds(window, |this, window, cx| {
            let bounds = window.window_bounds().get_bounds();
            this.pending_bounds = Some(crate::app_state::PersistedWindowBounds {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: bounds.size.width.into(),
                height: bounds.size.height.into(),
            });

            // Cancel any previously scheduled flush and schedule a new one.
            this._pending_bounds_flush = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;

                let _ = this.update(cx, |this, cx| {
                    if let Some(bounds) = this.pending_bounds.take() {
                        crate::app_state::update_config(cx, |config| {
                            config.window_bounds = Some(bounds);
                        });
                    }
                });
            }));
        })
        .detach();

        let config = crate::app_state::config(cx);

        // Keyboard shortcuts: Cmd+1..9 to jump to sidebar pages.
        let pages = Page::all();
        cx.bind_keys(
            pages
                .iter()
                .enumerate()
                .filter_map(|(i, _)| {
                    if i < 9 {
                        Some(KeyBinding::new(
                            &format!("cmd-{}", i + 1),
                            NavigateToPage(i),
                            None,
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
        );

        Self {
            focus_handle: cx.focus_handle(),
            title_bar,
            active_route: config.active_route,
            collapsed: config.sidebar_collapsed,
            home_page,
            form_page,
            http_lab_page,
            http_lab_testing_page,
            settings_page,
            notifications_page,
            diagnostics_page,
            error_playground_page,
            query_devtools_page,
            query_playground_page,
            query_devtools_v2_page,
            about_page,
            render_error: false,
            error_page: None,
            pending_bounds: None,
            _pending_bounds_flush: None,
        }
    }

    /// Flush any debounced window bounds to disk immediately.
    ///
    /// Called during shutdown to ensure the final window position is persisted
    /// even if the debounce timer has not yet fired.
    pub fn flush_pending_bounds(&mut self, cx: &mut Context<Self>) {
        if let Some(bounds) = self.pending_bounds.take() {
            crate::app_state::update_config(cx, |config| {
                config.window_bounds = Some(bounds);
            });
        }
        self._pending_bounds_flush = None;
    }

    /// Return the page view to render, considering the error boundary.
    ///
    /// If a render panic was detected since the last frame (via
    /// [`crate::app::lifecycle::take_render_panic`]), we swap in the
    /// [`RenderErrorPage`] fallback instead of the crashing page. The user can
    /// dismiss the error boundary with the "Reload Page" button (or by
    /// navigating to a different route), which clears the flag and retries.
    pub(crate) fn active_page_view(&mut self, cx: &mut Context<Self>) -> AnyView {
        // Check if a panic occurred since the last render.
        if crate::lifecycle::take_render_panic() {
            tracing::warn!(
                target: "gpui_starter::root",
                "render panic detected – activating error boundary"
            );
            self.render_error = true;
            let summary = crate::lifecycle::last_panic_summary()
                .unwrap_or_else(|| "An unknown error occurred.".to_string());
            self.error_page = Some(cx.new(|_| RenderErrorPage::new(summary)));
        }

        // If the error boundary is active, show the fallback view.
        if self.render_error {
            if let Some(ref error_page) = self.error_page {
                return error_page.clone().into();
            }
        }

        self.unchecked_active_page_view()
    }

    /// Return the active page view without checking for render panics.
    pub(crate) fn unchecked_active_page_view(&self) -> AnyView {
        match self.active_route.page_for_render() {
            Page::Home => self.home_page.clone().into(),
            Page::Form => self.form_page.clone().into(),
            Page::HttpLab => self.http_lab_page.clone().into(),
            Page::HttpLabTesting => self.http_lab_testing_page.clone().into(),
            Page::Settings => self.settings_page.clone().into(),
            Page::Notifications => self.notifications_page.clone().into(),
            Page::Diagnostics => self.diagnostics_page.clone().into(),
            Page::ErrorPlayground => self.error_playground_page.clone().into(),
            Page::QueryDevTools => self.query_devtools_page.clone().into(),
            Page::QueryPlayground => self.query_playground_page.clone().into(),
            Page::QueryDevToolsV2 => self.query_devtools_v2_page.clone().into(),
            Page::About => self.about_page.clone().into(),
        }
    }

    pub(crate) fn set_route(&mut self, route: AppRoute, cx: &mut Context<Self>) {
        if self.active_route == route {
            return;
        }
        let route_url = route.to_url();
        tracing::info!(target: "gpui_starter::root", route = ?route, route_url, "navigating");
        self.active_route = route.clone();
        self.render_error = false;
        self.error_page = None;
        crate::app_state::update_config(cx, |config| {
            config.active_route = route;
        });
        cx.notify();
    }
}
