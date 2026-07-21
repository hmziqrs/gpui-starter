---
title: "File watching in Rust desktop apps with notify"
description: "How to watch files for changes in Rust desktop apps using the notify crate: hot-reload, live configuration updates, and cross-platform event handling."
date: 2026-06-05
tags: [Rust, file-watching, notify]
draft: false
---

Every desktop app that loads files from disk eventually runs into the same question: how do I know when something changed? You might want to hot-reload a config file, or watch a log directory for new entries, or keep a text editor in sync with external changes on disk. Polling with a timer works, but it wastes CPU and has latency. The operating system already knows when files change. You just need to listen.

The `notify` crate is the standard way to do this in Rust. It wraps the native file change APIs on each platform (FSEvents on macOS, inotify on Linux, ReadDirectoryChangesW on Windows) behind a single interface.

## Setting up a basic watcher

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
notify = "8"
```

The core type is `RecommendedWatcher`, which picks the best backend for your OS automatically. Creating one requires a callback function that receives events:

```rust
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

fn main() -> Result<(), notify::Error> {
    let mut watcher = notify::recommended_watcher(|res| {
        match res {
            Ok(event) => println!("changed: {:?}", event),
            Err(e) => eprintln!("watch error: {e}"),
        }
    })?;

    watcher.watch(Path::new("./config.toml"), RecursiveMode::NonRecursive)?;

    // Keep the watcher alive
    std::thread::sleep(std::time::Duration::from_secs(60));
    Ok(())
}
```

That is the minimum viable watcher. `recommended_watcher` returns a `RecommendedWatcher` that calls your closure every time the filesystem reports a change. `RecursiveMode::NonRecursive` watches only the path you gave it. `RecursiveMode::Recursive` watches the directory and everything inside it.

One thing to know: the watcher stops when it is dropped. If you create it inside a function and return without storing it somewhere, it gets cleaned up immediately and you will never see events. This catches people off guard.

## Understanding event kinds

The callback receives `notify::Result<notify::Event>`. Each event has a `kind` field that tells you what happened, and a `paths` field telling you where it happened.

```rust
use notify::{Event, EventKind};

fn handle_event(event: Event) {
    match event.kind {
        EventKind::Create(_) => {
            println!("file created: {:?}", event.paths);
        }
        EventKind::Modify(kind) => {
            println!("file modified: {:?}", event.paths);
        }
        EventKind::Remove(_) => {
            println!("file removed: {:?}", event.paths);
        }
        EventKind::Access(_) => {
            // File was read (not changed). Usually ignorable.
        }
        EventKind::Other => {
            // Platform-specific meta-event. Usually safe to ignore.
        }
        _ => {}
    }
}
```

The modify and create kinds have sub-variants (like `ModifyKind::Data` for content changes vs `ModifyKind::Metadata` for permission changes). In practice, matching on the top-level category is enough for most apps. If you care specifically about content changes vs metadata changes, you can drill into the sub-variants.

There are also helper methods on `EventKind` for quick checks:

```rust
if event.kind.is_modify() {
    // handle any modification
}
if event.kind.is_create() {
    // handle any creation
}
```

## The event duplication problem

Here is the part that the README does not warn you about enough. Most file editors do not write files atomically. When you save a file in VS Code, it creates a temp file, writes to it, then renames it over the original. This means a single save can produce 3 to 5 events: a create for the temp file, one or two modify events, a remove for the old file, and a rename. Some editors also truncate the file first, which triggers an additional modify event with the file contents emptied.

This is not a bug in `notify`. It is how filesystem APIs work. The OS reports every individual operation, and applications compose multiple operations into what the user thinks of as "saving a file."

For configuration hot-reload, you usually want to debounce these events so you reload once instead of 5 times. You have two options: roll your own debounce with a timer, or use the `notify-debouncer-mini` crate.

```toml
[dependencies]
notify = "8"
notify-debouncer-mini = "0.6"
```

```rust
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use std::time::Duration;

let mut debouncer = new_debouncer(Duration::from_millis(200), None, |result: DebounceEventResult| {
    match result {
        Ok(events) => {
            for event in events {
                println!("debounced event: {:?}", event);
            }
        }
        Err(errors) => {
            for e in errors {
                eprintln!("error: {e}");
            }
        }
    }
})?;

debouncer.watcher()
    .watch(Path::new("./config.toml"), RecursiveMode::NonRecursive)?;
```

The debouncer waits 200ms after the last event before delivering. If 5 events arrive in 50ms (which is typical for a file save), you get one event instead of 5. Adjust the timeout based on your needs. 200ms works well for config reloads. For something like a live file browser, you might want 50ms to feel more responsive.

## Integrating with a UI framework

Watching files in a CLI tool is straightforward. Doing it inside a GUI app means dealing with threads. The `notify` watcher calls your callback from a background thread, and most UI frameworks require all state mutations to happen on the main thread.

The pattern I use is a channel between the watcher thread and the main loop:

```rust
use std::sync::mpsc;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event};

let (tx, rx) = mpsc::channel();

let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
    if let Ok(event) = res {
        let _ = tx.send(event);
    }
})?;

watcher.watch(Path::new("./config.toml"), RecursiveMode::NonRecursive)?;

// In your main loop or event loop tick:
while let Ok(event) = rx.try_recv() {
    if event.kind.is_modify() {
        reload_config();
    }
}
```

This separates the filesystem watching from the UI update. The background thread sends events into the channel. The main thread drains the channel during its update cycle, avoiding cross-thread UI mutations and data races.

## Watching directories recursively

Watching a single file works for config hot-reload. But what if you are building something like a file manager or a build tool that needs to track an entire directory tree?

```rust
watcher.watch(Path::new("./src"), RecursiveMode::Recursive)?;
```

That one flag change gives you events for every file and subdirectory under `./src`. There are two caveats.

First, platforms have limits on how many files you can watch simultaneously. On Linux, the default `inotify` limit is around 8192 watches (one per file or directory). For large monorepos, you can hit this ceiling. The fix is to increase `fs.inotify.max_user_watches` in sysctl, but your app cannot do that automatically. You should handle the error gracefully and tell the user what happened.

Second, newly created directories inside a recursive watch might not get watched on all platforms. The behavior differs between OSes. If you need to track directory creation, test on all target platforms.

## How gpui-starter uses notify

In [gpui-starter](/docs/getting-started/), we wrap `notify` in a service called `DesktopActions` that manages watcher lifetimes and reports state back to the UI. The service tracks how many watchers are active, exposes a snapshot of that state to the rendering layer, and cleans everything up on shutdown.

Here is the core of the watcher registration:

```rust
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

struct WatcherManager {
    next_id: u64,
    watchers: BTreeMap<u64, RecommendedWatcher>,
}

impl WatcherManager {
    fn watch(&mut self, path: PathBuf) -> Result<u64, notify::Error> {
        let id = self.next_id;
        self.next_id += 1;

        let mut watcher = notify::recommended_watcher(move |res| {
            match res {
                Ok(event) => tracing::debug!(kind = ?event.kind, "file changed"),
                Err(err) => tracing::warn!(error = %err, "watch error"),
            }
        })?;

        watcher.watch(&path, RecursiveMode::NonRecursive)?;
        self.watchers.insert(id, watcher);
        Ok(id)
    }

    fn unwatch(&mut self, id: u64) -> bool {
        self.watchers.remove(&id).is_some()
    }
}
```

Each watcher gets a numeric ID. The caller holds onto that ID and uses it to unregister later. This pattern avoids leaking file descriptors when a component unmounts or the user navigates away from a view that was watching a directory. The `shutdown` method clears all watchers at once during app teardown.

We use this for two things: watching the config directory for live preference changes, and watching the log directory for real-time log tailing in the diagnostics view.

## Cross-platform behavior differences

`notify` abstracts the platform APIs well, but the underlying behavior is not identical everywhere. These are the differences I have run into:

On macOS, FSEvents sometimes reports modify events for files that have not actually changed. This happens because macOS updates access timestamps on read. If you have `noatime` disabled (the default), just reading a file can trigger an event. Filter on `EventKind::Modify(ModifyKind::Data)` to ignore pure metadata changes.

On Linux, `inotify` does not report events for NFS mounts or certain pseudo-filesystems. If your app needs to watch network-mounted files, you will need a polling fallback.

On Windows, `ReadDirectoryChangesW` can miss events if the buffer overflows during heavy filesystem activity. The `notify` crate reports this as an `EventKind::Other` event. You should handle this case by re-scanning the directory.

None of these differences are fatal. Test on all target platforms and add fallback logic for edge cases. For most desktop apps watching local config files or project directories, `notify` works reliably across all three platforms.

## Error handling patterns

The `notify::Error` type carries two useful fields: `kind` (what went wrong) and `paths` (which paths are affected). The most common errors you will see:

- `PathNotFound` when you try to watch a path that does not exist yet
- `MaxFilesWatch` on Linux when you exceed the inotify limit
- `Io` for permission denied or other OS-level failures

A practical setup validates that the path exists before watching and handles startup errors without crashing:

```rust
fn safe_watch(path: &Path) -> Result<RecommendedWatcher, notify::Error> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    let mut watcher = notify::recommended_watcher(|res| {
        if let Err(e) = res {
            eprintln!("watch error: {e}");
        }
    })?;

    watcher.watch(path, RecursiveMode::Recursive)?;
    Ok(watcher)
}
```

The callback itself can also receive errors. These are runtime errors like buffer overflows or dropped watches. Log them, but do not crash. The watcher usually recovers on its own.

## When to use polling instead

`notify` is the right choice for most desktop apps. But there are cases where polling is genuinely better. If you are watching a file on a network drive, the native file watching APIs either do not work (NFS on Linux) or have high latency (SMB on macOS). A simple `std::fs::metadata` check every second or two is more reliable.

Another case is when you need content-level change detection. Filesystem events tell you *that* something changed, not *what* changed. If you need to compute a diff or detect specific content changes, you will end up reading the file anyway. In that scenario, polling with a content hash can be simpler than wiring up file events and then reading the file in response.

For a deeper look at how this fits into a real app architecture, check out the [architecture overview](/docs/architecture/) and the [theme system](/blog/theme-system-deep-dive/) which uses file watching internally for hot-reload during development.

## Getting started with gpui-starter

If you want to see this pattern in a working application, [gpui-starter](https://github.com/freeoxide/gpui-starter) ships with file watching built into the desktop actions service. The setup handles watcher registration, cleanup, and error reporting without extra configuration. Read the [getting started guide](/docs/getting-started/) to have a working app running in under five minutes.
