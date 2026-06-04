---
question: "How do I implement undo and redo in my GPUI app?"
description: "gpui-starter ships a command-pattern undo/redo stack with Cmd+Z / Cmd+Y support, menu integration, and an easy pattern for adding your own reversible operations."
category: "Features"
order: 12
---

Yes. gpui-starter includes a working undo/redo system right out of the box. The `undo_stack` module in `src/services/` implements a classic command pattern using two stacks (`past` and `future`). Every reversible operation gets recorded as an `UndoEntry` that carries its kind, human-readable labels, and a timestamp.

This is not a generic "diff the world" approach. You opt specific operations into the undo system, which gives you precise control over what counts as a single undo step and what the inverse action looks like.

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| **Cmd+Z** (macOS) / **Ctrl+Z** (Linux) | Undo the last entry |
| **Cmd+Y** (macOS) / **Ctrl+Y** (Linux) | Redo the last undone entry |

These bindings are registered in `cx.bind_keys()` during app initialization. The Edit menu updates its Undo and Redo items dynamically based on whether the `past` and `future` stacks have entries, graying out the actions when the stacks are empty. This menu wiring lives in `src/shell/menus.rs` and uses `commands::availability()` to check stack state.

For the full list of shortcuts, see [keyboard shortcuts](/faq/keyboard-shortcuts/).

## How it works internally

The undo system has three moving parts:

1. **`UndoState`** is a GPUI global that holds the two stacks, an `applying` guard flag, and a `last_rejected` string for diagnostics.
2. **`UndoModel`** is a private struct that operates on the state. It handles `record`, `pop_undo`, `pop_redo`, `push_redo`, and `push_undo`.
3. **`UndoKind`** is an enum that tags each entry with its operation type and the data needed to reverse or replay it.

Recording an entry pushes it onto `past` and clears `future`. This is standard branch-on-write behavior: once you make a new change, the redo history is gone. Undoing pops from `past`, runs the inverse action, then pushes the entry onto `future`. Redoing pops from `future`, runs the forward action, and pushes back onto `past`.

The `applying` boolean guard is the detail that prevents infinite loops. When an undo or redo fires, it sets `applying = true` before running the inverse/forward action. If that action tries to record itself back into the undo stack, `UndoModel::record` sees the flag and bails out. The guard clears once the action completes.

Currently the only implemented `UndoKind` variant is `ThemeMode`, which records light/dark mode switches. The stacks live in a GPUI global and have no fixed size limit. They grow until the app exits. If you need bounded memory, add a cap in `UndoModel::record` and drop the oldest `past` entry when `past.len()` exceeds your threshold.

For a broader look at how state flows through the app, see [state management patterns for Rust desktop apps](/blog/rust-desktop-state-management/) and the [architecture guide](/docs/architecture/).

## Adding undo support to custom operations

Adding a new reversible operation is a four-step process. Say you want to make a settings toggle undoable.

### 1. Add a variant to `UndoKind`

Open `src/services/undo_stack.rs` and extend the enum:

```rust
#[derive(Clone, Debug)]
pub enum UndoKind {
    ThemeMode { before: ThemeMode, after: ThemeMode },
    SettingsToggle {
        setting_name: String,
        before: bool,
        after: bool,
    },
}
```

### 2. Implement the inverse and forward logic

In the same file, update `apply_inverse` and `apply_forward`:

```rust
fn apply_inverse(kind: &UndoKind, cx: &mut App) {
    match kind {
        UndoKind::ThemeMode { before, .. } => {
            crate::app::set_theme_mode_with_record(*before, false, cx)
        }
        UndoKind::SettingsToggle { setting_name, before, .. } => {
            crate::app::set_setting_with_record(setting_name.clone(), *before, false, cx)
        }
    }
}

fn apply_forward(kind: &UndoKind, cx: &mut App) {
    match kind {
        UndoKind::ThemeMode { after, .. } => {
            crate::app::set_theme_mode_with_record(*after, false, cx)
        }
        UndoKind::SettingsToggle { setting_name, after, .. } => {
            crate::app::set_setting_with_record(setting_name.clone(), *after, false, cx)
        }
    }
}
```

Notice the `false` argument. That second parameter controls whether the call records itself into the undo stack. Setting it to `false` prevents the re-recording that the `applying` guard also catches.

### 3. Add a recording function

Follow the pattern of `record_theme_mode_change`:

```rust
pub fn record_settings_toggle(
    setting_name: String,
    before: bool,
    after: bool,
    cx: &mut App,
) {
    if before == after {
        return;
    }
    let state = cx.default_global::<UndoState>();
    let mut model = UndoModel::from_state(std::mem::take(state));
    model.record(UndoEntry {
        label: format!("Toggle {setting_name}"),
        undo_label: format!("Undo Toggle {setting_name}"),
        redo_label: format!("Redo Toggle {setting_name}"),
        created_at: AppTimestamp::now(),
        kind: UndoKind::SettingsToggle { setting_name, before, after },
    });
    *state = model.into_state();
}
```

### 4. Call the recorder at the mutation site

Wherever your settings toggle happens, call the recording function before (or alongside) the mutation:

```rust
undo_stack::record_settings_toggle(
    "notifications".to_string(),
    current_value,
    !current_value,
    cx,
);
```

That's all you need. The undo system handles the rest: stack management, the `applying` guard, menu label updates, and the Cmd+Z / Cmd+Y bindings.

## What gets recorded

Right now only theme mode changes (light/dark toggle) are tracked. Theme selection (switching between Tokyo Night, Catppuccin, etc.) is not undoable, because the [theme system](/docs/themes/) already provides instant switching from both the menu and the command launcher. For details on how theme mode and theme selection differ, see the [theme system deep dive](/blog/theme-system-deep-dive/).

## Diagnostics integration

When an undo or redo is attempted on an empty stack, `UndoModel` sets `last_rejected` to a reason string like `"nothing to undo"`. The [diagnostics page](/docs/architecture/) reads this field so you can see whether a shortcut press was silently rejected, which helps during development and debugging.

## Related topics

- [Keyboard shortcuts](/faq/keyboard-shortcuts/) for the full shortcut reference.
- [Theme system](/docs/themes/) for runtime theme switching and hot-reload.
- [State management patterns](/blog/rust-desktop-state-management/) for how GPUI globals, entities, and contexts fit together.
- [Architecture guide](/docs/architecture/) for the full app wiring diagram.
