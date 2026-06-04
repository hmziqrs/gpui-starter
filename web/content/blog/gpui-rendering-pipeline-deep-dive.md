---
title: "How GPUI renders: inside the GPU-accelerated pipeline"
description: "A technical deep dive into GPUI's rendering pipeline: Metal shaders, primitive batching, texture atlas, subpixel text, and GPU optimization techniques."
date: 2026-06-05
tags: [GPUI, rendering, deep-dive]
draft: false
---

Most Rust UI frameworks do their rendering on the CPU. GPUI takes a different route, handing nearly everything to the GPU through Metal on macOS (and Vulkan via wgpu on Linux). I spent time reading through the Zed source to understand how a frame actually gets from your Rust code to pixels on screen. Here is what happens inside GPUI's rendering pipeline.

## The frame lifecycle: three phases

Every frame in GPUI goes through three distinct phases before anything reaches the GPU. When you implement the `Element` trait, you see these phases directly:

```rust
impl Element for MyElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn request_layout(&mut self, _, window: &mut Window, cx: &mut App)
        -> (LayoutId, Self::RequestLayoutState) { /* ... */ }

    fn prepaint(&mut self, _, bounds: Bounds<Pixels>, _, window: &mut Window, cx: &mut App)
        -> Self::PrepaintState { /* ... */ }

    fn paint(&mut self, _, bounds: Bounds<Pixels>, _, _, window: &mut Window, cx: &mut App) {
        window.paint_quad(PaintQuad::filled(bounds, Background::Color(rgb(0xFF0000))));
    }
}
```

Layout runs first. GPUI uses Taffy (a Flexbox implementation) to compute sizes and positions for every element. This produces a `LayoutId` that tracks the element's computed bounds. Each element reports what size it wants, and the layout engine resolves the final coordinates.

Prepaint comes next. Here, elements commit their bounds to the current frame for hit-testing. This is also where you do any work that needs to know the final position but should happen before drawing, like text shaping (which I will get to shortly).

Paint is where actual primitives get emitted. Your element calls methods like `paint_quad`, `paint_path`, or `paint_glyph` to push visual data into a `Scene` object. The GPU has not been touched yet. At this point, GPUI is just building a list of things to draw.

## The Scene: GPUI's retained primitive buffer

The `Scene` struct is the central data structure that collects everything your UI wants to draw in a frame. Looking at the source, it holds sorted vectors of typed primitives:

```rust
pub struct Scene {
    pub shadows: Vec<Shadow>,
    pub quads: Vec<Quad>,
    pub paths: Vec<Path<ScaledPixels>>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
}
```

Each primitive carries a `DrawOrder` (a `u32`) that determines its z-position. When you call `push_layer` and `pop_layer`, GPUI tracks a stack of draw orders. Every primitive inserted into the scene gets tagged with the current layer's order, or a fresh one from the `BoundsTree` if no layer is active.

When painting finishes, `Scene::finish()` sorts each primitive vector by draw order. That sorting step is what makes batching work.

## Batch formation and draw call minimization

After sorting, GPUI walks all primitive vectors simultaneously using a `BatchIterator`. It peeks at the front of each vector, picks whichever has the lowest draw order, and groups consecutive primitives of the same type into a single batch. The result is a `PrimitiveBatch` enum:

```rust
pub enum PrimitiveBatch {
    Shadows(Range<usize>),
    Quads(Range<usize>),
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites { texture_id: AtlasTextureId, range: Range<usize> },
    SubpixelSprites { texture_id: AtlasTextureId, range: Range<usize> },
    PolychromeSprites { texture_id: AtlasTextureId, range: Range<usize> },
    Surfaces(Range<usize>),
}
```

Notice that sprite batches also group by `texture_id`. This is a standard GPU optimization: switching textures between draw calls is expensive. Keeping all sprites that reference the same atlas texture in one batch means fewer state changes on the GPU.

The Metal renderer then takes each batch and issues a single draw call per batch using instanced rendering. Instead of drawing one quad at a time, it packs all quads into an instance buffer and renders them with one `drawPrimitives` call. This is how GPUI renders an entire editor view with a small number of draw calls.

## The Metal renderer: one pipeline per primitive type

On macOS, `MetalRenderer` creates a separate `RenderPipelineState` for each primitive kind. Looking at the struct fields:

```rust
pub(crate) struct MetalRenderer {
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    // ...
}
```

Each pipeline has its own Metal shader. Quads get a simple vertex/fragment pair that handles rounded corners and background colors. Shadows use a blur radius parameter with the element's corner radii. Paths use 4x MSAA (multisample anti-aliasing) rasterized into an intermediate texture before being composited into the main framebuffer.

The renderer also uses an instance buffer pool. Instead of allocating fresh GPU buffers every frame, it recycles buffers from a pool. On Apple Silicon (unified memory), buffers use `StorageModeShared` with write-combined CPU caching, which is the right choice for data written once by the CPU and consumed once by the GPU.

## The texture atlas: where glyphs and images live

GPUI uses a texture atlas to avoid creating individual GPU textures for every glyph and image. The `MetalAtlas` maintains separate atlas lists for monochrome (A8Unorm) and polychrome (BGRA8Unorm) textures. Bin-packing is handled by `etagere`, a bucketed atlas allocator.

When a glyph needs to be rasterized, GPUI checks if it already exists in the atlas by looking up an `AtlasKey`. If it misses, the platform text system rasterizes the glyph, the atlas allocates space (potentially growing the texture up to 16384x16384), and uploads the bytes to the Metal texture. Future frames referencing the same glyph at the same subpixel position hit the cache and skip rasterization entirely.

This approach is common in GPU-accelerated text rendering. Chrome, Firefox, and macOS's Core Animation all use similar atlas strategies. The tradeoff is memory: the atlas consumes GPU memory proportional to the number of unique glyphs in use. For a text editor like Zed, this stays reasonable because you tend to use a limited set of characters repeatedly.

## Text rendering: shaping, subpixel, and caching

GPUI's text pipeline has three stages. First, font resolution maps a `Font` struct (family name, weight, style) to a `FontId` through the platform text system, falling back through a default stack if the requested font is unavailable.

Second, text shaping converts characters into positioned glyphs. The `WindowTextSystem::shape_line` method takes a `SharedString`, a font size, and an array of `TextRun` objects (each specifying a font, color, and underline/strikethrough style for a byte range). It produces a `ShapedLine` containing the glyph positions and decoration runs. Multi-line text goes through `shape_text`, which splits on newlines and optionally wraps at a given width using `LineWrapper`.

Third, rasterization renders each glyph into the texture atlas. GPUI supports subpixel rendering on macOS with 4 subpixel variants along the X axis (captured as `SUBPIXEL_VARIANTS_X = 4`). The `RenderGlyphParams` struct includes a `subpixel_variant: Point<DevicePixels>` that shifts the glyph by a fraction of a pixel before rasterization, producing different bitmaps for different subpixel positions. This gives noticeably sharper text at the cost of 4x the glyph cache entries.

The layout cache is also worth noting. `LineLayoutCache` retains layouts from the previous frame and reuses them when text has not changed. For an editor where most of the screen stays the same between keystrokes, this cache eliminates a huge amount of redundant shaping work.

Here is what text rendering looks like from the element side:

```rust
fn paint(&mut self, _: &mut Self::RequestLayoutState, _: &mut Self::PrepaintState,
         window: &mut Window, cx: &mut App) {
    let shaped_line = window.text_system().shape_line(
        "Hello, GPUI".into(),
        px(16.0),
        &[TextRun {
            len: 12,
            font: Font { family: ".SystemUIFont".into(), ..Default::default() },
            color: Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 },
            ..Default::default()
        }],
        None,
    );
    shaped_line.paint(Origin::zero(), bounds, &[], window, cx);
}
```

## Layers and compositing

GPUI supports explicit layers through `Scene::push_layer` and `pop_layer`. Layers create isolated rendering groups with their own draw order range. When the `BatchIterator` encounters a layer boundary, it acts as a compositing boundary: all primitives within a layer are rendered before any primitive from the next layer.

This matters for things like modals, popovers, and overlays. A dropdown menu renders into its own layer so it always appears above the content beneath it, regardless of the specific draw orders involved. The Metal renderer handles this by issuing separate render passes for each layer when needed, or by ensuring draw order through the sorting mechanism.

## Performance characteristics

A few numbers from running Zed and my own [GPUI applications](/docs/getting-started/). A typical editor frame with syntax highlighting, a file tree, and a terminal panel produces around 15 to 30 draw calls. The texture atlas for a monospace font at 14px with common ASCII characters sits around 512KB of GPU memory. Frame times on an M1 MacBook Air hover around 2 to 4ms for a full re-render of a 4K display.

The main bottleneck is usually text shaping, not GPU rendering. GPUI mitigates this with its layout cache and by using `SharedString` (an Arc-based string type that avoids heap allocations when the text has not changed between frames). If you profile a GPUI app, you will see most CPU time in the layout and prepaint phases, not in the paint phase or GPU submission.

## Building on GPUI

If you want to try building something with this pipeline, [gpui-starter](/docs/getting-started/) is a production-ready boilerplate that sets up Metal rendering, window management, theming, and persistence out of the box. You can find the full source and examples on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).

For more background on how GPUI's component model works, see the [getting started guide](/blog/getting-started-with-gpui/). If you are curious about how the theme system feeds colors into the rendering pipeline, the [theme system deep dive](/blog/theme-system-deep-dive/) covers that connection.
