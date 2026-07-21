---
title: "Building a file manager in Rust with GPUI"
description: "Architecture for a file manager built with Rust and GPUI: async file operations, tree views, and drag-and-drop."
date: 2026-06-05
tags: [Rust, file-manager, GPUI]
draft: false
---

File managers look simple on the surface. A tree on the left, a file list on the right, a breadcrumb bar on top. Every OS ships one. But building one that feels fast and correct is surprisingly hard. You have to handle thousands of directory entries, async I/O that must not block the UI thread, file system watchers that fire events faster than you can process them, and drag-and-drop across nested scroll regions.

I built a file manager with Rust and GPUI. Here is what the architecture looks like, where the hard parts are, and what I would do differently next time.

## Why GPUI for a file manager

A file manager is fundamentally a rendering stress test. You might have 50,000 files in a directory. Each one has a name, an icon, a size, a modified date, and a selection state. The user expects scrolling to be instant, search to feel responsive, and drag selection to track the mouse at 60fps.

GPUI renders straight to the GPU through Metal on macOS. No DOM, no CSS recalculation, no JavaScript event loop. Just a flexbox layout pass followed by batched draw calls. A good fit for UIs that need to render large lists without dropping frames.

Rust's standard library helps too. `std::fs`, `walkdir`, `notify`, `tokio`. The data layer is solved. The challenge is wiring it all into a responsive UI.

## The data model

The file manager has three core data types:

```rust
#[derive(Clone, Debug)]
struct FileEntry {
    id: FileId,
    name: String,
    path: PathBuf,
    kind: FileKind,
    size: u64,
    modified: SystemTime,
    is_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
enum FileKind {
    File,
    Directory,
    Symlink,
}

struct FileTree {
    root: PathBuf,
    entries: HashMap<PathBuf, Vec<FileEntry>>,
    expanded: HashSet<PathBuf>,
}
```

`FileTree` is the source of truth. Directory contents keyed by absolute path. The `expanded` set tracks which directories are open in the sidebar. When a directory expands, we fetch its children and cache them in `entries`. When it collapses, we remove it from `expanded` but keep the cached entries so re-expanding is instant.

This is a warm state pattern, similar to what I described in the [state management guide](/blog/state-management-gpui-entities/). The catalog lives as plain values. We only create `Entity<FileList>` for the active directory view.

## Async directory scanning

The first mistake I made was scanning directories synchronously. Calling `std::fs::read_dir` inside `entity.update()` blocks the render loop. On an NFS mount or a slow SSD, that freezes the entire app for seconds.

The fix is to move I/O into async tasks spawned through `cx.spawn()`:

```rust
fn scan_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
    let weak = cx.entity().downgrade();

    cx.spawn(async move |mut cx| {
        let entries = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            let mut dir = std::fs::read_dir(&path)?;
            while let Some(entry) = dir.next().transpose()? {
                let metadata = entry.metadata()?;
                results.push(FileEntry {
                    id: FileId::new(),
                    name: entry.file_name().to_string_lossy().into(),
                    path: entry.path(),
                    kind: if metadata.is_dir() {
                        FileKind::Directory
                    } else if metadata.is_symlink() {
                        FileKind::Symlink
                    } else {
                        FileKind::File
                    },
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or UNIX_EPOCH,
                    is_hidden: entry.file_name().to_string_lossy().starts_with('.'),
                });
            }
            results.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(results)
        })
        .await??;

        weak.update(&mut cx, |this, cx| {
            this.tree.entries.insert(path, entries);
            cx.notify();
        }).ok();
    })
    .detach();
}
```

`tokio::spawn_blocking` moves the blocking `read_dir` call off the async runtime's thread pool. The result flows back through `weak.update()` on the main thread. `cx.notify()` triggers a re-render with the new data.

I sort the entries on the blocking thread. Sorting 50,000 file names takes a few milliseconds, which is fine on a background thread but noticeable if you do it during `render()`. Keep the render path as cheap as possible.

## Rendering the tree view

The sidebar tree is a recursive component. Each expanded directory renders its children indented one level. Collapsed directories render a disclosure arrow and their name.

```rust
fn render_tree_node(
    path: &Path,
    depth: usize,
    tree: &FileTree,
    selected: &Option<PathBuf>,
    cx: &mut Context<FileManager>,
) -> impl IntoElement {
    let name = path.file_name().unwrap().to_string_lossy();
    let is_expanded = tree.expanded.contains(path);
    let is_selected = selected.as_ref() == Some(path);
    let children = tree.entries.get(path).cloned().unwrap_or_default();

    div()
        .pl(px(depth as f32 * 16.0))
        .when(is_selected, |el| el.bg(cx.theme().selection))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(4.0))
                .child(Icon::new(if is_expanded { ChevronDown } else { ChevronRight }))
                .child(Label::new(name.to_string())),
        )
        .when(is_expanded, |el| {
            el.children(
                children
                    .iter()
                    .filter(|c| c.kind == FileKind::Directory)
                    .map(|child| {
                        Self::render_tree_node(
                            &child.path,
                            depth + 1,
                            tree,
                            selected,
                            cx,
                        )
                    }),
            )
        })
}
```

Recursion depth is bounded by the file system. Directory trees rarely exceed 15 levels in practice. Add a depth limit (say, 50) to prevent pathological cases from blowing the stack.

The `children` vector is cloned for borrow-checker reasons. For a sidebar showing 200 visible nodes, the clone cost is negligible.

## Drag and drop

Drag and drop in a file manager means three things: dragging files between directories in the tree, dragging files onto the main list view, and dragging files out of the app entirely.

GPUI provides `on_mouse_event` and hitbox testing for the in-app case. Here is the skeleton:

```rust
// In the tree node element
window.on_mouse_event({
    let path = path.to_path_buf();
    let hitbox = hitbox.clone();
    move |event: &MouseDownEvent, phase, window, cx| {
        if hitbox.is_hovered(window) && phase.bubble() {
            cx.stop_propagation();
            // Start drag: store the dragged path in a DragState global
            cx.global_mut::<DragState>().dragging = Some(path.clone());
        }
    }
});

// In the directory drop target
window.on_mouse_event({
    let target_dir = target_dir.clone();
    let hitbox = hitbox.clone();
    move |event: &MouseUpEvent, phase, window, cx| {
        if hitbox.is_hovered(window) && phase.bubble() {
            let drag = &mut cx.global_mut::<DragState>();
            if let Some(source) = drag.dragging.take() {
                cx.spawn(async move |mut cx| {
                    tokio::fs::rename(&source, target_dir.join(source.file_name().unwrap())).await.ok();
                    weak.update(&mut cx, |this, cx| {
                        this.refresh(&source.parent().unwrap(), cx);
                        cx.notify();
                    }).ok();
                }).detach();
            }
        }
    }
});
```

I use a `DragState` global to track what is being dragged. Simpler than threading state through component props. The drag starts on `MouseDown` in the source, ends on `MouseUp` in the target. File moves happen asynchronously through `tokio::fs::rename`, and both directories get refreshed on completion.

For drag-out to Finder or other apps, you need platform-specific interop. macOS requires `NSPasteboard` writes through the `objc` crate. I have not implemented that yet.

## File system watching

The sidebar tree needs to stay current when files change on disk. The `notify` crate provides cross-platform `inotify`/FSEvents/ReadDirectoryChangesW wrappers:

```rust
fn start_watcher(&mut self, cx: &mut Context<Self>) {
    let weak = cx.entity().downgrade();
    let root = self.tree.root.clone();

    cx.spawn(async move |mut cx| {
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let mut watcher = notify::recommended_watcher(move |res| {
            if let Ok(event) = res {
                let _ = tx.try_send(event);
            }
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        while let Some(event) = rx.recv().await {
            if let Some(affected_dir) = event.paths.first().and_then(|p| p.parent()) {
                weak.update(&mut cx, |this, cx| {
                    this.scan_directory(affected_dir.to_path_buf(), cx);
                }).ok();
            }
        }
        Ok(())
    })
    .detach();
}
```

The channel has a bounded capacity of 256 events. When file system events arrive faster than we process them, the channel applies backpressure instead of growing unbounded. Each event triggers a rescan of its parent directory. This is coarse but reliable. A smarter approach would batch events within a short window (say, 100ms) and deduplicate affected directories before rescanning.

I learned this the hard way. Running `cargo build` in a watched directory generates thousands of file events in seconds. Without batching, the app spent more time scanning directories than rendering frames.

## What I would do differently

Three things I would change on a rewrite.

First, I would virtualize the file list from the start. Rendering 10,000 `div()` elements because the directory has 10,000 files works, but it allocates more than necessary. A virtualized list renders only the visible rows plus a small overscan buffer. GPUI does not ship a virtualized list component, so you have to build one yourself. Track scroll offset, compute visible range, render only those indices.

Second, I would store file metadata in a slab allocator instead of a `HashMap<PathBuf, Vec<FileEntry>>`. Path strings are heavy. A `Vec<FileEntry>` with path-based lookup through an integer index is more cache-friendly and uses less memory. The [architecture docs](/docs/architecture/) describe this pattern in more detail.

Third, I would add cancellation tokens to directory scans. Right now, collapsing a directory does not cancel its in-flight scan. If you rapidly expand and collapse a large directory, you burn CPU on scans whose results get thrown away. Passing a `CancellationToken` from the `tokio-util` crate into the scan future and checking it before the final `weak.update()` would fix this.

## Getting started

Building a file manager taught me a lot about where GPUI excels and where it needs more library support. The rendering performance is excellent. The async story is solid. What is missing is off-the-shelf components for things like virtualized lists and drag-and-drop.

If you want to experiment with GPUI yourself, [gpui-starter](/docs/getting-started/) gives you a working project with sidebar navigation, theme hot-reload, and i18n out of the box. The source is on [GitHub](https://github.com/freeoxide/gpui-starter). Clone it, run `cargo run`, and start building.
