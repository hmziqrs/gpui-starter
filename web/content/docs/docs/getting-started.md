---
title: Getting Started
description: Clone, build, and run gpui-starter in under two minutes. Covers prerequisites, project structure, and first steps.
---

## Prerequisites

- **Rust** 1.85 or later ([install via rustup](https://rustup.rs)). The project uses Rust edition 2024.
- **macOS** is the primary target. **Linux** (X11 + Wayland) and **Windows** compile but may have gaps.
- On Linux you need X11 and Wayland development headers. On Ubuntu: `sudo apt install libx11-dev libwayland-dev`.

## Quick start

```bash
git clone https://github.com/freeoxide/gpui-starter.git gpui-app
cd gpui-app
cargo run
```

First build takes 2-5 minutes because GPUI compiles from source. Subsequent builds are incremental and finish in seconds.

When the window opens you will see a sidebar with six pages: **Home**, **Form**, **Settings**, **About**, **Diagnostics**, and **Notifications**. Press `Cmd+K` (or `Ctrl+K` on Linux/Windows) to open the command palette. Switch themes from the Settings page to see all 21 options.

## Project structure

The codebase is split into four layers: shell (UI chrome), features (pages), services (business logic), and platform (OS abstraction).

```
gpui-app/
├── Cargo.toml              # Workspace: app + gpui-query crates
├── build.rs                # es-fluent asset tracking
├── i18n.toml               # rust-i18n configuration
├── src/
│   ├── main.rs             # Entry point
│   ├── lib.rs              # Re-exports
│   ├── app/                # Lifecycle hooks
│   ├── shell/              # UI chrome
│   │   ├── root.rs         # AppRoot layout: title bar + sidebar + content
│   │   ├── sidebar.rs      # Page enum with titles and icons
│   │   ├── title_bar.rs    # Custom title bar with menus
│   │   ├── route.rs        # Page routing
│   │   └── app_menu.rs     # Native menu bar
│   ├── features/           # Pages and feature modules
│   │   ├── command_palette.rs  # Cmd+K command palette
│   │   └── pages/
│   │       ├── home.rs         # Welcome page
│   │       ├── form_page.rs    # Registration form with validation
│   │       ├── settings.rs     # Dark mode, language, notifications
│   │       ├── about.rs        # About page
│   │       ├── diagnostics.rs  # Runtime diagnostics
│   │       └── notifications.rs # Notification center
│   ├── services/           # Business logic
│   │   ├── i18n.rs         # es-fluent initialization
│   │   ├── notifications/  # Notification backend + inbox
│   │   ├── crash_report.rs # Panic report generation
│   │   ├── telemetry.rs    # Telemetry with consent gate
│   │   ├── undo_stack.rs   # Generic undo/redo
│   │   └── updater.rs      # Auto-update with Ed25519 signing
│   ├── platform/           # OS abstraction
│   │   ├── desktop_shell/  # Tray icon (macOS)
│   │   ├── credentials/    # OS keyring via keyring crate
│   │   ├── process/        # Single-instance lock
│   │   └── network/        # HTTP + WebSocket clients
│   ├── persistence/        # SQLite database + migrations
│   ├── state/              # Config store + state migrations
│   ├── ui/                 # Reusable components, widgets, forms
│   └── testing.rs          # Test utilities
├── themes/                 # 21 JSON theme files (hot-reloadable)
└── i18n/
    ├── en/                 # English translations
    └── zh-CN/              # Simplified Chinese translations
```

## What you get

| Feature | Details |
|---------|---------|
| Multi-page navigation | Sidebar with 6 pages, URL-style routing |
| 21 themes | Catppuccin, Dracula, Tokyo Night, Nord, and more. Hot-reload on file save. |
| i18n | English + Simplified Chinese via es-fluent. Add languages by dropping FTL files. |
| Form validation | gpui-form + koruma with localized error messages |
| Command launcher | Cmd+K / Ctrl+K spotlight search across all actions |
| macOS tray | Menu bar icon with global hotkey |
| SQLite persistence | Schema migrations, config store, crash reports |
| Secure storage | OS keyring integration for secrets |
| Auto-updater | Signed manifests with Ed25519 verification |
| Undo/redo | Generic undo stack wired into forms and settings |
| Single-instance | Second launch focuses the existing window |
| Crash reporting | Panic reports saved to disk, viewable in Diagnostics |
| AccessKit | Accessibility tree for screen readers |
| Telemetry | Optional, consent-gated, opt-in from Settings |

## Your first change

Open `src/features/pages/home.rs` and change the welcome text:

```rust
// src/features/pages/home.rs
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    div()
        .p_4()
        .child("Hello from gpui-starter!") // <- change this line
}
```

Save the file. If you used `cargo watch -x run` or `cargo run`, the recompile finishes in a few seconds and you see the updated text.

To add a new page, create a file in `src/features/pages/`, register it in the `Page` enum inside `src/shell/route.rs`, and add a sidebar entry. The [routing guide](/docs/routing/) walks through the full process.

## Next steps

- [Themes](/docs/themes/) - customize colors or add your own theme with a JSON file
- [Internationalization](/docs/i18n/) - add new languages with Fluent FTL files
- [Forms](/docs/forms/) - build validated forms with gpui-form and koruma
- [Architecture](/docs/architecture/) - how entities, globals, and subscriptions work in GPUI
- [Command launcher](/docs/command-launcher/) - wire custom actions into the Cmd+K palette
- [Testing](/docs/testing/) - write tests for views, services, and entities

New to GPUI itself? Read the [introduction to GPUI](/blog/getting-started-with-gpui/) on the blog, or compare it against other options in our [Rust GUI frameworks comparison](/blog/rust-gui-frameworks-comparison-2026/).
