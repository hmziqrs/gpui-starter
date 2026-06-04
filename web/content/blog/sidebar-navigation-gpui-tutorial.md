---
title: "Building sidebar navigation in GPUI: a complete tutorial"
description: "How to implement a resizable sidebar with page routing in a GPUI desktop app. Covers Page enums, resizable panels, deep links, and keyboard shortcuts."
date: 2026-06-05
tags: [GPUI, navigation, tutorial]
draft: false
---

Sidebar navigation is one of the first things you build in a desktop app. In a web framework you would reach for a router library and define some paths. GPUI does not have URLs or a browser history, so the approach is different. You use a Rust enum, a match statement, and GPUI's entity system to keep everything in sync.

Below is how to build a resizable sidebar with multi-page navigation, keyboard shortcuts, and deep link support. Every code example comes from the gpui-starter project and can be run directly.

## Defining your pages

The routing table in a GPUI app is just an enum. Each variant represents one screen.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    Home,
    Form,
    HttpLab,
    Settings,
    Notifications,
    Diagnostics,
    About,
}
```

That is the whole routing setup. When the app needs to know which screen to show, it reads this value. When the user navigates somewhere, the code writes a new value. Because the enum lives inside a GPUI entity, the reactivity system handles re-rendering automatically.

The `Page` enum also carries display metadata. Each variant knows its own title and icon.

```rust
impl Page {
    pub fn title(&self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Form => "Form",
            Page::HttpLab => "HTTP Lab",
            Page::Settings => "Settings",
            Page::Notifications => "Notifications",
            Page::Diagnostics => "Diagnostics",
            Page::About => "About",
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Page::Home => IconName::Inbox,
            Page::Form => IconName::File,
            Page::Settings => IconName::Settings2,
            Page::About => IconName::Info,
            // ...
        }
    }
}
```

A static `all()` method returns every variant in display order. The sidebar iterates over this list to build its items.

```rust
pub fn all() -> &'static [Page] {
    &[Page::Home, Page::Form, Page::HttpLab, Page::Settings,
      Page::Notifications, Page::Diagnostics, Page::About]
}
```

If you add a new page, you add a variant. The compiler then tells you every place you forgot to handle it. This exhaustiveness checking is one of the better reasons to model routes as an enum rather than strings.

## Rendering the sidebar

The sidebar in gpui-starter uses the `Sidebar` component from `gpui_component`. It takes a header, groups of menu items, and supports a collapsed state.

Here is the core rendering logic inside `AppRoot::render`:

```rust
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
                    .child(Icon::new(IconName::Star)),
            ),
        ),
    )
    .child(
        SidebarGroup::new("Navigation").child(
            SidebarMenu::new().children(
                Page::all().iter().map(|&page| {
                    let is_active = !self.render_error
                        && active_page == page;
                    SidebarMenuItem::new(page.title())
                        .icon(Icon::new(page.icon()).small())
                        .active(is_active)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_route(
                                AppRoute::page(page), cx
                            );
                        }))
                }),
            ),
        ),
    );
```

The `active` flag highlights the current page. The `on_click` handler captures the `page` value by copy (the enum is `Copy`) and calls `set_route` on the root entity. Each sidebar item also has a right-click context menu for keyboard-accessible navigation, covered later.

## Making the sidebar resizable

A fixed-width sidebar works for simple apps, but users expect to resize panels. GPUI provides `h_resizable` for horizontal split layouts with draggable dividers.

```rust
let sidebar_panel = resizable_panel()
    .size(px(255.))
    .size_range(px(60.)..px(320.))
    .child(sidebar);

let content_panel = resizable_panel().child(
    v_flex()
        .flex_1()
        .h_full()
        .overflow_x_hidden()
        .child(header_bar)
        .child(page_content),
);

let mut layout = h_resizable("app-layout");
layout = layout.child(sidebar_panel).child(content_panel);
```

The `size_range` constraint prevents the sidebar from collapsing to nothing or eating the entire window. The `size` sets the default width. Users can drag the divider between 60px and 320px.

The sidebar also supports RTL layouts. When the locale is Arabic, Hebrew, Farsi, or Urdu, the panel order swaps so the sidebar appears on the right:

```rust
if rtl {
    layout = layout.child(content_panel).child(sidebar_panel);
} else {
    layout = layout.child(sidebar_panel).child(content_panel);
}
```

## How page switching works

When the user clicks a sidebar item, the click handler calls `set_route`. This is where the actual navigation happens.

```rust
fn set_route(&mut self, route: AppRoute, cx: &mut Context<Self>) {
    if self.active_route == route {
        return; // already on this page, skip
    }
    self.active_route = route.clone();
    self.render_error = false;
    self.error_page = None;
    crate::app_state::update_config(cx, |config| {
        config.active_route = route;
    });
    cx.notify();
}
```

The active route updates, then the route persists to the app config so it survives restarts, and `cx.notify()` tells GPUI that this entity changed and needs re-rendering.

On the next frame, GPUI calls the `render` method again. The content area picks the right view using a match statement:

```rust
fn unchecked_active_page_view(&self) -> AnyView {
    match self.active_route.page_for_render() {
        Page::Home => self.home_page.clone().into(),
        Page::Form => self.form_page.clone().into(),
        Page::HttpLab => self.http_lab_page.clone().into(),
        Page::Settings => self.settings_page.clone().into(),
        Page::Notifications => self.notifications_page.clone().into(),
        Page::Diagnostics => self.diagnostics_page.clone().into(),
        Page::About => self.about_page.clone().into(),
    }
}
```

Each page is an `Entity<T>` created once during `AppRoot::new`. The match returns a clone of the entity handle, which GPUI renders into the content panel. Pages are not destroyed when you switch screens; they stay alive and retain their state. Scrolling position, form input, and other transient data survive navigation.

No event bus. No prop drilling. The entity owns the route, and `cx.notify()` is the signal that something changed. You can read more about how entities work in the [architecture docs](/docs/architecture/).

## Keyboard shortcuts for navigation

In gpui-starter, Cmd+1 through Cmd+9 jump directly to the corresponding sidebar page.

First, bind the keys during initialization:

```rust
let pages = Page::all();
cx.bind_keys(
    pages.iter().enumerate()
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
```

Then handle the action in the render method:

```rust
.on_action(cx.listener(|this, action: &NavigateToPage, _, cx| {
    let pages = Page::all();
    if let Some(&page) = pages.get(action.0) {
        this.set_route(AppRoute::page(page), cx);
    }
}))
```

The action is a simple struct: `NavigateToPage(usize)`. It carries the 0-based index of the target page. The handler looks up the page and calls the same `set_route` method that the sidebar click uses. This means keyboard and click navigation hit the same code path, which avoids subtle inconsistencies.

## Deep links: parsing URLs into pages

Desktop apps can register custom URL schemes. When the OS sends your app a URL like `gpui-starter://settings`, you need to convert that string into a `Page` variant. gpui-starter wraps this in an `AppRoute` enum with a `parse_deep_link` method.

```rust
pub fn parse_deep_link(input: &str) -> Result<Self, AppError> {
    let url = Url::parse(input)?;
    if url.scheme() != APP_URL_SCHEME {
        return Err(AppError::InvalidDeepLink { /* ... */ });
    }

    let host = url.host_str().unwrap_or_default();
    let segments: Vec<&str> = url.path_segments()
        .map(|s| s.filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    match (host, segments.as_slice()) {
        ("home", []) => Ok(Self::Page(Page::Home)),
        ("form", []) => Ok(Self::Page(Page::Form)),
        ("settings", []) => Ok(Self::Page(Page::Settings)),
        ("settings", ["notifications"]) => Ok(Self::SettingsNotifications),
        _ => Err(AppError::InvalidDeepLink { /* ... */ }),
    }
}
```

The parser validates the scheme, checks the host against a whitelist, and sanitizes path segments for control characters and path traversal attempts. Security matters here because deep links can come from untrusted sources like browsers and email clients.

When the app receives a deep link, it goes through the same event pipeline as a sidebar click:

```rust
AppEventKind::DeepLinkReceived(link) => match AppRoute::parse_deep_link(&link) {
    Ok(route) => this.set_route(route, cx),
    Err(err) => events::emit_error(err, cx),
}
```

The destination is the same `set_route` call. Deep links, sidebar clicks, and Cmd+N shortcuts all converge on one code path. This makes the navigation behavior predictable and easy to test.

## Persisting the active route

Users expect the app to remember where they were. The route persists to the app config file on every navigation:

```rust
crate::app_state::update_config(cx, |config| {
    config.active_route = route;
});
```

On startup, `AppRoot::new` reads the saved route:

```rust
let config = crate::app_state::config(cx);
Self {
    active_route: config.active_route,
    collapsed: config.sidebar_collapsed,
    // ...
}
```

The sidebar collapsed state is also persisted. Users who collapse the sidebar will find it collapsed the next time they open the app. For more on how config persistence works, see the [getting started guide](/docs/getting-started/).

## What you get from this pattern

The enum-plus-match approach to sidebar navigation has practical advantages.

Add a page variant and the compiler lists every match arm you forgot to update. `Page::all()` drives the sidebar, keyboard shortcuts, and deep link parser from one list. Clicks, keyboard shortcuts, and deep links all call `set_route`, so there is no duplicated logic. Pages live as long-lived entities, meaning navigation does not destroy them and scroll position and form state survive. You can test routing by asserting `Page::from_str("settings")` returns the right variant, without rendering a UI or simulating clicks.

The tradeoff is that this model does not give you a browser-style back button or URL bar out of the box. If you need those, you would add a `Vec<Page>` history stack and wire back/forward buttons to it. For most desktop apps, the sidebar pattern covers what users expect.

The full implementation lives in the [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) repository. You can see the sidebar module in `src/shell/sidebar.rs`, the root layout in `src/shell/root.rs`, and the deep link parser in `src/shell/route.rs`. The [getting started guide](/docs/getting-started/) walks through cloning and running the project. If you want to understand how entities and the context system work before diving into navigation, the [architecture docs](/docs/architecture/) cover that in detail.
