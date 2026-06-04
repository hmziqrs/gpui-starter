---
title: "Building creative applications with Rust and GPUI"
description: "Using Rust and GPUI for creative desktop applications: design tools, image editors, and music production software."
date: 2026-06-05
tags: [Rust, creative, GPUI]
draft: false
---

Creative tools have specific demands. A pixel editor needs sub-millisecond pointer latency. A music sequencer needs sample-accurate timing. A vector design tool needs to redraw hundreds of layers at 60fps without dropping a frame. Most creative software is written in C++ for exactly these reasons. Rust with GPUI gives you comparable performance with better safety guarantees, and the developer experience is surprisingly good once you learn the patterns.

I want to walk through what it actually looks like to build creative applications in GPUI. Not toy demos, but the kind of software where frame timing matters and wrong memory access means corrupted user work.

## Why GPUI fits creative work

GPUI renders straight to the GPU through Metal on macOS (and Vulkan on other platforms). There is no DOM, no CSS engine, no JavaScript runtime sitting between your code and the screen. For a text editor or settings panel that difference is nice. For a canvas that redraws on every mouse move, it is the difference between fluid and frustrating.

The render pipeline runs in three passes: layout, prepaint, and paint. Each frame, GPUI figures out where things go, computes hit regions for input, then batches draw calls to the GPU. When you're building something like a drawing canvas where the user is dragging a brush at 120Hz, this matters. You can skip layout for unchanged elements and go straight to repainting the dirty region.

Memory safety is the other big reason. Creative apps manipulate large buffers of pixel data, audio samples, and geometry. A use-after-free bug in a C++ image editor can corrupt the canvas. The same bug in Rust is caught at compile time. That matters more when your users are working on files they spent hours on.

## A pixel canvas in GPUI

Let's build a minimal pixel drawing canvas to see the patterns. The core idea: maintain a framebuffer, render it as a texture, and forward pointer events to a drawing function.

```rust
struct PixelCanvas {
    width: usize,
    height: usize,
    buffer: Vec<u8>,       // RGBA, row-major
    brush_color: [u8; 4],
    brush_size: usize,
}

impl PixelCanvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            buffer: vec![255; width * height * 4], // white background
            brush_color: [0, 0, 0, 255],
            brush_size: 3,
        }
    }

    fn paint_pixel(&mut self, x: usize, y: usize) {
        let half = self.brush_size / 2;
        for dy in 0..self.brush_size {
            for dx in 0..self.brush_size {
                let px = x + dx as usize;
                let py = y + dy as usize;
                if px < self.width && py < self.height {
                    let offset = (py * self.width + px) * 4;
                    self.buffer[offset..offset + 4]
                        .copy_from_slice(&self.brush_color);
                }
            }
        }
    }
}
```

This is a flat RGBA buffer. No abstractions, no intermediate representations. For a pixel editor this is exactly what you want. The buffer maps directly to what the GPU needs to display.

## Rendering the canvas as an Element

GPUI uses the Element trait for anything that paints to the screen. Here is how you wrap the canvas buffer so GPUI can display it as a texture quad.

```rust
impl Element for CanvasElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = window.request_layout(
            Style {
                size: size(px(self.canvas.width as f32), px(self.canvas.height as f32)),
                ..Default::default()
            },
            vec![],
            cx,
        );
        (layout_id, ())
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        window.paint_quad(
            bounds,
            Corners::default(),
            Hsla::black(),
            Edges::default(),
            Hsla::black(),
        );
        // In a real app, upload self.canvas.buffer as a GPU texture
        // and paint it as a textured quad within bounds
    }
}
```

A production canvas would upload the buffer as a Metal texture and paint that instead of a flat quad. The [GPUI rendering guide](/blog/building-desktop-apps-rust-gpui/) covers the texture upload path in more detail. For now the key takeaway is that your data lives in plain Rust types and you control exactly when and how it hits the GPU.

## Handling pointer input for drawing

Creative tools live or die on input latency. GPUI gives you pointer events at native resolution, delivered through the window's event system. Here is how you wire up a brush stroke.

```rust
fn handle_pointer_move(
    canvas: &mut PixelCanvas,
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    _cx: &mut App,
) {
    let local_x = position.x - bounds.origin.x;
    let local_y = position.y - bounds.origin.y;

    let px = (local_x.0 as usize).min(canvas.width.saturating_sub(1));
    let py = (local_y.0 as usize).min(canvas.height.saturating_sub(1));

    canvas.paint_pixel(px, py);
    // Trigger a repaint for the affected region
    window.request_frame();
}
```

Each pointer move event paints pixels into the buffer and requests a new frame. On a 120Hz display that means your brush responds within 8 milliseconds of the physical mouse movement. That is fast enough to feel immediate.

For smoother strokes you would interpolate between the previous and current pointer position using Bresenham's line algorithm or something similar. The principle stays the same: write to the buffer, request a frame.

## Audio processing with Rust

Pixel data is one thing. Audio has harder real-time constraints. A dropped frame in a drawing app means a visual glitch. A dropped buffer in a DAW means an audible pop that ruins the take.

Rust's ownership model helps here. You can design your audio pipeline so that the callback that feeds the sound card has exclusive access to the mix buffer. No locks, no atomics, no chance of two threads writing to the same sample.

```rust
const SAMPLE_RATE: u32 = 44100;
const BUFFER_SIZE: usize = 256;

struct AudioEngine {
    mix_buffer: [f32; BUFFER_SIZE],
    oscillator: Oscillator,
}

impl AudioEngine {
    fn fill_buffer(&mut self, output: &mut [f32]) {
        for (i, sample) in output.iter_mut().enumerate() {
            let t = i as f32 / SAMPLE_RATE as f32;
            *sample = self.oscillator.next(t);
        }
    }
}

struct Oscillator {
    frequency: f32,
    phase: f32,
}

impl Oscillator {
    fn next(&mut self, t: f32) -> f32 {
        let sample = (self.phase * 2.0 * std::f32::consts::PI).sin();
        self.phase += self.frequency / SAMPLE_RATE as f32;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        sample * 0.5
    }
}
```

This is a bare-bones sine oscillator. A real DAW would mix multiple tracks, apply effects, and handle MIDI input. But the structure scales: your audio callback owns the mix buffer exclusively, fills it, and hands it to the sound card. No garbage collector pauses. No reference counting overhead. Just math into memory.

## Undo and redo for creative state

Creative apps need undo. Every brush stroke, every filter, every parameter change needs to be reversible. The [undo system in gpui-starter](/blog/undo-redo-rust-desktop-apps/) uses a command pattern, and it works well for creative tools too.

```rust
struct StrokeAction {
    pixels: Vec<(usize, usize, [u8; 4])>,  // (x, y, previous_color)
}

impl UndoAction for StrokeAction {
    fn undo(&self, canvas: &mut PixelCanvas) {
        for &(x, y, prev_color) in &self.pixels {
            let offset = (y * canvas.width + x) * 4;
            canvas.buffer[offset..offset + 4].copy_from_slice(&prev_color);
        }
    }

    fn redo(&self, canvas: &mut PixelCanvas) {
        for &(x, y, color) in &self.pixels {
            let offset = (y * canvas.width + x) * 4;
            canvas.buffer[offset..offset + 4].copy_from_slice(&color);
        }
    }
}
```

Each stroke records the previous pixel colors before painting. Undo restores them. Redo reapplies the stroke colors. For large canvases this can get expensive in memory, and production apps use tile-based snapshots or copy-on-write instead. But the command pattern gives you a clean interface regardless of the storage strategy.

## Threading considerations

Creative apps have two threads that matter: the UI thread and the processing thread. GPUI runs your UI on the main thread. Heavy computation (image filters, audio rendering, export) should run on background threads and communicate results back through channels.

```rust
fn apply_gaussian_blur(
    canvas: &PixelCanvas,
    radius: f32,
) -> impl Future<Output = PixelCanvas> {
    let buffer = canvas.buffer.clone();
    let width = canvas.width;
    let height = canvas.height;

    async move {
        // Run the blur on a background thread
        let blurred = spawn_blocking(move || {
            gaussian_blur_rgba(&buffer, width, height, radius)
        }).await;

        PixelCanvas {
            width,
            height,
            buffer: blurred,
            brush_color: canvas.brush_color,
            brush_size: canvas.brush_size,
        }
    }
}
```

The UI stays responsive while the blur runs. When it finishes, the async block resolves and you swap in the new canvas buffer. GPUI's entity system handles the thread-safe state update. The [state management with GPUI entities](/blog/state-management-gpui-entities/) post covers this pattern in depth.

## What about cross-platform?

GPUI currently targets macOS with Metal rendering. Linux support through Vulkan is in progress. If you need to ship on Windows today, GPUI is not ready for that.

For creative tools this is less of a blocker than it sounds. Many professional creative apps ship on macOS first. Final Cut Pro, Logic Pro, and Sketch are macOS exclusive. Affinity Designer launched on Mac before expanding. If your target audience is designers and artists, macOS coverage gets you most of the way there.

## When to pick something else

GPUI is a good fit for creative apps that need custom rendering and tight frame budgets. It is overkill for a simple form app or a settings utility. If your app is mostly lists and text inputs, [Tauri or Iced](/blog/gpui-vs-egui-vs-iced/) will get you there faster with less boilerplate.

For creative tools, GPUI gives you GPU-direct rendering, Rust's memory safety, and control over the paint pipeline through the Element trait. There is no DOM or browser compositing layer to work around. You write Rust, it draws pixels.

## Getting started

If you want to experiment with GPUI for creative applications, [gpui-starter](/docs/getting-started/) provides a working boilerplate with multi-page navigation, theming, a command palette, and the usual setup work already done. The source is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate) and the docs cover everything from your first component to shipping a signed macOS build.
