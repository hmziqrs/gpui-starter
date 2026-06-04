---
title: Error Boundaries
description: Action-based error triggers with graceful degradation and user-facing error messages.
---

Errors in a GUI app are not all the same. A failed HTTP request can show an inline message. A missing file can retry silently. But a panic during render is different: the window is on screen, the user is looking at it, and the next frame is about to crash again. The error boundary system in gpui-starter catches render-path panics, swaps in a fallback view, and lets the user recover without losing the entire window.

## How it works

The system has three layers: a thread-local render guard, a custom panic hook, and an action-based trigger mechanism in `AppRoot`.

When `AppRoot::render` calls `active_page_view`, it wraps the call with `enter_render_path()`. This sets a thread-local flag that is true only for the duration of that call. If a panic fires while the flag is set, the custom panic hook records it in a static `AtomicBool`. On the next render pass, `AppRoot` reads that flag, stops rendering the crashing page, and shows `RenderErrorPage` instead.

The user sees a centered error summary and a "Reload Page" button. Clicking reload clears the boundary state and retries the original page. Navigating to a different page via the sidebar also clears it.

This approach exists because GPUI renders through an `extern "C"` Metal callback. Rust's unwinder cannot safely cross that boundary, so a real render panic is process-fatal. The boundary intercepts the panic signal before the next frame attempt, giving the app a chance to show something instead of crashing.

### Flow diagram

```
Render pass starts
  enter_render_path() — set thread-local flag
    page.render()
      panic! (or trigger via action)
  RenderPathGuard drops — clear thread-local flag

Panic hook fires
  Detects in_render_path() == true
  Sets RENDER_PANIC_OCCURRED atomic flag
  Writes crash report to disk
  Stores panic summary

Next render pass
  AppRoot::active_page_view()
    take_render_panic() returns true
    self.render_error = true
    Returns RenderErrorPage entity

User clicks "Reload Page"
  ReloadCurrentPage action dispatched
  self.render_error = false
  self.error_page = None
  cx.notify() — re-render original page
```

## The render guard

The thread-local guard lives in `src/app/lifecycle.rs`. It is an RAII struct that sets a flag on creation and clears it on drop.

```rust
std::thread_local! {
    static IN_RENDER_PATH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn enter_render_path() -> impl Drop {
    IN_RENDER_PATH.with(|c| c.set(true));
    RenderPathGuard
}

pub fn in_render_path() -> bool {
    IN_RENDER_PATH.with(|c| c.get())
}
```

`AppRoot::render` uses it like this:

```rust
.child({
    let _render_guard = crate::lifecycle::enter_render_path();
    self.active_page_view(cx)
}),
```

The guard only wraps the page content rendering. The title bar, sidebar, status bar, and dialog layers render outside the guard, so a panic in those areas is not caught by the boundary. This is intentional: those components are stable application chrome that should never panic. If they do, the process exit is the correct behavior.

## The panic hook

`install_panic_hook()` in `lifecycle.rs` replaces the default panic handler with one that checks `in_render_path()`. If the panic originated in the render path, it sets `RENDER_PANIC_OCCURRED` and stores the panic summary string. It also writes a crash report file to disk with the full backtrace, app version, OS info, and any recent error messages tracked by `error_surface::report`.

```rust
std::panic::set_hook(Box::new(move |info| {
    let summary = info.to_string();
    // Store summary for the error page to display
    LAST_PANIC_SUMMARY.get_or_init(|| Mutex::new(None))
        .lock().ok()
        .map(|mut v| *v = Some(summary.clone()));

    if in_render_path() {
        RENDER_PANIC_OCCURRED.store(true, Ordering::SeqCst);
    }

    // Write crash report to {data_dir}/crash_reports/{uuid}.json
    if let Some(data_dir) = APP_DATA_DIR.get() {
        let report = CrashReport::new(summary, backtrace, is_render, recent_errors);
        write_crash_report(&report, data_dir).ok();
    }

    previous(info); // chain to default hook
}));
```

Crash reports are JSON files stored in the application data directory. Each report includes a UUID, the panic message, a captured backtrace, the app version, OS and architecture, whether the panic was render-path related, and up to 20 recent error messages from the error surface. Reports can be uploaded to a configured endpoint on next launch.

## Action-based trigger

Real render panics are fatal in GPUI. To test the error boundary without killing the process, gpui-starter uses an action-based trigger.

```rust
#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct TriggerRenderError {
    pub message: String,
}
```

`AppRoot` listens for this action in its render method:

```rust
.on_action(cx.listener(|this, action: &TriggerRenderError, _, cx| {
    this.render_error = true;
    this.error_page = Some(cx.new(|_| {
        RenderErrorPage::new(action.message.clone())
    }));
    cx.notify();
}))
```

This activates the same fallback UI that a real render panic would produce, but without any actual panic. The Error Playground page uses this to let you test boundary recovery interactively.

## The fallback view

`RenderErrorPage` is a simple centered layout with three elements: an error title in the danger color, the panic summary text, and a reload button.

```rust
pub struct RenderErrorPage {
    summary: String,
}

impl Render for RenderErrorPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .min_h_full()
            .items_center()
            .justify_center()
            .gap_4()
            .p_8()
            .child(
                v_flex()
                    .items_center()
                    .gap_3()
                    .max_w(px(480.))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().danger)
                            .child("Render Error"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(self.summary.clone())),
                    )
                    .child(
                        Button::new("reload-current-page")
                            .label("Reload Page")
                            .on_click(|_, _, cx| {
                                cx.dispatch_action(&ReloadCurrentPage);
                            }),
                    ),
            )
    }
}
```

The reload button dispatches `ReloadCurrentPage`, which `AppRoot` handles by clearing `render_error` and `error_page`, then calling `cx.notify()` to re-render. If the original page still panics, the boundary activates again immediately. If the panic was transient (for example, bad data that has since been cleared), the page renders normally.

## The error surface

The error boundary handles render panics. For everything else, there is the error surface in `src/services/error_surface.rs`. This is a separate system that records non-fatal errors in a capped in-memory buffer (200 entries) and persists them to SQLite as a secondary store.

```rust
error_surface::report(
    "Failed to connect to update server",
    AppErrorSeverity::Warning,
    ErrorCategory::Network,
    vec![ErrorAction::Retry, ErrorAction::Dismiss],
    cx,
);
```

Each error record carries an ID, timestamp, severity, category, message, and a list of suggested actions. Categories are `Network`, `Storage`, `Rendering`, `Config`, and `System`. The error surface feeds into the Diagnostics page, where you can browse error history and inspect individual records.

The panic hook also reads from the error surface. Up to 20 recent error messages are attached to crash reports, giving you context about what was happening before the panic.

## Crash detection on next launch

gpui-starter writes a marker file at startup. If the process crashes, the marker remains on disk. On the next launch, the app checks for the marker and can show a recovery prompt or log the incident. On clean shutdown, the marker is removed.

```rust
// At startup:
lifecycle::write_crash_marker();

// At clean shutdown:
lifecycle::remove_crash_marker();

// At next startup:
if let Some(marker) = lifecycle::check_previous_crash() {
    tracing::warn!("Previous session crashed: {marker}");
}
```

This pairs with the crash report files. The marker tells you that a crash happened; the report file in `crash_reports/` tells you why.

## What the boundary does not catch

The error boundary only intercepts panics that occur inside the render path. These scenarios fall outside its scope:

- `tokio::spawn` tasks that panic are isolated by the runtime. They log an error but do not trigger the boundary.
- Panics during initialization, shutdown, or event handlers that run outside `active_page_view` propagate normally.
- Failed HTTP requests, filesystem errors, and SQLite errors are `Result` values, not panics. Handle them with `match` or `?` and report through the error surface.

For background task errors, use inline error display. The Error Playground page demonstrates this pattern with HTTP errors, filesystem errors, and async timeouts that all show their results inline rather than activating the boundary.

## Related

- [Architecture](/docs/architecture/): entity ownership patterns and the render loop structure that the boundary hooks into
- [Error handling and boundaries in Rust GUI apps](/blog/rust-desktop-error-boundaries/): the design blog post with the reasoning behind each layer
- [Rust desktop debugging techniques](/blog/rust-desktop-debugging-techniques/): reading crash reports and using the diagnostics page
- [Testing](/docs/testing/): how to test error boundary activation in unit tests using `TriggerRenderError`
