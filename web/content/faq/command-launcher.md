---
question: "How do I use the Cmd+K command launcher in a GPUI app?"
description: "The Cmd+K command palette provides fuzzy search across all registered app actions: page navigation, theme switching, language changes, undo/redo, and custom commands. Covers built-in actions, keyboard shortcuts, and how to register your own commands."
category: "Features"
order: 9
---

The command launcher is a Spotlight-style search overlay. Press **Cmd+K** (macOS) or **Ctrl+K** (Linux/Windows) to open it. Pressing **/** also works. Start typing and the list filters in real time using case-insensitive substring matching against both the command title and subtitle.

## What you can do from the launcher

Every registered action in the app is searchable. Out of the box:

- **Navigate** to Home, Form, Settings, Notifications, Diagnostics, or About pages
- **Switch themes** by name (Light Mode, Dark Mode, plus any of the 21 bundled themes)
- **Run tasks** like the demo background task or a network connectivity check
- **Open folders** (logs, config) directly in the system file manager
- **Copy diagnostics** to clipboard
- **Undo/Redo** the last reversible action
- **Change language** between available locales (en, zh-CN)

Commands that depend on runtime conditions show as disabled with a reason. For example, Undo appears grayed out when the undo stack is empty, and Copy Diagnostics is unavailable when the clipboard backend is missing.

## Keyboard shortcuts inside the launcher

| Key | Action |
|-----|--------|
| Up / Down | Move through the filtered list (wraps around) |
| Enter | Execute the selected command and close the launcher |
| Escape | Dismiss without running anything |

## How search filtering works

The launcher performs case-insensitive substring matching. Type "light" and it matches "Light Mode". Type "set" and it matches both "Settings" (title) and "Open the Settings page" (subtitle). When the query is empty, all commands are shown.

## Adding your own command

Commands live in `src/commands.rs`. The process is four steps:

1. Add a variant to the `CommandId` enum
2. Add a `CommandSpec` entry to the `registry()` function with a title, subtitle, and icon
3. Optionally add an availability check in `availability()` for commands that depend on runtime state
4. Add a handler arm in `execute()`

```rust
// 1. Add to CommandId enum
pub enum CommandId {
    // ... existing variants
    ExportData,
}

// 2. Add to registry()
command(CommandId::ExportData, "Export Data", "Export current data to CSV", IconName::Download),

// 4. Handle in execute()
CommandId::ExportData => {
    crate::export::run_export(cx);
}
```

The launcher picks up new registry entries automatically. You never need to touch `launcher.rs` itself.

## Technical details

The launcher opens as a `WindowKind::PopUp` window (620x460px, frameless, blurred background) centered on the primary display. A `LauncherOpen` global prevents double-opening. When the user selects a command, a `LauncherEvent::Act` is dispatched through the parent `LauncherRoot` entity, which calls `commands::execute()` and closes the popup.

For the full implementation walkthrough, see the [command launcher docs](/docs/command-launcher/).

## Related

- [Command launcher documentation](/docs/command-launcher/) for architecture, search internals, and popup window configuration
- [Building a command palette in Rust with GPUI](/blog/command-palette-rust-tutorial/) for a step-by-step tutorial
- [Building a command launcher (Cmd+K) in GPUI](/blog/building-command-launcher-gpui/) for the cross-window event design
- [Theme system](/docs/themes/) for theme switching commands
- [Architecture overview](/docs/architecture/) for GPUI patterns used in the launcher
- [Keyboard shortcuts FAQ](/faq/keyboard-shortcuts/) for the full hotkey reference
