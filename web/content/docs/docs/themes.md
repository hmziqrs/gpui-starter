---
title: Themes
description: Configure and customize themes in gpui-starter with 40 built-in variants across 24 theme families, file-based hot-reload, and runtime switching via actions.
---

## How themes work

gpui-starter delegates all theme management to `gpui-component`'s `ThemeRegistry`. Each theme is a JSON file in the project's `themes/` directory. The registry loads every file at startup, watches the directory for changes, and applies new colors to all open windows without a restart.

The theme system is wired in `src/app/mod.rs` during app initialization. On launch, it calls `ThemeRegistry::watch_dir()` pointing at `{CARGO_MANIFEST_DIR}/themes`, then restores the persisted theme name from the app's config file. If you want to see how the full app wires together, check the [architecture overview](/docs/architecture).

## Built-in theme families

The `themes/` directory ships 24 JSON files that expand into **40 selectable variants** (some files define multiple modes). Here is every theme family and what you get from each:

| Family | Variants | Mode |
|--------|----------|------|
| Adventure | Adventure, Adventure Time | Dark |
| Alduin | Alduin | Dark |
| Asciinema | Asciinema | Dark |
| Ayu | Ayu Light, Ayu Dark | Both |
| Catppuccin | Latte, Frappe, Macchiato, Mocha | Light + Dark |
| Crush | Crush Light, Crush Dark | Both |
| Everforest | Everforest Light, Everforest Dark | Both |
| Fahrenheit | Fahrenheit | Dark |
| Flexoki | Flexoki Light, Flexoki Dark | Both |
| Gruvbox | Gruvbox Light, Gruvbox Dark | Both |
| Harper | Harper | Light |
| High Contrast | High Contrast Light, High Contrast Dark | Both |
| Hybrid | Hybrid Light, Hybrid Dark | Both |
| Jellybeans | Jellybeans | Dark |
| Kibble | Kibble | Dark |
| macOS Classic | macOS Classic Light, macOS Classic Dark | Both |
| Matrix | Matrix | Dark |
| Mellifluous | Mellifluous Light, Mellifluous Dark | Both |
| Molokai | Molokai Light, Molokai Dark | Both |
| Solarized | Solarized Light, Solarized Dark | Both |
| Spaceduck | Spaceduck | Dark |
| Tokyo Night | Tokyo Night, Tokyo Storm, Tokyo Moon | Dark |
| Twilight | Twilight | Dark |

You can switch between any of these from the app menu (Theme submenu) or programmatically with the `SwitchTheme` action covered below.

## Theme JSON structure

Each file in `themes/` defines one theme family. A family can contain multiple variants (light, dark, or several shades) inside the `"themes"` array. Here is the skeleton:

```json
{
  "name": "My Theme Family",
  "author": "Your Name",
  "url": "https://github.com/you/my-theme",
  "$schema": "https://github.com/longbridge/gpui-component/raw/refs/heads/main/.theme-schema.json",
  "themes": [
    {
      "name": "My Theme Dark",
      "mode": "dark",
      "colors": {
        "background": "#1a1b26",
        "foreground": "#c0caf5",
        "primary.background": "#7aa2f7",
        "primary.foreground": "#1a1b26",
        "border": "#292e42",
        "panel.background": "#292e42",
        "title_bar.background": "#161720",
        "title_bar.border": "#292e42",
        "muted.foreground": "#565f89",
        "base.red": "#f7768e",
        "base.green": "#9ece6a",
        "base.yellow": "#e0af68",
        "base.blue": "#7aa2f7",
        "base.cyan": "#7dcfff"
      },
      "highlight": {
        "editor.foreground": "#c0caf5",
        "editor.background": "#1a1b26",
        "syntax": {
          "keyword": { "color": "#f7768e" },
          "string": { "color": "#9ece6a" },
          "function": { "color": "#7aa2f7" },
          "comment": { "color": "#565f89", "font_style": "italic" }
        }
      }
    }
  ]
}
```

The `"colors"` block controls the component palette (backgrounds, borders, accent colors, status colors). The `"highlight"` block controls syntax highlighting and editor chrome. The `$schema` line gives you autocomplete and validation in editors like VS Code.

For a real-world example with every available token, open `themes/tokyonight.json` in the repo. It defines three variants (Night, Storm, Moon) with complete `colors`, `highlight`, and `syntax` sections.

## Adding a custom theme

1. Create a new `.json` file in the `themes/` directory.
2. Copy the structure from an existing theme file and change the colors, name, and mode.
3. Save the file. The app picks it up immediately via hot-reload.

```bash
# Copy an existing theme as a starting point
cp themes/tokyonight.json themes/my-theme.json

# Edit the name, colors, and highlight tokens
# The app reloads the theme the moment you save
```

No restart, no recompile. The `ThemeRegistry::watch_dir()` call detects file changes and reloads the JSON on the spot.

### Minimum viable theme

You do not need to fill in every token. The component system falls back to defaults for missing keys. A minimal theme only needs `name`, `mode`, and a handful of colors:

```json
{
  "name": "Minimal",
  "themes": [
    {
      "name": "Minimal Dark",
      "mode": "dark",
      "colors": {
        "background": "#111111",
        "foreground": "#eeeeee",
        "primary.background": "#4488ff",
        "primary.foreground": "#ffffff",
        "border": "#333333"
      }
    }
  ]
}
```

Missing tokens like `panel.background` or `title_bar.background` inherit from `background`. Missing `base.*` colors fall back to sensible defaults. Start minimal, then add tokens as you notice gaps in the UI.

## Hot-reload in detail

Theme files are watched via `ThemeRegistry::watch_dir()` during app init in `src/app/mod.rs`:

```rust
let persisted_theme = persisted.theme.clone();
if let Err(err) = gpui_component::ThemeRegistry::watch_dir(
    std::path::PathBuf::from(format!("{}/themes", env!("CARGO_MANIFEST_DIR"))),
    cx,
    move |cx| {
        if let Some(theme) = gpui_component::ThemeRegistry::global(cx)
            .themes()
            .get(persisted_theme.as_str())
            .cloned()
        {
            gpui_component::Theme::global_mut(cx).apply_config(&theme);
        }
    },
) {
    tracing::error!("Failed to watch themes directory: {}", err);
}
```

When a file changes on disk, the callback fires. If the changed file contains the user's current theme, the registry re-applies it and all windows refresh. If the changed file is a different theme, it gets loaded into the registry but does not interrupt the active theme.

During theme development, you can edit colors in your JSON file, hit save, and see the result in the running app within milliseconds. No build step involved.

## Runtime theme switching

### Switch to a named theme

Dispatch the `SwitchTheme` action with the exact theme name (must match the `"name"` field in the JSON):

```rust
use crate::app::SwitchTheme;
use gpui::SharedString;

// Switch to Tokyo Night
cx.dispatch_action(Box::new(SwitchTheme(SharedString::from("Tokyo Night"))));
```

The action handler looks up the theme in the global registry and applies it:

```rust
cx.on_action(|switch: &SwitchTheme, cx| {
    if let Some(config) = gpui_component::ThemeRegistry::global(cx)
        .themes()
        .get(&switch.0)
        .cloned()
    {
        gpui_component::Theme::global_mut(cx).apply_config(&config);
    }
    cx.refresh_windows();
});
```

### Toggle light/dark mode

Use `SwitchThemeMode` to flip between light and dark without changing the theme family:

```rust
use crate::app::SwitchThemeMode;
use gpui_component::ThemeMode;

// Switch to light mode
cx.dispatch_action(Box::new(SwitchThemeMode(ThemeMode::Light)));

// Switch to dark mode
cx.dispatch_action(Box::new(SwitchThemeMode(ThemeMode::Dark)));
```

Both actions are wired into the app menu under the Theme submenu, so end users can switch from the menu bar without writing code. The menu is built in `src/shell/menus.rs` using `ThemeRegistry::global(cx).sorted_themes()`.

## Theme persistence

The app observes the global `Theme` state and writes the current theme name and scrollbar preference to the config file on every change:

```rust
cx.observe_global::<gpui_component::Theme>(move |cx| {
    let theme_name = cx.theme().theme_name().to_string();
    let scrollbar_show = cx.theme().scrollbar_show;
    crate::app_state::update_config(cx, |config| {
        config.theme = theme_name;
        config.scrollbar_show = Some(scrollbar_show);
    });
})
.detach();
```

The config file lives at `{config_dir}/state.json` (resolved by the platform filesystem module) and looks like this:

```json
{
  "theme": "Tokyo Night",
  "scrollbar_show": "scrolloff"
}
```

On next launch, the app reads this file, finds the persisted theme name, and applies it before the first window renders. If the saved theme no longer exists (you deleted the JSON file), the registry falls back to the default theme silently.

## Theme-aware UI code

When building custom components, read colors from the active theme instead of hardcoding values. The `ActiveTheme` trait from `gpui_component` gives you access to the current palette:

```rust
use gpui_component::ActiveTheme;

fn render_my_panel(cx: &mut ViewContext<MyView>) -> impl IntoElement {
    let bg = cx.theme().colors.background;
    let fg = cx.theme().colors.foreground;
    let border = cx.theme().colors.border;

    div()
        .bg(bg)
        .text_color(fg)
        .border_1()
        .border_color(border)
        .p_4()
        .child("Theme-aware content")
}
```

Your components will update automatically when the user switches themes. To learn more about building UI components that respond to state changes, see the [getting started guide](/docs/getting-started).

## Related topics

- [Architecture overview](/docs/architecture) for how theme initialization fits into the full app lifecycle.
- [Command launcher](/docs/command-launcher) for wiring theme switching into the Cmd+K palette.
- [Theme system deep dive](/blog/theme-system-deep-dive) for the history and design decisions behind the theme registry.
- [Building desktop apps with Rust and GPUI](/blog/building-desktop-apps-rust-gpui) for more about the framework.
