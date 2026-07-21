---
title: "Error handling and boundaries in Rust GUI apps"
description: "Designing error boundaries for Rust GUI applications: crash recovery, user-facing messages, and graceful degradation patterns that keep your app alive."
date: 2026-06-05
tags: [Rust, errors, patterns]
draft: false
---

I once watched a user spend forty minutes filling out a form in a desktop app, only to have it vanish because a network call failed and the whole window closed. No autosave. No error message. Just gone. That moment shaped how I think about errors in GUI software.

Rust's `Result` type gives you compile-time guarantees that errors won't go silently unhandled. But in a GUI app, "handling" an error means something different than in a CLI tool or a library. You can't just print to stderr and exit. There is a window on screen. There is a user staring at it. The real problem is keeping the app usable when things go wrong.

## Where errors come from in GUI apps

GUI errors cluster into four buckets.

IO and filesystem errors happen when reading config files, writing to SQLite, or accessing the keychain. Permissions change, disks fill up, files get deleted between reads. These hit at arbitrary times, not just at startup.

Network errors come from API calls, update checks, telemetry pings. Timeouts, DNS failures, TLS handshakes that go wrong. Network errors are the most common class in any app that talks to a server, and they're also the most recoverable.

State errors crop up when a required entity was already released, or an async task completed after the user navigated away, or a subscription fired with a payload that doesn't match the current schema. These are logic bugs that Rust's type system catches at compile time most of the time, but not always.

Render errors are the scariest. The GPU context was lost. A font couldn't be loaded. A layout constraint produced an impossible geometry. Any of these can make the entire window go blank.

A good error boundary strategy treats each bucket differently.

## Building error boundaries

React popularized the pattern: a component that catches errors in its subtree and shows a fallback UI instead of crashing. You can do the same thing in Rust, and the ownership model actually makes it easier to reason about.

### The boundary trait

An error boundary is any component that can catch a failure in its children and render something else. In GPUI, this looks like a wrapper entity that holds either a child or an error state:

```rust
pub enum BoundaryState<T> {
    Active(T),
    Failed(ErrorInfo),
}

pub struct ErrorInfo {
    pub message: String,
    pub recoverable: bool,
    pub source: ErrorSource,
    pub occurred_at: Instant,
}

pub struct ErrorBoundary<T: Render> {
    state: BoundaryState<T>,
    retry_count: usize,
    max_retries: usize,
}
```

The boundary wraps a child component. If the child signals a failure (more on how below), the boundary swaps to the `Failed` variant and renders a fallback. The user sees a message and a retry button instead of a crash.

### Triggering the boundary

How the child tells the boundary something went wrong is the hard part. You have two options.

The first is an explicit action. The child emits a typed action that the boundary listens for:

```rust
#[derive(Clone)]
pub struct RecoverableError {
    pub context: String,
    pub source: ErrorSource,
}

impl<T: Render> ErrorBoundary<T> {
    fn handle_error(&mut self, action: &RecoverableError, cx: &mut Context<Self>) {
        self.retry_count += 1;
        if self.retry_count <= self.max_retries {
            // Attempt automatic retry with backoff
            let delay = Duration::from_millis(500 * 2u64.pow(self.retry_count as u32));
            cx.spawn(|this, mut cx| async move {
                cx.background_executor().timer(delay).await;
                this.update(&mut cx, |this, cx| this.retry(cx));
            }).detach();
        } else {
            self.state = BoundaryState::Failed(ErrorInfo {
                message: action.context.clone(),
                recoverable: true,
                source: action.source.clone(),
                occurred_at: Instant::now(),
            });
        }
    }
}
```

The second option is a panic hook. For errors that you didn't anticipate (index out of bounds, unwrap on None), you set a custom panic hook that catches the panic and routes it to a boundary instead of aborting:

```rust
fn setup_panic_hook(app: &mut Application) {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        // Write to crash report file
        if let Ok(dir) = crash_report_dir() {
            let _ = std::fs::write(dir.join("last_crash.txt"), &msg);
        }
        // Don't abort; let the GPUI render loop continue
        // The boundary will pick up the stale state on next render
    }));
}
```

I prefer the action-based approach for expected errors and the panic hook as a last resort. The panic hook keeps the process alive but the app is in an undefined state, so you should offer the user a restart button rather than pretending everything is fine.

## What to show the user

Error messages in GUI apps are terrible more often than not. "An error occurred." "Something went wrong." "Error code 0x80070005." None of these help.

A good error message tells the user what happened, what data was affected, and what they can do about it:

```rust
// Bad
"Failed to save document."

// Better
"Could not save 'meeting-notes.md' because the disk is full. \
 Try deleting unused files or choose a different save location."
```

For the boundary fallback UI, put a clear description of what failed on screen, along with an indication of whether the user's data is safe, and an action button (retry, restart, or dismiss).

```rust
impl<T: Render> Render for ErrorBoundary<T> {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        match &self.state {
            BoundaryState::Active(child) => div().child(child.clone()),
            BoundaryState::Failed(info) => div()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .child(
                    div()
                        .text_sm()
                        .text_color(Color::Error.color())
                        .child(info.message.clone())
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(Color::Muted.color())
                        .child("Your work has been saved. You can retry or restart the app.")
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Button::new("retry", "Retry").on_click(cx.listener(|this, _, cx| {
                            this.retry(cx);
                        })))
                        .child(Button::new("restart", "Restart App").on_click(|_event, _cx| {
                            std::process::exit(0); // Or use your app's restart mechanism
                        }))
                ),
        }
    }
}
```

## Crash reporting

Some errors you can't catch. A segfault in a C dependency. An out-of-memory condition. A panic in a thread you don't control. For these, you need crash reporting that happens before the process exits.

The approach in [gpui-starter](/docs/getting-started/) is to set up a panic hook on startup that writes a crash report to a known directory:

```rust
pub fn setup_crash_reporting() {
    std::panic::set_hook(Box::new(|info| {
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let report = format!(
            "=== Crash Report ===\n\
             Time: {}\n\
             Location: {}\n\
             Message: {}\n\
             Thread: {:?}\n\
             Backtrace:\n{}",
            timestamp,
            info.location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default(),
            info.payload()
                .downcast_ref::<&str>()
                .unwrap_or(&"unknown"),
            std::thread::current().name(),
            std::backtrace::Backtrace::capture(),
        );

        if let Some(dir) = dirs::data_local_dir() {
            let crash_dir = dir.join("gpui-starter").join("crashes");
            let _ = std::fs::create_dir_all(&crash_dir);
            let _ = std::fs::write(
                crash_dir.join(format!("crash-{}.txt", timestamp)),
                &report,
            );
        }
    }));
}
```

On the next launch, the app checks for crash reports and offers to send them or display them. This is also where you'd integrate with a service like Sentry if you want remote crash reporting. The important part is getting consent before sending anything off the user's machine. Read more about this in the [telemetry and diagnostics guide](/docs/diagnostics/).

## Graceful degradation

Error boundaries handle individual component failures. Graceful degradation handles systemic ones. The idea is the same: keep the app running with reduced functionality rather than showing nothing.

A practical pattern is the capability check. At startup and periodically during runtime, you probe each subsystem and record whether it's available:

```rust
pub struct AppCapabilities {
    pub network: bool,
    pub filesystem: bool,
    pub notifications: bool,
    pub keychain: bool,
    pub gpu_acceleration: bool,
}

impl AppCapabilities {
    pub fn probe() -> Self {
        Self {
            network: probe_network(),
            filesystem: probe_fs_access(),
            notifications: probe_notification_permission(),
            keychain: probe_keyring_access(),
            gpu_acceleration: probe_gpu(),
        }
    }
}
```

Any feature that depends on a disabled capability shows a muted state or a "connect to continue" prompt instead of a broken UI. The sidebar navigation still works. The settings page still loads. The user can still read cached data. You just disable the actions that require the missing capability.

This matters most on first launch. If the network is unreachable and you block the entire UI waiting for a config fetch, the user sees a blank screen and assumes the app is broken. If you show the cached config (or sensible defaults) with a small banner saying "Working offline," the user understands the situation and can still get work done.

## The retry problem

Retrying failed operations seems straightforward until you think about side effects. A save request that times out might have succeeded on the server. Retrying it creates a duplicate. An authentication request that fails might leave the token in a partially valid state.

The rules I follow:

- GET requests are safe to retry automatically with exponential backoff.
- POST/PUT requests require user confirmation before retry.
- File writes should be atomic (write to temp, then rename). If the temp write fails, retry is safe because the original file is untouched.
- Auth flows should reset state completely before retry.

## Error handling is UX

The thread connecting all of these patterns: treat error handling as a UX problem, not a systems problem. The `Result` type is a systems tool. It tells you what went wrong and where. The error boundary is a UX tool. It decides what the user sees and what they can do next.

If you're building a Rust desktop app and want to see these patterns in a working codebase, [gpui-starter](https://github.com/freeoxide/gpui-starter) implements error boundaries, crash reporting, capability probing, and graceful degradation out of the box. The [getting started guide](/docs/getting-started/) walks through the setup. You can also read about [state management patterns](/blog/state-management-gpui-entities/) and [testing strategies](/blog/testing-gpui-applications/) for related architectural decisions.
