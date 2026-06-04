---
question: "How do I add or create a custom theme in gpui-starter?"
description: "Add a new theme by creating a JSON file in the themes/ directory with your color palette. The app hot-reloads it instantly with no recompile."
category: "Features"
order: 6
---

Themes are JSON files, not Rust code. You create a `.json` file in the `themes/` directory, and the app picks it up the moment you save. No recompile, no restart.

## The fastest way: copy an existing theme

Grab an existing theme and change the colors. Tokyo Night is a good starting point because it has multiple variants you can study.

```bash
cp themes/tokyonight.json themes/my-theme.json
```

Open the new file and change the `name`, `author`, and `colors`. Save it. The app reloads within milliseconds.

## Minimum viable theme file

You do not need to fill in every token. Missing keys fall back to defaults. Here is the smallest file that produces a working theme:

```json
{
  "name": "My Theme Family",
  "author": "Your Name",
  "themes": [
    {
      "name": "My Theme Dark",
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

Missing tokens like `panel.background` or `title_bar.background` inherit from `background`. Missing `base.*` colors (red, green, yellow, blue, cyan) fall back to sensible defaults. Start with this, then add tokens as you notice gaps in the UI.

## Full theme structure

A complete theme file looks like this. The `"colors"` block controls component colors. The `"highlight"` block controls syntax highlighting and editor chrome.

```json
{
  "name": "My Theme Family",
  "author": "Your Name",
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

The `$schema` line gives you autocomplete and validation in VS Code and other editors. For a real-world reference with every available token, open `themes/tokyonight.json` in the repo.

## Color tokens and what they control

| Token | What it affects |
|-------|----------------|
| `background` | Base layer behind all content |
| `foreground` | Default text color |
| `primary.background` | Accent color for buttons, links, active items, focus rings |
| `primary.foreground` | Text on top of the primary accent color |
| `border` | Lines between sections, input borders, header underlines |
| `panel.background` | Sidebar, card, and panel backgrounds |
| `title_bar.background` | Custom titlebar fill |
| `muted.foreground` | Secondary text, timestamps, placeholders |
| `base.red/green/yellow/blue/cyan` | Status and semantic colors |

When designing a palette, start with `background` and `foreground`. Aim for a contrast ratio above 7:1. Then pick `primary.background` as your accent. The [WebAIM contrast checker](https://webaim.org/resources/contrastchecker/) is a quick way to verify ratios. GPUI renders text with subpixel antialiasing on macOS, so perceived contrast is slightly higher than the raw numbers suggest, but the checker gives a solid baseline.

## Switching themes at runtime

After adding your JSON file, switch to it from the app menu (Theme submenu) or the command launcher (Cmd+K). Type the exact `"name"` from your JSON and select it. The change takes effect in one frame.

You can also switch programmatically:

```rust
use crate::app::SwitchTheme;
use gpui::SharedString;

cx.dispatch_action(Box::new(SwitchTheme(SharedString::from("My Theme Dark"))));
```

## Multiple variants in one file

One JSON file can define several variants (for example, a light and dark version of the same palette):

```json
{
  "name": "My Theme",
  "themes": [
    {
      "name": "My Theme Light",
      "mode": "light",
      "colors": { "background": "#ffffff", "foreground": "#1a1a1a", "..." : "..." }
    },
    {
      "name": "My Theme Dark",
      "mode": "dark",
      "colors": { "background": "#1a1a1a", "foreground": "#ffffff", "..." : "..." }
    }
  ]
}
```

Each variant appears as a separate entry in the theme switcher.

## Hot-reload during development

The `ThemeRegistry::watch_dir()` call in `src/app/mod.rs` watches the `themes/` directory for changes. When you save a JSON file:

- If it contains your current theme, the app re-applies it to all windows immediately.
- If it is a different theme, it gets loaded into the registry but does not interrupt what you are using.

This means you can tweak colors, hit save, and see the result in the running app within milliseconds. No build step.

## Theme-aware component code

When building your own components, read colors from the active theme instead of hardcoding hex values. The `ActiveTheme` trait from `gpui_component` provides access to the current palette:

```rust
use gpui_component::ActiveTheme;

fn render_my_panel(cx: &mut ViewContext<MyView>) -> impl IntoElement {
    let bg = cx.theme().colors.background;
    let fg = cx.theme().colors.foreground;

    div().bg(bg).text_color(fg).p_4().child("Theme-aware content")
}
```

Components that read from `cx.theme()` update automatically when the user switches themes. See the [themes documentation](/docs/themes/) for the full API reference.

## Related

- [Themes documentation](/docs/themes/) for the complete token reference, persistence, and runtime switching API.
- [Architecture overview](/docs/architecture/) for how theme initialization fits into the app lifecycle.
- [Command launcher](/docs/command-launcher/) for wiring theme switching into the Cmd+K palette.
- [Theme system deep dive](/blog/theme-system-deep-dive/) for the design decisions behind the registry and hot-reload mechanism.
- [Creating a theme system in GPUI with hot-reload](/blog/theme-system-gpui-tutorial/) for a step-by-step tutorial on building theme-aware components.
