---
title: "Rust vs C++ for GUI development: safety meets performance"
description: "Choosing between Rust and C++ for desktop GUI development: memory safety tradeoffs, Qt vs GPUI, ecosystem maturity, and real-world performance comparisons."
date: 2026-06-05
tags: [Rust, C++, GUI]
draft: false
---

C++ has owned desktop GUI development for decades. Qt, wxWidgets, MFC, GTKmm. The list goes on. If you've shipped a native desktop app, chances are you wrote it in C++. Rust is the newcomer here, and the GUI ecosystem is still young. But the safety story is compelling enough that it's worth a serious look, even if C++ still wins on library breadth today.

I've spent time building desktop apps in both. Here's what I've found.

## The memory safety question

This is the obvious starting point. C++ gives you manual memory management with smart pointers as an opt-in safety net. Rust gives you compile-time ownership checking with escape hatches (`unsafe`) when you need them. The difference shows up in practice.

In C++, a use-after-free in a callback attached to a widget can crash your app hours into a session. A dangling reference in a signal handler can corrupt heap state silently. These bugs are hard to reproduce and harder to track down. I once spent three days hunting a heap corruption bug caused by a Qt signal firing after the receiving object was destroyed. The fix was one line. Finding it was the problem.

Rust's borrow checker prevents entire classes of these bugs at compile time. If you try to use a reference to a destroyed widget, the code simply won't compile. This is not theoretical. It's a daily quality-of-life improvement when building UI code, which tends to be callback-heavy and stateful.

```rust
struct App {
    count: usize,
    label: Entity<Label>,
}

impl App {
    fn increment(&mut self, cx: &mut Context<Self>) {
        self.count += 1;
        self.label.update(cx, |label, cx| {
            label.set_text(format!("Count: {}", self.count), cx);
        });
    }
}
```

In this Rust code, the compiler guarantees that `self` and `label` can't be accessed concurrently in a way that causes data races. In C++, you'd rely on discipline, code review, and runtime debugging.

That said, the borrow checker has real friction with UI patterns. Parent-child widget hierarchies, observer patterns, and bidirectional data bindings all require careful design to satisfy the ownership model. C++ lets you write these patterns naturally. Rust makes you think harder about who owns what. Sometimes that's good. Sometimes it's just slow.

## Qt vs the Rust GUI ecosystem

Qt is the elephant in the room. It's been around since 1995. It has mature widgets for everything: tables, trees, rich text, charts, 3D rendering, networking, database access, printing. The documentation fills thousands of pages. There are books, courses, Stack Overflow answers for every conceivable problem.

```cpp
// Qt: a working table view with sorting in about 10 lines
QTableView *table = new QTableView;
QStandardItemModel *model = new QStandardItemModel(this);
model->setHorizontalHeaderLabels({"Name", "Value"});
table->setModel(model);
table->setSortingEnabled(true);
table->show();
```

Rust has nothing that matches Qt's breadth. [egui](https://github.com/emilk/egui) is great for tools and prototypes. [iced](https://iced.rs/) follows the Elm architecture and is principled but still maturing. GPUI, the framework behind Zed, renders directly to the GPU and is the most performance-focused option. I compared them in detail in [my GPUI vs egui vs iced post](/blog/gpui-vs-egui-vs-iced/).

The honest assessment: if you need a feature-complete widget set tomorrow, Qt is still the answer. If you need a spreadsheet widget, a rich text editor, or a charting library, Qt has it and Rust doesn't.

But GPUI is catching up fast for certain classes of applications. Text editors, code editors, and tools that need 60fps rendering are where it shines. The rendering pipeline skips the DOM and CSS overhead entirely, going straight to Metal or Vulkan. I wrote about [why Rust and GPUI make sense for desktop apps](/blog/building-desktop-apps-rust-gpui/) in more detail.

## Build systems and tooling

C++ build systems are famously painful. CMake is the de facto standard, and it works, but the syntax is its own language. You'll deal with generator expressions, target properties, and platform-specific quirks. Qt adds qmake or CMake integration on top. Getting a reproducible build across Linux, macOS, and Windows is a rite of passage.

```cmake
find_package(Qt6 REQUIRED COMPONENTS Core Gui Widgets)
target_link_libraries(myapp PRIVATE Qt6::Core Qt6::Gui Qt6::Widgets)
```

Rust uses Cargo. One file. One command to build. Cross-compilation is a single target flag away (though you still need platform SDKs linked). The difference in day-to-day workflow is significant.

```toml
[dependencies]
gpui = { git = "https://github.com/zed-industries/zed" }
rusqlite = { version = "0.31", features = ["bundled"] }
```

Cargo handles dependency resolution, compilation, and linking in a way that just works. No `configure` scripts. No `Makefile` debugging. No header search path issues. For a solo developer or small team, this saves real time.

Where C++ tooling still wins: debuggers. LLDB and GDB understand C++ natively. Qt Creator integrates profiling, memory analysis, and a visual debugger. Rust debugging works through GDB/LLDB but the experience is rougher, especially when stepping through generic code or async runtimes. This gap matters when you're chasing a rendering bug that only reproduces on one machine.

## Performance: closer than you'd think

Both languages compile to native code. Both give you zero-cost abstractions. Both let you write SIMD, call system APIs directly, and control memory layout.

C++ has the edge in predictable latency. The language doesn't insert hidden bounds checks or drop glue. For GUI work, this matters less than people think. Modern CPUs branch-predict bounds checks effectively, and Rust's safety overhead is measured in single-digit percentage points for most UI workloads.

GPUI's GPU-accelerated rendering is competitive with Qt's raster renderer for most use cases. Qt Quick (QML + GPU rendering) is a closer comparison, and performance there is similar. The bottleneck for most GUI apps isn't the language. It's the rendering approach and the layout algorithm.

Where Rust pulls ahead is concurrency safety. UI apps increasingly do work on background threads: network requests, file indexing, data processing. Rust's type system prevents data races by construction. In C++, you're back to relying on mutexes, atomics, and careful code review. One missed lock in a callback that runs on the wrong thread can cause intermittent crashes that take weeks to diagnose.

```rust
// Rust: the type system prevents sharing mutable state across threads
// without explicit synchronization
let data = Arc::new(Mutex::new(vec![]));
let data_clone = data.clone();

std::thread::spawn(move || {
    let mut locked = data_clone.lock().unwrap();
    locked.push(42); // Safe: the Mutex guards access
});
```

## The FFI bridge

Here's a practical point that doesn't get enough attention. If you need existing C/C++ libraries, and you will, C++ just calls them. No wrappers, no binding generation, no fighting with `bindgen`. Qt integrates with OpenGL, Vulkan, database drivers, and system APIs natively.

Rust's FFI story is solid but adds overhead. You'll reach for `bindgen` to generate bindings, write `unsafe` wrapper functions, and manage the boundary between safe Rust and unsafe C. It works. It's well-documented. But it's extra work that C++ doesn't require.

On the flip side, Rust speaks C ABI cleanly, which means you can expose Rust libraries to C++, Swift, Python, and Node.js with minimal effort. If you're building a library that other languages will consume, Rust has an advantage here.

## When to pick what

Pick C++ with Qt if you need a broad widget set, need to ship on many platforms with a proven toolkit, or have a team that already knows Qt well. The ecosystem advantage is real and won't disappear soon.

Pick Rust if memory safety matters for your application domain, if you're building a text editor or GPU-heavy tool where GPUI's rendering model fits, or if you're starting fresh and can afford to build custom widgets. The [state management patterns](/blog/state-management-gpui-entities/) in GPUI are cleaner than anything I've used in C++ frameworks.

For a deeper look at getting started with Rust GUI development, check out the [GPUI getting started guide](/docs/getting-started/).

## Where things are headed

Rust's GUI ecosystem is growing fast. GPUI powers Zed, a production editor with real users. egui has a loyal following in the tools and gamedev space. iced keeps shipping releases. The widget gap is narrowing.

C++ isn't standing still either. Qt 6 cleaned up the API surface and improved QML performance. But the language itself carries decades of baggage. Undefined behavior, header files, slow compilation. These are structural problems that no library can fix.

If you're starting a new desktop project today and don't need Qt's widget catalog, Rust is worth the investment. The compiler catches bugs that would ship in C++. The tooling is simpler. The concurrency model is safer. And frameworks like GPUI are proving that Rust can deliver real-time GPU-accelerated UI.

---

If you want to see what a production Rust desktop app looks like, [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) is a boilerplate with multi-page navigation, 21 themes, i18n, form validation, and SQLite persistence. The [getting started guide](/docs/getting-started/) walks through setting it up in under ten minutes.
