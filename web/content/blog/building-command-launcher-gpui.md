---
title: "Building a command launcher (Cmd+K) in GPUI"
description: "Walk through how gpui-starter implements a VS Code-style command palette as a floating popup in GPUI with search filtering and cross-window events."
date: 2026-05-27
tags: [GPUI, Rust, desktop]
draft: false
---

You know the drill. Hit Cmd+K in VS Code, type a few characters, and you're where you need to be. Spotlight does it on macOS. Raycast built an entire product around it. Every serious desktop app needs a fast way to reach any action without grabbing the mouse.

gpui-starter ships with a command launcher that works the same way. Press Cmd+K (or `/`), start typing, and the list filters as you go. Here's how it's built using [GPUI's window system](/docs/architecture/), action dispatch, and global state.

## Two pieces: a popup window and a command list

The launcher splits into two concerns. First, the command registry: a static list of actions the app knows about. Second, the popup window: a floating overlay that appears on top of the main app, shows a search input, and lets the user pick a command.

The registry knows nothing about windows or UI. The popup knows nothing about what commands actually do. They talk through a thin event layer and GPUI's global state.

## Registering commands

Commands live in `src/commands.rs`. Each command has an ID (an enum variant), a display title, a subtitle, and an icon. The `registry()` function returns the full list:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandId {
    OpenHome,
    OpenSettings,
    ThemeLight,
    ThemeDark,
    StartDemoTask,
    Undo,
    Redo,
    // ...
}

pub struct CommandSpec {
    pub id: CommandId,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub icon: IconName,
}

pub fn registry() -> Vec<CommandSpec> {
    vec![
        command(CommandId::OpenHome, "Home", "Open the Home page", IconName::Inbox),
        command(CommandId::OpenSettings, "Settings", "Open the Settings page", IconName::Settings2),
        command(CommandId::ThemeLight, "Light Mode", "Switch to light theme", IconName::Sun),
        command(CommandId::ThemeDark, "Dark Mode", "Switch to dark theme", IconName::Moon),
        // ...
    ]
}
```

Every command is a variant of `CommandId`. The `execute()` function pattern-matches on the ID and runs the corresponding logic:

```rust
pub fn execute(id: CommandId, cx: &mut App) {
    match id {
        CommandId::OpenHome => navigate(Page::Home, cx),
        CommandId::OpenSettings => navigate(Page::Settings, cx),
        CommandId::ThemeLight => crate::app::set_theme_mode(ThemeMode::Light, cx),
        CommandId::ThemeDark => crate::app::set_theme_mode(ThemeMode::Dark, cx),
        CommandId::StartDemoTask => crate::tasks::start_demo_task(cx),
        CommandId::Undo => { let _ = crate::undo_stack::undo(cx); }
        CommandId::Redo => { let _ = crate::undo_stack::redo(cx); }
        // ...
    }
}
```

There's also an `availability()` function that checks whether a command should show as greyed out. "Copy Diagnostics" is disabled when the clipboard backend isn't available. "Undo" is disabled when there's nothing on the undo stack. This keeps the UI honest: you can't click something that won't work.

## Opening a floating popup window

GPUI has a `WindowKind::PopUp` variant that creates a floating window. No titlebar, renders on top of other windows, and supports a blurred background. This is what makes the launcher feel like a native overlay instead of a separate window.

The `open_launcher()` function in `src/launcher.rs` handles the setup:

```rust
pub fn open_launcher(cx: &mut App) {
    // Prevent double-open
    if cx.try_global::<LauncherOpen>().is_some_and(|g| g.0) {
        return;
    }
    cx.set_global(LauncherOpen(true));

    let window_w = px(620.);
    let window_h = px(460.);

    // Center on the primary display
    let bounds = if let Some(display) = cx.primary_display() {
        let db = display.bounds();
        let x = db.origin.x + (db.size.width - window_w) / 2.;
        let y = db.origin.y + db.size.height * 0.12;
        Bounds { origin: point(x, y), size: size(window_w, window_h) }
    } else {
        Bounds { origin: point(px(200.), px(120.)), size: size(window_w, window_h) }
    };

    cx.spawn(async move |cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: true,
            is_resizable: false,
            window_background: WindowBackgroundAppearance::Blurred,
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let launcher_root = cx.new(|cx| LauncherRoot::new(window, cx));
            cx.new(|cx| Root::new(launcher_root, window, cx).bg(transparent_black()))
        })
    }).detach();
}
```

`WindowBackgroundAppearance::Blurred` gives the popup that frosted-glass effect on macOS. The window centers on the primary display at 12% from the top, which feels natural for a launcher (too high and it's in the menu bar; too low and you're scanning down). The `LauncherOpen` global prevents spawning duplicate popups if someone mashes Cmd+K.

## Searching and filtering

The `Launcher` view holds the full list of commands as `LauncherItem` structs and a `filtered` vector of indices. When the user types in the search input, `refilter()` runs:

```rust
fn refilter(&mut self, cx: &mut Context<Self>) {
    let q = self.input.read(cx).value().to_lowercase();
    self.filtered = if q.is_empty() {
        (0..self.items.len()).collect()
    } else {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.title.to_lowercase().contains(&q)
                    || item.subtitle.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    };
    self.selected_index = 0;
    cx.notify();
}
```

Right now this uses plain substring matching. That's fast enough for the number of commands in a typical desktop app (gpui-starter has about 15). If you wanted fuzzy matching, you'd swap the `contains` call for a fuzzy scoring function and sort by score. The rest of the architecture stays the same. I went with substring matching because it's predictable: users see exactly what they type. Fuzzy matching can feel magical when it works and confusing when it doesn't.

The filtered list renders each item as a row with an icon, title, and subtitle. Arrow keys cycle through the list. Mouse hover updates the selection. Clicking or pressing Enter triggers the action.

## Talking back to the main window

The popup window runs in its own GPUI context. It can't directly mutate the main app's state. Instead, it uses GPUI's global system and an event queue.

When the user picks a command, `LauncherRoot` receives a `LauncherEvent::Act` and calls `commands::execute()`. For navigation commands, `execute()` calls `events::emit()`, which pushes an `AppEventKind::Navigate` into a global queue:

```rust
// In the main app root (src/root.rs):
cx.observe_global::<AppEventQueue>(|this, cx| {
    for event in events::drain(cx) {
        match event.kind {
            AppEventKind::Navigate(route) => this.set_route(route, cx),
            AppEventKind::AppError { message, severity } => {
                crate::error_surface::report(message, severity, cx);
                cx.notify();
            }
            // ...
        }
    }
}).detach();
```

The main window observes the `AppEventQueue` global. When the launcher emits a navigation event, the main window picks it up on the next render cycle and updates the active page. For theme changes, `execute()` calls `set_theme_mode()` directly through the shared `App` context. That works because theme state lives in a global, not scoped to a particular window.

This is a pragmatic pattern. GPUI globals act as a shared bus. The launcher window doesn't hold a reference to the main window. It mutates global state and trusts the main window to react. For a launcher that opens and closes in under a second, this is plenty.

## Wiring the keyboard shortcut

The keyboard shortcut is registered during app initialization:

```rust
// src/app.rs
cx.bind_keys([
    KeyBinding::new("cmd-k", ToggleSearch, None),
    KeyBinding::new("/", ToggleSearch, None),
]);
```

The main `AppRoot` view handles the `ToggleSearch` action by calling `crate::launcher::open_launcher(cx)`. Two shortcuts, one action. Cmd+K for muscle memory from VS Code, `/` for quick access when your hands are already on the keyboard.

## Where to go from here

The command launcher in gpui-starter is about 440 lines of Rust. It covers a pattern you'll see in most GPUI apps: register actions, open a popup window, filter with a search input, communicate through globals. If you're building a desktop app with GPUI, this is a reasonable starting point.

Some ideas for extending it: add fuzzy matching with a crate like `fuzzy-matcher`, group commands by category so the list isn't flat, or track recently used commands and pin them to the top. The registry pattern stays the same regardless.

Grab [gpui-starter](/docs/getting-started/) and run `cargo run` to try the launcher yourself. Press Cmd+K and search for any command in the app. If you want more context on how the window system works, check out [the architecture guide](/docs/architecture/) or my earlier post on [building multi-page navigation in GPUI](/blog/building-multi-page-navigation-gpui/).
