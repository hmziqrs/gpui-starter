---
title: "GPUI vs Tauri: which Rust framework for your desktop app?"
description: "Comparing GPUI and Tauri for Rust desktop development: architecture, performance, ecosystem, and when to pick each framework."
date: 2026-06-05
tags: [GPUI, Tauri, comparison]
draft: false
---

If you are building a desktop application in Rust, two names come up a lot: GPUI and Tauri. They both use Rust on the backend, and they both compile to native binaries. That is where the similarities end. One is a GPU-first UI framework that renders everything itself. The other wraps a webview and lets you write your UI in JavaScript. The choice between them is not about which is better. It is about what kind of app you are building and what you are willing to learn.

I have shipped projects with both. Here is how I think about the decision.

## The architecture split

GPUI is a retained mode, GPU-accelerated UI framework. It draws pixels directly through a custom rendering pipeline built on Metal (macOS) and Vulkan (Linux). There is no browser, no DOM, no HTML. You describe your interface in Rust, and GPUI renders it frame by frame on the GPU. The framework powers Zed, so you can see what it is capable of by opening that editor.

```rust
fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .p_4()
        .gap_2()
        .child("Hello, GPUI")
        .child(
            Button::new("click-me", "Click me")
                .on_click(cx.listener(|this, _event, cx| {
                    this.count += 1;
                    cx.notify();
                }))
        )
}
```

Tauri takes a different path. Your frontend is HTML, CSS, and JavaScript running in the system webview. Your backend is Rust, exposed through an IPC command system. The two sides communicate through serialized messages. You can use React, Vue, Svelte, or plain DOM. Tauri gives you the shell and the Rust integration. The UI is whatever web stack you already know.

```rust
#[tauri::command]
async fn save_file(path: String, contents: String) -> Result<String, String> {
    std::fs::write(&path, &contents)
        .map(|_| "saved".into())
        .map_err(|e| e.to_string())
}
```

```javascript
// frontend
const result = await invoke("save_file", {
    path: "/tmp/data.txt",
    contents: "hello"
});
```

These are fundamentally different approaches. GPUI is one language, one rendering pipeline, one mental model. Tauri is two languages, two runtimes, and an IPC bridge between them. Neither approach is wrong. They optimize for different things.

## Performance and resource usage

GPUI renders through the GPU natively. Layout, text shaping, painting, and compositing all happen on the graphics card. The CPU does very little rendering work. A GPUI app typically uses 30-80 MB of RAM at idle. The binary size for a basic app lands somewhere between 5 and 15 MB. Startup is fast because there is no webview to initialize.

Tauri uses the operating system's built-in webview (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux). This means memory usage depends heavily on what your frontend does. A simple Tauri app might sit at 50-100 MB. A React app with heavy state management can easily climb to 200+ MB. The binary itself is small, often under 10 MB, but the runtime cost is the webview. Tauri's advantage over Electron is that it does not bundle Chromium. It reuses what the OS already has.

For raw rendering throughput, GPUI wins. The GPU pipeline is purpose-built for UI rendering. Text scrolls at 120fps without thinking about it. The [GPUI performance model](/docs/performance/) is designed around maintaining consistent frame times even under heavy workload. Tauri is fast enough for most applications, but you are subject to webview rendering performance, which varies by platform and frontend complexity.

## Cross-platform reach

This is where Tauri pulls ahead by a wide margin. Tauri supports Windows, macOS, and Linux out of the box. Tauri 2.0 adds iOS and Android as mobile targets. You write your frontend once, write your Rust commands once, and the framework handles platform differences. The plugin ecosystem covers file system access, notifications, clipboard, biometrics, and dozens of other platform APIs.

GPUI currently runs on macOS and Linux. Windows support is in progress but not stable. There is no mobile target. If you need to ship on Windows or mobile today, GPUI is not ready for you. This is the single biggest practical difference between the two frameworks.

## Developer experience and learning curve

Tauri has the gentler onboarding. If you already know React or Vue, you already know how to build a Tauri frontend. The Rust side is mostly writing `#[tauri::command]` functions with normal Rust types. The [Tauri documentation](https://v2.tauri.app) is thorough. The plugin system is mature. You can get a working app in an afternoon.

GPUI demands more upfront investment. You need to learn the framework's concepts: entities, views, contexts, the reactive system, the element model. The documentation is improving but still sparse compared to Tauri. Many APIs are best understood by reading Zed's source code. Error messages can be cryptic. The learning curve is real, and I would not pretend otherwise.

But once you are over that curve, something interesting happens. You are writing everything in Rust. No context switching between TypeScript and Rust. No IPC serialization overhead. No debugging across two separate runtimes. The mental model is one language, one type system, one build tool. For complex applications, that unity pays dividends over time.

## Ecosystem and maturity

Tauri's ecosystem is larger. npm gives you access to any frontend library. The Tauri plugin registry has official plugins for common platform features. The community is active, with regular releases and a governance structure. Companies ship production Tauri apps. The framework has been stable since 1.0.

GPUI's ecosystem is smaller but growing. The framework is extracted from Zed, which means it is battle-tested for editor-scale applications. Third-party components exist but are fewer. The community is concentrated around the Zed GitHub organization. The API is still evolving, and breaking changes happen. If you read my [comparison of Rust GUI frameworks](/blog/gpui-vs-egui-vs-iced/), you will notice GPUI's ecosystem is the weakest of the major options in terms of breadth.

## When to pick Tauri

Pick Tauri when:

- You need Windows or mobile support.
- Your team already knows web technologies.
- You want access to the npm ecosystem for UI components.
- You are building a tool, dashboard, or form-heavy application where native rendering performance is not critical.
- You want to ship something fast with a shallow learning curve.

Tauri is pragmatic. It meets you where you are. If you have a web developer on your team, they can be productive in Tauri on day one.

## When to pick GPUI

Pick GPUI when:

- You are building a performance-sensitive desktop application (editor, creative tool, real-time monitoring).
- You want everything in Rust and are willing to invest in learning the framework.
- Your target platform is macOS or Linux.
- You need fine-grained control over rendering and layout.
- You are building something where 60-120fps consistency matters.

GPUI is opinionated and demanding. It asks more of you upfront. In return, you get a single-language app with a rendering pipeline that does not compromise.

## A practical example

To make the difference concrete, here is how you handle a button click in each framework.

**Tauri** (Rust backend + JavaScript frontend):

```rust
// src-tauri/src/main.rs
#[tauri::command]
fn process_data(input: String) -> String {
    format!("Processed: {}", input.to_uppercase())
}
```

```javascript
// src/App.jsx
async function handleClick() {
    const result = await invoke("process_data", { input: "hello" });
    console.log(result); // "Processed: HELLO"
}
```

**GPUI** (Rust everywhere):

```rust
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    div()
        .child(
            Button::new("process", "Process")
                .on_click(cx.listener(|this, _event, cx| {
                    this.output = format!("Processed: {}", this.input.to_uppercase());
                    cx.notify();
                }))
        )
        .child(&self.output)
}
```

Same result. Different ergonomics. The Tauri version requires thinking in two languages and managing the IPC boundary. The GPUI version keeps everything in one place but demands familiarity with the framework's reactive patterns.

## The bottom line

Tauri and GPUI are not really competing. They are solving different problems for different teams. Tauri is the right call if you want cross-platform reach, web technology familiarity, and fast time to market. GPUI is the right call if you are building a desktop application where rendering performance, single-language codebase, and GPU-native rendering are priorities.

If you want to explore GPUI, the [getting started guide](/docs/getting-started/) walks through setting up a project from scratch. For a deeper look at how GPUI apps are structured, the [architecture overview](/docs/architecture/) covers the core concepts. The [gpui-starter repository](https://github.com/hmziqrs/gpui-boilerplate) provides a production-ready boilerplate with navigation, theming, i18n, SQLite persistence, and auto-updating, so you can skip the scaffolding and start building.
