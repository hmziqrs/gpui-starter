---
title: "Building a text editor in Rust with GPUI"
description: "Architecture and implementation patterns for building a text editor in Rust using GPUI: rope-based buffers, GPU rendering, syntax highlighting, and performance."
date: 2026-06-05
tags: [Rust, text-editor, GPUI]
draft: false
---

Text editors are one of those problems that sound simple until you build one. Moving a cursor around a string is easy. Doing it efficiently across a 50,000-line file with syntax highlighting, multi-cursor editing, and a 16ms frame budget is hard.

I want to walk through how you'd architect a text editor using GPUI, the GPU-accelerated UI framework from the Zed team. GPUI gives you direct GPU rendering, a retained component model, and Rust's performance guarantees. It is the same framework that powers Zed itself, so the patterns here are proven against real workloads.

## Why GPUI for a text editor

Most text editors render through layers of abstraction. Electron-based editors go through JavaScript, a DOM, a CSS layout engine, and Chromium's compositor before anything hits the screen. That adds latency at every step. A keystroke has to travel through the JS event loop, update a React state tree, trigger a re-render, diff the virtual DOM, apply patches to the real DOM, then composite. Each step costs milliseconds.

GPUI skips all of that. Your Rust code talks directly to the GPU through Metal or Vulkan. Layout is computed in Rust using a flexbox algorithm. There is no DOM, no JavaScript runtime, no CSS parser between your buffer and the pixels.

For a text editor specifically, this matters because you need to do three things fast: measure text, lay out lines, and paint glyphs. GPUI's three-phase render pipeline (layout, prepaint, paint) gives you a natural place to do each of these with fine-grained control. You can cache text shaping results between frames and only recompute the lines that actually changed.

## The buffer: rope or gap buffer

The first architectural decision is how to store the text. A flat `String` means every insert or delete shifts all subsequent bytes. That is O(n) per edit and gets slow fast on large files. Two common solutions: gap buffers and ropes.

Gap buffers keep a single contiguous allocation with a gap at the cursor position. Inserts go into the gap in O(1). Moves jump the gap in O(n). This works well for a single cursor editing one location, which is why Neovim and Emacs use this approach internally.

Ropes split text into a balanced tree of leaf chunks (typically 1-4KB each). Inserts, deletes, and searches are O(log n). Ropes handle multiple cursors and large files better because edits to different parts of the document touch different leaves. This is what Zed uses, and it is what I would recommend for a new editor built on GPUI.

Here is a minimal rope-based buffer struct:

```rust
struct TextBuffer {
    rope: Rope,
    version: u32,
    // Track line starts for O(1) line number lookups
    line_starts: Vec<usize>,
}

impl TextBuffer {
    fn insert(&mut self, offset: usize, text: &str) {
        self.rope.insert(offset, text);
        self.version += 1;
        self.recompute_line_starts_around(offset, text.len());
    }

    fn delete(&mut self, range: Range<usize>) {
        self.rope.remove(range.clone());
        self.version += 1;
        self.recompute_line_starts_around(range.start, 0);
    }

    fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    fn text_for_line(&self, line: usize) -> Rope {
        let start = self.line_starts[line];
        let end = self.line_starts.get(line + 1)
            .copied()
            .unwrap_or(self.rope.len());
        self.rope.slice(start..end)
    }
}
```

The `version` field is important. It lets the rendering layer detect whether the buffer changed since the last frame and skip recomputation when nothing moved. You will see this pattern everywhere in GPUI: cache aggressively, invalidate on version change.

## The editor component

In GPUI, your editor is an `Entity<Editor>` that holds the buffer, cursor state, and selection ranges. The entity system gives you safe mutable access without fighting the borrow checker, since all state access goes through `entity.update(cx, |state, cx| ...)`.

```rust
struct Editor {
    buffer: Entity<TextBuffer>,
    cursors: Vec<Cursor>,
    selections: Vec<Selection>,
    scroll_offset: Point<Pixels>,
    viewport_lines: usize,
    syntax_highlights: Option<SyntaxHighlights>,
}

struct Cursor {
    position: usize,
    column_preference: Option<usize>,
}

struct Selection {
    anchor: usize,
    head: usize,
}

impl Editor {
    fn new(cx: &mut App) -> Entity<Self> {
        let buffer = cx.new(|_| TextBuffer::new());
        cx.new(|_| Self {
            buffer,
            cursors: vec![Cursor { position: 0, column_preference: None }],
            selections: vec![],
            scroll_offset: Point::default(),
            viewport_lines: 40,
            syntax_highlights: None,
        })
    }
}
```

Keyboard input goes through GPUI's action system. You register keybindings in your component and handle them in the entity:

```rust
impl Editor {
    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "backspace" {
            self.delete_backwards(cx);
        } else if let Some(ch) = event.keystroke.key.chars().next() {
            if !event.keystroke.modifiers.platform {
                self.insert_char(ch, cx);
            }
        }
    }

    fn insert_char(&mut self, ch: char, cx: &mut Context<Self>) {
        self.buffer.update(cx, |buf, _| {
            for cursor in &self.cursors {
                buf.insert(cursor.position, &ch.to_string());
            }
        });
        // Move cursors forward
        for cursor in &mut self.cursors {
            cursor.position += ch.len_utf8();
        }
        cx.notify();
    }
}
```

The `cx.notify()` call at the end is how you tell GPUI that this entity changed and needs to re-render. GPUI does not re-render on every state change. It waits for you to opt in. This is the right default for a text editor where you might update the buffer multiple times in a single key handler (for auto-closing brackets, indentation adjustments) before you want to paint.

## Rendering lines with the Element trait

A text editor is one of the few UI components where the high-level `Render` trait is not enough. You need direct control over painting because you are drawing hundreds of styled text runs per frame. This is where GPUI's `Element` trait comes in.

The Element trait gives you three phases: `request_layout`, `prepaint`, and `paint`. For a text editor, the breakdown looks like this:

1. `request_layout`: Tell GPUI the editor wants to fill its allocated space.
2. `prepaint`: Shape text for visible lines, compute hitboxes for mouse interaction.
3. `paint`: Draw background quads for selections, render styled text runs, paint the cursor.

```rust
impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = window.request_layout(
            Style {
                size: Size {
                    width: relative(1.).into(),
                    height: relative(1.).into(),
                },
                ..Default::default()
            },
            Vec::<LayoutId>::new(),
            cx,
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> EditorPrepaintState {
        let editor = self.editor.read(cx);
        let buffer = editor.buffer.read(cx);
        let line_height = self.line_height;
        let first_visible_line = (editor.scroll_offset.y.0 / line_height.0) as usize;
        let visible_count = (bounds.size.height.0 / line_height.0).ceil() as usize;

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        EditorPrepaintState {
            hitbox,
            first_line: first_visible_line,
            visible_count,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        prepaint: &mut EditorPrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.read(cx);
        let buffer = editor.buffer.read(cx);
        let line_height = self.line_height;

        for i in 0..prepaint.visible_count {
            let line_idx = prepaint.first_line + i;
            if line_idx >= buffer.line_count() { break; }

            let y = bounds.origin.y
                + px((i as f32) * line_height.0)
                - px(editor.scroll_offset.y.0 % line_height.0);

            let line_text = buffer.text_for_line(line_idx);

            window.paint_text(
                line_text.to_string(),
                self.text_style.clone(),
                bounds.origin + point(px(0.), y),
                paint_text_options(),
            );
        }

        // Paint cursor
        if let Some(cursor) = editor.cursors.first() {
            let cursor_line = buffer.line_for_offset(cursor.position);
            let cursor_col = buffer.column_for_offset(cursor.position);
            let cursor_x = bounds.origin.x + px(cursor_col as f32 * self.char_width.0);
            let cursor_y = bounds.origin.y
                + px((cursor_line - prepaint.first_line) as f32 * line_height.0);

            window.paint_quad(fill(
                Bounds::new(
                    point(cursor_x, cursor_y),
                    size(px(2.), line_height),
                ),
                cx.theme().cursor,
            ));
        }
    }
}
```

This is simplified, but it shows the core idea. Only visible lines get shaped and painted. Lines outside the viewport do zero work. The `line_height` and `char_width` values come from the font metrics and stay constant across frames, so you cache them once.

## Syntax highlighting

Syntax highlighting in a GPU-rendered editor is a rendering concern, not a storage concern. You do not store styled text in the buffer. The buffer holds plain bytes. Highlighting is a separate layer that assigns token types (keyword, string, comment, function) to byte ranges, and the paint phase looks up colors from a theme.

Tree-sitter is the standard choice here. It builds a concrete syntax tree incrementally. When you edit the buffer, it only re-parses the affected region, which keeps things fast even on large files.

The integration pattern looks like this:

```rust
struct SyntaxHighlights {
    tree: Tree,
    highlights: Vec<(Range<usize>, HighlightId)>,
    version: u32,
}

impl TextBuffer {
    fn update_syntax(&mut self, parser: &mut Parser, old_tree: Option<&Tree>) {
        let new_tree = parser.parse_with(
            |offset, _| self.rope.byte_slice(offset..).to_string().into_bytes(),
            old_tree,
        );
        if let Some(tree) = new_tree {
            // Walk the tree and collect token ranges
            let highlights = self.collect_highlights(&tree);
            self.syntax_highlights = Some(SyntaxHighlights {
                tree,
                highlights,
                version: self.version,
            });
        }
    }
}
```

During the paint phase, you intersect each visible line's byte range with the highlight ranges. Each intersection becomes a styled text run. GPUI's `paint_text` can render multiple runs with different styles in a single call, so you batch them per line.

## Scroll performance

Scrolling is where most editor prototypes fall apart. The naive approach (re-render everything on every scroll event) works at 100 lines. At 100,000 lines it stutters.

There are two things you need to get right.

First, do not recompute anything for lines outside the viewport. Compute your first visible line from the scroll offset, render only what fits on screen plus one line of overflow above and below for smooth partial-line scrolling.

Second, cache text shaping results. Shaping (converting characters to positioned glyphs) is expensive. Use the buffer version to invalidate caches only when text actually changes. Scrolling without editing means you reuse shaped text from the previous frame, just at different y coordinates.

```rust
struct ShapedLineCache {
    shapes: HashMap<usize, ShapedLine>,
    buffer_version: u32,
}

impl ShapedLineCache {
    fn get_or_shape(
        &mut self,
        line_idx: usize,
        line_text: &str,
        font: &Font,
        buffer_version: u32,
    ) -> &ShapedLine {
        if self.buffer_version != buffer_version {
            self.shapes.clear();
            self.buffer_version = buffer_version;
        }
        self.shapes.entry(line_idx)
            .or_insert_with(|| shape_line(line_text, font))
    }
}
```

This cache lives inside your `Editor` entity and persists across frames. A scroll event changes the scroll offset but not the buffer version, so every shaped line is a cache hit.

## Handling large files

Files over 100MB need special treatment. You cannot hold the entire rope in memory without consequence, and you cannot syntax-highlight the whole thing.

Load the file in chunks. Keep a sliding window of the visible region plus a few thousand lines of context above and below. As the user scrolls, you load new chunks and evict old ones. The rope's tree structure makes this natural because you can swap leaf nodes without rebuilding the tree.

For syntax highlighting on large files, tree-sitter's incremental parsing helps but still has limits. You can restrict highlighting to the visible viewport plus a small lookahead region. The rest of the file gets no highlighting until the user scrolls there. Zed does this, and most users never notice because they are not looking at line 47,000 while editing line 12.

## Undo and redo

Undo is a requirement, not a feature. The standard approach is an undo stack of operations rather than full snapshots. Each operation records what changed and how to reverse it:

```rust
enum EditOp {
    Insert { offset: usize, text: String },
    Delete { offset: usize, text: String },
}

struct UndoStack {
    ops: Vec<EditOp>,
    index: usize,
}

impl UndoStack {
    fn push(&mut self, op: EditOp) {
        self.ops.truncate(self.index);
        self.ops.push(op);
        self.index += 1;
    }

    fn undo(&mut self) -> Option<&EditOp> {
        if self.index > 0 {
            self.index -= 1;
            Some(&self.ops[self.index])
        } else {
            None
        }
    }
}
```

You group consecutive single-character inserts into one undo entry (so typing a word is one undo step, not ten). This grouping is a heuristic, but it is what users expect.

For a deeper treatment of undo patterns in Rust desktop apps, see the [undo and redo guide](/blog/undo-redo-rust-desktop-apps/).

## Closing notes

Building a text editor in GPUI comes down to four things: a rope-based buffer for efficient edits, an entity that holds cursor and selection state, an Element implementation that only renders visible lines, and a syntax highlighting layer that stays in sync incrementally. GPUI's architecture maps cleanly to these concerns. The three-phase render pipeline gives you the control you need without forcing you to manage GPU resources yourself.

If you want to see these patterns in a working app, [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) provides a production-ready boilerplate with multi-page navigation, theme support, SQLite persistence, and all the scaffolding you need to start building. The [getting started guide](/docs/getting-started/) walks through setting up your first project in a few minutes.
