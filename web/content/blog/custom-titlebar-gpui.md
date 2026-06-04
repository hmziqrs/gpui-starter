---
title: "Building a custom title bar in GPUI"
description: "Replace the native macOS, Windows, or Linux title bar with a custom one in GPUI. Covers drag regions, traffic light buttons, and platform quirks."
date: 2026-05-13
tags: [GPUI, Rust, design]
draft: false
---

The native title bar on macOS, Windows, and Linux does its job. But when your app has its own theme system with specific colors and spacing, that strip of system chrome at the top of the window looks wrong. A custom title bar lets you control the background, the layout, and what goes in that space: navigation breadcrumbs, a search field, action buttons, or anything else.

gpui-starter ships a working custom title bar. Here is how it works and what I learned building it.

## Why replace the native title bar

The simplest reason is visual consistency. Your app picks colors and borders from a theme. The native title bar ignores all of that. It draws its own background, its own separator line, its own text style. On a dark-themed app, the native title bar sits there looking like it was pasted in from Finder.

The other reason is space. The title bar is the widest horizontal strip in your window, and the native version wastes most of it on centered text you cannot interact with. In Zed, the title bar holds the file path and git branch. In Figma, it holds zoom controls and sharing buttons. In gpui-starter, it holds the app menu, a settings dropdown, a search button, and a notifications button. Five things that would otherwise need their own toolbar row.

If you spend hours in an app, the chrome around the content matters more than you think. The title bar is the first thing your eyes land on.

## Making the native title bar transparent

You tell GPUI to hide the native title bar in your window options:

```rust
let options = WindowOptions {
    window_bounds: Some(WindowBounds::Windowed(window_bounds)),
    titlebar: Some(TitleBar::title_bar_options()),
    window_min_size: Some(gpui::Size {
        width: px(480.),
        height: px(320.),
    }),
    kind: WindowKind::Normal,
    ..Default::default()
};
```

The `title_bar_options()` function returns configuration that tells GPUI to render the title bar as transparent and positions the macOS traffic light buttons:

```rust
pub fn title_bar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: None,
        appears_transparent: true,
        traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
    }
}
```

On macOS, `appears_transparent: true` hides the native title text and background but keeps the close, minimize, and zoom buttons visible. On Linux, you also need `window_decorations: Some(gpui::WindowDecorations::Client)` to switch to client-side decorations. The [getting started guide](/docs/docs/getting-started) covers the full window setup if you need more context there.

## Recreating the drag region

Once you remove the native title bar, users have nothing to grab to move the window. You have to rebuild that yourself.

The GPUI title bar component handles this with mouse event tracking and `window.start_window_move()`:

```rust
div()
    .id("title-bar")
    .h(TITLE_BAR_HEIGHT)
    .on_mouse_down(MouseButton::Left, window.listener_for(&state, |state, _, _, _| {
        state.should_move = true;
    }))
    .on_mouse_up(MouseButton::Left, window.listener_for(&state, |state, _, _, _| {
        state.should_move = false;
    }))
    .on_mouse_move(window.listener_for(&state, |state, _, window, _| {
        if state.should_move {
            state.should_move = false;
            window.start_window_move();
        }
    }))
```

This works in three steps. First, mouse down sets a `should_move` flag. On the first mouse move while the flag is set, the code calls `start_window_move()` and clears the flag immediately. This hands control to the operating system's built-in window move behavior, so the drag feels native. Clearing the flag right away means the move only starts once per drag gesture.

Interactive elements inside the title bar (buttons, menus, dropdowns) must call `cx.stop_propagation()` on their mouse down events. Without this, clicking your search button would also trigger a window drag. I have lost more time to missing `stop_propagation()` calls than I want to admit.

## Handling each platform

The title bar is 34 pixels tall on all platforms. The left padding differs: 80 pixels on macOS to leave room for the traffic lights, 12 pixels on Linux and Windows.

### macOS

The traffic light buttons are rendered by the system. GPUI positions them with `traffic_light_position` and they work without extra code. Double-clicking the title bar calls `window.titlebar_double_click()`, which triggers the macOS system behavior (minimize or zoom, depending on what the user configured in System Settings).

### Windows

The component renders its own control buttons: minimize, maximize, close. Each button is wired to the correct window action through GPUI's `WindowControlArea` API.

### Linux

Linux needs the most manual work. The component renders control buttons and handles click events directly:

```rust
.when(is_linux, |this| {
    this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
        window.prevent_default();
        cx.stop_propagation();
    })
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        match icon {
            Self::Minimize => window.minimize_window(),
            Self::Restore | Self::Maximize => window.zoom_window(),
            Self::Close { .. } => window.remove_window(),
        }
    })
})
```

Right-clicking the title bar on Linux also shows the window menu via `window.show_window_menu()`.

If you want to understand how the title bar fits into the full view hierarchy, the [architecture doc](/docs/docs/architecture) has a diagram of the component tree.

## How gpui-starter puts it together

The `AppTitleBar` struct wraps the generic `TitleBar` component and adds app-specific content: the menu bar on the left, settings and action buttons on the right.

```rust
pub struct AppTitleBar {
    app_menu_bar: Entity<AppMenuBar>,
    settings: Entity<SettingsDropdown>,
    child: TitleBarChild,
}
```

The render method puts the menu bar in one flex child and the right-side controls in another. The right section has a fixed `on_mouse_down` handler that stops propagation, so clicking anywhere in that area does not start a window drag. This is the pattern I recommend: partition the title bar into a drag zone and an interactive zone. Keep it simple.

The `child` field is a closure that returns an `AnyElement`. Different pages inject custom content into the title bar without the title bar knowing about them. It is a lightweight form of dependency injection that keeps the component flexible.

The `SettingsDropdown` is a focus-tracked div that uses the dropdown menu system to let users change font size and border radius at runtime. Changes go through `Theme::global_mut(cx)` and trigger a window refresh. For more on how theme tokens like `title_bar` and `title_bar_border` work, see the [themes guide](/docs/docs/themes).

## Wiring it into the root layout

In `root.rs`, the `AppRoot` creates the title bar during initialization:

```rust
let title_bar = cx.new(|cx| AppTitleBar::new(title, window, cx));
```

The render method places it at the top of the vertical flex layout, above the sidebar and content area:

```rust
v_flex()
    .size_full()
    .child(self.title_bar.clone())
    .child(/* sidebar + content */)
    .child(/* status bar */)
```

The title bar is an entity, not a function call that returns an element. It manages its own state and re-renders independently when its focus handle or dropdown state changes. The rest of the layout does not care about title bar updates. If you are new to GPUI entities, the [GPUI entity system explained](/blog/gpui-entity-system-explained) post breaks down how they work.

## Gotchas

The biggest one is the `stop_propagation()` calls. If you add a new interactive element to the title bar and forget to stop propagation on its mouse down, clicking it will start a window drag instead of doing what you intended. Test every button, every dropdown, every clickable thing you add to the title bar.

The title bar height is a constant at 34 pixels. If you change it, you also need to update the `traffic_light_position` on macOS so the buttons stay vertically centered. These two values are coupled and there is no way around that.

On Linux, client-side decorations mean your app is responsible for drawing and handling its own window controls. If you skip this, your users get a borderless window with no way to minimize or close it except the keyboard.

## Next steps

If you are building a GPUI app from scratch, [gpui-starter](/docs/docs/getting-started) has the custom title bar, drag regions, platform-specific handling, and theme integration all wired up. You can also read about how the [sidebar navigation](/blog/sidebar-navigation-gpui) and [theme system](/blog/theme-system-deep-dive) work in gpui-starter to see how the title bar ties into the rest of the UI.
