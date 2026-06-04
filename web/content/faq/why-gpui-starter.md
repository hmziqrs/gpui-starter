---
question: "Why should I use gpui-starter instead of building from scratch?"
description: "gpui-starter is a production-ready GPUI boilerplate with themes, i18n, forms, navigation, auto-update, and more so you skip weeks of wiring and ship your app faster."
category: "Getting Started"
order: 2
---

Starting a GPUI project from `cargo init` means solving the same problems everyone hits: page routing, theme switching, form validation, secure credential storage, auto-updates, i18n, and platform integrations like system tray. Each one is a side quest that pulls you away from your actual app. gpui-starter ships all of that working out of the box.

## What you get

The boilerplate includes every piece of infrastructure a desktop app needs before it does anything interesting:

- Multi-page navigation with a sidebar, route matching, and page lifecycle hooks. See [routing docs](/docs/routing) and the [architecture overview](/docs/architecture) for how it fits together.
- 21 built-in themes with live switching and hot reload during development. Add your own by editing a TOML file. Full details in [themes](/docs/themes) and the [theme system post](/blog/theme-system-deep-dive).
- i18n for English and Chinese using es-fluent and Fluent message files. Adding a language is one `.ftl` file. Covered in [i18n docs](/docs/i18n).
- Form validation via derive macros backed by the koruma validation crate. No hand-written match arms. See [forms](/docs/forms).
- Cmd+K command launcher with fuzzy search. Documented in [command launcher](/docs/command-launcher) and the [building a command launcher tutorial](/blog/building-command-launcher-gpui).
- macOS system tray with menu items and click handlers. See [system tray tutorial](/blog/system-tray-rust-macos-tutorial).
- Secure credential storage through the OS keychain via the `keyring` crate. Covered in [secure storage](/docs/secure-storage).
- SQLite persistence with rusqlite for local data. The [SQLite + Rust desktop tutorial](/blog/sqlite-rust-desktop-tutorial) walks through the patterns used.
- Auto-updater with Ed25519-signed manifests so users get updates without manual downloads.
- Custom titlebar, global hotkeys, single-instance enforcement, native notifications, undo/redo, AccessKit accessibility, crash reporting, and a diagnostics page.

## The philosophy: boilerplate, not framework

All the code lives in `src/`. There is no crate wrapping the decisions away from you. Every module is readable, every dependency is in your `Cargo.toml`, and every architectural choice is documented in the [architecture guide](/docs/architecture). When you need to change how routing works or swap out the storage layer, you edit the code directly. No plugin system, no abstraction tax.

This is the same approach the [Zed editor](https://zed.dev) takes with GPUI: own your rendering pipeline, own your state, own your build. The difference is that gpui-starter gives you a working starting point instead of a blank file.

## When it makes sense

Use gpui-starter when you want to build a native desktop app with GPUI and would rather spend your first week on features instead of infrastructure. If you are evaluating whether GPUI is the right choice at all, the [GPUI vs Tauri comparison](/blog/gpui-vs-tauri-framework) and the [Rust GUI frameworks comparison for 2026](/blog/rust-gui-frameworks-comparison-2026) cover the tradeoffs.

If you are coming from Electron or Tauri, the [migrating from Electron to Rust guide](/blog/migrating-electron-to-rust) maps the conceptual shifts.

## When it does not fit

If your app is a single-window utility with no navigation or settings, you probably do not need a boilerplate. A bare `cargo init` with the `gpui` crate is lighter. gpui-starter adds value when you would otherwise reinvent the pieces listed above.

## Get started

```bash
git clone https://github.com/hmziqrs/gpui-boilerplate.git my-app
cd my-app
cargo run
```

The [getting started guide](/docs/getting-started) covers the project structure and first steps. The [scaling a GPUI app to production](/blog/scaling-gpui-prototype-to-production) post covers what to think about once your app grows past the prototype stage.
