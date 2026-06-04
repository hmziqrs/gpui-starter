---
question: "How do I customize themes and switch between dark and light mode?"
description: "gpui-starter ships 40 theme variants from 24 families as JSON files. Themes hot-reload from disk, switch instantly at runtime via the menu or Cmd+K, and persist across restarts."
category: "Features"
order: 5
---

Themes are JSON files in the `themes/` directory, loaded at startup by `gpui-component`'s `ThemeRegistry`. Every color in the app references a semantic token like `background`, `foreground`, or `primary.background` rather than raw hex values, so swapping one JSON file changes the entire UI.

## What ships in the box

24 theme families expand into 40 selectable variants covering dark, light, and high-contrast modes. The bundled families include Catppuccin (Latte, Frappe, Macchiato, Mocha), Tokyo Night (Night, Storm, Moon), Solarized (Light, Dark), Gruvbox (Light, Dark), Everforest (Light, Dark), Ayu (Light, Dark), High Contrast (Light, Dark), and 16 more. For the full table, see the [themes documentation](/docs/themes).

## Switching themes at runtime

You can change the active theme three ways, none of which require a restart:

- App menu: the Theme submenu lists every loaded variant, sorted alphabetically.
- Command launcher: open with Cmd+K, type a theme name, hit Enter. Takes effect in one frame.
- Code: dispatch the `SwitchTheme` action with the exact name from the JSON file, or use `SwitchThemeMode` to toggle light/dark without changing the family. Code samples are in the [themes docs](/docs/themes/#runtime-theme-switching).

The chosen theme name is written to `{config_dir}/state.json` on every switch and restored on next launch. If the saved theme file no longer exists, the registry falls back to the default silently.

## Adding or editing a theme

Create a `.json` file in `themes/`, fill in the required fields (`name`, `mode`, `colors`), and save. The registry watches the directory via `ThemeRegistry::watch_dir()` and loads new files within milliseconds. You only need five or six color tokens to start; missing tokens like `panel.background` inherit from `background`, and `base.*` status colors fall back to built-in defaults. Copy `themes/tokyonight.json` as a starting point since it exercises every available token.

The JSON schema at `.theme-schema.json` provides autocomplete and validation in VS Code and other editors that support `$schema`.

## Hot-reload during development

Because themes live on disk as JSON and the registry watches for file changes, you can edit colors in your theme file, hit save, and see the result in the running app immediately. No recompile, no restart. This works for both built-in and custom themes. The [theme system deep dive](/blog/theme-system-deep-dive) covers how the watch callback is wired and what happens when a changed file matches the active theme.

## Theme-aware component code

Use the `ActiveTheme` trait from `gpui_component` to read colors from the current palette in your own components:

```rust
use gpui_component::ActiveTheme;

fn render_my_panel(cx: &mut ViewContext<MyView>) -> impl IntoElement {
    div()
        .bg(cx.theme().colors.background)
        .text_color(cx.theme().colors.foreground)
        .border_color(cx.theme().colors.border)
        .child("Automatically follows the active theme")
}
```

Components built this way update on every theme switch without extra wiring. The [getting started guide](/docs/getting-started) shows this pattern in context.

## Further reading

- [Themes documentation](/docs/themes) for the complete JSON reference, all 40 variants, and the full code for `SwitchTheme`/`SwitchThemeMode`.
- [Architecture overview](/docs/architecture) for how theme initialization fits into the app lifecycle.
- [Theme system deep dive](/blog/theme-system-deep-dive) for design decisions behind the registry and token vocabulary.
- [Building desktop apps with Rust and GPUI](/blog/building-desktop-apps-rust-gpui) for a broader look at the framework.
