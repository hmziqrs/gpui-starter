---
title: "Why Rust + GPUI for desktop apps"
description: "Why Rust and GPUI beat Electron for desktop apps: lower memory, faster rendering, compile-time safety, and no bundled browser. A practical comparison."
date: 2026-05-05
tags: [Rust, desktop, architecture]
draft: false
---

Electron owns the desktop app space right now. Huge ecosystem, ships everywhere, and most developers already know the stack. But you pay for that convenience in binary bloat, memory hunger, and a constant tug-of-war with performance. That tradeoff gets worse as your app grows.

I think Rust + GPUI is a better foundation for the kinds of desktop apps people actually rely on every day.

## The problem with Electron

Every Electron app bundles a full Chromium instance. Hello World weighs in at 150MB+. Slack eats 1GB of RAM with a few workspaces open. VS Code, probably the best Electron app ever built, still chokes on large files and gets sluggish when you install enough extensions.

These are not edge cases. They are what happens when you run a web browser as your UI framework.

The numbers are hard to ignore. A minimal Electron app starts around 130MB on disk and sits at 80 to 120MB of RAM at idle. A native app doing the same thing? 5 to 15MB. Discord routinely hits 500MB. Notion hovers around 400MB for a single workspace. Figma's desktop client does real GPU rendering through WebGL, but it still carries an entire Chromium process underneath.

The root cause is redundancy. Every Electron window runs its own renderer process: V8 engine, DOM implementation, CSS parser, layout engine, compositor, networking stack. You are running a copy of Chrome for every window. On a machine with 8GB of RAM, three Electron apps can eat a quarter of available memory before the user does anything.

## Why not Tauri?

Tauri fixes the worst parts of Electron by using the system webview instead of Chromium. Binary size drops from hundreds of megabytes to single digits. Idle memory falls to 30 or 40MB. Real improvements.

But you are still writing HTML, CSS, and JavaScript. The same web stack that makes Electron apps feel slow on native interactions. WebView2 on Windows, WebKit on macOS, WebKitGTK on Linux. Each one has its own rendering quirks, its own JavaScript performance characteristics, its own bugs you get to work around. Cross-platform consistency becomes an exercise in wrangling three different browsers.

Tauri works well for simple apps. Tools that are basically forms with a native frame around them. Once you need complex layouts, real-time rendering, or heavy UI interactions, you hit the same webview performance ceiling. The DOM was not built for 60fps rendering of thousands of elements. CSS layout is fine for web pages but introduces unpredictable latency when you need tight frame budgets. JavaScript's single-threaded model means your UI logic and business logic fight for the same execution time.

I covered this comparison in more detail in my [Rust desktop frameworks overview](/blog/rust-desktop-frameworks-compared/).

## GPUI's rendering model

GPUI renders straight to the GPU. No webview, no native widget toolkit. Metal on macOS, Vulkan elsewhere, through a thin abstraction layer. There is no DOM, no CSS engine, no JavaScript runtime between your code and the pixels on screen.

The pipeline has three phases. Layout: GPUI computes sizes and positions with a flexbox-based system, similar to the web but without the legacy baggage. Prepaint: hitboxes get created, text runs get shaped, and the framework prepares spatial data for interaction. Paint: quads, text, and decorations get batched into GPU draw calls.

```rust
impl Element for CounterButton {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn request_layout(
        &mut self, _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window, cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = window.request_layout(
            Style {
                size: size(px(120.), px(36.)),
                ..default()
            },
            vec![],
            cx,
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self, _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>, _: &mut (),
        window: &mut Window, _: &mut App,
    ) -> Hitbox {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self, _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>, _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window, cx: &mut App,
    ) {
        window.paint_quad(paint_quad(
            bounds, Corners::all(px(6.)),
            cx.theme().background,
        ));
        window.on_mouse_event({
            let hitbox = hitbox.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if hitbox.is_hovered(window) && phase.bubble() {
                    cx.stop_propagation();
                }
            }
        });
    }
}
```

This direct pipeline gives you single-digit millisecond render times for complex UIs. No garbage collector pauses. No JIT warmup. No layout thrashing from a DOM designed for documents. Text rendering goes through proper font shaping pipelines. Memory usage stays predictable because there are no hidden allocation spikes from a browser engine deciding to cache something.

### What this feels like in practice

The first time I opened Zed after months in VS Code, the difference was immediate. File tree navigation felt instant. Search results rendered as fast as I could type. No micro-stutters, no delayed repaints. That is what direct GPU rendering buys you.

## The Rust advantage

Performance is the obvious sell for Rust. Correctness matters more, though, especially for software people rely on every day.

The type system eliminates entire classes of bugs at compile time. Null pointer exceptions, which account for a large share of production crashes in C++ and Java apps, cannot happen in Rust. The `Option<T>` type forces you to handle the absence of a value explicitly. The compiler will not let you forget.

```rust
fn load_config(path: &Path) -> Result<Config, AppError> {
    let raw = fs::read_to_string(path)
        .map_err(AppError::Io)?;
    let config: Config = toml::from_str(&raw)
        .map_err(AppError::Parse)?;
    // No null checks needed. No defensive coding.
    // If this function returns, the config is valid.
    Ok(config)
}
```

Data races, the source of some of the hardest bugs to reproduce and fix in concurrent code, are ruled out by the borrow checker. If your code compiles, it is data-race free. Not "probably." Not "if you followed the conventions." Guaranteed by the type system.

This matters for desktop apps. A crash in a background thread that corrupts user data is not an annoyance. It is a trust violation. Users do not care whether the bug was in your code or a third-party library. They just know your app ate their work. Rust's safety guarantees extend across your entire dependency tree because the same rules apply to every crate you pull in.

There is also the practical benefit of safe refactoring. In large codebases, the fear of breaking something undocumented drags on velocity. Rust turns that fear into a compile error. Change a function signature, and the compiler shows you every call site that needs updating. Reorder a struct field, and every pattern match that destructures it flags immediately. The feedback loop is tight and deterministic.

### No garbage collector

No runtime pause. No stop-the-world event. No unpredictable latency spike right when the user is dragging a slider or typing in a search box. Rust's ownership model means memory gets freed at deterministic, predictable points. For UI work this matters. Frame budgets are measured in single-digit milliseconds. A 10ms GC pause is the difference between smooth and stuttering.

## The honest tradeoffs

GPUI is newer and less mature than Electron or Qt. The ecosystem is smaller. The learning curve is steeper if you are not familiar with Rust, and steeper still if you are not used to building UI without a DOM.

Documentation is still growing. You will occasionally need to read source code to understand how something works. The community is active but small compared to the web development world, so Stack Overflow will not have answers for every question. If you are evaluating the full tradeoff matrix, the [architecture guide](/docs/architecture/) covers what gpui-starter handles out of the box versus what you would need to build yourself.

The fundamentals are solid, though. GPUI powers Zed, a production code editor used by thousands of developers daily. Code editors are among the most demanding desktop applications: large files, real-time syntax highlighting, multi-cursor editing, project-wide search, extension systems, terminal emulators, all running simultaneously without dropping frames. If GPUI handles that workload, it can handle most things you would throw at it.

## Getting started

If you want to try it, [gpui-starter](/docs/getting-started/) gives you a working desktop app in under five minutes. No configuration, no boilerplate. Just `cargo run`.

The starter includes multi-page navigation, 21 themes with hot-reload, i18n, form validation, a command palette, macOS tray integration, SQLite persistence, undo/redo, single-instance enforcement, notifications, auto-update with Ed25519 signing, and crash reporting. Enough to see how a real GPUI app hangs together without writing everything from scratch.

Check out the [setup guide](/docs/getting-started/) to get running, or read the [architecture overview](/docs/architecture/) if you want to understand the structure first. Questions? Open an issue on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).
