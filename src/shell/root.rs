use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, Sizable as _,
    resizable::{h_resizable, resizable_panel},
    sidebar::{Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    v_flex,
};

use crate::sidebar::Page;
use crate::title_bar::AppTitleBar;
use crate::views::{
    AboutPage, DiagnosticsPage, FormPage, HomePage, HttpLabPage, HttpLabTestingPage,
    NotificationsPage, QueryDevToolsPage, QueryDevToolsV2Page, QueryPlaygroundPage, SettingsPage,
};
use crate::{
    app::ToggleSearch,
    events::{self, AppEventKind},
    routes::AppRoute,
};

// ---------------------------------------------------------------------------
// RTL locale detection
// ---------------------------------------------------------------------------

/// Returns `true` when the given locale string corresponds to an RTL script.
///
/// Recognized RTL locales: Arabic (ar*), Hebrew (he*), Farsi (fa*), Urdu (ur*).
fn is_rtl_locale(locale: &str) -> bool {
    locale
        .split('-')
        .next()
        .map(|primary| matches!(primary, "ar" | "he" | "fa" | "ur"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Keyboard navigation action
// ---------------------------------------------------------------------------

/// Navigate directly to a sidebar page by index (0-based).
#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct NavigateToPage(pub usize);

/// Re-navigate to the current page (triggers a route refresh).
#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct RefreshPage;

pub struct AppRoot {
    focus_handle: FocusHandle,
    title_bar: Entity<AppTitleBar>,
    active_route: AppRoute,
    collapsed: bool,
    home_page: Entity<HomePage>,
    form_page: Entity<FormPage>,
    http_lab_page: Entity<HttpLabPage>,
    http_lab_testing_page: Entity<HttpLabTestingPage>,
    settings_page: Entity<SettingsPage>,
    notifications_page: Entity<NotificationsPage>,
    diagnostics_page: Entity<DiagnosticsPage>,
    query_devtools_page: Entity<QueryDevToolsPage>,
    query_playground_page: Entity<QueryPlaygroundPage>,
    query_devtools_v2_page: Entity<QueryDevToolsV2Page>,
    about_page: Entity<AboutPage>,
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
        let query_devtools_page = cx.new(|cx| QueryDevToolsPage::new(window, cx));
        let query_playground_page = cx.new(|cx| QueryPlaygroundPage::new(window, cx));
        let query_devtools_v2_page = cx.new(|cx| QueryDevToolsV2Page::new(window, cx));
        let about_page = cx.new(|_| AboutPage::new());

        // Eagerly register QueryClient global so DevTools page can observe it.
        if !cx.has_global::<gpui_query::client::QueryClient>() {
            cx.set_global(gpui_query::client::QueryClient::new(
                gpui_query::CachePolicy::default(),
                gpui_query::RequestPolicy::default(),
            ));
        }

        // Register v2 QueryClient global.
        if !cx.has_global::<gpui_query_v2::client::QueryClient>() {
            cx.set_global(gpui_query_v2::client::QueryClient::new());
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
        cx.observe_window_bounds(window, |_, window, cx| {
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
            query_devtools_page,
            query_playground_page,
            query_devtools_v2_page,
            about_page,
        }
    }

    fn active_page_view(&self) -> AnyView {
        match self.active_route.page_for_render() {
            Page::Home => self.home_page.clone().into(),
            Page::Form => self.form_page.clone().into(),
            Page::HttpLab => self.http_lab_page.clone().into(),
            Page::HttpLabTesting => self.http_lab_testing_page.clone().into(),
            Page::Settings => self.settings_page.clone().into(),
            Page::Notifications => self.notifications_page.clone().into(),
            Page::Diagnostics => self.diagnostics_page.clone().into(),
            Page::QueryDevTools => self.query_devtools_page.clone().into(),
            Page::QueryPlayground => self.query_playground_page.clone().into(),
            Page::QueryDevToolsV2 => self.query_devtools_v2_page.clone().into(),
            Page::About => self.about_page.clone().into(),
        }
    }

    fn set_route(&mut self, route: AppRoute, cx: &mut Context<Self>) {
        if self.active_route == route {
            return;
        }
        let route_url = route.to_url();
        tracing::info!(target: "gpui_starter::root", route = ?route, route_url, "navigating");
        self.active_route = route.clone();
        crate::app_state::update_config(cx, |config| {
            config.active_route = route;
        });
        cx.notify();
    }
}

impl Focusable for AppRoot {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = std::time::Instant::now();
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);
        let page_title = self.active_route.title();
        let active_page = self.active_route.page_for_render();
        let rtl = is_rtl_locale(&crate::app::current_locale(cx));

        let sidebar = Sidebar::new("app-sidebar")
            .w(relative(1.))
            .border_0()
            .collapsed(self.collapsed)
            .header(
                v_flex().w_full().gap_4().child(
                    SidebarHeader::new().w_full().child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(cx.theme().radius_lg)
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .size_8()
                            .flex_shrink_0()
                            .child(Icon::new(IconName::Star)),
                    ),
                ),
            )
            .child(
                SidebarGroup::new("Navigation").child(SidebarMenu::new().children(
                    Page::all().iter().map(|page| {
                        let page = *page;
                        SidebarMenuItem::new(page.title())
                            .icon(Icon::new(page.icon()).small())
                            .active(active_page == page)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_route(AppRoute::page(page), cx);
                            }))
                            // Context menu: right-click on sidebar items.
                            .context_menu(move |menu, _window, _cx| {
                                menu.menu_with_icon(
                                    "Navigate",
                                    Icon::new(IconName::ArrowRight),
                                    Box::new(NavigateToPage(page as usize)),
                                )
                                .separator()
                                .menu_with_icon(
                                    "Refresh",
                                    Icon::new(IconName::Redo2),
                                    Box::new(RefreshPage),
                                )
                                .separator()
                                .menu_with_icon(
                                    "Settings",
                                    Icon::new(IconName::Settings2),
                                    Box::new(NavigateToPage(Page::Settings as usize)),
                                )
                            })
                    }),
                )),
            );

        // RTL: reverse sidebar position and flex direction
        let sidebar_panel = resizable_panel()
            .size(px(255.))
            .size_range(px(60.)..px(320.))
            .child(sidebar);

        let content_panel = resizable_panel().child(
            v_flex()
                .flex_1()
                .h_full()
                .overflow_x_hidden()
                .child(
                    div()
                        .id("header")
                        .p_4()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::BOLD)
                                .child(page_title),
                        ),
                )
                .child(
                    div()
                        .id("page")
                        .flex_1()
                        .overflow_y_scroll()
                        .child(self.active_page_view()),
                ),
        );

        // In RTL locales the sidebar appears on the right; swap panel order.
        let mut layout = h_resizable("app-layout");
        if rtl {
            layout = layout.child(content_panel).child(sidebar_panel);
        } else {
            layout = layout.child(sidebar_panel).child(content_panel);
        }

        let content_area = div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &ToggleSearch, _, cx| {
                crate::launcher::open_launcher(cx);
            }))
            // Cmd+1..9 → NavigateToPage handler
            .on_action(cx.listener(|this, action: &NavigateToPage, _, cx| {
                let pages = Page::all();
                if let Some(&page) = pages.get(action.0) {
                    this.set_route(AppRoute::page(page), cx);
                }
            }))
            // Context menu action handlers
            .on_action(cx.listener(|this, _: &RefreshPage, _, cx| {
                let current = this.active_route.page_for_render();
                // Force a re-render by calling notify, since set_route
                // no-ops when the route is unchanged.
                cx.notify();
                tracing::info!(target: "gpui_starter::root", page = ?current, "page refreshed");
            }))
            .flex_1()
            .overflow_hidden()
            .child(layout);

        tracing::debug!(
            target: "gpui_starter::root::render",
            route = %self.active_route.title(),
            page = ?active_page,
            tasks_active = crate::tasks::active_count(cx),
            elapsed_us = render_started.elapsed().as_micros() as u64,
            "AppRoot render prepared"
        );

        v_flex()
            .size_full()
            .child(self.title_bar.clone())
            .child(content_area)
            .child(crate::status_bar::render(&self.active_route, cx))
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}
