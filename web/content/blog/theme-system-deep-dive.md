---
title: "Deep dive: the 21-theme system"
description: "How gpui-starter's theme system works internally, from 10 semantic color tokens to runtime hot-reloading across 21 built-in themes."
date: 2026-05-03
tags: [GPUI, themes, design]
draft: false
---

gpui-starter ships 21 built-in themes. Not stubs or placeholders. Fully designed color schemes covering light, dark, and high-contrast modes, all switchable at runtime. This post walks through how the theme system actually works, from the token vocabulary down to the hot-reload mechanism.

If you just want to pick a theme and go, see the [themes documentation](/docs/themes/). This post is for anyone building custom themes or theme-aware components.

## Theme definition

Each theme is a Rust struct implementing the `Theme` trait:

```rust
pub struct CatppuccinMocha;

impl Theme for CatppuccinMocha {
    fn name(&self) -> &str { "Catppuccin Mocha" }
    fn colors(&self) -> ThemeColors {
        ThemeColors {
            background: rgb(0x1e1e2e),
            foreground: rgb(0xcdd6f4),
            accent: rgb(0x89b4fa),
            surface: rgb(0x313244),
            border: rgb(0x45475a),
            // ... more tokens
        }
    }
}
```

Every color in the app references a semantic token, never a raw hex value. Switching themes swaps every color at once. No orphaned values, no half-applied palettes.

## How ThemeColors works

`ThemeColors` is the central struct. It holds exactly ten semantic color fields, and every component in the framework reads from one of them. The struct is small by design. Fewer tokens means less ambiguity when you design a new theme, and it means every token actually gets used somewhere in the UI.

Here is what each field controls.

`background` is the base layer. It fills the window behind all other content. Pick something your eyes can tolerate staring at for eight hours.

`foreground` is the default text color. It sits on top of `background`, so contrast ratio matters here more than anywhere else. Get this pair wrong and the rest of the theme will not save you.

`accent` is the action color. Buttons, active tabs, selected items, links, and focus rings all pull from this token. A good accent is saturated enough to stand out against both `background` and `surface`, but not so loud that it fights with text for attention.

`surface` is the elevated-layer color. Sidebars, cards, panels, and popovers sit on `surface` instead of `background` to create visual depth. In most dark themes it is slightly lighter than `background`. In light themes it is usually white or near-white.

`border` draws lines between sections, around inputs, and under headers. It should be visible enough to define structure but muted enough that the UI does not look like a wireframe. Many themes set this to a low-opacity version of `foreground`.

`error`, `warning`, and `success` are status colors. Red, yellow, and green are the conventional picks, but the exact shades should harmonize with the rest of the palette. A bright cherry red that works on a neutral dark background can look garish on a warm-toned theme like Gruvbox.

`muted` is for secondary or inactive text. Placeholder text in inputs, timestamps, disabled labels. Usually a desaturated midpoint between `background` and `foreground`.

`text_dim` is similar to `muted` but reserved for text that needs to be readable while clearly secondary. Tooltips and metadata labels are the common cases.

This fixed vocabulary is intentional. It avoids the problem that plagues CSS-in-JS systems where you end up with thirty shades of gray and no guidance on which one to use where.

## The 21 themes

The built-in set covers the most popular editor palettes. Catppuccin in Latte, Frappe, Macchiato, and Mocha. Dracula with its purple accents on a dark base. Nord with an arctic blue palette that feels calm without being boring. Gruvbox in Dark and Material variants, both built around warm earth tones. Tokyo Night in Storm and Night variants, which lean into deep blues and purples. Solarized Dark and Light, using the amber-and-blue complement scheme. One Dark Pro. Rose Pine in Dawn, Moon, and the original. Everforest Dark. Kanagawa in Wave and Dragon variants. A Default Light and a Default Dark round out the list.

If your favorite palette is missing, adding it is straightforward (see the walkthrough below).

## Live hot-reloading

Themes switch at runtime through the command launcher (Cmd+K). The change is instant. No restart. This works because GPUI's rendering pipeline re-queries theme colors on every frame rather than caching them at startup.

```rust
fn switch_theme(cx: &mut AppContext, theme: &'static dyn Theme) {
    cx.set_global::<CurrentTheme>(CurrentTheme(theme));
    cx.refresh();
}
```

The `refresh()` call triggers a re-render of the entire view tree. Since GPUI is GPU-accelerated, this completes in a single frame with no visible flicker. You can cycle through all 21 themes in a few seconds and watch each one render cleanly.

This mechanism is worth understanding if you plan to build theme-aware components. `ThemeColors` is stored as a GPUI global, so any component reads the current theme at render time by calling `cx.theme()`. There is no subscription system, no event bus, no reactive primitive to wire up. You read the global, you use the colors, and GPUI handles the rest. In a retained-mode GPU UI framework, state changes propagate through the render loop without requiring the developer to manage invalidation manually.

## Creating a custom theme

Here is a walkthrough for building a complete theme, using GitHub's dark mode as inspiration.

First, create a new file at `src/theme/github_dark.rs`:

```rust
use crate::theme::{Theme, ThemeColors};
use gpui::rgb;

pub struct GitHubDark;

impl Theme for GitHubDark {
    fn name(&self) -> &str { "GitHub Dark" }

    fn colors(&self) -> ThemeColors {
        ThemeColors {
            background: rgb(0x0d1117),  // deep navy-black
            foreground: rgb(0xe6edf3),  // off-white
            accent: rgb(0x58a6ff),      // GitHub blue
            surface: rgb(0x161b22),     // slightly lighter than bg
            border: rgb(0x30363d),      // subtle gray-blue
            error: rgb(0xf85149),       // GitHub red
            warning: rgb(0xd29922),     // amber
            success: rgb(0x3fb950),     // GitHub green
            muted: rgb(0x484f58),       // mid gray for borders
            text_dim: rgb(0x8b949e),    // secondary text
        }
    }
}
```

Next, register the theme so the command launcher can find it. Open `src/theme/mod.rs`, add the module, and insert it into the registry:

```rust
mod github_dark;

pub fn register_all_themes(registry: &mut ThemeRegistry) {
    registry.register(github_dark::GitHubDark);
    // ... existing registrations
}
```

Run `cargo run`, open the command launcher with Cmd+K, type "GitHub Dark", and select it. The entire app switches in one frame.

When designing your own palette, start with `background` and `foreground` and aim for a contrast ratio above 7:1 if possible. Then pick `accent` to complement or pop against those two. `surface` should be close to `background` but distinguishable. `border` should be visible without being heavy. The status colors (`error`, `warning`, `success`) can follow convention. `muted` and `text_dim` fill in the gaps.

The [WebAIM contrast checker](https://webaim.org/resources/contrastchecker/) is a quick way to test ratios. GPUI renders text with subpixel antialiasing on macOS, so actual perceived contrast may be slightly higher than the raw math suggests, but the checker gives a reasonable baseline.

## Why fixed tokens matter

The fixed-token approach keeps themes composable and portable. You never wonder whether a component expects a raw color or a token, because it always expects a token. That is what makes it possible to ship 21 themes that all look correct without per-theme patch files or override styles.

Ten tokens will not cover every edge case, though. Some components need a color that does not map cleanly to any of the ten. In those cases I have found it better to add a new semantic token to the struct (and update all 21 themes) than to smuggle a raw hex value into component code. Adding the token upfront means theme number 22 works without extra fixes.

For the full token reference, see the [themes documentation](/docs/themes/). For more on how the command launcher integrates with theme switching, see the [Cmd+K launcher post](/blog/cmdk-launcher/).
