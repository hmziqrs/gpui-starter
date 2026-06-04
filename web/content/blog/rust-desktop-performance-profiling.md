---
title: "Performance profiling for Rust desktop apps"
description: "How to profile Rust desktop applications with frame time analysis, memory tracking, and bottleneck identification in GPUI and native GUI frameworks."
date: 2026-06-05
tags: [Rust, performance, profiling]
draft: false
---

A Rust desktop app that compiles and runs correctly can still feel wrong. Janky scrolling, delayed input responses, sudden memory spikes that make the OS swap. These problems don't show up in unit tests. You need to measure what the app is actually doing at runtime, and that means profiling.

I have spent a lot of time staring at flamegraphs and frame timing data from GPUI applications. Here is what I learned about finding and fixing performance problems in Rust desktop apps.

## Why performance profiling matters for desktop apps

Desktop users have specific expectations. A window should respond to input within 16ms (60fps) or 8ms (120fps on ProMotion displays). Animations should be smooth. Memory should stay flat, not grow every time you switch pages. When any of these break, users notice immediately because the problem is right there on screen, not hidden behind a loading spinner.

Rust gives you a performance ceiling that is higher than most alternatives. No garbage collection pauses, no JIT warmup, minimal runtime overhead. But that ceiling does not matter if your code allocates on every frame, blocks the main thread with synchronous I/O, or recomputes layouts unnecessarily. You still have to write code that respects the frame budget.

## Setting up frame time measurement

The first thing I add to any GPUI app is frame timing instrumentation. GPUI already has [built-in rendering hooks](/docs/getting-started/) that fire on every frame. You can use these to measure how long each frame takes from start to finish.

```rust
use gpui::WindowBounds;
use std::time::Instant;

pub struct FrameTimer {
    last_frame: Instant,
    frame_times: Vec<f64>,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self {
            last_frame: Instant::now(),
            frame_times: Vec::with_capacity(300),
        }
    }

    pub fn record_frame(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame).as_secs_f64() * 1000.0;
        self.frame_times.push(delta);

        // Keep last 300 frames (5 seconds at 60fps)
        if self.frame_times.len() > 300 {
            self.frame_times.remove(0);
        }

        self.last_frame = now;
    }

    pub fn average_frame_time(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64
    }

    pub fn p99_frame_time(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let mut sorted = self.frame_times.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[(sorted.len() as f64 * 0.99) as usize]
    }
}
```

The average tells you the baseline. The p99 tells you where the stutters are. If your average is 5ms but your p99 is 40ms, you have intermittent jank that averages out but users will feel.

### Displaying a frame time overlay

During development I render a small overlay in the corner of the window that shows the current frame time, average, and worst frame in the last 5 seconds. This is the single most useful debugging tool I have.

```rust
impl Render for FrameTimeOverlay {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        let avg = self.timer.average_frame_time();
        let p99 = self.timer.p99_frame_time();

        let color = if p99 < 16.67 {
            gpui::rgb(0x4ade80) // green
        } else if p99 < 33.33 {
            gpui::rgb(0xfacc15) // yellow
        } else {
            gpui::rgb(0xf87171) // red
        };

        div()
            .absolute()
            .top_2()
            .right_2()
            .bg(gpui::rgb(0x000000cc))
            .rounded_md()
            .p_2()
            .text_color(color)
            .text_size(px(11.0))
            .child(format!(
                "avg: {:.1}ms | p99: {:.1}ms",
                avg, p99
            ))
    }
}
```

Green means you are within budget. Yellow means some frames are dropping. Red means users are seeing visible stutter. Keep this overlay visible while you work and you will catch regressions as they happen.

## CPU profiling with flamegraphs

Frame timing tells you there is a problem. Flamegraphs tell you where it is.

On macOS, `Instruments` (part of Xcode) is the best tool for profiling Rust binaries. The Time Profiler instrument gives you a call tree with inclusive and exclusive time per function. On Linux, `perf record` and `perf report` do the same job, and you can generate flamegraphs with [Flamegraph.rs](https://github.com/flamegraph-rs/flamegraph).

```bash
# Install flamegraph tool
cargo install flamegraph

# Profile your binary
cargo flamegraph --bin your-app --pid $(pgrep your-app)
```

This produces an SVG you can open in a browser. Each bar is a function call. Width represents time on CPU. Click to zoom in.

### Common GPUI bottleneck patterns

After profiling several GPUI apps, I keep seeing the same categories of problems:

**1. Excessive re-renders.** GPUI is reactive. When state changes, views re-render. If your state model is coarse (one big struct), changing one field re-renders everything. The fix is to split state into smaller entities so that changes are scoped.

```rust
// Bad: changing theme re-renders the entire app
struct AppState {
    theme: Theme,
    user_data: UserData,
    navigation: NavState,
}

// Better: each piece of state is its own entity
// Changing theme only re-renders views that read theme
entity.map_mut(|state: &mut ThemeState, cx| {
    state.set_theme(new_theme);
    cx.notify();
});
```

We cover this pattern in more detail in the [state management guide](/docs/getting-started/).

**2. Allocating on every frame.** String formatting, `Vec` construction, and `format!` calls inside `render()` create garbage. GPUI's rendering is efficient but it cannot save you from allocating 500 strings per frame. Pre-compute what you can outside the render function, and use `&str` slices where possible.

```rust
// Bad: allocates every frame
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    let label = format!("Count: {}", self.count); // new String every frame
    div().child(label)
}

// Better: avoid allocation in the hot path
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    div().child(format!("Count: {}", self.count).leak())
}
```

Actually, leaking is bad advice for long-running apps. The real fix is to cache formatted strings and only update them when the count changes. The point is: look at your allocations.

**3. Blocking the main thread.** Synchronous file reads, database queries, or network calls on the main thread will cause frame drops. GPUI has async support. Use it.

```rust
// Bad: blocks the render thread
let data = std::fs::read_to_string("config.toml").unwrap();

// Good: async, non-blocking
cx.spawn(|this, mut cx| async move {
    let data = tokio::fs::read_to_string("config.toml")
        .await
        .unwrap();
    this.update(&mut cx, |this, cx| {
        this.set_config(data);
        cx.notify();
    });
})?;
```

If you want to see how async state management works at scale, the [gpui-query approach to async data fetching](/blog/state-management-gpui-entities/) covers patterns that avoid blocking entirely.

## Memory profiling

Rust does not have a garbage collector, which means memory usage is predictable but not automatic. You still leak memory if you hold references too long, grow caches without bounds, or accumulate event listeners.

### Tracking allocations with DHAT

[DHAT](https://valgrind.org/docs/manual/dh-manual.html) (part of Valgrind) tracks every allocation in your program and shows you which code paths allocate the most memory, how long allocations live, and how many are never freed.

```bash
valgrind --tool=dhat --your-app
```

On macOS, Instruments has an Allocations instrument that does similar tracking. Filter by your app's binary name to ignore system allocations.

### Checking for leaks with address sanitizer

Rust's `unsafe` blocks can leak memory just like C code. Even safe Rust can leak if you create reference cycles with `Rc` or keep things in global collections. The address sanitizer catches use-after-free and buffer overflows, while leak sanitizer catches unfreed allocations.

```toml
# In .cargo/config.toml
[build]
rustflags = ["-Zsanitizer=address"]
```

You need a nightly toolchain for sanitizers:

```bash
rustup run nightly cargo build --target x86_64-apple-darwin
```

### Watching RSS in production

For runtime monitoring, track the Resident Set Size periodically:

```rust
use std::fs;

fn get_memory_mb() -> f64 {
    #[cfg(target_os = "macos")]
    {
        let pid = std::process::id();
        let info = fs::read_to_string(format!("/proc/{}/statm", pid))
            .unwrap_or_default();
        // macOS: use sysctl or task_info
        0.0
    }
    #[cfg(target_os = "linux")]
    {
        let pid = std::process::id();
        let statm = fs::read_to_string(format!("/proc/{}/statm", pid))
            .unwrap_or_default();
        let rss_pages: f64 = statm.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        rss_pages * 4.0 / 1024.0 // 4KB pages to MB
    }
}
```

Log this value every 30 seconds during testing. If RSS grows linearly over hours, you have a leak.

## Profiling GPU-bound rendering

GPUI renders through the GPU via Metal on macOS. If your frame times are high but CPU profiles look clean, the bottleneck might be GPU-side: too many draw calls, large textures being uploaded every frame, or complex clipping paths.

The Metal Performance Shaders profiler (accessed through Instruments with the Metal template) shows GPU time per draw call. Look for individual draws that take more than a millisecond or total GPU time that exceeds your frame budget.

Common fixes include combining elements that share the same styling into single draw calls (which GPUI does automatically for most cases), reducing texture resolution for off-screen content, and avoiding per-frame texture uploads.

## A profiling workflow that works

Here is the process I follow when optimizing a GPUI app:

1. Run with the frame overlay on. Get a baseline for average and p99 frame time. If both are under budget, you are done.
2. Reproduce the jank. Find the specific action that causes frame time spikes. Scrolling? Opening a new view? Loading data?
3. Profile with Instruments or perf during the jank. Look for functions with high exclusive time. Focus on functions you own, not framework internals.
4. Fix the hottest function. This is usually one of the three patterns I described above: unnecessary re-renders, per-frame allocations, or blocking I/O.
5. Measure again. If the fix worked, average and p99 should drop. If they did not, you profiled the wrong thing.

Repeat until p99 stays under 16.67ms during normal use.

## Profiling in practice with gpui-starter

The [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) project includes a built-in frame time debugger that renders timing data directly in the window. It tracks render time, layout time, and draw time separately so you can tell whether a slow frame is caused by your code or by the rendering pipeline. You can find the setup instructions in the [getting started guide](/docs/getting-started/).

Good performance comes from measuring, finding the slowest part, and fixing it. Rust gives you the tools to make things fast once you know where to look. The profiling tools I covered here, frame timing, flamegraphs, allocation tracking, and GPU profiling, are the same ones I use every day. Start with the frame overlay and follow the jank.
