---
title: "GPUI vs egui: retained vs immediate mode GUI in Rust"
description: "A technical comparison of GPUI (retained mode) and egui (immediate mode) for building Rust desktop applications, with code examples and real tradeoffs."
date: 2026-06-05
tags: [GPUI, egui, comparison]
draft: false
---

If you are building a desktop app in Rust, you will eventually face one question: retained mode or immediate mode? GPUI and egui represent the two ends of that spectrum in Rust's GUI ecosystem. One holds onto your UI tree and updates it surgically. The other throws everything away and redraws from scratch each frame. Both approaches work. They just work differently, and the right choice depends on what you are building.

I have shipped code with both. Here is what I learned.

## The core difference: how state and rendering interact

Immediate mode GUIs, the egui model, re-emit the entire UI description every frame. Your code runs in a tight loop, calling functions like `ui.label()` and `ui.button()` dozens or hundreds of times per second. The framework collects these calls, lays them out, renders them, then forgets about them. Next frame, you do it all again.

Retained mode GUIs, the GPUI model, build a tree of elements once and keep it around. When something changes, you update the relevant piece of the tree and the framework re-renders just that subtree. The tree persists across frames. The framework owns it, diffs it, and optimizes rendering based on what actually changed.

This distinction affects how you structure your code, handle state, think about performance, and what kind of APIs feel natural.

## egui: immediate mode in practice

egui's API feels like writing a description of what should be on screen right now.

```rust
egui::CentralPanel::default().show(&ctx, |ui| {
    ui.heading("Settings");
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut self.name);
    });
    ui.add(egui::Slider::new(&mut self.volume, 0.0..=100.0).text("Volume"));
    if ui.button("Save").clicked() {
        self.save_settings();
    }
});
```

You mutate state directly (`&mut self.name`), and the next frame picks up the change automatically. No signals, no observers, no subscription management. The simplicity is hard to overstate. You can teach someone this API in ten minutes.

State management in egui is almost invisible. Because you redraw every frame, the framework always has the current state. You can store a counter as a plain `u32` and increment it in a click handler. It just works. For small to medium tools, this works well.

The cost shows up at scale. When your UI has hundreds of widgets, re-describing the entire tree every frame becomes CPU work you would rather avoid. egui mitigates this with caching and early-out optimizations, but the fundamental model means you are doing more work per frame than a retained framework needs to.

Layout in egui is also limited. There is no flexbox, no grid system, no CSS-like styling. You get horizontal and vertical layouts, and you can nest them. For tools and debug panels, that is usually enough. For a polished application with complex responsive layouts, you will fight the layout system.

## GPUI: retained mode in practice

GPUI takes the opposite approach. You implement a `render` method that returns a tree of elements, and the framework holds onto that tree between frames.

```rust
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .p_4()
        .gap_2()
        .child(
            div()
                .flex()
                .gap_2()
                .child("Name:")
                .child(
                    TextInput::new(self.input.clone())
                        .placeholder("Enter name")
                )
        )
        .child(
            Button::new("save-btn", "Save")
                .on_click(cx.listener(|this, _event, cx| {
                    this.save_settings(cx);
                }))
        )
}
```

When state changes, you call `cx.notify()` on the relevant view. GPUI re-invokes that view's `render` method, diffs the new tree against the old one, and updates only what changed. The GPU receives minimal draw commands. Text that did not change does not get re-rendered. Layout that stayed the same does not get recomputed.

This model scales well. A complex application with thousands of elements can run at 60fps because most frames touch a small fraction of the tree. Zed, the code editor built on GPUI, handles massive files and real-time collaboration while staying responsive. That is not an accident. The retained architecture makes it possible.

State management is more explicit. You work with GPUI's entity system: `Model<T>` for background state, `View<T>` for UI-visible state. These provide thread-safe access to your data. When you mutate a model, you do it inside a callback that GPUI schedules correctly.

```rust
let model = cx.new_model(|_cx| MyState::default());
cx.update_model(&model, |state, cx| {
    state.counter += 1;
    cx.notify();
});
```

This is more boilerplate than egui's `&mut self`. The tradeoff is that GPUI handles concurrent access safely. Multiple views can observe the same model. Async operations can update state without blocking the UI thread. The entity system is the reason GPUI can do things like real-time collaborative editing without data races.

## Performance: where the models diverge

egui renders through a backend, typically wgpu. It works. But egui was designed for correctness and ease of use, not raw rendering throughput. The CPU does layout and draw call preparation every frame. For most tools, this is fine. At 60fps with a few hundred widgets, you will not notice.

GPUI renders natively through the GPU using Metal on macOS and Vulkan on Linux. Layout, text shaping, and compositing all happen in GPU-optimized passes. The framework was built from day one to render Zed, which means it was built to handle the worst case: large files, thousands of syntax-highlighted characters, multiple panes, and smooth scrolling simultaneously.

The benchmark that matters is not "how fast can you render 100 buttons." It is "how does the framework behave when your application grows complex." egui degrades gradually. GPUI stays fast longer because it only re-renders what changed.

If you are building a tool panel for a game engine, egui's performance is more than adequate. If you are building a text editor, a design tool, or anything with heavy scrolling and real-time updates, GPUI's rendering architecture is a real advantage.

## Styling and layout

egui uses a programmatic style API. You set colors, fonts, and spacing through function calls and struct fields. There is a theming system, but it is basic. You can change colors and fonts globally, and you can override per-widget. There is no selector system, no cascading, nothing like CSS.

```rust
ui.style_mut().override_text_style = Some(egui::TextStyle::Heading);
ui.colored_label(egui::Color32::RED, "Error");
```

GPUI uses a CSS-like styling system. You write styles as method chains on elements, and the framework handles layout through a flexbox model.

```rust
div()
    .flex()
    .items_center()
    .justify_between()
    .px_4()
    .py_2()
    .bg(gpui::rgb("#1e1e2e"))
    .text_color(gpui::rgb("#cdd6f4"))
    .child("Hello")
```

This is more expressive. You can build complex responsive layouts that would require significant effort in egui. The tradeoff is that GPUI's styling API has more surface area to learn. But if you have ever written CSS, the mental model transfers directly.

For a deeper look at how theming works in GPUI, see the [theme system deep dive](/blog/theme-system-deep-dive/).

## Ecosystem and maturity

egui has the larger ecosystem. It has been around longer, has more contributors, and has crates for everything from plotting to syntax highlighting to docking. The documentation is thorough. There are many examples. If you hit a problem, someone has probably solved it before.

GPUI's ecosystem is smaller. The framework was built for and extracted from Zed, which means some APIs are designed with an editor's needs in mind. Third-party components exist but are fewer. The documentation is improving but still sparse compared to egui.

If ecosystem size matters to you, egui wins. If rendering performance and a retained architecture matter more, GPUI has the edge.

## When to use which

Use egui when:
- You need a GUI for a Rust tool or utility quickly
- You are building debug overlays, game editor panels, or data visualizations
- Your UI has fewer than a few hundred interactive elements
- You value simplicity and fast iteration over visual polish

Use GPUI when:
- You are building a production desktop application
- Complex layout, smooth scrolling, and responsive interactions are requirements
- You need GPU-native rendering performance
- Your application will have thousands of elements on screen simultaneously

The boundary is not sharp. You can build decent apps in egui and you can build simple tools in GPUI. But each framework's architecture pushes you toward certain kinds of applications. Fighting that push is possible but not fun.

## Why I chose GPUI for gpui-starter

gpui-starter exists to help people build real desktop applications in Rust. Not prototypes. Not internal tools. Applications with multiple pages, forms, settings, themes, and the kind of polish users expect from native software.

GPUI is the right foundation for that because retained mode scales better for complex UIs, the GPU rendering pipeline handles demanding layouts without frame drops, and the entity system makes concurrent state management tractable. These are the things that matter when your app grows beyond a single panel.

The cost is real. GPUI is harder to learn. The docs are thinner. You will spend more time reading source code than you would with egui. But the result is an application that performs like native software should.

If you want to try building with GPUI, the [getting started guide](/docs/getting-started/) walks through project setup and core concepts. The full boilerplate with navigation, theming, i18n, and persistence is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate). For more background on the framework itself, the [GPUI introduction post](/blog/getting-started-with-gpui/) covers the basics.
