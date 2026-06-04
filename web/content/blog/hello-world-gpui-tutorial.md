---
title: "Hello World in GPUI: your first Rust desktop app"
description: "A step-by-step tutorial to build a Hello World desktop app with GPUI. From cargo init to a working window with interactive UI."
date: 2026-06-05
tags: [GPUI, tutorial, beginner]
draft: false
---

So you want to write a desktop app in Rust. Not a web app wrapped in Electron, not a Tauri shell around HTML. A real native app, rendered on the GPU, with no JavaScript in sight. GPUI makes that possible, and this tutorial gets you from zero to a working window with interactive UI.

GPUI is the UI framework that powers [Zed](https://zed.dev), the code editor. The Zed team open-sourced it, and it has grown into a capable framework for building desktop applications. It runs on Metal (macOS) and Vulkan (Linux), renders at 60fps, and gives you a declarative API that feels more like SwiftUI than anything in the Rust ecosystem.

This post walks through every step: creating the project, adding the dependency, writing the app code, and understanding what each piece does.

## Prerequisites

You need Rust installed. If you don't have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

GPUI renders through the GPU. On macOS that means Metal, which ships with the OS. On Linux you need Vulkan drivers and the X11 or Wayland development headers. Check the [getting started guide](/docs/getting-started/) for platform-specific setup.

This tutorial assumes macOS, but the code works the same on Linux.

## Create the project

Start with a fresh Cargo project:

```bash
cargo init hello-gpui
cd hello-gpui
```

Open `Cargo.toml` and add GPUI as a dependency:

```toml
[dependencies]
gpui = { git = "https://github.com/zed-industries/zed" }
```

GPUI is not on crates.io yet. It lives in the Zed monorepo, so we pull it from GitHub. This is standard for the ecosystem right now. The API is stable enough for real applications, and the Zed team keeps the public surface well-documented.

## Write the entry point

Replace the contents of `src/main.rs` with this:

```rust
use gpui::*;

fn main() {
    App::new().run(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    size(px(480.0), px(320.0)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Hello GPUI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |cx| cx.new_view(|_cx| HelloWorld),
        )
        .expect("failed to open window");
    });
}
```

Here is what each part does.

`App::new()` creates the application instance. Think of it as the event loop and the GPU context bundled together. The `.run()` call starts the loop and gives you a closure with an `AppContext` (aliased as `cx`).

Inside that closure, `cx.open_window()` creates a new OS window. The `WindowOptions` struct controls the window size, title bar, and other native chrome. We center a 480 by 320 pixel window and set the title to "Hello GPUI".

The second argument to `open_window` is a closure that returns the root view. `cx.new_view()` creates a GPUI view from our `HelloWorld` struct. We define that struct next.

## Define the view

GPUI views are Rust structs that implement the `Render` trait. The struct holds your state, and `Render` describes what the UI looks like for a given state. Add this below `main`:

```rust
struct HelloWorld;

impl Render for HelloWorld {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .child(
                div()
                    .text_xl()
                    .text_color(gpui::black())
                    .child("Hello, GPUI!")
            )
    }
}
```

That is the entire component. No macros, no DSL, no code generation. Just a Rust struct and a method that returns a tree of elements.

`div()` creates a container element. The method chains are layout and style instructions: flexbox column layout, center everything, fill the available space. The `.child()` call nests another `div` inside with text content. If you know CSS flexbox, this reads naturally.

`IntoElement` is the trait that lets different types (div, text, custom components) live in the same tree. The `impl IntoElement` return type means you can return anything that implements it.

## Run it

```bash
cargo run
```

The first build takes a while because GPUI compiles from source. Subsequent builds are fast. When it finishes, you should see a small window with "Hello, GPUI!" centered on screen.

If you get a Metal or Vulkan error, your GPU drivers need attention. On macOS, make sure you are not running in a headless SSH session. On Linux, verify your Vulkan setup with `vulkaninfo`.

## Add interactivity

Static text is a start, but real apps respond to input. Let's add a button that counts clicks.

```rust
struct ClickCounter {
    count: usize,
}

impl ClickCounter {
    fn new() -> Self {
        Self { count: 0 }
    }
}

impl Render for ClickCounter {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .gap_4()
            .child(
                div()
                    .text_2xl()
                    .child(format!("Clicked {} times", self.count))
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(gpui::rgb(0x3b82f6))
                    .text_color(gpui::white())
                    .cursor_pointer()
                    .child("Click me")
                    .on_click(cx.listener(|this, _event, cx| {
                        this.count += 1;
                        cx.notify();
                    }))
            )
    }
}
```

Update `main` to use `ClickCounter` instead of `HelloWorld`:

```rust
|cx| cx.new_view(|_cx| ClickCounter::new()),
```

The important parts:

State lives in plain struct fields. `count` is just a `usize`. No observables or signals. When you need to update state, you mutate the field directly.

`cx.listener()` creates a callback closure. The first argument is `&mut Self`, giving you mutable access to the struct. The second is the event data (which we ignore with `_event`). The third is the view context.

`cx.notify()` tells GPUI this view needs to re-render. After the click handler mutates `self.count`, `cx.notify()` queues a re-render. GPUI calls `render` again, the new count appears in the UI, and the cycle continues.

This is the core data flow in GPUI: mutate state, call `cx.notify()`, render runs again. There is no virtual DOM diffing or reactivity graph. You decide when to re-render.

## How GPUI renders

Understanding what happens under the hood helps when you start building larger apps.

GPUI uses a retained-mode architecture. Your `render` method produces a tree of elements, and GPUI keeps that tree around between frames. When you call `cx.notify()`, GPUI walks the tree, finds what changed, and sends only the differences to the GPU. This is the same approach SwiftUI and Flutter use, and it is the reason GPUI can hit 60fps without you thinking about frame budgets.

The GPU rendering path goes through Metal on macOS and Vulkan on Linux. Text is rasterized by the framework. Layout is a custom implementation that handles flexbox, absolute positioning, and overflow. You never touch the GPU directly; the element tree is your abstraction layer.

For a deeper look at how the rendering pipeline works and how to profile it, see [the architecture docs](/docs/architecture/).

## Styling basics

GPUI uses method chaining for styles instead of CSS. Here is a card-like container with padding, a border, and a shadow:

```rust
div()
    .p_6()
    .rounded_lg()
    .border_1()
    .border_color(gpui::rgb(0xe5e7eb))
    .shadow_md()
    .bg(gpui::white())
    .child("Card content")
```

The naming follows Tailwind conventions. `p_6()` is padding, `rounded_lg()` is border radius, `shadow_md()` is a drop shadow. If you have used Tailwind, the mapping is direct.

Colors use `gpui::rgb()` with hex values or named constants like `gpui::black()` and `gpui::white()`. There is no CSS file, no style preprocessing, and no class name string concatenation. Styles are Rust code, which means the compiler checks them.

When your app grows past a few components, you will want a theme system instead of hardcoding colors everywhere. The [theme system overview](/docs/themes/) explains how to define reusable color tokens and switch between light and dark modes.

## What to try next

You have a working app. Some directions worth exploring:

GPUI supports opening multiple windows and managing a view stack for single-window apps with pages. The [routing documentation](/docs/routing/) covers this pattern.

Real apps fetch data, read files, and talk to databases. GPUI's `cx.spawn()` runs async tasks and feeds results back to the main thread. The [getting started guide](/docs/getting-started/) has a full example.

GPUI provides a test harness that renders views without a GPU, so you can write unit tests for your UI logic. See [testing GPUI apps](/blog/testing-gpui-applications/) for patterns.

## Starting from a real boilerplate

Writing everything from scratch teaches you the fundamentals, but production apps need more than a single file. Navigation, theme support, keyboard shortcuts, and persistence all require wiring that takes time to get right.

[gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) is a production-ready boilerplate that sets all of this up for you. It includes a sidebar with multi-page navigation, 21 themes with hot-reload, i18n with Fluent, form validation, a Cmd+K command palette, macOS tray integration, SQLite persistence, undo/redo, single-instance enforcement, and auto-updates with Ed25519 signing.

You can read the [getting started docs](/docs/getting-started/) to see what is included and how to customize it. Clone the repo, run `cargo run`, and start building on top of a working foundation instead of wiring up window management yourself.
