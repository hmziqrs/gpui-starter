---
title: Documentation
description: Build production-ready desktop apps with Rust and GPUI. Themes, i18n, forms, SQLite, auto-updater, and more.
template: splash
hero:
  title: gpui-starter
  tagline: Build desktop apps with Rust and GPUI. Themes, i18n, forms, and launcher out of the box.
  actions:
    - text: Get Started
      link: /docs/getting-started/
      icon: right-arrow
    - text: GitHub
      link: https://github.com/hmziqrs/gpui-boilerplate
      icon: github
      variant: secondary
---

## What is gpui-starter

gpui-starter is a Rust boilerplate for [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the GPU-accelerated UI framework that powers the Zed editor. It ships with the things you would end up building anyway: multi-page navigation, 21 themes with hot-reload, i18n in English and Chinese, form validation, a Cmd+K command launcher, macOS system tray, SQLite persistence, secure storage via the OS keyring, auto-updater with Ed25519 signing, crash reporting, and accessibility through AccessKit.

You get a working app in `cargo run`, not a "hello world" label.

## Quick start

```bash
git clone https://github.com/hmziqrs/gpui-boilerplate.git gpui-app
cd gpui-app
cargo run
```

The app window opens with six pages: Home, Form, Settings, About, Diagnostics, and Notifications. Edit any page in `src/` and save to see changes reflected on rebuild. See the [getting started guide](/docs/getting-started/) for prerequisites and project structure.

## Feature overview

### Navigation and routing

Multi-page sidebar navigation with type-safe route definitions. Each route maps to a render function, and the sidebar highlights the active page automatically. See [routing](/docs/routing/) for the route registry pattern.

### Themes

21 built-in themes loaded from JSON files in `themes/`. Hot-reload watches the directory and applies changes without restarting. Catppuccin Mocha, Ayu Dark, One Dark, and 18 others ship by default. Read the [themes guide](/docs/themes/) for adding custom themes.

### Internationalization

English and Chinese (zh-CN) translations managed by es-fluent. Translation files live in `i18n/` and compile into typed constants, so a missing key is a compile error, not a runtime surprise. See [i18n configuration](/docs/i18n/) for the setup.

### Form validation

Built on gpui-form and koruma. Define validation rules as Rust structs and get inline error messages, field-level dirty tracking, and submit handling. The [forms documentation](/docs/forms/) covers the full API.

### Command launcher

Press **Cmd+K** to open a Spotlight-style search overlay. It fuzzy-matches across all registered actions and dispatches the selected one. You can register custom commands through the action registry. See the [command launcher docs](/docs/command-launcher/) for registration and customization.

### Data persistence

SQLite via rusqlite, embedded in-process. Migrations run on startup. The storage layer handles CRUD for settings, notification history, and app state. For sensitive data like tokens and credentials, the [secure storage](/docs/secure-storage/) module uses the OS keyring (macOS Keychain, Linux Secret Service, Windows Credential Manager).

### Auto-updater and crash reporting

The updater checks a remote endpoint for new versions, verifies the manifest with Ed25519 signatures, and prompts the user to install. Crash reports capture panic backtraces and write them to a local directory for diagnostics. See the [architecture overview](/docs/architecture/) for how both subsystems are wired.

### Notifications

Desktop notifications with inline history, dismiss/retry actions, and persistence across restarts. Covered in the [notifications guide](/docs/notifications/).

### Accessibility and testing

AccessKit provides screen reader support and semantic tree generation. The [testing guide](/docs/testing/) covers unit tests for views and integration patterns.

## Documentation pages

| Page | What it covers |
|------|---------------|
| [Getting Started](/docs/getting-started/) | Prerequisites, quick start, project structure |
| [Architecture](/docs/architecture/) | GPUI API patterns, entity management, globals, subscriptions |
| [Routing](/docs/routing/) | Route registry, sidebar, active page tracking |
| [Themes](/docs/themes/) | Built-in themes, hot-reload, custom theme format |
| [i18n](/docs/i18n/) | es-fluent setup, adding languages, compile-time checks |
| [Forms](/docs/forms/) | Validation rules, error display, submit handling |
| [Command Launcher](/docs/command-launcher/) | Cmd+K overlay, fuzzy search, action registry |
| [Secure Storage](/docs/secure-storage/) | OS keyring integration, credential management |
| [Notifications](/docs/notifications/) | Desktop notifications, history, persistence |
| [Testing](/docs/testing/) | Unit and integration test patterns for GPUI views |
| [Performance](/docs/performance/) | Profiling, render optimization, frame-time debugging |

## Learn more from the blog

- [Hello World GPUI Tutorial](/blog/hello-world-gpui-tutorial/) if you are new to GPUI concepts
- [Scaling a GPUI Prototype to Production](/blog/scaling-gpui-prototype-to-production/) for architectural decisions at scale
- [Theme System Deep Dive](/blog/theme-system-deep-dive/) for how the theme engine works internally
- [Building a Command Launcher in GPUI](/blog/building-command-launcher-gpui/) for the full Cmd+K implementation walkthrough
