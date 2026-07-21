---
title: "Getting started with Rust desktop development in 2026"
description: "A beginner guide to building desktop applications with Rust in 2026: tooling, frameworks, and building your first app."
date: 2026-06-05
tags: [Rust, desktop, beginner]
draft: false
---

If you want to build a desktop app in 2026 and you are not already committed to Electron, Rust deserves a serious look. The language has matured, the tooling is solid, and there are now multiple UI frameworks that can ship real applications. I have been writing Rust desktop apps for over a year, and the experience today is unrecognizable compared to where things stood in 2024.

This post covers the practical side of getting started: what to install, which framework to pick, how to structure a project, and what trips people up when they make the switch from web or C++ development.

## Why Rust for desktop apps

Three things make Rust a good fit for desktop development. First, memory safety without garbage collection. Desktop apps run for hours or days. A memory leak or use-after-free bug that might go unnoticed in a short-lived HTTP handler becomes a real problem when your app sits in the background all day. Rust eliminates entire classes of these bugs at compile time.

Second, performance is predictable. There is no garbage collector pause, no JIT warmup period. Your app's first frame renders as fast as its thousandth. Users notice this. A desktop app that opens instantly and responds to every click within a few milliseconds feels different from one that has occasional stutters.

Third, the Rust toolchain is genuinely good. Cargo handles dependency management, building, testing, and cross-compilation from a single config file. You do not need to learn a separate build system like CMake or Gradle. One `Cargo.toml` file does the job.

## Setting up your environment

Install Rust with rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

That gives you `rustc`, `cargo`, and `rustup`. On macOS, you also need Xcode Command Line Tools (`xcode-select --install`). Some frameworks need platform-specific libraries: Tauri needs `webkit2gtk` on Linux, GPUI needs Metal support on macOS.

For editing, pretty much any editor works. VS Code with the rust-analyzer extension is the most common setup. JetBrains RustRover is another solid option if you prefer an IDE experience. I use Neovim with rust-analyzer and it handles everything fine.

Verify the install works:

```bash
cargo new hello-desktop && cd hello-desktop
cargo run
```

You should see "Hello, world!" printed to the terminal. If that works, the toolchain is ready.

## Picking a framework

This is where most people get stuck, because there are several options and they serve different needs. I wrote a longer comparison in my [Rust GUI frameworks guide](/blog/rust-gui-frameworks-comparison-2026/), but here is the short version:

**Tauri** if you want to use web technologies (HTML, CSS, JavaScript) for the frontend and Rust for the backend. It uses the operating system's native WebView instead of bundling Chromium, so binaries stay small (3-5 MB) and RAM usage is low (30-60 MB at idle). This is the most mature option and the easiest to adopt if your team already knows web development. I cover Tauri alongside other options in my [Electron alternatives post](/blog/electron-alternatives-2026/).

**GPUI** if you want everything in Rust, no web view, no JavaScript, no compromises on native feel. GPUI is a GPU-accelerated UI framework built by the Zed editor team. It renders directly through Metal on macOS and Vulkan on Linux. The result is fast: 60+ fps with complex layouts, and the entire app is one Rust codebase. The tradeoff is that GPUI is newer, macOS-only for now (Linux support is progressing), and has a smaller community. I wrote a full [getting started with GPUI](/blog/getting-started-with-gpui/) walkthrough if you want to go deeper.

**egui** if you need something fast to build and do not care about a polished visual design. egui is immediate mode: you describe the UI every frame, and the framework handles the rest. Great for developer tools and internal utilities. Not great for consumer-facing applications where custom styling matters.

**iced** if you like the Elm architecture. State, update, view. Messages flow through a pure update function. Clean and predictable, but the ecosystem is smaller than egui's.

For the rest of this post, I will focus on building with GPUI since it gives the most "Rust-native" desktop experience and is the framework I use day to day.

## Building your first GPUI app

Create a new project and add GPUI as a dependency:

```bash
cargo new my-desktop-app
cd my-desktop-app
```

Then edit `Cargo.toml`:

```toml
[package]
name = "my-desktop-app"
version = "0.1.0"
edition = "2021"

[dependencies]
gpui = { git = "https://github.com/zed-industries/zed" }
```

Now replace `src/main.rs` with a minimal app:

```rust
use gpui::*;

struct HelloWorld;

impl Render for HelloWorld {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .bg(rgb("#1e1e2e"))
            .text_xl()
            .text_color(rgb("#cdd6f4"))
            .child("Hello from Rust desktop!")
    }
}

fn main() {
    App::new().run(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(800.0), px(600.0)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("My Desktop App".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_cx, cx| cx.new_view(|_cx| HelloWorld),
        ).unwrap();
    });
}
```

Run it:

```bash
cargo run
```

You will see an 800x600 window with centered text on a dark background. No HTML, no CSS files, no JavaScript. Pure Rust. That `div()` builder pattern is how you build every UI element in GPUI: chain methods to set layout, colors, padding, and children.

## How GPUI actually works

The core idea is simple. You have Rust structs that implement the `Render` trait. The `render` method describes what the UI should look like given the current state. When state changes, you call `cx.notify()` on the view's context, and GPUI re-renders that view.

There is no virtual DOM diffing. GPUI uses a retained-mode architecture. It holds onto the UI tree between frames and updates only the parts that changed. This is the same approach SwiftUI and Flutter use, and it avoids the cost of re-creating the entire tree on every state change.

State lives in your structs. A counter looks like this:

```rust
struct Counter {
    count: i32,
}

impl Counter {
    fn new(cx: &mut ViewContext<Self>) -> Self {
        Self { count: 0 }
    }

    fn increment(&mut self, cx: &mut ViewContext<Self>) {
        self.count += 1;
        cx.notify();
    }
}

impl Render for Counter {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .child(format!("Count: {}", self.count))
            .child(
                div()
                    .cursor_pointer()
                    .bg(rgb("#89b4fa"))
                    .rounded(px(8.0))
                    .px(px(16.0))
                    .py(px(8.0))
                    .child("Increment")
                    .on_click(cx.listener(|this, _event, cx| {
                        this.increment(cx);
                    })),
            )
    }
}
```

`cx.listener()` creates a callback that borrows `&mut Self` and the view context. This is how you wire up event handlers: button clicks, keyboard input, mouse movement, whatever you need.

## Common mistakes I see beginners make

**Ignoring async.** Desktop apps need to talk to networks, read files, and query databases. These are all async operations. GPUI has first-class async support through its entity system, but it takes a shift in thinking. You cannot just `.await` inside a `render` method. You spawn async tasks and update state when they complete. My [async patterns for Rust desktop](/blog/rust-desktop-async-patterns/) post covers this in detail.

**Fighting the layout system.** GPUI uses flexbox. If you have not used flexbox before, spend an hour learning it. CSS-Tricks has a good guide. Most layout problems in GPUI come from not understanding how `flex`, `items_center`, and `justify_center` interact. The [layout and style documentation](/docs/architecture/) covers GPUI specifics.

**Overcomplicating state.** GPUI does not need a state management library. Your structs are your state. You pass data down through method arguments and bubble events up through callbacks. If you find yourself reaching for a global store, you probably need to restructure your component hierarchy instead. That said, for genuinely shared state like user preferences or app config, GPUI's global system handles it. I wrote about this in my [state management with GPUI entities](/blog/state-management-gpui-entities/) post.

**Not profiling early enough.** Rust makes it easy to write fast code, but that does not mean all Rust code is fast. Cloning large structs on every render, doing expensive computation in `render()` methods, and blocking the main thread with synchronous I/O are all common performance traps. The [performance profiling guide](/blog/rust-desktop-performance-profiling/) has specifics on finding these issues.

## Project structure that scales

Once you move past a single-file app, you need some structure. After building several GPUI projects, this is what I settled on:

```
src/
  main.rs          # Entry point, window setup
  app.rs           # Root application state
  pages/
    home.rs        # Individual screens
    settings.rs
    dashboard.rs
  components/
    sidebar.rs     # Reusable UI pieces
    button.rs
    modal.rs
  services/
    database.rs    # Business logic, I/O
    network.rs
    auth.rs
  themes.rs        # Color definitions
  i18n.rs          # Localization strings
```

The `pages/` directory holds full-screen views. `components/` holds reusable pieces. `services/` handles everything that is not UI: database queries, HTTP requests, file I/O. This separation keeps the rendering code clean and makes testing business logic straightforward.

## What I wish I knew when I started

Rust's learning curve is real, especially around the borrow checker and lifetime annotations. For desktop development specifically, the main friction points are: understanding how `cx` (the context) flows through the component tree, getting comfortable with the entity system for async operations, and learning GPUI's styling API (which is different from CSS even though it uses similar concepts like flexbox).

The good news: once these concepts click, development moves fast. The compiler catches most mistakes before the app even runs. Refactoring is safe because the type system prevents you from breaking things silently. And the resulting binary is small, fast, and has no runtime dependencies.

If you want to skip the setup and start building immediately, [gpui-starter](/docs/getting-started/) gives you a working project with sidebar navigation, theme switching, i18n, form validation, SQLite persistence, and a command palette out of the box. The source is on [GitHub](https://github.com/freeoxide/gpui-starter) if you want to see how everything fits together before building your own.
