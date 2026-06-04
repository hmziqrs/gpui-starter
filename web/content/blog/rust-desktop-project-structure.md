---
title: "Structuring a Rust desktop application project"
description: "How to organize a Rust desktop app project: modules, crates, file layout, and separation of concerns for maintainable long-term codebases."
date: 2026-06-05
tags: [Rust, architecture, guide]
draft: false
---

I have worked on enough Rust desktop projects to know that the file structure you pick in week one determines how painful month six is going to be. Rust's module system gives you a lot of rope. You can hang yourself with a single 3000-line `main.rs`, or you can split things into so many tiny crates that compilation takes forever and you spend half your time fixing version mismatches in `Cargo.toml`.

This post is about the middle ground. I will walk through how I structure Rust desktop apps, using concrete examples from a real GPUI project with ~130 source files and 11 top-level modules. The principles apply regardless of which GUI framework you use.

## Start with what your app actually does

Before you create a single directory, list the distinct responsibilities your app has. Not abstract concerns like "business logic" and "presentation." Actual things your code needs to do. For a typical desktop app, that list looks something like this:

- Start up, check if another instance is running, initialize the window
- Render a shell with sidebar, title bar, and content area
- Handle navigation between pages
- Persist user settings to disk
- Talk to the network (HTTP, WebSocket)
- Show notifications
- Manage undo/redo history
- Handle internationalization
- Report crashes and telemetry

Each of those becomes a module. Some become modules with sub-modules. The point is that the structure maps to things you can point at and explain to a new contributor in one sentence. "The `services` module holds background logic that doesn't render anything." That is clear. "The `utils` module holds helper functions" is not.

## The top-level layout

Here is the directory structure I use. This is from an actual project built with [GPUI](/blog/getting-started-with-gpui/), but the same split works for Tauri, Dioxus, or anything else running Rust on the desktop.

```
src/
  main.rs
  lib.rs
  app/
    lifecycle.rs
  shell/
    root.rs
    sidebar.rs
    title_bar.rs
    status_bar.rs
    route.rs
    menus.rs
    app_menu.rs
  features/
    command_palette.rs
    pages/
      home.rs
      settings.rs
      about.rs
      ...
  services/
    storage.rs
    notifications/
    i18n.rs
    telemetry.rs
    undo_stack.rs
    crash_report.rs
    ...
  state/
    config_store.rs
    migrations.rs
  persistence/
    sqlite/
      db_migrations.rs
  platform/
    credentials/
    desktop_shell/
    dialogs/
    filesystem/
    input/
    network/
    process/
  ui/
    components/
    widgets/
    forms/
    theme/
    helpers.rs
  foundation/
    errors.rs
    ids.rs
    time.rs
    validation.rs
  runtime/
    events.rs
    capabilities.rs
```

That is 11 top-level modules. Let me explain the reasoning behind each one and the boundaries between them.

## `main.rs` and `lib.rs`: the entry points

`main.rs` should be short. Mine is 28 lines. It does three things: checks if another instance is already running (single-instance guard), sets up the GPUI runtime with assets, and calls `app::init(cx)`. That is it.

```rust
fn main() {
    let preflight = single_instance::preflight();
    if !preflight.should_start {
        return;
    }

    let app_runtime = gpui_platform::application().with_assets(Assets);
    app_runtime.run(move |cx| {
        app::init(cx);
        cx.activate(true);
        app::create_new_window("My App", cx);
    });
}
```

All the real initialization lives in `lib.rs`, which declares every module and re-exports the public API. The reason `main.rs` stays thin is that tests, benchmarks, and integration tests import from `lib.rs`. If you cram everything into `main.rs`, you cannot test any of it from outside.

```rust
// lib.rs
pub mod app;
pub mod shell;
pub mod features;
pub mod services;
pub mod state;
pub mod persistence;
pub mod platform;
pub mod ui;
pub mod foundation;
pub mod runtime;
```

## `shell/`: the chrome around your content

The shell module owns everything the user sees regardless of which page they are on. Title bar, sidebar, status bar, menus, and the root container that holds it all together. This is separate from `features/pages/` because the shell does not contain feature logic. It renders the frame.

Routing belongs here too. The `route.rs` file defines which page is active and how navigation works. Keeping it in `shell/` makes sense because the shell is what switches content in and out. See the [sidebar navigation tutorial](/blog/sidebar-navigation-gpui-tutorial/) for a deeper look at how this works in practice.

## `features/`: self-contained user-facing features

This is where individual pages and feature modules live. Each page is its own file. The `command_palette.rs` is a feature, not a page, because it is a floating overlay that can appear anywhere.

The rule for this module: nothing in `features/` imports from other features. If the settings page needs to read state that the home page also uses, that state lives in `services/` or `state/`. Features can depend on services and state, but never on each other. This prevents the tangled web that kills larger projects.

## `services/`: background logic with no UI

Services handle all the non-UI logic in the app. They manage storage, notifications, telemetry, i18n, crash reporting, undo history, HTTP clients, and anything else that runs behind the scenes. A service has no render method. It does not build UI elements. It exposes an API that features and the shell can call.

```rust
// services/notifications/mod.rs
pub mod backend;
pub mod inbox;
pub mod service;

// The service struct holds state but never renders anything.
// Features call into it to post notifications.
// The shell reads from the inbox to show a badge.
```

The [state management patterns](/blog/rust-desktop-state-management/) article goes deeper into how services interact with GPUI's entity system. The short version: services are typically `Global` or singleton `Entity<T>` values registered during `app::init`.

## `state/` and `persistence/`: data that outlives a session

I split data concerns into two modules. `state/` holds the in-memory configuration store. This is the thing the app reads and writes to during normal operation. It knows nothing about SQLite or files. It is just a Rust struct with getters and setters, wrapped in a GPUI entity.

`persistence/` handles serialization and storage. SQLite migrations live here. When the app starts, `persistence` loads data from disk and feeds it into `state`. When state changes, `persistence` writes it back.

This separation means you can swap SQLite for TOML files or a remote API without touching any code in `state/` or `features/`. The persistence layer is an implementation detail behind a trait.

## `platform/`: OS-specific abstractions

Anything that talks to the operating system goes here. File system access, credential storage via the OS keyring, native dialogs, network stack, process management, and the desktop shell integration (tray icon, window management). Each sub-module wraps a platform API behind a Rust interface so the rest of the app does not need to know whether it is running on macOS, Linux, or Windows.

```rust
// platform/credentials/mod.rs
pub fn store(key: &str, value: &str) -> anyhow::Result<()> {
    // Delegates to keyring crate on macOS, secret-service on Linux
}

pub fn retrieve(key: &str) -> anyhow::Result<String> {
    // Same delegation
}
```

When you need to add a new platform dependency, you add one file here. The [secure credential storage](/blog/secure-credential-storage/) post covers this pattern in detail. Keeping platform code isolated makes cross-platform ports far less painful.

## `ui/`: reusable rendering primitives

Components, widgets, form helpers, and theme definitions. Nothing in this module knows about app-specific features. A `Button` widget does not know what a settings page is. A form component does not know what fields the user is editing. These are pure rendering building blocks.

The `theme/` sub-module holds color palettes, spacing constants, and the hot-reload theme system. Read the [theme system deep dive](/blog/theme-system-deep-dive/) for how that works.

## `foundation/` and `runtime/`: shared utilities and events

`foundation/` is for small, dependency-free utilities: error types, ID generation, time formatting, input validation. These are the kinds of things that would otherwise end up in a `utils.rs` file that grows without bound. The rule here is strict: nothing in `foundation/` imports from any other app module. It is a leaf dependency.

`runtime/` holds the event system and capability checks. The event bus lets any part of the app emit and listen for app-wide events (`AppEventKind::DeepLinkReceived`, `AppEventKind::ThemeChanged`, etc.) without creating circular dependencies between modules.

## When to split into workspace crates

The project I described above is a single crate with 11 modules. That works well up to about 150-200 source files. Beyond that, compile times start to hurt because Cargo rechecks the entire crate on every change.

The fix is to split leaf modules into workspace crates. You can see this in the `Cargo.toml`:

```toml
[workspace]
members = [".", "crates/gpui-query", "crates/gpui-query-v2"]
```

The async query library (`gpui-query`) started as code inside `services/`. Once it matured and had a clear API boundary, I moved it to its own crate. Now it compiles independently, has its own test suite, and could be published to crates.io if I wanted.

The rule of thumb: keep things in one crate until you feel compile time pain. Then extract modules that have zero app-specific dependencies into workspace crates. Do not pre-split. You will waste time managing crate boundaries that turn out to be wrong.

## Common mistakes I have made

Putting network code in feature modules. This creates a situation where the home page, the settings page, and the diagnostics page all import `reqwest` directly and each manage their own HTTP client. Move it to a service. One client, one retry policy, one place to add logging.

Making `state/` a dumping ground. If a struct does not survive across page transitions or get persisted, it does not belong in `state/`. Keep per-page state in the page module itself as a local entity.

Skipping `lib.rs`. If all your code lives in `main.rs`, you cannot write integration tests, and your binary cannot be used as a library by other tools. Always put the real code in `lib.rs`.

Over-abstracting too early. I once spent a week building a generic "plugin system" for a desktop app that ended up with three plugins. I should have started with direct module calls and waited until I actually had enough plugins to justify the abstraction.

## The test for whether your structure works

Open your project after not looking at it for three weeks. Pick a feature you want to add. Time how long it takes you to figure out where to put the new code and what to import. If it takes more than a couple of minutes, your module boundaries are wrong or your naming is unclear.

A good structure lets you answer "where does this belong?" in under 30 seconds. If you have to grep the codebase to find where similar things live, that is a signal that the organization does not match how you actually think about the app.

---

If you want to see this structure in a working project, [gpui-starter](/docs/getting-started/) is a boilerplate repository with all of these modules wired together: shell, features, services, state, persistence, platform, and the rest. It runs out of the box with `cargo run` and includes things like [form validation](/docs/forms/), [i18n](/docs/i18n/), and a [command launcher](/docs/command-launcher/) so you can see how each piece fits without building from zero. The full source is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).
