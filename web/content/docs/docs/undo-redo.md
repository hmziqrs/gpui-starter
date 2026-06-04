---
title: Undo and Redo
description: "Command pattern undo/redo stack with Cmd+Z/Cmd+Y keyboard shortcuts."
---

The undo stack (`src/services/undo_stack.rs`) implements a command-pattern history for reversible app actions. Every time the user performs an action that can be reversed, like switching the theme from light to dark, the system records a snapshot of the before and after state. Pressing Cmd+Z (or Ctrl+Z on Linux/Windows) pops the most recent entry, applies the inverse, and pushes it onto the redo stack. Pressing Cmd+Y (or Ctrl+Y) replays it.

This page covers the data model, how to record actions, how undo/redo flows through the menu and command palette, and how to extend the system with new reversible operations. For background on why desktop apps need first-class undo and how the command pattern maps to GPUI's global state model, see [Architecture](/docs/architecture/). For keyboard shortcut wiring, see [Getting Started](/docs/getting-started/).

## Data model

The stack is a GPUI global called `UndoState`. It holds two `Vec<UndoEntry>` stacks and a guard flag.

```rust
#[derive(Clone, Debug, Default)]
pub struct UndoState {
    pub past: Vec<UndoEntry>,
    pub future: Vec<UndoEntry>,
    pub applying: bool,
    pub last_rejected: Option<String>,
}

impl Global for UndoState {}
```

| Field | Purpose |
|-------|---------|
| `past` | Entries available for undo. Last element is the most recent. |
| `future` | Entries available for redo. Last element is the most recently undone. |
| `applying` | Guard flag. When `true`, new `record` calls are ignored to prevent recursive entries during undo/redo application. |
| `last_rejected` | Human-readable reason when an undo or redo was attempted but the relevant stack was empty. |

Each entry carries metadata about what changed.

```rust
#[derive(Clone, Debug)]
pub struct UndoEntry {
    pub label: String,
    pub undo_label: String,
    pub redo_label: String,
    pub created_at: AppTimestamp,
    pub kind: UndoKind,
}
```

`label` is a short description for debug logging. `undo_label` and `redo_label` are shown in menu items (for example, "Undo Theme Switch"). `kind` is the typed payload that tells the system how to apply the inverse and forward operations.

### UndoKind enum

```rust
#[derive(Clone, Debug)]
pub enum UndoKind {
    ThemeMode { before: ThemeMode, after: ThemeMode },
}
```

Right now the only reversible action is theme mode changes. Each variant stores both the before and after states so the inverse and forward functions have everything they need. Adding a new variant is straightforward; see [Extending with new actions](#extending-with-new-actions) below.

## Initialization

Call `undo_stack::initialize(cx)` during app startup. This installs the global and registers the `undo_stack` capability with the capability system.

```rust
// In your app init function
undo_stack::initialize(cx);
```

After initialization, `can_undo(cx)` and `can_redo(cx)` return `Option<String>` reflecting the current state. The menu system and command palette use these to enable or disable the Undo and Redo items.

## Recording an action

Use `record_theme_mode_change` to push a theme switch onto the stack.

```rust
pub fn set_theme_mode(mode: ThemeMode, cx: &mut App) {
    set_theme_mode_with_record(mode, true, cx);
}

pub fn set_theme_mode_with_record(mode: ThemeMode, record: bool, cx: &mut App) {
    let before = cx.theme().mode;
    gpui_component::Theme::change(mode, None, cx);
    if record {
        crate::undo_stack::record_theme_mode_change(before, mode, cx);
    }
    cx.refresh_windows();
}
```

The `record` flag is the control point. When the user clicks "Dark Mode" in the menu, `record` is `true` and the change is pushed onto the undo stack. When an undo operation restores the previous theme, `record` is `false` because the undo system itself is making the change. This prevents the undo from recording itself, which would create an infinite loop.

The `record_theme_mode_change` function also short-circuits if `before == after`. Toggling to the mode you are already on produces no entry.

### What happens inside record

1. Acquires `UndoState` as `default_global` (creates it if missing, though `initialize` already installed it).
2. Transfers ownership into `UndoModel`, the internal logic struct.
3. Checks `model.applying`. If `true`, returns immediately. This is the guard against recursive recording.
4. Pushes the new entry onto `past`.
5. **Clears `future`**. This is standard command-pattern behavior: once you perform a new action after an undo, the redo history is discarded.
6. Resets `last_rejected` to `None`.

## Undo and redo

```rust
undo_stack::undo(cx); // returns bool
undo_stack::redo(cx); // returns bool
```

Both return `true` if an entry was applied, `false` if the relevant stack was empty.

### Undo flow step by step

1. Pop the last entry from `past`. If empty, set `last_rejected` to `"nothing to undo"` and return `false`.
2. Set `applying = true`. This prevents `apply_inverse` from re-recording.
3. Call `apply_inverse(&entry.kind, cx)`. For `ThemeMode`, this calls `set_theme_mode_with_record(before, false, cx)`. The `false` flag means the theme change is applied without pushing a new undo entry.
4. Set `applying = false`.
5. Push the entry onto `future` (making it available for redo).
6. Return `true`.

Redo follows the same structure but in the opposite direction: pop from `future`, call `apply_forward`, push onto `past`.

### Apply functions

```rust
fn apply_inverse(kind: &UndoKind, cx: &mut App) {
    match kind {
        UndoKind::ThemeMode { before, .. } => {
            crate::app::set_theme_mode_with_record(*before, false, cx)
        }
    }
}

fn apply_forward(kind: &UndoKind, cx: &mut App) {
    match kind {
        UndoKind::ThemeMode { after, .. } => {
            crate::app::set_theme_mode_with_record(*after, false, cx)
        }
    }
}
```

Each variant maps to a single function call that applies the state change with recording disabled. When you add new `UndoKind` variants, you add new match arms here.

## Menu integration

The Edit menu renders Undo and Redo items. Their enabled state is driven by the command availability system.

```rust
// In menus.rs
MenuItem::action("Undo", ExecuteCommand(CommandId::Undo))
    .disabled(!commands::availability(CommandId::Undo, cx).enabled),
MenuItem::action("Redo", ExecuteCommand(CommandId::Redo))
    .disabled(!commands::availability(CommandId::Redo, cx).enabled),
```

The availability function checks the undo stack directly:

```rust
CommandId::Undo => CommandAvailability {
    enabled: crate::undo_stack::can_undo(cx).is_some(),
    disabled_reason: crate::undo_stack::can_undo(cx)
        .is_none()
        .then_some("No undo available".into()),
},
```

When the user selects Undo from the menu, the `ExecuteCommand(CommandId::Undo)` action fires. The command executor calls `undo_stack::undo(cx)` and discards the `bool` result (the menu will refresh on the next frame and disable itself if the stack is now empty).

The menu also observes `UndoState` as a GPUI global. Any mutation to the stack triggers `update_app_menu`, which rebuilds the menu bar with fresh enabled/disabled states. This keeps the Edit menu in sync without manual refresh calls.

```rust
cx.observe_global::<crate::undo_stack::UndoState>({
    let title = title.clone();
    let app_menu_bar = app_menu_bar.clone();
    move |cx| {
        update_app_menu(title.clone(), app_menu_bar.clone(), cx);
    }
}).detach();
```

## Command palette integration

Undo and Redo appear in the command palette (Cmd+K). Their availability is the same as the menu: `can_undo` / `can_redo` return `Some(String)` when an entry exists. When the stack is empty, the commands show as disabled with the reason "No undo available" or "No redo available".

To invoke from code:

```rust
commands::execute(CommandId::Undo, cx);
commands::execute(CommandId::Redo, cx);
```

See [Command Launcher](/docs/command-launcher/) for how the palette renders command availability.

## Reading the state

```rust
// Get a snapshot of the full state (cloned)
let state = undo_stack::snapshot(cx);

// Check if undo is available (returns the undo_label, or None)
if let Some(label) = undo_stack::can_undo(cx) {
    println!("Can undo: {}", label);
}

// Check redo availability
if let Some(label) = undo_stack::can_redo(cx) {
    println!("Can redo: {}", label);
}
```

These are read-only queries against the GPUI global. They are cheap (clone of a small struct or string) and safe to call from any context with access to `&App`.

## Extending with new actions

Adding a new reversible action requires changes in four places.

### Step 1: Add an UndoKind variant

In `src/services/undo_stack.rs`, extend the enum:

```rust
#[derive(Clone, Debug)]
pub enum UndoKind {
    ThemeMode { before: ThemeMode, after: ThemeMode },
    LocaleChange { before: String, after: String },
}
```

### Step 2: Add a record function

Following the pattern of `record_theme_mode_change`:

```rust
pub fn record_locale_change(before: String, after: String, cx: &mut App) {
    if before == after {
        return;
    }
    let state = cx.default_global::<UndoState>();
    let mut model = UndoModel::from_state(std::mem::take(state));
    model.record(UndoEntry {
        label: "Switch Language".to_string(),
        undo_label: "Undo Language Switch".to_string(),
        redo_label: "Redo Language Switch".to_string(),
        created_at: AppTimestamp::now(),
        kind: UndoKind::LocaleChange { before, after },
    });
    *state = model.into_state();
}
```

### Step 3: Handle apply and reverse

Add match arms in `apply_inverse` and `apply_forward`:

```rust
fn apply_inverse(kind: &UndoKind, cx: &mut App) {
    match kind {
        UndoKind::ThemeMode { before, .. } => {
            crate::app::set_theme_mode_with_record(*before, false, cx)
        }
        UndoKind::LocaleChange { before, .. } => {
            crate::app::set_locale_without_record(before, cx)
        }
    }
}

fn apply_forward(kind: &UndoKind, cx: &mut App) {
    match kind {
        UndoKind::ThemeMode { after, .. } => {
            crate::app::set_theme_mode_with_record(*after, false, cx)
        }
        UndoKind::LocaleChange { after, .. } => {
            crate::app::set_locale_without_record(after, cx)
        }
    }
}
```

The apply functions must call a variant of your setter that skips recording. Otherwise the undo itself gets recorded as a new action, and you get an infinite toggle loop.

### Step 4: Call the record function at the call site

Where the user action originates (menu handler, button click, command palette execution):

```rust
let before = current_locale(cx).to_string();
set_locale("zh-CN", cx);
undo_stack::record_locale_change(before, "zh-CN".to_string(), cx);
```

Or add a `record` parameter to your setter, like `set_theme_mode_with_record` does.

## Testing

The undo model has unit tests in `src/services/undo_stack.test.rs`. The tests cover the core invariants:

- Recording clears redo history. After a new action, any pending redo entries are discarded.
- Empty stack rejection. Calling `pop_undo` or `pop_redo` on an empty model returns `None` and sets `last_rejected` with a reason string.

```rust
#[test]
fn record_clears_redo_history() {
    let mut model = UndoModel {
        future: vec![sample_entry()],
        ..UndoModel::default()
    };
    model.record(sample_entry());
    assert_eq!(model.past.len(), 1);
    assert!(model.future.is_empty());
}
```

To test a new `UndoKind` variant, create a sample entry with your variant and verify that `record` / `pop_undo` / `push_redo` behave correctly. For integration tests that exercise `apply_inverse` and `apply_forward`, use the GPUI test harness described on the [Testing](/docs/testing/) page.

## Design constraints

Properties of the current implementation to be aware of:

- No size limit on stacks. `past` and `future` grow without bound. For most desktop apps this is fine because reversible actions are infrequent. If you add high-frequency actions (text editing, canvas drawing), add a cap to `UndoModel::record`.
- Single global scope. There is one undo stack for the entire app. There is no per-window or per-document scoping. If your app has multiple documents, you would need to key the stack by document ID.
- Synchronous apply. `apply_inverse` and `apply_forward` run synchronously on the main thread. If your reversible action involves async work (network calls, file I/O), you need to handle that outside the undo stack and only record the entry after the async work completes.
