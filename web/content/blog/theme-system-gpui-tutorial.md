---
title: "Creating a theme system in GPUI with hot-reload"
description: "Tutorial: build a theme system for GPUI that reloads from JSON files at runtime without restarting the application."
date: 2026-06-05
tags: [GPUI, themes, tutorial]
draft: false
---

Most GUI frameworks treat themes as compile-time constants. You pick a color scheme, bake it into the binary, and ship it. If you want to add a theme, you cut a new release. That works fine for small apps. It gets painful once you have 20 themes and users who want to tweak colors without waiting for a build cycle.

GPUI (the GPU-accelerated UI framework from the Zed editor) takes a different approach. Its component library ships a theme registry that reads JSON files from disk and applies them at runtime. This covers how that system works, how to hook into it for hot-reload, and where the tradeoffs sit.

## The structure of a GPUI theme

A theme in GPUI is a JSON file containing a `ThemeSet`. Each set can hold multiple theme variants (for example, Tokyo Night ships three variants in one file). The top-level structure looks like this:

```json
{
  "name": "Tokyo",
  "author": "Folke Lemaitre",
  "themes": [
    {
      "name": "Tokyo Night",
      "mode": "dark",
      "colors": {
        "background": "#1a1b26",
        "foreground": "#c0caf5",
        "primary.background": "#7aa2f7",
        "border": "#292e42"
      }
    }
  ]
}
```

Each theme inside the set is a `ThemeConfig` with a name, a mode (`light` or `dark`), and a flat map of color tokens. The token names use dot notation (`primary.background`, `list.active.border`, `title_bar.border`) to describe where each color applies. There are around 80 tokens covering backgrounds, foregrounds, borders, chart colors, and syntax highlighting styles.

The color system has built-in fallback logic. If you omit `success.background`, it falls back to the `green` base color. If you omit `primary.hover.background`, it computes a blend of `primary` and `background` at a set opacity. This means a minimal theme file can be surprisingly short. You specify the colors you care about and let the system derive the rest.

## The ThemeRegistry: loading themes from disk

The `ThemeRegistry` is a GPUI global that holds every loaded theme. On startup, it loads a baked-in default theme from a `include_str!` macro, then reads any `.json` files from a themes directory on disk.

The loading logic, simplified:

```rust
fn reload(&mut self) -> Result<()> {
    let mut themes = vec![];

    if self.themes_dir.exists() {
        for entry in std::fs::read_dir(&self.themes_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = std::fs::read_to_string(&path)?;
                match serde_json::from_str::<ThemeSet>(&content) {
                    Ok(set) => themes.extend(set.themes),
                    Err(e) => tracing::error!("ignored invalid theme: {}, {}", path.display(), e),
                }
            }
        }
    }

    // Re-seed with defaults, then insert custom themes
    self.themes.clear();
    for theme in self.default_themes.values() {
        self.themes.insert(theme.name.clone(), Rc::new((*theme).clone()));
    }
    for theme in themes {
        if !self.themes.contains_key(&theme.name) {
            self.themes.insert(theme.name.clone(), Rc::new(theme));
        }
    }
    Ok(())
}
```

The registry is a `HashMap<SharedString, Rc<ThemeConfig>>`. Duplicate names are ignored: defaults win, then first-come-first-served among custom themes. This prevents user themes from accidentally overriding the built-in defaults, keeping the app in a known state even if someone drops a malformed file in the themes directory.

## Applying a theme to the running app

The `Theme` struct is another GPUI global. It holds the resolved color values (as `Hsla`), the current mode, font settings, border radii, and a reference to the active `ThemeConfig`. When you call `Theme::change(mode, window, cx)`, it looks up the right config from the registry and calls `apply_config`:

```rust
pub fn apply_config(&mut self, config: &Rc<ThemeConfig>) {
    let default_colors = if config.mode.is_dark() {
        ThemeColor::dark()
    } else {
        ThemeColor::light()
    };

    self.colors.apply_config(&config, &default_colors);
    self.mode = config.mode;
}
```

The `apply_config` method on `ThemeColor` runs through every token. For each one, it checks if the config provides a value. If yes and the hex parse succeeds, it uses that color. If no, it falls back to the default palette. Some tokens have computed fallbacks: `primary_hover` blends `primary` into `background` at 90% opacity, `danger` falls back to `red`, and so on.

This fallback chain is what makes the theme system practical. You do not need to specify 80 colors to ship a theme. Specify 10 or 15 core tokens and the rest derive themselves. The [theme schema documentation](/docs/themes/) lists every token and its fallback behavior.

## Hot-reload with filesystem watching

The registry sets up a `notify` watcher on the themes directory. When any file changes, it reloads all themes and notifies the rest of the app:

```rust
fn _watch_themes_dir(themes_dir: PathBuf, cx: &mut App) -> anyhow::Result<()> {
    let (tx, rx) = smol::channel::bounded(100);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = &res {
            match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(_)
                | notify::EventKind::Remove(_) => {
                    if let Err(err) = tx.send_blocking(res) {
                        tracing::error!("Failed to send theme event: {:?}", err);
                    }
                }
                _ => {}
            }
        }
    })?;

    cx.spawn(async move |cx| {
        watcher.watch(&themes_dir, notify::RecursiveMode::Recursive)?;
        while (rx.recv().await).is_ok() {
            _ = cx.update(Self::reload_themes);
        }
    }).detach();

    Ok(())
}
```

The watcher sends file events through a bounded channel. The async receiver calls `reload_themes` on each event, which re-reads every JSON file in the directory. After reloading, the registry triggers `cx.observe_global::<ThemeRegistry>` in its init function:

```rust
cx.observe_global::<ThemeRegistry>(|cx| {
    let mode = Theme::global(cx).mode;
    let name = Theme::global(cx).theme_name().clone();

    // Re-apply the active theme from the refreshed registry
    if let Some(theme) = ThemeRegistry::global(cx).themes().get(&name).cloned() {
        Theme::global_mut(cx).apply_config(&theme);
    }

    Theme::change(mode, None, cx);
    cx.refresh_windows();
}).detach();
```

You edit a JSON file in the themes directory, save it, and the app picks up the change within milliseconds. No recompile, no restart. Every open window repaints with the new colors.

## Tradeoffs

Filesystem watching is not free. The `notify` crate uses platform-native APIs (FSEvents on macOS, inotify on Linux, ReadDirectoryChangesW on Windows), which are efficient but not instant. On macOS you get sub-second latency. On Linux with a slow disk, it can lag. The current implementation re-reads every file on any change, rather than diffing. For 20 small JSON files this is negligible. If you had hundreds of themes, you would want to track which file changed and reload only that one.

The bounded channel (capacity 100) handles event coalescing. Rapid saves produce many events, but the receiver processes them one at a time. If the channel fills, excess events are silently dropped, which is fine: the next successful reload picks up the latest state.

Because `Theme` is a GPUI global, theme switching is synchronous on the main thread. A reload blocks rendering until all colors are parsed. In practice this takes under a millisecond even with 20 themes, but it is worth knowing if you plan to load themes from a network source instead of local files.

## Persisting the user's choice

Selecting a theme at runtime only matters if the selection survives a restart. In gpui-starter, the `AppConfig` struct stores the theme name:

```rust
pub struct AppConfig {
    pub theme: String,
    // ...other settings
}
```

When the user picks a theme, the app calls `update_config` which mutates the config and schedules a debounced save to disk (300ms cooldown, so rapid changes do not hammer the filesystem). On next launch, the saved theme name is looked up in the registry and applied before the first window opens.

You can read more about this pattern in the [configuration persistence guide](/docs/configuration/).

## Writing your own theme

To create a custom theme, drop a JSON file in the themes directory. A minimal example:

```json
{
  "name": "My Theme",
  "themes": [
    {
      "name": "My Dark",
      "mode": "dark",
      "colors": {
        "background": "#0d1117",
        "foreground": "#e6edf3",
        "primary.background": "#58a6ff",
        "border": "#30363d",
        "muted.background": "#161b22",
        "muted.foreground": "#8b949e"
      }
    }
  ]
}
```

Save it, and if your app is running with the watcher active, the new theme appears in the selector immediately. If the colors look wrong, edit the file and save again. The hot-reload cycle is fast enough to treat it like live CSS editing.

For a full list of tokens and their fallback chains, check the [theme token reference](/docs/themes/). The schema is also available as a JSON Schema file that you can plug into your editor for autocomplete.

## Wrapping up

The GPUI theme system combines a JSON-based theme format with generous fallbacks, a registry that loads themes from disk at runtime, and a filesystem watcher that reloads on change. The fallback logic means themes can be small. The registry decouples themes from compile time. The watcher closes the loop for live editing.

If you want to see this system running in a real app with 21 themes, a settings page, and undo/redo for theme switches, check out [gpui-starter on GitHub](https://github.com/freeoxide/gpui-starter) or follow the [getting started guide](/docs/getting-started/) to set up the project locally.
