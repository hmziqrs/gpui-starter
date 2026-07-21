---
title: "App lifecycle management in Rust: startup to shutdown"
description: "Managing the lifecycle of a Rust desktop application: startup sequences, state machines, shutdown handling, and crash recovery."
date: 2026-06-05
tags: [Rust, lifecycle, architecture]
draft: false
---

A desktop application starts, runs, and stops. How you manage each transition determines whether your app feels professional or flaky. Skip a shutdown step and you lose user data. Skip a crash check and you silently corrupt state on the next launch. The lifecycle is not boilerplate you paste in and forget. Every other feature depends on it.

How to structure startup, runtime state tracking, graceful shutdown, and crash recovery in a Rust desktop app. The patterns come from building [gpui-starter](https://github.com/freeoxide/gpui-starter), a boilerplate for GPUI desktop applications.

## Modeling lifecycle as a state machine

The first thing to get right is the enumeration. Your app is never in a vague "doing stuff" state. It is in exactly one of a small number of well-defined stages:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleStage {
    Starting,
    Running,
    ShuttingDown,
    Crashed,
}
```

Four stages. Not forty. You can reason about every piece of code by asking "which stage am I in?" A notification service checks `Running` before displaying. A shutdown handler checks `ShuttingDown` to avoid starting new async work. The crash recovery path fires only on `Crashed`.

Each stage transition gets a timestamp. This is not optional. When a user reports "the app took 30 seconds to open," you need to know which startup step consumed those seconds. Pair the stage with an optional step string:

```rust
pub struct LifecycleState {
    pub stage: LifecycleStage,
    pub updated_at: AppTimestamp,
    pub startup_step: Option<String>,
    pub shutdown_step: Option<String>,
    pub last_startup_error: Option<String>,
    pub last_shutdown_error: Option<String>,
    pub last_error: Option<String>,
}
```

In GPUI, this struct implements the `Global` trait, so it is accessible from any context without threading references through function signatures. The same idea applies to other frameworks: put the lifecycle state somewhere global and read it from anywhere.

## The startup sequence

A good startup sequence is a pipeline. Each step depends on the previous one, and each step is individually measurable. Here is the order that works for a non-trivial desktop app:

1. Panic hook installation happens first, before any other code can fail. If something goes wrong during initialization itself, you want the crash report.
2. Crash marker check determines whether the previous launch crashed. If it did, you can show a recovery dialog or upload a report.
3. Component initialization sets up the UI framework.
4. Application state loads persisted config, window bounds, user preferences from disk.
5. Service initialization starts databases, secure storage, HTTP clients, notification backends, telemetry.
6. Key bindings and actions wire up keyboard shortcuts and menu items.

Each step gets its own timing log. The output looks like this:

```
component_init done elapsed_ms=12
app_state_init done elapsed_ms=8
logging_init done elapsed_ms=3
db_migrations complete version=3 elapsed_ms=45
runtime_services_init done elapsed_ms=67
startup complete total_elapsed_ms=142
```

I have seen apps where startup is one monolithic function. When something takes too long, you stare at 200 lines of init code guessing which call is the bottleneck. Individual timing per step fixes this. You know exactly where to look.

### Crash detection on launch

Write a marker file when the app starts. Delete it on clean shutdown. If the marker file exists on the next launch, the previous run crashed:

```rust
fn crash_marker_path() -> PathBuf {
    std::env::temp_dir().join("myapp.crash-marker")
}

pub fn write_crash_marker() {
    let path = crash_marker_path();
    let pid = std::process::id();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let _ = fs::write(&path, format!("pid={pid}\nstarted_at={timestamp}\n"));
}

pub fn check_previous_crash() -> Option<String> {
    let path = crash_marker_path();
    if path.exists() {
        fs::read_to_string(&path).ok()
    } else {
        None
    }
}

pub fn remove_crash_marker() {
    let path = crash_marker_path();
    let _ = fs::remove_file(&path);
}
```

On startup, call `write_crash_marker()`. On clean shutdown, call `remove_crash_marker()`. When `check_previous_crash()` returns `Some(...)` at the top of `main`, you know the last run did not finish cleanly. You can log a warning, queue a crash report upload, or offer to reset settings.

## Panic hooks and crash recovery

Rust panics unwind the stack by default. In a desktop app, an unhandled panic in a background task should not kill the entire process. And a panic during rendering should show an error boundary, not a blank window.

Install a custom panic hook early in startup, before any other code runs:

```rust
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let summary = info.to_string();

        // Capture the backtrace
        let backtrace = std::backtrace::Backtrace::capture();
        let bt_string = match backtrace.status() {
            std::backtrace::BacktraceStatus::Captured => backtrace.to_string(),
            _ => String::new(),
        };

        // Write crash report to disk
        if let Some(data_dir) = APP_DATA_DIR.get() {
            let report = CrashReport::new(summary.clone(), bt_string, is_render);
            let _ = write_crash_report(&report, data_dir);
        }

        // Store the panic summary for the error boundary to read
        LAST_PANIC_SUMMARY.get_or_init(|| Mutex::new(None))
            .lock()
            .map(|mut slot| *slot = Some(summary))
            .ok();

        // Call the previous hook (prints to stderr)
        previous(info);
    }));
}
```

The important distinction: separate render-path panics from background panics. A render panic should trigger an in-app error boundary. A background panic should log the error and keep running. You track this with a thread-local flag:

```rust
std::thread_local! {
    static IN_RENDER_PATH: Cell<bool> = const { Cell::new(false) };
}

static RENDER_PANIC_OCCURRED: AtomicBool = AtomicBool::new(false);
```

Set the flag to `true` right before rendering a page, `false` right after. The panic hook reads this flag and sets `RENDER_PANIC_OCCURRED` only when the panic came from the render path. The next frame checks this atomic and swaps in an error boundary view instead of retrying the crashed page.

## Graceful shutdown

Shutdown is harder than startup because it is time-constrained. The user clicked Quit and the OS expects the process to exit promptly. You cannot block the main thread for five seconds while SQLite flushes to disk. But you also cannot skip flushing and lose data.

The solution is an ordered shutdown pipeline with a timeout:

```rust
cx.on_action(|_: &Quit, cx| {
    set_stage(LifecycleStage::ShuttingDown, cx);

    // Flush debounced state that has not been written yet
    set_shutdown_step("flush_window_bounds", cx);
    flush_window_bounds(cx);

    // Drain background tasks with a cap
    set_shutdown_step("drain_tasks", cx);
    let drain = drain_with_timeout(Duration::from_secs(5), cx);

    cx.spawn(async move |cx| {
        drain.await;

        cx.update(|cx| {
            set_shutdown_step("stop_ipc", cx);
            single_instance::shutdown(cx);

            set_shutdown_step("flush_config", cx);
            force_save(cx);

            set_shutdown_step("flush_storage", cx);
            storage::shutdown(cx);

            set_shutdown_step("flush_telemetry", cx);
            telemetry::shutdown(cx);

            set_shutdown_step("flush_logs", cx);
            logging::shutdown(cx);

            set_shutdown_step("remove_crash_marker", cx);
            remove_crash_marker();

            set_shutdown_step("quit", cx);
            cx.quit();
        });
    }).detach();
});
```

The order matters. You stop accepting new work first (IPC, single-instance listener), then flush persisted state (config, storage), then flush observability (telemetry, logs). The crash marker is removed last, after everything else succeeded, so a crash at any earlier step still gets detected on the next launch.

### The drain timeout

That `drain_with_timeout` call is important. Background tasks might be in the middle of network requests or file writes. You give them a bounded window to finish. Five seconds is a reasonable default. If they finish early, the shutdown continues immediately. If they do not, you proceed anyway and accept the potential data loss. Waiting indefinitely is worse.

## The main function

Everything ties together in `main`. The entry point should be short and declarative. No business logic. Just wiring:

```rust
fn main() {
    // Single-instance check before anything else
    let preflight = single_instance::preflight();
    if !preflight.should_start {
        return;
    }

    let app_runtime = gpui_platform::application().with_assets(Assets);
    app_runtime.run(move |cx| {
        app::init(cx);

        // Install IPC listener for subsequent launches
        if let Some(runtime) = preflight.runtime {
            single_instance::install(runtime, cx);
        }

        // Forward deep link from second instance
        if let Some(link) = preflight.initial_deep_link {
            events::emit(AppEventKind::DeepLinkReceived(link), cx);
        }

        cx.activate(true);
        app::create_new_window("My App", cx);
    });
}
```

The single-instance check happens before the UI framework initializes. If another copy is already running, the second process forwards its arguments over IPC and exits. The user never sees a duplicate window. I wrote about this pattern in detail in [single-instance apps in Rust](/blog/single-instance-rust-app/).

## Tracking startup progress

For apps with non-trivial initialization, consider exposing the current startup step through your diagnostics or debug UI. This lets you see where the app is in the boot sequence without digging through log files. It also helps with user-facing progress indicators if your startup takes more than a second or two.

The `set_startup_step` calls scattered through the init function update a global string that any component can read. Combined with per-step timing logs, you get a complete picture of where startup time goes.

## What not to do

A few anti-patterns I have learned the hard way:

Do not skip the crash marker. You will ship a bug that crashes on launch, and without that marker you have no way to detect it on the next run. The file is two lines of text. Write it.

Do not put async work on the critical startup path without a timeout. A DNS lookup for a telemetry endpoint should not block your window from appearing. Fire and forget, or spawn it after the window is visible.

Do not forget to flush debounced state on shutdown. If you batch config writes every 500 milliseconds, the last batch might still be in memory when the user quits. Force a flush before you call `cx.quit()`.

Do not remove the crash marker until the very end of shutdown. If you remove it early and then the storage flush fails and panics, you lose the crash signal. Last thing, after everything else succeeds.

## Wrapping up

Application lifecycle management is not glamorous work. It is plumbing. But it is the kind of plumbing that prevents the class of bugs that only show up in production on someone else's machine. A well-defined state machine, ordered initialization with timing, crash detection and recovery, and a timed shutdown pipeline cover most of what you need.

If you are building a GPUI desktop app, [gpui-starter](https://github.com/freeoxide/gpui-starter) ships with all of these patterns wired in: lifecycle state tracking, crash markers, panic hooks with error boundaries, and ordered shutdown. The [getting started guide](/docs/docs/getting-started/) walks through the project structure. For more on how state flows through a GPUI app, see [state management in GPUI](/blog/state-management-gpui-entities/) and [scaling a GPUI prototype to production](/blog/scaling-gpui-prototype-to-production/).
