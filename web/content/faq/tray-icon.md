---
question: "How do I add a system tray icon to a Rust desktop app on macOS?"
description: "gpui-starter ships a macOS menu bar tray icon built with the tray-icon crate. Left-click opens the command launcher; no native code or plugins needed."
category: "Features"
order: 10
---

gpui-starter includes a macOS menu bar tray icon that stays visible while the app runs. Left-clicking it opens the [command launcher](/docs/command-launcher/) (the same popup that Cmd+K triggers). There is no right-click context menu.

## What actually happens when you click

The tray icon sits in `src/platform/desktop_shell/tray.rs`, gated behind `#[cfg(target_os = "macos")]`. On click:

1. The `tray-icon` crate fires a `TrayIconEvent::Click` through a channel receiver.
2. A background loop (polled every 50 ms) picks up that event and calls [`crate::launcher::open_launcher`](/docs/command-launcher/), which opens a 620x460 px popup window centered near the top of the primary display.
3. The same loop also listens for the `global-hotkey` Alt+Space binding, so the tray click and the keyboard shortcut share the same code path.

The icon itself is a 36x36 magnifying glass rendered entirely in Rust as an RGBA template image (no asset file on disk). It uses `with_icon_as_template(true)` so macOS adapts it to light and dark menu bar appearances automatically.

## The code

```rust
// src/platform/desktop_shell/tray.rs
pub fn setup(cx: &mut App) {
    let icon = build_icon(); // 36x36 RGBA template image
    let Ok(tray) = TrayIconBuilder::new()
        .with_icon(icon)
        .with_icon_as_template(true)
        .with_tooltip("Open Launcher  (⌥Space)")
        .with_menu_on_left_click(false)
        .build()
    else {
        tracing::error!("failed to create system tray icon");
        return;
    };
    Box::leak(Box::new(tray));

    cx.spawn(async move |cx| {
        let bg = cx.background_executor();
        loop {
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = ev {
                    cx.update(crate::launcher::open_launcher);
                }
            }
            while GlobalHotKeyEvent::receiver().try_recv().is_ok() {
                cx.update(crate::launcher::open_launcher);
            }
            bg.timer(Duration::from_millis(50)).await;
        }
    }).detach();
}
```

It is called once from `main.rs`:

```rust
#[cfg(target_os = "macos")]
gpui_starter::tray::setup(cx);
```

## Dependencies

The tray uses two crates from the Tauri team:

- `tray-icon` v0.24 wraps `NSStatusItem` on macOS, the system tray on Linux, and the notification area on Windows. In gpui-starter it is configured with `default-features = false` to keep the binary small.
- `global-hotkey` v0.8 registers the Alt+Space shortcut. The same event loop handles both tray clicks and hotkey presses.

Both are cross-platform. The `#[cfg(target_os = "macos")]` gate exists because the rest of the shell code only targets macOS right now, not because the crates themselves are macOS-only.

## Removing or customizing the tray

To remove the tray icon, delete the `tray::setup(cx)` call in `main.rs` and remove the `tray` module from `src/platform/desktop_shell/mod.rs`. Nothing else depends on it.

To add a right-click menu, pass `.with_menu()` to the `TrayIconBuilder` with an `muda::Menu`. The `with_menu_on_left_click(false)` setting means left-click fires a raw click event instead of opening the menu, which is why left-click currently opens the launcher instead of showing a context menu.

## Related

- [Command launcher docs](/docs/command-launcher/) for the popup window that the tray click opens
- [System tray Rust macOS tutorial](/blog/system-tray-rust-macos-tutorial/) for a standalone walkthrough of adding a tray icon from scratch
- [Architecture overview](/docs/architecture/) for how the tray fits into the platform shell layer
- [Single-instance apps in Rust](/blog/single-instance-rust-app/) for how the tray coexists with the single-instance lock
