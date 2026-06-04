---
title: "Routing and navigation"
description: "Multi-page navigation with Rust enums, sidebar, deep link parsing, and keyboard shortcuts in gpui-starter"
---

Routing in gpui-starter is built around two enums: `Page` defines what appears in the sidebar, and `AppRoute` adds sub-routes and deep link support. The whole system lives in `src/shell/sidebar.rs` and `src/shell/route.rs`. No router crate, no string matching, just exhaustive pattern matching the compiler can check.

If you are new to the project, start with the [getting started guide](/docs/getting-started) to get the app running first. The [architecture overview](/docs/architecture) covers how routing fits into the broader app structure.

## The Page enum

The sidebar is driven by `Page` in `src/shell/sidebar.rs`. Each variant corresponds to a top-level view:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    Home,
    Form,
    HttpLab,
    HttpLabTesting,
    Settings,
    Notifications,
    Diagnostics,
    QueryDevTools,
    QueryPlayground,
    QueryDevToolsV2,
    ErrorPlayground,
    About,
}
```

`Page` provides three methods you will use everywhere:

| Method | Return type | What it does |
|--------|-------------|--------------|
| `title(&self)` | `&'static str` | Human-readable label for sidebar and header |
| `icon(&self)` | `IconName` | Icon rendered next to the sidebar entry |
| `all()` | `&'static [Page]` | Ordered list used to build the sidebar menu |

The sidebar iterates `Page::all()` to build its menu. Add a variant, update these three methods, and the sidebar picks it up automatically. No separate route config file to maintain.

## The AppRoute type

`src/shell/route.rs` defines `AppRoute`, a two-level routing type. It wraps `Page` for standard navigation and adds sub-routes for specific destinations within a page:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppRoute {
    Page(Page),
    SettingsNotifications,
}
```

`SettingsNotifications` maps to `Page::Settings` through `page_for_render()`. That means the settings page renders, but your code can tell it is showing the notifications subsection specifically.

| Method | Return type | What it does |
|--------|-------------|--------------|
| `page(page)` | `Self` | Constructor for standard pages |
| `page_for_render(&self)` | `Page` | Collapses sub-routes to their parent page |
| `title(&self)` | `&'static str` | Header title for the current route |
| `to_url(&self)` | `String` | Serializes to a deep link URL |
| `parse_deep_link(input)` | `Result<Self, AppError>` | Parses a deep link string into a route |

The default route is `AppRoute::Page(Page::Home)` via the `Default` impl.

## Render dispatch

`AppRoot` in `src/shell/root.rs` stores an `active_route: AppRoute` field. The `unchecked_active_page_view` method pattern-matches on the resolved `Page` and returns the corresponding entity as `AnyView`:

```rust
fn unchecked_active_page_view(&self) -> AnyView {
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
```

There is also an `active_page_view` method that wraps this with an error boundary check. If a render panic was detected since the last frame, it swaps in a `RenderErrorPage` fallback instead. The user can dismiss it with "Reload Page" or by navigating away. Read more about this pattern in the [architecture doc](/docs/architecture).

## Changing pages

`set_route` updates `active_route`, persists the new route to the config file, clears any active error boundary, and calls `cx.notify()` to trigger a re-render:

```rust
fn set_route(&mut self, route: AppRoute, cx: &mut Context<Self>) {
    if self.active_route == route {
        return; // no-op if already on this route
    }
    let route_url = route.to_url();
    tracing::info!(route = ?route, route_url, "navigating");
    self.active_route = route.clone();
    self.render_error = false;
    self.error_page = None;
    crate::app_state::update_config(cx, |config| {
        config.active_route = route;
    });
    cx.notify();
}
```

Because the active route is stored in `AppConfig`, the last viewed page is restored on the next launch. The config system is covered in the [architecture overview](/docs/architecture).

Sidebar items call `set_route` on click:

```rust
SidebarMenuItem::new(page.title())
    .icon(Icon::new(page.icon()).small())
    .active(!self.render_error && active_page == page)
    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
        this.set_route(AppRoute::page(page), cx);
    }))
```

Note the `!self.render_error &&` guard on the `active` state. When the error boundary is showing, no sidebar item highlights, which signals to the user that something went wrong on the current page.

## Keyboard navigation

gpui-starter binds `Cmd+1` through `Cmd+9` to jump directly to the first nine pages in `Page::all()`. This happens during `AppRoot::new`:

```rust
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
```

The `NavigateToPage` action handler looks up the page at that index and calls `set_route`. There is also a `RefreshPage` action bound to the context menu on each sidebar item, which forces a re-render via `cx.notify()` without changing the route.

The [command launcher](/docs/command-launcher) provides another way to navigate: open it with `Cmd+K` and type a page name.

## Deep link handling

The app registers the `gpui-starter://` URL scheme with the OS. When macOS opens a URL with this scheme, here is the flow:

1. `single_instance::preflight()` inspects CLI args for any argument starting with `gpui-starter://`.
2. If the app is already running, the deep link is forwarded to the primary instance via IPC (local socket) or a queue file fallback.
3. The primary instance emits `AppEventKind::DeepLinkReceived(link)` into the global `AppEventQueue`.
4. `AppRoot` observes the event queue and calls `AppRoute::parse_deep_link` on the URL:

```rust
cx.observe_global::<events::AppEventQueue>(|this, cx| {
    for event in events::drain(cx) {
        match event.kind {
            AppEventKind::Navigate(route) => this.set_route(route, cx),
            AppEventKind::DeepLinkReceived(link) => {
                match AppRoute::parse_deep_link(&link) {
                    Ok(route) => this.set_route(route, cx),
                    Err(err) => events::emit_error(err, cx),
                }
            }
            // ...
        }
    }
})
.detach();
```

### Deep link validation

`parse_deep_link` does not just pattern-match. It runs a validation pipeline before route matching:

1. **URL parsing** via the `url` crate. Malformed input returns `AppError::InvalidDeepLink`.
2. **Scheme check** against the `APP_URL_SCHEME` constant (`gpui-starter`).
3. **Host allowlist** checked against `VALID_HOSTS`. Unknown hosts are rejected.
4. **Path segment validation** that blocks traversal sequences (`..`, `.`) and forbidden characters (`/`, `\`, `\0`, `<`, `>`, `|`, `"`).
5. **Query parameter sanitization** that strips control characters from both keys and values.

This matters because deep links come from outside the app. A malicious URL in a web page or email should not be able to inject control characters or path traversal sequences into your routing logic.

### Deep link URL map

After validation, the host and path segments map to routes:

| URL | Route |
|-----|-------|
| `gpui-starter://home` | `Page(Home)` |
| `gpui-starter://form` | `Page(Form)` |
| `gpui-starter://http` | `Page(HttpLab)` |
| `gpui-starter://httplab-testing` | `Page(HttpLabTesting)` |
| `gpui-starter://settings` | `Page(Settings)` |
| `gpui-starter://settings/notifications` | `SettingsNotifications` |
| `gpui-starter://notifications` | `Page(Notifications)` |
| `gpui-starter://diagnostics` | `Page(Diagnostics)` |
| `gpui-starter://error-playground` | `Page(ErrorPlayground)` |
| `gpui-starter://query-devtools` | `Page(QueryDevTools)` |
| `gpui-starter://query-playground` | `Page(QueryPlayground)` |
| `gpui-starter://query-devtools-v2` | `Page(QueryDevToolsV2)` |
| `gpui-starter://about` | `Page(About)` |

URLs with an unsupported scheme or unknown host return `AppError::InvalidDeepLink`.

## Adding a new page

Adding a route requires changes in three files.

### 1. Add a Page variant

In `src/shell/sidebar.rs`, add the variant and update the three methods:

```rust
pub enum Page {
    Home,
    Form,
    // ...existing variants...
    Profile, // new
}
```

Update `title`, `icon`, and `all` to include the variant. The sidebar menu rebuilds from `Page::all()`, so the new entry appears automatically.

### 2. Add an AppRoute mapping

In `src/shell/route.rs`, add the URL mapping if you want deep link support:

```rust
// In to_url:
Self::Page(Page::Profile) => "gpui-starter://profile".to_string(),

// In VALID_HOSTS array:
const VALID_HOSTS: &[&str] = &[
    "home", "form", /* ... */, "profile",
];

// In parse_deep_link match:
("profile", []) => Ok(Self::Page(Page::Profile)),
```

Do not skip adding the host to `VALID_HOSTS`. Without it, `parse_deep_link` rejects the URL before reaching the match arm.

### 3. Add a render branch

In `src/shell/root.rs`, create an entity field on `AppRoot`, initialize it in `new`, and add a match arm in `unchecked_active_page_view`:

```rust
// Field on AppRoot:
profile_page: Entity<ProfilePage>,

// In AppRoot::new:
let profile_page = cx.new(|cx| ProfilePage::new(window, cx));

// In unchecked_active_page_view:
Page::Profile => self.profile_page.clone().into(),
```

That is it. Three files, three changes, and the compiler will tell you if you missed anything because the `match` is exhaustive.

## Testing routes

The route module has a companion test file at `src/shell/route.test.rs`. It covers valid deep links, unknown hosts, path traversal attempts, and scheme validation. When you add a new route, add a test case for its deep link URL:

```rust
#[test]
fn parse_profile_deep_link() {
    let route = AppRoute::parse_deep_link("gpui-starter://profile").unwrap();
    assert_eq!(route, AppRoute::Page(Page::Profile));
}
```

For more on testing GPUI applications, see the [testing guide](/docs/testing).
