---
title: "Single-instance apps in Rust: preventing duplicate windows"
description: "Prevent duplicate Rust desktop app instances with file locks and IPC forwarding. Covers cross-platform deep link handling and queue file fallbacks."
date: 2026-05-23
tags: [Rust, desktop, architecture]
draft: false
---

Double-click an app icon. Nothing happens. Click again. Still nothing. Minimize all windows and discover two copies running side by side, each with its own state, its own open files, its own idea of what "saved" means.

This is the single-instance problem. It shows up the moment your desktop app handles persistent state. Config files, local databases, background processes. If two copies touch the same resource concurrently, you get corrupted data. I ran into this the first week of shipping [gpui-starter](/docs/getting-started/), and it took down a user's preferences file within hours.

## Why single-instance matters

Resource contention is the obvious problem. Two processes writing to the same SQLite database, the same preferences file, or the same log output will step on each other. The dangerous part is timing. It works fine in testing, then fails in production at the worst moment.

User confusion is harder to measure but just as real. If someone opens a file from Finder and your app is already running, they expect the file to open in the existing window. Launching a second copy, even briefly, feels broken. The window flashes and disappears, and the user wonders whether anything happened.

The fix is to detect that an instance is already running, forward any arguments or deep links to it, and exit the second process immediately.

## The file-lock approach

On macOS and Linux, the standard technique is a file lock. You pick a well-known path (usually in a runtime or cache directory) and try to acquire an exclusive lock on it. If the lock succeeds, you are the first instance. If it fails, someone else got there first.

The `single_instance` crate wraps this into a straightforward API:

```rust
use single_instance::SingleInstance;

const INSTANCE_NAME: &str = "com.myapp.instance";

let instance = SingleInstance::new(INSTANCE_NAME)?;

if instance.is_single() {
    // We are the first instance. Start the app normally.
} else {
    // Another instance is already running. Forward arguments and exit.
}
```

The crate uses `flock` on Unix and a named mutex on Windows, giving cross-platform behavior without platform-specific code. The lock is held for the lifetime of the `SingleInstance` struct. When the process exits, the OS releases the lock automatically.

## Forwarding arguments to the running instance

Detecting a duplicate is half the problem. The other half is communicating with the instance that is already running. When a user clicks a `myapp://settings` link in a browser, the OS launches your app (or focuses it if running). If the app is already running, the second launch needs to send that URL to the first instance before exiting.

There are two common approaches: IPC through local sockets, and a filesystem-based queue file as a fallback.

gpui-starter uses both. The IPC path is primary because it is fast and reliable. The queue file exists for environments where local sockets are unavailable or restricted (certain sandboxed macOS setups, some Linux container configurations).

Here is the core of the IPC forwarder, simplified from the actual implementation:

```rust
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Stream, prelude::*,
};
use std::io::{BufRead, BufReader, Write};

fn send_forwarded_link(ipc_name: &str, link: &str) -> Result<(), String> {
    let name = resolve_ipc_name(ipc_name).map_err(|e| e.to_string())?;
    let mut stream = Stream::connect(name).map_err(|e| e.to_string())?;
    writeln!(stream, "{link}").map_err(|e| e.to_string())
}

fn resolve_ipc_name(name: &str) -> std::io::Result<Name<'_>> {
    if GenericNamespaced::is_supported() {
        name.to_ns_name::<GenericNamespaced>()
    } else {
        name.to_fs_name::<GenericFilePath>()
    }
}
```

The `GenericNamespaced` check handles the platform split. On Windows and macOS, named pipes work natively. On Linux, the fallback is a Unix domain socket file. The `interprocess` crate abstracts this so the calling code does not need to branch on its own.

### The listener on the primary instance

The primary instance sets up a listener thread that accepts connections, reads one line per connection, and dispatches the payload as an application event:

```rust
fn start_ipc_listener(ipc_name: String, cx: &mut App) {
    let name = resolve_ipc_name(&ipc_name).expect("resolve ipc name");
    let listener = ListenerOptions::new().name(name).create_sync().unwrap();

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let mut reader = BufReader::new(conn);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() {
                let link = line.trim().to_string();
                if link.starts_with("myapp://") {
                    // Send `link` to the main thread via a channel
                }
            }
        }
    });
}
```

One connection, one line, one payload. This keeps the protocol simple and avoids framing issues.

## Deep link handling in practice

Deep links are the most common reason a second instance gets launched. The OS registers a URL scheme (like `gpui-starter://settings`), and when the user clicks a link with that scheme, the OS invokes your binary with the URL as a command-line argument.

The flow looks like this:

1. The OS launches your binary with `myapp://open/file.txt` as an argument.
2. Your `main` function runs the single-instance check.
3. If another instance is running, the URL gets forwarded via IPC and the second process exits.
4. The running instance receives the URL and opens the file or navigates to the right view.

In gpui-starter, this is wired through the event system. The forwarded link arrives as an `AppEventKind::DeepLinkReceived` event, and any part of the app can subscribe to handle it:

```rust
// In main.rs
let preflight = single_instance::preflight();
if !preflight.should_start {
    return;
}

app.run(move |cx| {
    if let Some(runtime) = preflight.runtime {
        single_instance::install(runtime, cx);
    }
    if let Some(link) = preflight.initial_deep_link {
        events::emit(events::AppEventKind::DeepLinkReceived(link), cx);
    }
});
```

The `preflight` function does all the work: checks for a running instance, forwards any deep link if one exists, and returns a struct that tells `main` whether to proceed or exit. This keeps `main.rs` small. The decision logic lives in one place.

## The queue file fallback

When IPC is unavailable, gpui-starter falls back to a queue file in the system cache directory. The second instance appends the deep link to the file and exits. The primary instance polls this file every 450ms, drains any new lines, and dispatches them as events.

Slower and less elegant than IPC, but it works everywhere, including environments where socket creation is restricted. The implementation marks the IPC capability as "degraded" in the capabilities system, so the app can report this state to the user or to telemetry.

## What to watch for

File locks are per-machine, not per-user. If your app supports multiple user accounts running simultaneously, include the user ID in the instance name. Otherwise the second user's launch will silently fail.

On macOS, the system tries to help by sending an `application:openURL:` delegate message to the running instance instead of launching a new process. This is the ideal path, and it means your IPC forwarding code will rarely run on macOS. But it is not guaranteed (terminal launches bypass this), so you need both paths.

On Linux, make sure your socket path is under `$XDG_RUNTIME_DIR` or `/tmp`. Paths longer than 108 characters (the Unix socket path limit) will silently fail on some kernels.

## Closing notes

Single-instance enforcement seems optional until you ship without it. The implementation in gpui-starter is about 300 lines of Rust, handles three platforms, includes a queue file fallback, and wires through the event system so any component can react to forwarded deep links.

If you are building a desktop app in Rust, add single-instance support early. It is much harder to retrofit after you have already shipped state management that assumes exclusive access.

The [architecture guide](/docs/architecture/) covers how single-instance fits into the broader app lifecycle. For more on the event system that dispatches forwarded links, see the [crash reporting post](/blog/rust-desktop-crash-reporting/), which uses the same event bus pattern.

The [getting started guide](/docs/getting-started/) shows the full setup.
