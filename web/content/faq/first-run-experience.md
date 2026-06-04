---
question: "How does the first-run setup and onboarding work?"
description: "gpui-starter detects fresh installs and shows a one-time setup panel for locale, notifications, and other defaults. Runs once after install or when the user resets app state."
category: "Features"
order: 15
---

gpui-starter detects when the app launches for the first time and shows an inline setup panel on the home page. The panel lets users pick a locale and toggle native notifications before dismissing it. The flow runs exactly once, then never appears again unless explicitly retriggered.

## How detection works

On startup, the app loads its persisted config from a JSON state file managed by the [`config_store`](/docs/architecture/) module. The `first_run_completed` field in `AppConfig` defaults to `false`. When no config file exists yet (a brand new install), the defaults are used, so the first-run flow activates automatically.

The three functions in `src/services/first_run.rs` are the entire public API:

- `is_pending(cx)` reads the flag from global config
- `complete(cx)` sets it to `true` and persists immediately via the debounced save pipeline
- `reset(cx)` sets it back to `false` for testing

There is no state machine or multi-step wizard. One boolean, one check.

## What the user sees

When `is_pending` returns `true`, the home page renders an inline setup panel above the normal content. The panel is 520px wide, bordered, and contains three controls:

1. Locale selection: two buttons for English and Simplified Chinese, styled with a selected state so the user can see which is active
2. Native notifications toggle: a switch that calls [`set_native_notifications_enabled`](/docs/notifications/) under the hood
3. Finish setup button: calls `first_run::complete`, which persists the flag and causes the panel to disappear on the next render

All changes (locale, notifications) take effect immediately, even before the user finishes setup. The "Finish setup" button only dismisses the panel. The user can also ignore it entirely and use the app normally.

## Where the config is stored

The `AppConfig` struct lives in `src/state/config_store.rs`. It serializes to a JSON file at the platform-specific config directory (managed by the `dirs` crate). The save pipeline uses atomic writes via `AtomicWriteFile` with a 300ms debounce. If the config file is corrupt on load, the app quarantines it as `.json.bad` and falls back to defaults, which means first-run activates again.

For a deeper look at how config persistence works, see the [configuration patterns](/blog/rust-desktop-configuration-patterns/) post and the [app lifecycle](/blog/rust-desktop-lifecycle-management/) walkthrough.

## Customizing the welcome panel

The setup panel lives in `src/features/pages/home.rs` inside the `Render` impl for `HomePage`. It is built with the same GPUI view primitives as the rest of the app. To add your own steps (theme selection, data import, keyboard shortcut walkthrough), add new children to the `v_flex()` block that renders when `first_run_pending` is `true`.

The config struct supports arbitrary new fields with `#[serde(default)]`, so you can add a `first_run_step: u32` or similar without breaking existing installs. New fields get their default value on load.

## Retriggering during development

The diagnostics page has a "Reset First-Run" button that calls `first_run::reset`. This sets the flag back to `false` and the setup panel reappears on the next home page render. You can also call `reset` from a debug action or a test:

```rust
crate::first_run::reset(cx);
```

The diagnostics page also shows "First Run Pending: Yes/No" in its status table, so you can verify the flag state without navigating to the home page.

## How this connects to the rest of the app

The first-run module is intentionally tiny because it delegates everything to existing systems. Locale changes go through the same [i18n pipeline](/docs/i18n/) used by settings. Notification toggles use the same [notification backend](/docs/notifications/) that runs during normal operation. Config persistence uses the same debounced save that handles every other setting. There is no parallel code path.

If you want to understand how the startup sequence loads config before the first render, the [architecture guide](/docs/architecture/) covers the initialization order. The [getting started tutorial](/blog/getting-started-with-gpui/) walks through the full app lifecycle from `main()` to first paint.

## Further reading

- The [state management patterns](/blog/rust-desktop-state-management/) post explains how `AppConfig` integrates with GPUI's global entity system.
- The [scaling GPUI to production](/blog/scaling-gpui-prototype-to-production/) post discusses how to extend first-run into a multi-step onboarding wizard for larger apps.
- The [notifications tutorial](/blog/notifications-rust-desktop-tutorial/) covers the native notification backend that the setup panel toggles.
- The [internationalization guide](/blog/internationalization-gpui-apps/) explains the Fluent-based locale system behind the locale selector.
