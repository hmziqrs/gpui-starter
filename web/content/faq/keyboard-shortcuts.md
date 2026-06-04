---
question: "What keyboard shortcuts are available and how do I customize them?"
description: "Complete list of keyboard shortcuts in gpui-starter, how key bindings work under the hood, and how to add your own."
category: "Features"
order: 11
---

gpui-starter ships with keyboard shortcuts for the actions you use most. Shortcuts fall into two buckets: in-app key bindings registered through GPUI's action system, and a system-wide global hotkey that works even when the window is not focused.

## Built-in shortcuts

| Shortcut | Action |
|----------|--------|
| **Cmd+K** or **/** | Open the command launcher with fuzzy search |
| **Escape** | Dismiss overlays (launcher, dialogs) |
| **Cmd+Z** | Undo (theme switches, setting changes) |
| **Cmd+Y** | Redo |
| **Cmd+Q** (macOS) | Quit the app |
| **Alt+F4** (Linux/Windows) | Quit the app |

On macOS, a system-wide **Alt+Space** hotkey brings the app to the foreground from any other application. This uses the `global_hotkey` crate and lives in the `shortcuts` module. You can toggle it on or off from the Settings page, and the choice persists across restarts in the app configuration file.

## How key bindings are registered

In-app bindings are registered with `cx.bind_keys()` inside `app::init`. Each `KeyBinding::new` call maps a key combination to an action type:

```rust
cx.bind_keys([
    KeyBinding::new("cmd-k", ToggleSearch, None),
    KeyBinding::new("/", ToggleSearch, None),
]);
```

The third argument is an optional context. Bindings with `None` apply globally. Context-scoped bindings, like the arrow keys in the launcher overlay, pass a context string so they only fire when that overlay is active. This is the same pattern Zed uses internally.

GPUI matches key bindings from most specific to least specific. A binding scoped to a particular focus context wins over a global binding for the same key. This lets you override defaults inside specific views without collisions.

See the [architecture docs](/docs/architecture/) for how actions flow through the entity system, and the [command launcher docs](/docs/command-launcher/) for how the Cmd+K overlay registers its own scoped bindings.

## Adding your own shortcuts

Call `cx.bind_keys()` during initialization with your action type and preferred key combination. Define an action struct, register it, and bind a key to it:

```rust
#[derive(gpui::Action)]
struct MyAction;

fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-n", MyAction, None),
    ]);

    cx.on_action(|_action: &MyAction, cx| {
        // your handler logic
    });
}
```

For a walkthrough of a real shortcut-driven feature, see the [command palette tutorial](/blog/command-palette-rust-tutorial/) and the [Cmd+K implementation deep dive](/blog/building-command-launcher-gpui/). The [getting started guide](/docs/getting-started/) covers the full project layout including where binding registration fits in the init sequence.
