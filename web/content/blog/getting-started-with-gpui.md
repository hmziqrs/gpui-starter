---
title: "Getting started with GPUI: a new paradigm for Rust desktop apps"
description: "Build your first Rust desktop app with GPUI, Zed's GPU-accelerated UI framework. Covers setup, components, async state, and navigation in one walkthrough."
date: 2026-05-09
tags: [GPUI, Rust, desktop]
draft: false
---

GPUI is a GPU-accelerated UI framework built by the [Zed](https://zed.dev) team. It is fast and written entirely in Rust. If you have been looking for an alternative to Electron or Qt for desktop apps, GPUI is worth a look.

This post covers what GPUI is, why its design differs from most UI frameworks, and how to get a working app running in minutes. By the end you will understand how GPUI components work, how async state fits in, and where to go next.

## Why GPUI?

Most desktop frameworks render UI on the CPU. That is fine for simple forms, but it falls apart with complex layouts, animations, or large lists. The CPU already has enough general-purpose work to do. Offloading rendering to the GPU frees it up and gives you smooth frame rates without much effort on your part.

GPUI uses a retained-mode architecture. If you have used SwiftUI or Flutter's widget tree, the idea is similar: you describe your UI as a tree of elements, and the framework holds onto that tree between frames, updating only the parts that change. Immediate-mode frameworks like Dear ImGui re-draw the entire UI every frame. Retained mode tends to perform better for real applications because the framework can skip unchanged subtrees and batch GPU draw calls. The API is declarative. You write code that describes what the interface should look like given the current state, not step-by-step instructions for how to draw it. When state changes, you call `cx.notify()` and GPUI re-renders the affected component. There is no virtual DOM diffing or reactivity graph. Just Rust structs and method calls.

A few more things worth knowing: GPUI has first-class async support through its entity system, it ships with a built-in theme system, and there is zero JavaScript involved. Everything runs as native Rust.

## Setting up a new project

The fastest way to start is with the boilerplate project:

```bash
git clone https://github.com/hmziqrs/gpui-boilerplate.git
cd gpui-app
cargo run
```

You will see a working desktop app with a sidebar, multiple pages, theme switching, and i18n support. The boilerplate gives you enough structure to start building without having to wire up the window, event loop, and navigation from scratch.

GPUI requires Metal on macOS. Linux support is in progress using Vulkan. Windows is not yet supported. If you are on macOS, everything should work out of the box with a recent Rust toolchain.

## Project structure

A typical gpui-starter project looks like this:

```
src/
├── app.rs          # AppRoot: the main window
├── page/
│   ├── home.rs     # Individual pages
│   ├── settings.rs
│   └── mod.rs
├── theme/
│   ├── mod.rs      # 21 built-in themes
│   └── catppuccin.rs
├── i18n/
│   ├── en.ftl      # English strings
│   └── zh-CN.ftl   # Chinese strings
└── command.rs      # Cmd+K launcher
```

The `app.rs` file is the entry point. It creates the main window, sets up navigation, and initializes global state. Think of it as the root of your component tree.

The `page/` directory holds individual views. Each page is a Rust struct that implements `Render`. Navigation between pages is handled through GPUI's context system, which lets you push and pop views from a central stack. If you want the full breakdown of how routing works, see the [routing guide](/docs/docs/routing).

The `theme/` directory contains theme definitions. GPUI themes are just Rust modules that set colors, font sizes, border radii, and other visual tokens. The boilerplate ships with 21 themes including Catppuccin, Nord, and Solarized, so you can pick something that looks reasonable without designing from scratch.

The `i18n/` directory uses Fluent (`.ftl`) files for localization. If you do not need multiple languages you can ignore it, but the plumbing is there if your app needs it. Our [i18n tutorial](/blog/internationalization-gpui-apps) covers the setup in more detail.

The `command.rs` file implements a command palette triggered by Cmd+K. This is a pattern borrowed from VS Code and Zed itself. It reads well as a reference for how to wire up keyboard shortcuts and modal overlays in GPUI. For a deeper walkthrough of that pattern, see the [command palette tutorial](/blog/command-palette-rust-tutorial).

## Your first component

GPUI components are Rust structs that implement `Render`. Here is a simple counter:

```rust
struct Counter {
    count: usize,
}

impl Render for Counter {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(
                Button::new("increment", "Count +1")
                    .on_click(cx.listener(|this, _, cx| {
                        this.count += 1;
                        cx.notify();
                    }))
            )
            .child(format!("Count: {}", self.count))
    }
}
```

The component state lives in plain fields on the struct. The `render` method returns a tree of elements built with method chaining. `cx.listener` creates a callback that has mutable access to the component's state. After modifying state, you call `cx.notify()` to tell GPUI that this component needs to re-render.

There are no hooks or dependency arrays. The model is simple: your struct holds the state, `render` describes the UI for that state, and `cx.notify()` triggers a re-render when state changes.

## Async state and side effects

Real applications need to fetch data, read files, and do work that takes time. GPUI handles this through its entity system. You can spawn async tasks from a component and update state when they complete.

Here is an example that fetches data from an API and displays it:

```rust
struct UserProfile {
    user: Option<String>,
    loading: bool,
}

impl UserProfile {
    fn new(cx: &mut ViewContext<Self>) -> Self {
        let mut profile = Self {
            user: None,
            loading: true,
        };
        profile.fetch_user(cx);
        profile
    }

    fn fetch_user(&mut self, cx: &mut ViewContext<Self>) {
        self.loading = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let response = reqwest::get("https://api.example.com/user")
                .await
                .unwrap()
                .text()
                .await
                .unwrap();

            this.update(cx, |this, cx| {
                this.user = Some(response);
                this.loading = false;
                cx.notify();
            }).ok();
        }).detach();
    }
}

impl Render for UserProfile {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div().child(if self.loading {
            "Loading...".into_any_element()
        } else {
            div().child(format!("User: {}", self.user.as_deref().unwrap_or("Unknown")))
                .into_any_element()
        })
    }
}
```

The `cx.spawn` call runs an async block on GPUI's runtime. When the async work finishes, `this.update` gives you mutable access back on the main thread, where it is safe to modify state and call `cx.notify()`. This pattern keeps async work off the UI thread while avoiding locks or channels for state updates.

If you have used Swift's async/await with `@MainActor`, the model should feel familiar. GPUI guarantees that entity updates run on the main thread, so you never need to worry about concurrent access to your component state. For a deeper look at how entities work internally, see our post on the [GPUI entity system](/blog/gpui-entity-system-explained).

## Tradeoffs to keep in mind

GPUI is fast and the programming model is clean, but there are real constraints. Windows support does not exist yet. The ecosystem is young compared to Electron or Tauri, so you will find fewer third-party components and fewer Stack Overflow answers. The framework itself is still evolving, and breaking changes in upstream GPUI can require migration work.

For internal tools, developer utilities, and apps targeting macOS, those tradeoffs are manageable. For cross-platform consumer apps that need to ship on Windows today, you may want to wait or evaluate alternatives like [Tauri](/blog/gpui-vs-tauri-framework) first.

## What to try next

Once you have the boilerplate running and understand the basics of `Render` and `cx.notify()`, here are some next steps.

Add a new page to the `page/` directory. Copy `home.rs` as a starting point, implement `Render` with your own layout, and register the page in the router inside `app.rs`. This teaches you how navigation works and how components compose.

Pick a theme from the `theme/` directory and modify it. Change some colors, adjust spacing tokens, run the app, and see what happens. Theme changes are applied at runtime, so you get fast feedback.

Build a small data-driven view. Fetch something from a public API using the async pattern shown above, and render it in a list. This exercises the full cycle: async work, state update, re-render.

Read the [architecture guide](/docs/docs/architecture) to understand how GPUI's entity and context systems work under the hood. That will make the callback patterns and `cx.spawn` feel less magical and more like straightforward consequences of the design.

If you want to customize the look of your app, check out the [theme reference](/docs/docs/themes) for a full list of available tokens and how to define your own. And if you are deciding between GPUI and other Rust GUI options, our [GPUI vs egui comparison](/blog/gpui-vs-egui-rust-gui) is a good place to start.
