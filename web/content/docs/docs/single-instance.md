---
title: Single-Instance Guard
description: Ensure only one app instance runs with IPC forwarding for deep links.
---

When a user double-clicks a file association or follows a URL scheme link, the OS may launch a second copy of your application. Without a guard, you get two windows, two event loops, two sets of resources. The single-instance module in gpui-starter prevents this by acquiring a process-level lock on startup and forwarding any deep link arguments to the already-running instance.

The implementation lives in `src/platform/process/single_instance.rs`. It provides a preflight check that runs before the GPUI runtime starts, plus an IPC forwarding layer that sends deep links from secondary launches to the primary process.

If you are new to the project, the [getting started guide](/docs/getting-started) covers the build steps. The [architecture overview](/docs/architecture) explains how the module fits into the larger app structure.

## How it works

The guard operates in two phases: a synchronous preflight before GPUI initializes, and an async forwarding pipeline that runs for the lifetime of the primary instance.

```
Second launch starts
  -> preflight() acquires lock via SingleInstance
  -> Lock is already held by primary instance
  -> Deep link argument found (e.g. gpui-starter://open/project-42)
  -> Send link to primary via IPC local socket
  -> IPC fails? Append to queue file as fallback
  -> Second process exits immediately

Primary instance
  -> preflight() acquires lock successfully
  -> install() starts IPC listener + queue file poller
  -> Incoming deep link arrives
  -> Emits AppEventKind::DeepLinkReceived(link)
  -> App handles the event (navigate, open file, etc.)
```

The primary instance holds a lock through the `single_instance` crate, which uses platform-specific mutex primitives. On macOS this is a file lock; on Linux it uses abstract Unix domain sockets. The lock is released automatically when the primary process exits.

## Preflight check

The `preflight()` function runs before `gpui_platform::application().run()`. It returns a `Preflight` struct that tells `main()` whether to proceed.

```rust
let preflight = single_instance::preflight();
if !preflight.should_start {
    // Secondary instance: deep link was forwarded, exit now.
    return;
}
let startup_runtime = preflight.runtime;
let startup_deep_link = preflight.initial_deep_link;

let app_runtime = gpui_platform::application().with_assets(Assets);
app_runtime.run(move |cx| {
    app::init(cx);
    if let Some(runtime) = startup_runtime {
        single_instance::install(runtime, cx);
    }
    if let Some(link) = startup_deep_link {
        events::emit(events::AppEventKind::DeepLinkReceived(link), cx);
    }
    cx.activate(true);
    app::create_new_window("My App", cx);
});
```

The `Preflight` struct has three fields:

| Field | Type | Purpose |
|-------|------|---------|
| `should_start` | `bool` | `true` if this is the primary instance or if the lock failed (fails open) |
| `runtime` | `Option<SingleInstanceRuntime>` | The lock handle and IPC resources; `None` for secondary instances |
| `initial_deep_link` | `Option<String>` | The deep link argument from the current launch, if any |

When the lock acquisition itself fails (permissions issue, stale lock), the guard fails open. The app starts normally without single-instance protection rather than refusing to launch.

## IPC forwarding

When a secondary instance detects that the primary already holds the lock, it attempts to forward any deep link argument through a local socket connection.

```rust
fn send_forwarded_link_via_ipc(ipc_name: &str, link: &str) -> Result<(), std::io::Error> {
    let name = resolve_ipc_name(ipc_name)?;
    let mut stream = Stream::connect(name)?;
    writeln!(stream, "{link}")
}
```

The primary instance runs a listener thread (`gpui-ipc-forwarder`) that accepts connections, reads one line per connection, and sends valid deep links through a channel. The main async loop polls this channel every 180ms, batches any received links, and emits `AppEventKind::DeepLinkReceived` for each one.

The IPC name is resolved platform-appropriately. On systems that support namespaced sockets (macOS, Linux with abstract sockets), it uses a namespaced name like `com.gpui-starter.app.forwarder`. Otherwise it falls back to a filesystem socket path in the cache directory.

## Queue file fallback

If IPC fails (the listener thread has not started yet, or the socket is unavailable), the secondary instance writes the deep link to a queue file on disk.

```rust
fn append_forwarded_link(path: &PathBuf, link: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{link}");
        }
        Err(err) => {
            eprintln!("failed forwarding deep-link to primary instance: {err}");
        }
    }
}
```

The primary instance polls this file every 450ms, drains its contents, and emits events for each link found. The file lives at a platform-specific cache path:

- **macOS:** `~/Library/Caches/com.gpui-starter.GPUI Starter/runtime/forwarded-deep-links.queue`
- **Linux:** `~/.cache/gpui-starter/runtime/forwarded-deep-links.queue`
- **Fallback:** `$TMPDIR/gpui-starter-forwarded-deep-links.queue`

The queue file approach is slower than IPC but handles the case where the IPC listener has not finished initializing by the time a second launch arrives. It also works on systems where local sockets are restricted.

## Handling deep link events

Once the primary instance receives a forwarded deep link, it emits `AppEventKind::DeepLinkReceived(String)` through the app event queue. Your code subscribes to this event the same way as any other app event.

```rust
// In your event processing loop:
match event.kind {
    AppEventKind::DeepLinkReceived(url) => {
        // Parse the URL and navigate or open a resource
        if url.starts_with("gpui-starter://open/") {
            let path = url.trim_start_matches("gpui-starter://open/");
            navigate_to(path, cx);
        }
    }
    _ => {}
}
```

The deep link URL is a plain string. The module only validates that it starts with the `gpui-starter://` scheme prefix. Parsing the path and query parameters is your responsibility.

## Shutdown

The `shutdown()` function is called during app teardown to stop the IPC listener thread cleanly.

```rust
pub fn shutdown(cx: &mut App) {
    if let Some(runtime) = cx.try_global::<SingleInstanceRuntime>() {
        runtime.ipc_running.store(false, Ordering::SeqCst);
        // Nudge the blocking listener accept loop so it can observe ipc_running = false.
        let _ = send_forwarded_link_via_ipc(&runtime.ipc_name, "__shutdown__");
    }
}
```

It sets an `AtomicBool` flag and sends a dummy message to unblock the accept loop. The lock is released automatically when the process exits.

## Configuration

The module uses a few constants that you can change in `single_instance.rs`:

| Constant | Default | Purpose |
|----------|---------|---------|
| `INSTANCE_NAME` | `com.gpui-starter.app.instance` | Lock name for the `single_instance` crate |
| `SCHEME` | `gpui-starter://` | URL scheme prefix for deep link detection |
| IPC name | `com.gpui-starter.app.forwarder` | Local socket name for IPC forwarding |
| Queue poll interval | 450ms | How often the primary checks the fallback queue file |
| IPC poll interval | 180ms | How often forwarded IPC messages are flushed to the event queue |

To use a different URL scheme (for example, `myapp://`), change the `SCHEME` constant and register the scheme with the OS. On macOS this requires adding a `CFBundleURLSchemes` entry in your `Info.plist`. On Linux you need a `.desktop` file with `MimeType=x-scheme-handler/myapp`.

## Capability reporting

The module registers two capabilities in the diagnostics system:

- `single_instance` -- reports whether the lock was acquired successfully
- `second_instance_forwarding` -- reports IPC status; marks as degraded when the queue file fallback is in use

You can inspect these from the Diagnostics page in the app or through the `crate::capabilities` API. This helps when troubleshooting why deep links are not forwarding on a particular system.

## Disabling the guard

If you want multiple instances (for testing or multi-window workflows), you can skip the preflight check in `main.rs`:

```rust
fn main() {
    // Remove these lines to allow multiple instances:
    // let preflight = single_instance::preflight();
    // if !preflight.should_start { return; }

    let app_runtime = gpui_platform::application().with_assets(Assets);
    app_runtime.run(move |cx| {
        app::init(cx);
        cx.activate(true);
        app::create_new_window("My App", cx);
    });
}
```

Alternatively, run the binary with a different `INSTANCE_NAME` by modifying the constant, which lets you run isolated instances side by side.

## Platform notes

**macOS:** File locks are reliable. The OS also sends `application:openURL:` events for URL schemes, which can complement or replace IPC forwarding depending on your integration.

**Linux:** Abstract Unix domain sockets work on most distributions. On systems without namespace support (some minimal containers), the module falls back to filesystem sockets. The queue file fallback covers edge cases where the socket path is not writable.

**Windows:** The `single_instance` crate uses a named mutex. IPC uses named pipes. Both are well-supported. The queue file fallback uses `%TEMP%`.

## See also

- [Getting started](/docs/getting-started) for project setup and structure
- [Architecture](/docs/architecture) for how modules connect across the app
- [Routing and navigation](/docs/routing) for handling navigations triggered by deep links
- [Notifications](/docs/notifications) for showing alerts when deep link handling fails
- [Single-instance apps in Rust: preventing duplicate windows](/blog/single-instance-rust-app) for a tutorial walkthrough of the implementation
