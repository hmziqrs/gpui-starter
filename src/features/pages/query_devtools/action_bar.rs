use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable,
    button::Button,
    h_flex,
};
use gpui_query::client::QueryClient;
use gpui_query::QueryKeyFilter;

use super::QueryDevToolsPage;

// ---------------------------------------------------------------------------
// Action Bar
// ---------------------------------------------------------------------------

pub(crate) fn render_action_bar(cx: &mut Context<QueryDevToolsPage>) -> Div {
    // Extract theme colors upfront to avoid holding a borrow on cx across
    // the mutable borrow required by cx.listener().
    let theme = cx.theme();
    let radius_lg = theme.radius_lg;
    let border = theme.border;
    let muted = theme.muted;
    let _ = theme;

    let has_client = cx.has_global::<QueryClient>();

    let invalidate = Button::new("devtools-invalidate-all")
        .outline()
        .label("Invalidate All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.invalidate_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let reset = Button::new("devtools-reset-all")
        .outline()
        .label("Reset All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.reset_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let gc = Button::new("devtools-gc")
        .outline()
        .label("GC")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.gc(cx, now_ms);
                });
                cx.notify();
            }
        }));

    let remove = Button::new("devtools-remove-all")
        .outline()
        .label("Remove All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.remove_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let clear = Button::new("devtools-clear")
        .outline()
        .label("Clear")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, _cx| {
                    client.clear();
                });
                cx.notify();
            }
        }));

    div()
        .rounded(radius_lg)
        .border_1()
        .border_color(border)
        .bg(muted)
        .p_4()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .mb_2()
                .child("Actions"),
        )
        .child(
            h_flex().gap_2().flex_wrap().children(vec![invalidate, reset, gc, remove, clear]),
        )
}
