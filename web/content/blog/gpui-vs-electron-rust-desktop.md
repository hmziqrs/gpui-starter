---
title: "GPUI vs Electron: Rust vs JavaScript for desktop apps"
description: "An honest comparison of GPUI and Electron for desktop app development: binary size, memory, startup time, and rendering performance."
date: 2026-06-05
tags: [GPUI, Electron, comparison]
draft: false
---

I have shipped desktop applications with both Electron and native UI frameworks. Electron gets you from zero to working app fast. It also ships a full Chromium instance with every install and burns through RAM like it is free. GPUI takes the opposite approach: you write Rust, the framework renders on the GPU, and the resulting binary is small and fast. Neither is universally better. The right choice depends on what you are building, who your users are, and how much complexity you are willing to absorb.

This post compares GPUI and Electron across the dimensions that actually matter: binary size, memory usage, startup time, rendering architecture, developer experience, and ecosystem maturity. I will use real numbers wherever possible.

## What GPUI and Electron actually are

Electron wraps Chromium and Node.js into a single runtime. Your app runs as a web page inside a browser process, with Node.js APIs available for filesystem access, child processes, and native OS integration. You write JavaScript or TypeScript, style with CSS, and use whatever frontend framework you like. VS Code, Slack, Discord, and Figma all use Electron.

GPUI is a GPU-accelerated UI framework written in Rust. It was built by the [Zed](https://zed.dev) team to power their code editor and is now available as a standalone crate. You write Rust code that describes a tree of UI elements. The framework diffs that tree across frames and renders only what changed, using your GPU through Metal (macOS) or Vulkan. There is no browser, no JavaScript runtime, no HTML, and no CSS.

```rust
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .p_4()
        .gap_2()
        .child(
            h1("Hello, GPUI")
                .text_2xl()
                .text_color(gpui::white())
        )
        .child(
            Button::new("click-me", "Click me")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.count += 1;
                    cx.notify();
                }))
        )
}
```

That is a complete component. No JSX, no virtual DOM diffing, no build step. Rust compiles it, the GPU renders it.

## Binary size and memory

A minimal Electron app bundles Chromium and Node.js. That gives you a baseline of roughly 130-180 MB for the installed application, before you add any of your own code. A GPUI app compiles to a single native binary. The "hello world" equivalent is around 3-5 MB. Real apps with dependencies land somewhere between 8 and 20 MB.

Memory tells a similar story. Electron's renderer process alone typically consumes 150-300 MB of RAM for a moderately complex UI. Add the main process, GPU process, and utility processes, and you are looking at 400-600 MB total for an app that does not feel particularly heavy. GPUI apps routinely sit at 30-80 MB for comparable UI complexity because there is no browser engine in memory. The Zed editor itself, which is a full-featured IDE, idles around 100-150 MB.

Startup time follows the same pattern. Electron needs to spin up a Node.js runtime, launch Chromium, parse your HTML/JS bundle, and paint the first frame. On a modern MacBook, that is 1.5-3 seconds for a cold start. GPUI apps typically reach their first painted frame in 200-500 milliseconds because there is no runtime to initialize. Your code is already compiled. The GPU context is the only setup cost.

## Rendering architecture

Electron renders through Chromium's compositor, which ultimately talks to your GPU via ANGLE or native OpenGL. This works well. Chromium has had decades of optimization for web rendering. But you are paying for abstractions designed for the web: the CSS box model, the DOM tree, JavaScript garbage collection pauses, layout recalculation on every style change.

GPUI renders directly through the graphics API. On macOS that is Metal, on Linux it is Vulkan. The framework maintains a retained element tree. When state changes, you call `cx.notify()` and GPUI re-renders the affected component. It then diffs the element tree, updates only the changed fragments, and batches GPU draw calls. There is no intermediate DOM, no style recalculation pass, no garbage collector.

For most applications, both approaches are fast enough. The difference shows up in edge cases: large file trees, heavy scrolling lists, frequent state updates. GPUI's architecture gives you more headroom because there is less overhead between your state change and the pixels on screen.

If you want to understand how GPUI's element and rendering model works in detail, I wrote about it in the [getting started guide](/blog/getting-started-with-gpui/).

## Developer experience

Electron wins this category decisively. It lets you use the entire JavaScript ecosystem. React, Vue, Svelte, Tailwind, npm packages, TypeScript. If you have built anything for the web, you already know how to build an Electron app. The feedback loop is fast: hot reload works, DevTools work, you can inspect and debug your UI the same way you debug a website. VS Code is itself an Electron app, so you get good self-hosted tooling.

GPUI's developer experience is improving but still rough. You write Rust, which means compile times are a factor. Incremental builds take 2-5 seconds on a decent machine. There is no hot reload for UI code. Debugging is standard Rust debugging: `println!`, `tracing`, or a debugger. There is no equivalent of Chrome DevTools for inspecting a GPUI element tree at runtime.

The API itself is pleasant once you learn it. GPUI uses a declarative style with method chaining, and the type system catches a lot of mistakes at compile time. But the learning curve is real. You need to understand Rust's ownership model, GPUI's entity system, and the framework's approach to async state. I covered some of these patterns in the [state management with GPUI entities](/blog/state-management-gpui-entities/) post.

## Ecosystem and maturity

Electron has been around since 2013. It has thousands of Stack Overflow answers, mature plugins for every native integration you can think of, and a well-documented API. If you need to talk to Bluetooth, USB, system trays, or native menus, someone has already packaged it.

GPUI is young. The public API is still evolving. Documentation exists but is thin compared to Electron's. The crate ecosystem around GPUI is small. You will likely end up writing your own bindings for OS APIs that Electron gives you for free.

That said, GPUI inherits everything from the Rust ecosystem. Need SQLite? Use rusqlite. Need HTTP? Use reqwest. Need cryptography? Use ring or ed25519-dalek. The Rust crate ecosystem is mature and high-quality. The gap is specifically in pre-built UI components and native OS integration layers.

For a practical look at what you can already build with GPUI, check out the [building desktop apps with Rust and GPUI](/blog/building-desktop-apps-rust-gpui/) post.

## When to pick Electron

Pick Electron when:

- Your team already knows JavaScript or TypeScript well.
- You need to ship something fast, and polished UI matters more than raw performance.
- You depend on npm packages that have no Rust equivalent.
- You need to run on Windows, macOS, and Linux with identical behavior.
- Your app is content-heavy (rendering web content, rich text, complex CSS layouts).

Electron's cross-platform story is mature. GPUI currently has first-class macOS support, good Linux support, and no Windows support yet. If Windows is a hard requirement, GPUI is not ready for you today.

## When to pick GPUI

Pick GPUI when:

- Memory usage and binary size are important (embedded contexts, performance-sensitive users).
- You prefer compile-time guarantees over runtime flexibility.
- Your app is tool-like: editors, dashboards, developer utilities, data viewers.
- You want a single compiled binary with no runtime dependencies.
- You are comfortable writing Rust and dealing with a younger framework's rough edges.

GPUI works well for applications where the UI is a means to an end. Code editors, file managers, database browsers, terminal emulators. The kind of software where developers are the primary users and performance is a feature.

## The middle ground

Some projects split the difference. Tauri uses a Rust backend with a web frontend, giving you smaller binaries than Electron while keeping the HTML/CSS/JS rendering model. It is a pragmatic choice if you want Rust on the backend but are not ready to commit to a fully native UI. The tradeoff is that you still carry a webview dependency, and the communication bridge between Rust and the web layer adds complexity.

## Getting started with GPUI

If you want to try GPUI without scaffolding everything from scratch, [gpui-starter](/docs/getting-started/) is a boilerplate project with multi-page navigation, 21 built-in themes, i18n, form validation, a command launcher, macOS tray integration, and SQLite persistence. It handles the common setup work so you can focus on building your actual application.

The full source is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate). Clone it, run `cargo run`, and you have a working GPUI desktop app in seconds.
