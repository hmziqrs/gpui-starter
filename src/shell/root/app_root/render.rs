use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    resizable::{h_resizable, resizable_panel},
    sidebar::{Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    v_flex,
};

use crate::app::ToggleSearch;
use crate::routes::AppRoute;
use crate::sidebar::Page;
use crate::views::{ReloadCurrentPage, RenderErrorPage, TriggerRenderError};

use super::super::actions::{NavigateToPage, RefreshPage, is_rtl_locale};
use super::super::frame_time;
use super::state::AppRoot;

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = std::time::Instant::now();
        let sheet_layer = gpui_component::Root::render_sheet_layer(window, cx);
        let dialog_layer = gpui_component::Root::render_dialog_layer(window, cx);
        let notification_layer = gpui_component::Root::render_notification_layer(window, cx);
        let page_title = if self.render_error {
            "Render Error"
        } else {
            self.active_route.title()
        };
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
                            .active(!self.render_error && active_page == page)
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
                .child(div().id("page").flex_1().overflow_y_scroll().child({
                    let _render_guard = crate::lifecycle::enter_render_path();
                    self.active_page_view(cx)
                })),
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
            // Error boundary: reload clears the error state and retries the page.
            .on_action(cx.listener(|this, _: &ReloadCurrentPage, _, cx| {
                tracing::info!(
                    target: "gpui_starter::root",
                    "reloading page after render error"
                );
                this.render_error = false;
                this.error_page = None;
                cx.notify();
            }))
            // Error boundary: action-based trigger (used by error playground to
            // test the boundary UI without causing a real render panic, which is
            // process-fatal in GPUI due to the Metal extern "C" callback).
            .on_action(cx.listener(|this, action: &TriggerRenderError, _, cx| {
                tracing::info!(
                    target: "gpui_starter::root",
                    message = %action.message,
                    "error boundary activated via TriggerRenderError action"
                );
                this.render_error = true;
                this.error_page = Some(cx.new(|_| RenderErrorPage::new(action.message.clone())));
                cx.notify();
            }))
            .flex_1()
            .overflow_hidden()
            .child(layout);

        let elapsed_us = render_started.elapsed().as_micros() as u64;

        // Persist frame time for the status-bar readout.
        frame_time::store_frame_time(elapsed_us);

        if frame_time::is_slow_frame(elapsed_us) {
            tracing::warn!(
                target: "gpui_starter::root::render",
                route = %self.active_route.title(),
                elapsed_us,
                threshold_us = frame_time::SLOW_FRAME_THRESHOLD_US,
                "slow frame detected"
            );
        }

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
