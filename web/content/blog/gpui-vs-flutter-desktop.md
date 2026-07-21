---
title: "GPUI vs Flutter for Desktop: Rust vs Dart Compared"
description: "Comparing GPUI and Flutter for desktop app development: language ergonomics, rendering performance, and ecosystem maturity in 2026."
date: 2026-06-05
tags: [GPUI, Flutter, comparison]
draft: false
---

If you're building a desktop app in 2026, you've probably at least glanced at Flutter. Google's framework has millions of users, thousands of packages, and runs on every platform under the sun. GPUI is the new kid on the block: a Rust-based GPU-accelerated UI framework that powers the Zed editor. I've spent serious time with both. Here's what I found.

## The core difference: Rust vs. Dart

This comparison starts and ends with the language. Dart is garbage-collected, optionally typed, and designed to feel familiar to JavaScript and Java developers. Rust has no garbage collector, enforces ownership and borrowing at compile time, and has a learning curve that makes most developers question their life choices for the first month.

That sounds like Rust loses. It doesn't.

The ownership model means GPUI apps have predictable memory usage and zero garbage collection pauses. When you're building something like a code editor or a real-time monitoring dashboard, those pauses matter. I've profiled Flutter desktop apps where GC stalls caused visible frame drops during heavy scroll. In Rust, the compiler tells you exactly where memory is freed. No surprises at runtime.

```rust
// GPUI: Model state with compile-time safety
struct AppState {
    count: usize,
    items: Vec<String>,
}

impl AppState {
    fn increment(&mut self) {
        self.count += 1;
        self.items.push(format!("Item {}", self.count));
    }
}
```

Compare that to Dart, where you'd write the same thing without the borrow checker complaints but with the trade-off of runtime GC pressure.

```dart
// Flutter: Same concept, GC-managed
class AppState extends ChangeNotifier {
  int _count = 0;
  List<String> _items = [];

  void increment() {
    _count++;
    _items.add('Item $_count');
    notifyListeners();
  }
}
```

Both work. The Dart version is shorter. The Rust version gives you a compiler guarantee that no two threads mutate `AppState` simultaneously without synchronization.

## Rendering architecture

GPUI renders directly through the GPU using a custom pipeline built on Metal (macOS) and Vulkan (Linux/Windows). Every frame is drawn from scratch: layout, style, paint commands are recalculated and submitted as GPU draw calls. There's no DOM, no intermediate representation layer, no CSS parsing overhead.

Flutter uses Skia (or Impeller on newer builds) as its rendering engine. It also talks to the GPU. The difference is that Flutter maintains a retained rendering tree: widgets are objects that persist across frames, and the framework diffs them to decide what to redraw.

Which is better depends on what you're doing. Flutter's retained model makes it easy to build complex form layouts with animations because the framework handles the diffing. GPUI's immediate-mode approach is faster for apps that redraw large portions of the screen each frame, like text editors or data visualizations.

In practice, I see Flutter hitting 60fps consistently on moderate hardware. GPUI targets 120fps and often hits it on the same machine. Whether that matters depends on your users. If you're building an internal tool for a company that issues 2020 MacBook Airs, the difference is academic. If you're building something where frame timing matters, GPUI has a real edge.

## Ecosystem and packages

This is where Flutter destroys GPUI. Flutter's pub.dev has over 40,000 packages. Need a date picker? Twenty options. Charting library? Fifteen. Bluetooth, camera, file system access? All there, with documentation and examples.

GPUI's ecosystem is tiny. You're mostly limited to what Zed's team has built and what the community has contributed. That means fewer pre-built widgets, fewer integrations, and more code you have to write yourself.

But there's a counterpoint. Flutter's package quality varies enormously. I've been burned by packages that were abandoned mid-version, had breaking changes without documentation, or silently broke on desktop despite claiming support. When you're working with GPUI, you're closer to the metal. If something breaks, you can read the source and fix it. The Rust compiler catches entire categories of bugs that would slip through in Dart.

For [getting started with GPUI](/docs/getting-started/), expect to write more foundational code. You'll build your own components rather than importing them. That's slower upfront but gives you exact control over behavior and performance.

## Developer tooling

Flutter's hot reload is genuinely excellent. Change a widget, save, and the app updates in under a second. GPUI has hot reload support too, but it's newer and less polished. The feedback loop is fast enough to be useful, just not as seamless as Flutter's.

Dart's tooling is mature. The analyzer, formatter, and test runner all work well together. Rust's tooling is also strong: `cargo test`, `cargo clippy`, `rust-analyzer` in your editor. The difference is compile times. A clean Flutter build takes seconds. A clean GPUI build can take minutes, especially with dependencies like `tree-sitter` or large generated code.

During active development, incremental Rust compilation keeps things manageable. But that first build after `cargo clean` is a coffee break.

## Cross-platform support

Flutter advertises write-once-run-anywhere. For mobile, it mostly delivers. For desktop, the story is more complicated. Platform channels for native APIs require writing Kotlin/Swift/C++ glue code. Platform-specific bugs are common. Text input on Linux has been a persistent pain point.

GPUI currently targets macOS first, with Linux support improving fast and Windows support in progress. It does not target mobile or web. If you need a mobile app alongside your desktop app, GPUI is the wrong choice period.

I wrote about some of these tradeoffs in the context of [Rust desktop development](/blog/rust-desktop-apps-gpui/) in an earlier post.

## When to pick which

Use Flutter if:

- You need Android and iOS support alongside desktop
- Your team already knows Dart or JavaScript
- You need to ship fast with lots of pre-built components
- Your app is a CRUD interface, form-heavy tool, or e-commerce frontend

Use GPUI if:

- Desktop-only is fine, especially macOS-first
- You want predictable performance without GC pauses
- Your app does heavy text rendering, code editing, or real-time data display
- Your team is comfortable with Rust or wants to invest in it

## Building a list view

Here's a filtered list in GPUI:

```rust
fn render_list(items: &[String], filter: &str) -> impl IntoElement {
    let filtered: Vec<&String> = items
        .iter()
        .filter(|item| item.to_lowercase().contains(&filter.to_lowercase()))
        .collect();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(filtered.iter().map(|item| {
            div()
                .px_3()
                .py_2()
                .bg(cx.theme().surface)
                .rounded_md()
                .text_sm()
                .child(item.clone())
        }))
}
```

And the same in Flutter:

```dart
Widget buildList(List<String> items, String filter) {
  final filtered = items
      .where((item) => item.toLowerCase().contains(filter.toLowerCase()))
      .toList();

  return ListView.builder(
    itemCount: filtered.length,
    itemBuilder: (context, index) {
      return ListTile(
        title: Text(filtered[index]),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(8),
        ),
      );
    },
  );
}
```

The GPUI version uses a builder pattern for layout. The Flutter version uses a declarative widget tree. Both are readable. The GPUI version gives you more control over rendering behavior at the cost of verbosity.

## My take

I pick GPUI when performance and control are the priority. I pick Flutter when speed of development and platform coverage matter more. Neither is objectively better. They solve different problems for different teams.

If you want to try GPUI without starting from zero, [gpui-starter](/docs/getting-started/) is a production-ready boilerplate with multi-page navigation, 21 themes, i18n, form validation, SQLite persistence, and auto-updating. It handles the tedious setup so you can focus on building your app. The source is on [GitHub](https://github.com/freeoxide/gpui-starter).
