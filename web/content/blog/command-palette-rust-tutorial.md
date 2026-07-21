---
title: "Building a command palette in Rust with GPUI"
description: "Step-by-step tutorial to build a VS Code-style command palette (Cmd+K) with fuzzy search in a Rust desktop app."
date: 2026-06-05
tags: [GPUI, launcher, tutorial]
draft: false
---

Hit Cmd+K in VS Code, type two characters, and you're there. Spotlight does the same thing on macOS. Raycast built a whole product around it. A command palette touches almost every part of your app: actions, global state, window management, keyboard input, and rendering.

I'm going to walk through building one in GPUI. By the end you'll have a floating popup with a search input, a filtered command list, keyboard navigation, and the plumbing to talk back to the main window. The real implementation in [gpui-starter](/docs/getting-started/) is about 460 lines of Rust. We'll build a slightly simplified version here.

## Defining commands with an enum

Start with a type that enumerates every action your palette can trigger. An enum gives you exhaustive matching and zero-cost dispatch.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandId {
    OpenHome,
    OpenSettings,
    ThemeLight,
    ThemeDark,
    Undo,
    Redo,
}

pub struct CommandSpec {
    pub id: CommandId,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub icon: IconName,
}
```

Each command carries display metadata: a title, a short description, and an icon. The `registry()` function returns the full list. Commands that need to be conditionally available (like Undo when there's nothing to undo) handle that through a separate `availability()` check, not by mutating the registry at runtime.

```rust
pub fn registry() -> Vec<CommandSpec> {
    vec![
        command(CommandId::OpenHome, "Home", "Open the Home page", IconName::Inbox),
        command(CommandId::OpenSettings, "Settings", "Open the Settings page", IconName::Settings2),
        command(CommandId::ThemeLight, "Light Mode", "Switch to light theme", IconName::Sun),
        command(CommandId::ThemeDark, "Dark Mode", "Switch to dark theme", IconName::Moon),
        command(CommandId::Undo, "Undo", "Undo last reversible action", IconName::Undo2),
        command(CommandId::Redo, "Redo", "Redo last reversed action", IconName::Redo2),
    ]
}
```

When the user picks a command, you pattern-match on the enum and run the behavior. No hash maps, no stringly-typed dispatch, no reflection.

```rust
pub fn execute(id: CommandId, cx: &mut App) {
    match id {
        CommandId::OpenHome => navigate(Page::Home, cx),
        CommandId::OpenSettings => navigate(Page::Settings, cx),
        CommandId::ThemeLight => set_theme_mode(ThemeMode::Light, cx),
        CommandId::ThemeDark => set_theme_mode(ThemeMode::Dark, cx),
        CommandId::Undo => { let _ = undo(cx); }
        CommandId::Redo => { let _ = redo(cx); }
    }
}
```

The `execute()` function takes `&mut App`, GPUI's shared application context. Both the palette and the main window share the same `App`, so global state mutations are visible everywhere.

## Opening a floating popup window

GPUI has a `WindowKind::PopUp` variant that creates a floating window with no titlebar. It renders on top of other windows and supports a blurred background. This is what makes the palette feel like an overlay rather than a separate app window.

```rust
pub fn open_palette(cx: &mut App) {
    // Prevent double-open
    if cx.try_global::<PaletteOpen>().is_some_and(|g| g.0) {
        return;
    }
    cx.set_global(PaletteOpen(true));

    let window_w = px(620.);
    let window_h = px(460.);

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

        cx.open_window(options, |_window, cx| {
            let palette = cx.new(|_window, cx| Palette::new(cx));
            cx.new(|_cx| palette)
        })
    }).detach();
}
```

Three things to notice. `WindowBackgroundAppearance::Blurred` gives the frosted-glass look on macOS. The window is centered horizontally and positioned at 12% from the top of the display, which is roughly where VS Code and Spotlight put their palettes. The `PaletteOpen` global prevents opening a second palette while the first is showing. It's a one-field struct with the `Global` trait:

```rust
pub struct PaletteOpen(pub bool);
impl Global for PaletteOpen {}
```

GPUI globals live in the `App` context, shared across all windows. When the palette closes, you set `PaletteOpen(false)` and the next Cmd+K works again.

## The palette view: search input and filtered results

The palette is a single GPUI view with a search bar, a scrollable results list, and a footer with keyboard hints.

```rust
pub struct Palette {
    focus_handle: FocusHandle,
    input: Entity<InputState>,
    selected_index: usize,
    items: Vec<CommandSpec>,
    filtered: Vec<usize>,
}
```

`items` holds the full command list from `registry()`. `filtered` is a vector of indices into `items` representing the current search results. When the user types, `refilter()` rebuilds this index list.

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

This uses substring matching, which is fine for a typical app with 15-20 commands. If you want fuzzy matching, swap the `contains` call for a scoring function (the `fuzzy-matcher` crate works well) and sort by score. The rest of the architecture stays the same.

The input is wired through GPUI's subscription system. When the `InputState` entity emits a `Change` event, `refilter()` runs. When it emits `PressEnter`, the palette fires the selected command and dismisses itself.

```rust
cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
    InputEvent::Change => this.refilter(cx),
    InputEvent::PressEnter { .. } => this.act(cx),
    _ => {}
}).detach();
```

## Keyboard navigation with actions

Arrow keys and Escape are handled through GPUI's action system, not raw key events. You define action types and bind keys to them within a context:

```rust
actions!(palette, [SelectNext, SelectPrev, Dismiss]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some("Palette")),
        KeyBinding::new("up", SelectPrev, Some("Palette")),
        KeyBinding::new("escape", Dismiss, Some("Palette")),
    ]);
}
```

The `"Palette"` context string means these bindings are only active when the palette view has focus. Your main window's key bindings don't interfere, and the palette's bindings don't leak out.

In the `Render` implementation, you attach handlers to the root element:

```rust
.on_action(cx.listener(|this, _: &SelectNext, _, cx| {
    if !this.filtered.is_empty() {
        this.selected_index = (this.selected_index + 1) % this.filtered.len();
        cx.notify();
    }
}))
.on_action(cx.listener(|this, _: &SelectPrev, _, cx| {
    if !this.filtered.is_empty() {
        this.selected_index = if this.selected_index == 0 {
            this.filtered.len() - 1
        } else {
            this.selected_index - 1
        };
        cx.notify();
    }
}))
```

This wraps around: Down on the last item jumps to the first, Up on the first jumps to the last. I find wrap-around more natural for a keyboard-driven palette because you never get stuck.

## Rendering the result list

Each result row shows an icon, a title, and a subtitle. The selected row gets a highlight background, and mouse hover updates the selection so clicking always fires the highlighted item.

```rust
h_flex()
    .id(display_ix)
    .px_3()
    .py_2()
    .gap_3()
    .items_center()
    .rounded(theme.radius)
    .cursor_pointer()
    .when(is_selected, |el| el.bg(theme.list_active))
    .when(!is_selected, |el| el.hover(|el| el.bg(theme.list_hover)))
    .on_mouse_move(cx.listener(move |this, _, _, cx| {
        if this.selected_index != display_ix {
            this.selected_index = display_ix;
            cx.notify();
        }
    }))
    .on_click(cx.listener(move |this, _, _, cx| {
        this.selected_index = display_ix;
        this.act(cx);
    }))
    .child(Icon::new(icon).small())
    .child(
        v_flex().flex_1().overflow_hidden()
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(title))
            .child(div().text_xs().text_color(theme.muted_foreground).truncate().child(subtitle)),
    )
```

The `truncate()` call adds ellipsis when text overflows, preventing long titles from breaking the layout.

## Dismissing on focus loss

The palette should close itself when the user clicks outside it. GPUI exposes window activation events:

```rust
cx.observe_window_activation(window, |_, window, cx| {
    if !window.is_window_active() {
        cx.set_global(PaletteOpen(false));
        window.remove_window();
    }
}).detach();
```

When macOS deactivates the popup, you clean up the global flag and remove the window immediately. Calling `remove_window()` directly avoids a one-frame flash: macOS changes the blur treatment on deactivated windows, so a deferred close would produce a visible glitch.

## Wiring the keyboard shortcut

The palette opens from a key binding registered during app initialization:

```rust
// During app initialization
cx.bind_keys([
    KeyBinding::new("cmd-k", ToggleSearch, None),
    KeyBinding::new("/", ToggleSearch, None),
]);
```

The main view handles the action:

```rust
.on_action(cx.listener(|_, _: &ToggleSearch, _, cx| {
    crate::palette::open_palette(cx);
}))
```

Two shortcuts, one action. Cmd+K for VS Code muscle memory, `/` for quick access. The `/` binding is fine in a list view but would conflict in a text field, which is why GPUI's context system matters: bindings scoped to `"Palette"` don't fire in the main view and vice versa.

## Communicating with the main window

The palette window runs in its own GPUI window context. It can't directly call methods on the main window's view. Two patterns bridge this gap.

For theme changes, `execute()` calls `set_theme_mode()` through the shared `App` context. Theme state is a GPUI global, not scoped to any window, so both windows see the update.

For navigation, the palette pushes an event into a global queue:

```rust
fn navigate(page: Page, cx: &mut App) {
    events::emit(AppEventKind::Navigate(AppRoute::page(page)), cx);
}
```

The main window observes this queue and reacts:

```rust
cx.observe_global::<AppEventQueue>(|this, cx| {
    for event in events::drain(cx) {
        match event.kind {
            AppEventKind::Navigate(route) => this.set_route(route, cx),
            // ...
        }
    }
}).detach();
```

GPUI globals act as a lightweight event bus here. The palette doesn't hold a reference to the main window. It mutates global state and the main window picks up changes on the next render cycle. For more on this pattern, see the [state management with GPUI entities](/blog/state-management-gpui-entities/) post.

## What to add next

The version here uses substring matching. For a real app, you'd want fuzzy matching with score-based ranking, command grouping by category (Navigation, Theme, Editing), and recently used commands surfaced at the top. None of these changes affect the architecture. You're just changing the sort order in `refilter()` and maybe adding header elements between groups.

One thing I'd caution against: don't make the palette do too much. It's an action launcher, not a file picker or a REPL. Keep it focused on dispatching commands. If you need file search, build a separate picker with its own data source.

The full working implementation lives in the [gpui-starter](https://github.com/freeoxide/gpui-starter) repo. Grab it from [the getting started guide](/docs/getting-started/), run `cargo run`, and press Cmd+K to try it. The source is in `src/features/command_palette.rs` and `src/services/commands.rs`.
