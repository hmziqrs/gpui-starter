---
title: "Building developer tools with Rust and GPUI"
description: "How to build developer tools like debuggers, profilers, and dashboards as desktop apps using Rust and GPUI."
date: 2026-06-05
tags: [Rust, developer-tools, GPUI]
draft: false
---

Developer tools have specific demands that most UI frameworks struggle with. Rendering thousands of log lines without stuttering. Real-time data pipelines feeding into live charts. Tight frame budgets, because a profiler that itself drops frames defeats the purpose. All of this while keeping memory usage predictable, since the tool is often running alongside the very thing it is measuring.

Rust and GPUI are a strong combination for this class of application. What follows explains why, with concrete patterns for building common developer tool UIs.

## What makes developer tools different

A typical form app renders once and waits for input. A developer tool renders continuously. A log viewer might ingest hundreds of messages per second. A profiler timeline redraws on every frame while a recording is active. A memory dashboard polls process stats at 30Hz and updates a dozen visualizations.

These workloads share a few traits:

- High-frequency state updates. Your UI state changes dozens or hundreds of times per second, not once per user action.
- Large data volumes. A single trace file can be hundreds of megabytes. You cannot load it all into DOM nodes.
- Tight latency requirements. A 50ms rendering pause in a text editor is noticeable. A 50ms pause in a frame debugger means you missed the frame you were trying to inspect.
- Complex nested layouts. Split panes, resizable columns, tree views with inline editors, and overlays that need pixel-perfect positioning.

Web-based tools like Chrome DevTools work because the browser team controls the entire stack. But when you are building a standalone desktop tool, shipping an entire Chromium instance to render a log viewer is a hard sell. The overhead is real, and it is measurable.

## Why GPUI fits

GPUI renders directly to the GPU through Metal (macOS) or Vulkan (Linux). There is no DOM, no CSS engine, no JavaScript runtime between your Rust code and the pixels. The render pipeline goes through three phases: layout, prepaint, and paint. Each phase produces typed state that the next phase consumes. You pay for exactly the work you do.

For developer tools specifically, this matters in two ways.

Frame times are predictable. No garbage collector pause, no JIT warmup, no layout thrashing. A 16ms frame budget is realistic even with complex UIs because the render work is batched into GPU draw calls and the layout system is a clean flexbox implementation without decades of web legacy.

Memory is deterministic. Rust's ownership model means allocations happen where you say they happen. There are no hidden allocation spikes from a browser engine deciding to cache something. When your profiler tool is measuring another process's memory, the last thing you want is your own tool's memory usage jumping unpredictably.

## Building a real-time log viewer

A log viewer that tails a file and renders new lines as they arrive is a good starting point.

```rust
struct LogViewer {
    entries: Vec<LogEntry>,
    auto_scroll: bool,
    filter: Option<LogLevel>,
}

struct LogEntry {
    timestamp: Instant,
    level: LogLevel,
    message: String,
}

impl LogViewer {
    fn start_watching(&mut self, path: &Path, cx: &mut ViewContext<Self>) {
        let path = path.to_owned();
        cx.spawn(async move |this, cx| {
            let mut lines = tail_file(&path).await;
            while let Some(line) = lines.next().await {
                let entry = parse_log_line(&line);
                this.update(cx, |viewer, cx| {
                    viewer.entries.push(entry);
                    // Keep only the last 10,000 entries in memory
                    if viewer.entries.len() > 10_000 {
                        viewer.entries.drain(0..1000);
                    }
                    cx.notify();
                }).ok();
            }
        }).detach();
    }
}
```

The file watching runs on an async task spawned through `cx.spawn`. When a new line arrives, we update the component state on the main thread via `this.update`. We cap the in-memory buffer at 10,000 entries to keep rendering fast. GPUI only re-renders the component when `cx.notify()` is called, so high-frequency updates from the file watcher do not cause unnecessary layout work.

The render method maps entries to elements:

```rust
impl Render for LogViewer {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .size_full()
            .overflow_y_scroll()
            .children(self.entries.iter().map(|entry| {
                div()
                    .px_2()
                    .py_1()
                    .text_color(match entry.level {
                        LogLevel::Error => Color::Error,
                        LogLevel::Warn => Color::Warn,
                        _ => Color::Muted,
                    })
                    .child(entry.message.clone())
            }))
    }
}
```

This renders 10,000 entries at under 8ms per frame on my M2 MacBook Pro. The same approach in a webview starts to stutter around 2,000 entries because each entry creates a real DOM node with its own layout and paint cost. GPUI batches these into a single scrollable container with one set of GPU draw calls.

## Building a metrics dashboard

Dashboards are another common pattern. You have multiple data sources feeding numbers at you, and you need to display them as charts, gauges, or sparklines. The hard part is managing multiple concurrent data streams without blocking the UI.

```rust
struct MetricsDashboard {
    cpu_history: VecDeque<f64>,
    memory_history: VecDeque<f64>,
    request_rate: f64,
    error_count: usize,
}

impl MetricsDashboard {
    fn start_polling(&mut self, cx: &mut ViewContext<Self>) {
        cx.spawn(async move |this, cx| {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                let metrics = fetch_system_metrics().await;
                this.update(cx, |dashboard, cx| {
                    dashboard.cpu_history.push_back(metrics.cpu_percent);
                    dashboard.memory_history.push_back(metrics.memory_mb);
                    dashboard.request_rate = metrics.requests_per_sec;
                    dashboard.error_count = metrics.errors;
                    // Keep 60 seconds of history at 10Hz polling
                    if dashboard.cpu_history.len() > 600 {
                        dashboard.cpu_history.pop_front();
                    }
                    if dashboard.memory_history.len() > 600 {
                        dashboard.memory_history.pop_front();
                    }
                    cx.notify();
                }).ok();
            }
        }).detach();
    }
}
```

Each metric gets its own history buffer. The polling runs at 10Hz (every 100ms), which is fast enough for smooth sparklines but not so fast that it creates rendering pressure. The `VecDeque` acts as a ring buffer, keeping exactly 600 data points for 60 seconds of visible history.

GPUI's canvas element lets you draw arbitrary shapes, so a sparkline is a matter of computing pixel coordinates and painting line segments:

```rust
fn sparkline(data: &VecDeque<f64>, color: Hsla, width: f32, height: f32) -> Div {
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let range = (max - min).max(1.0);

    canvas(
        move |_bounds, window, cx| {
            let points: Vec<Point<Pixels>> = data.iter().enumerate().map(|(i, v)| {
                let x = px((i as f32 / data.len() as f32) * width);
                let y = px(height - ((v - min) / range) as f32 * height);
                point(x, y)
            }).collect();

            // Draw line segments between consecutive points
            for pair in points.windows(2) {
                window.paint_line(pair[0], pair[1], px(1.5), color);
            }
        },
        |_, _, _, _| {},
    )
    .size(Size { width: px(width), height: px(height) })
}
```

No SVG, no canvas element with a JavaScript API, no intermediate abstraction. You compute pixel coordinates and draw lines. The GPU batches them. The result is a sparkline that renders in microseconds, not milliseconds.

## Handling concurrent data pipelines

Developer tools often juggle multiple data sources. A debugger reads process memory while also capturing stdout. A profiler records stack traces while also tracking allocation events. These streams arrive independently and at different rates.

In GPUI, you spawn a separate async task for each stream. Each task pushes data into the component's state through `this.update`. Because entity updates are serialized on the main thread, you never have data races between streams. The component sees a consistent snapshot of all streams on every render.

```rust
impl DebuggerView {
    fn attach(&mut self, pid: u32, cx: &mut ViewContext<Self>) {
        // Stream 1: stack traces
        self.spawn_trace_collector(pid, cx);
        // Stream 2: stdout capture
        self.spawn_output_capture(pid, cx);
        // Stream 3: memory allocations
        self.spawn_allocation_tracker(pid, cx);
    }

    fn spawn_trace_collector(&self, pid: u32, cx: &mut ViewContext<Self>) {
        cx.spawn(async move |this, cx| {
            let mut traces = collect_stack_traces(pid).await;
            while let Some(trace) = traces.next().await {
                this.update(cx, |view, cx| {
                    view.traces.push(trace);
                    cx.notify();
                }).ok();
            }
        }).detach();
    }
}
```

Each collector runs independently. They do not block each other. They do not block the UI. When one of them produces data, it updates the shared state and triggers a re-render. The component's `render` method sees the latest data from all three streams because updates are atomic from the component's perspective.

Getting this right in JavaScript is harder. You have to manage a mutex or coordinate channels, and `setState` batches in ways that are hard to predict. The GPUI entity system handles the serialization for you.

## Performance considerations

Here are the numbers I see when building these tools.

A log viewer rendering 10,000 visible lines maintains 60fps on an M2 MacBook Pro with frame times around 6ms. Memory usage sits around 45MB with a 50MB log file loaded. The same tool built as a React app with a virtualized list (react-window) uses roughly 180MB and drops frames when scrolling fast through a large dataset.

A real-time dashboard with six sparklines updating at 10Hz renders in under 3ms per frame. The polling overhead is negligible because the async tasks sleep between ticks and do not spin.

These numbers are not benchmarks in a controlled environment. They are what I measured while building actual tools. Your numbers will vary depending on your data and your layout. But the pattern holds: native GPU rendering with no intermediate abstraction layer gives you headroom that web-based tools do not have.

The main tradeoff is ecosystem. GPUI does not have a package registry full of pre-built chart components. You will write your own sparklines, your own tree views, your own table implementations. This takes more upfront work. But you get components that do exactly what you need, with no dead code from features you never use, and no performance cliffs from abstraction layers you cannot control.

If you are building a developer tool and the web stack's overhead is becoming a problem, Rust and GPUI are worth a serious look. The [architecture docs](/docs/architecture/) explain how the entity and context systems work in detail. The [getting started guide](/docs/getting-started/) has a working app running in under five minutes.

[gpui-starter](https://github.com/freeoxide/gpui-starter) gives you a project scaffold with navigation, themes, async state management, and a command palette wired up and ready to extend. You can find the full setup instructions at [getting started](/docs/getting-started/).
