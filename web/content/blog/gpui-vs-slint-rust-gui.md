---
title: "GPUI vs Slint: two approaches to Rust GUI development"
description: "A practical comparison of GPUI and Slint for building Rust desktop apps: design philosophy, rendering architecture, and when each framework makes sense."
date: 2026-06-05
tags: [GPUI, Slint, comparison]
draft: false
---

If you're building a desktop application in Rust and want a GPU-accelerated UI, two names come up: GPUI and Slint. They both render through the GPU. They're both written in Rust. That's about where the similarities end. One is a code-first framework born from a production code editor. The other is a markup-first toolkit designed for cross-platform and embedded use. I've worked with both, and the choice between them depends heavily on what you're building and how you think about UI code.

## The fundamental split: code vs markup

GPUI builds interfaces in Rust. You compose elements using a builder API, chain style methods, and nest closures. The UI is Rust all the way down. Here's what a simple component looks like:

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
                .on_click(|_event, cx| {
                    println!("clicked");
                })
        )
}
```

Slint takes a different path. You write your UI in a separate `.slint` markup language, then compile it into Rust structs you interact with from your application code:

```slint
export component HelloWorld inherits Window {
    callback clicked;
    in-out property <string> name: "World";
    VerticalLayout {
        Text {
            text: "Hello, " + root.name;
            color: green;
        }
        Button {
            text: "Click me";
            clicked => { root.clicked(); }
        }
    }
}
```

```rust
let app = HelloWorld::new()?;
app.set_name("GPUI".into());
app.on_clicked(|| println!("clicked"));
app.run()?;
```

This isn't a small difference. It affects how you structure your project, how you debug, how you onboard new developers, and what your build pipeline looks like.

## GPUI: Rust-native, opinionated, fast

GPUI is the framework powering [Zed](https://zed.dev), the code editor. It's a retained mode framework that renders directly through the GPU using Metal on macOS. The framework handles layout, text shaping, input, and compositing on its own terms rather than layering abstractions over platform APIs.

The performance is real and measurable. Zed opens large files and scrolls through them at 120fps because GPUI was built specifically for that kind of workload. Text rendering goes through a custom pipeline. Layout is computed in Rust and issued as GPU draw calls. There's no DOM, no virtual DOM, no intermediate representation slowing things down.

### Where GPUI works well

Complex, interactive desktop applications benefit most from GPUI. Code editors, IDEs, design tools, anything where you need tight control over rendering and responsive input handling. I built [gpui-starter](/docs/getting-started/) as a production boilerplate for exactly this use case, because GPUI's speed is hard to match when you're dealing with large text buffers or frequent redraws.

The reactive model is another strength. GPUI uses a subscription-based reactivity system where views subscribe to the state they depend on. When state changes, only the affected views re-render. No diffing. No virtual DOM reconciliation. The framework knows exactly what changed because you told it what to watch.

### The tradeoffs

GPUI is macOS-only right now. Windows and Linux support is in progress but not shipping. If you need cross-platform support today, this is a hard blocker.

The API is also still evolving. Zed Labs built GPUI for their own product, so the framework prioritizes Zed's needs. Documentation exists but is thinner than what you'd find for more established frameworks. You'll end up reading source code, which is fine if you're comfortable with that but adds friction if you're not.

GPUI doesn't have a markup layer. Every piece of UI is Rust code. For small teams that iterate fast and want type safety across their entire stack, this is an advantage. For teams that want designers to tweak layouts without touching Rust, it's a limitation.

## Slint: declarative, cross-platform, embedded-ready

Slint is a UI toolkit from Slint.dev (the company). It uses a custom `.slint` markup language for interface definitions, compiles them at build time, and supports multiple rendering backends including wgpu, Qt, and a software renderer. It runs on macOS, Windows, Linux, and microcontrollers.

The cross-platform story is Slint's clearest advantage. One `.slint` file renders on all supported platforms with native-feeling widgets via the standard library components. The company also provides commercial licensing and support, which matters if you're building a product and want a vendor behind the framework.

### The .slint language and tooling

Slint ships with a live preview tool. You edit a `.slint` file and see changes in real time without recompiling Rust. For teams with dedicated UI developers, this workflow is productive. The designer can iterate on layout and styling while the Rust developer handles business logic separately.

The language itself is declarative with property bindings, callbacks, and conditional/loop constructs. It compiles down to efficient Rust code. Properties are reactive: changing a bound value automatically updates anything that depends on it.

```slint
export component Counter inherits Window {
    in-out property <int> count: 0;
    callback increment;
    VerticalLayout {
        Text {
            text: "Count: " + root.count;
            font-size: 24px;
        }
        Button {
            text: "Increment";
            clicked => { root.increment(); }
        }
    }
}
```

### Where Slint works well

Embedded systems are Slint's home turf. It runs on microcontrollers with as little as 300KB RAM. If you're building an industrial interface, an automotive dashboard, or an IoT device display, Slint is one of very few Rust options that can target those constraints.

Cross-platform desktop apps benefit too. Slint's wgpu backend gives you GPU acceleration everywhere, and the Qt backend gives you native platform integration where you need it. The [architecture overview](/docs/architecture/) in the gpui-starter docs covers some of these rendering tradeoffs from the GPUI side, and the same principles apply: GPU rendering matters for anything with frequent redraws.

### The tradeoffs

The `.slint` language is another thing to learn. It's not Rust, it's not CSS, it's not QML. It's its own thing with its own semantics. The learning curve isn't steep, but it's there, and it means your UI code lives in a different mental model than your application code.

The abstraction layer between `.slint` markup and Rust logic introduces a boundary. Passing data between Slint properties and Rust state requires explicit glue code. For simple apps this is manageable. For complex applications with lots of shared state, the boundary becomes something you design around.

Slint's rendering performance is good, but it's not in the same category as GPUI for text-heavy applications. Slint renders through wgpu or Qt, which adds abstraction layers. For most applications this is fine. If you're building something like a code editor that needs to render and re-render thousands of glyphs at 120fps, GPUI's purpose-built text pipeline has an edge.

## Direct comparison

| Aspect | GPUI | Slint |
|--------|------|-------|
| **UI definition** | Rust code (builder API) | `.slint` markup language |
| **Platforms** | macOS (Windows/Linux in progress) | macOS, Windows, Linux, embedded |
| **Rendering** | Metal (custom GPU pipeline) | wgpu, Qt, software renderer |
| **Text performance** | Custom shaping pipeline, very fast | Good, uses system text layout |
| **Reactivity** | Subscription-based, fine-grained | Property bindings |
| **Tooling** | Rust toolchain only | Live preview designer, LSP |
| **License** | AGPL / commercial | GPL / commercial |
| **Ideal for** | Text editors, IDEs, tools | Cross-platform apps, embedded UIs |

## Licensing

Both frameworks use a dual license model: open source (AGPL for GPUI, GPL for Slint) with commercial licenses available. If you're building proprietary software, you'll need a commercial license for either one. The pricing models differ, so check both. This isn't a differentiator in terms of which to pick, but it's a cost you should factor in early rather than late.

## Which one should you pick

Use GPUI if you're building a macOS-first desktop application where performance matters. Code editors, text-heavy tools, developer utilities. The Rust-all-the-way approach means one language, one type system, one build step. It's productive if you're comfortable in Rust and don't need cross-platform support today.

Use Slint if you need to ship on multiple platforms or target embedded hardware. The markup separation between UI and logic is a real architectural advantage when you have designers on the team or when your UI changes more often than your business logic. The live preview workflow alone can save hours of compile-wait-test cycles.

Use neither if you need web deployment. Iced has a web backend (I compared the three in an [earlier post](/blog/gpui-vs-egui-vs-iced/)). Slint has experimental wasm support but it's not mature. GPUI doesn't target the web at all.

## My choice

I chose GPUI for gpui-starter because I was building a developer tool that needs to be fast on macOS. The Rust-native approach fit how I work. But I've used Slint for embedded projects, and for that domain it's clearly the better option. These frameworks aren't really competing with each other. They're solving different problems for different audiences, and Rust is better off having both.

If you want to try GPUI for yourself, check out the [getting started guide](/docs/getting-started/) or browse the [source on GitHub](https://github.com/hmziqrs/gpui-boilerplate). The boilerplate includes multi-page navigation, theme hot-reload, i18n, form validation, and everything else you'd need to skip the setup and start building.
