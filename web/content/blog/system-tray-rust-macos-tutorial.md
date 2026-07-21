---
title: "Adding a macOS system tray icon to your Rust app"
description: "Step-by-step guide to adding a system tray icon and menu to a Rust desktop application on macOS, using the tray-icon crate."
date: 2026-06-05
tags: [Rust, macOS, tutorial]
draft: false
---

Your app runs in the background, doing useful work. The user closed the window because they needed the screen space. Now they want it back. Without a system tray icon, their only option is to dig through Spotlight or the Dock to relaunch. With one, they click a tiny magnifying glass in the menu bar and the window reappears.

This post walks through adding a macOS system tray icon to a Rust desktop app using the `tray-icon` crate. The same approach works on Windows and Linux with minor differences in how the icon is rendered.

## What you need

The `tray-icon` crate is a cross-platform library from the Tauri team. It wraps the native tray APIs on each OS: `NSStatusItem` on macOS, the system tray area on Linux, and the notification area on Windows.

Add it to your `Cargo.toml`:

```toml
[dependencies]
tray-icon = { version = "0.24", default-features = false }
```

Disabling default features strips out the `libappindicator` dependency on Linux if you don't need it, which keeps compile times and binary size down.

You also need an image to display. On macOS, tray icons are template images: monochrome glyphs that the system automatically adapts to light and dark mode. We'll generate one programmatically so we don't have to bundle an asset file.

## Building the icon from pixels

macOS expects template images to be grayscale with an alpha channel. The system handles coloring them to match the menu bar. The `Icon::from_rgba` function takes a raw RGBA buffer:

```rust
fn build_icon() -> tray_icon::Icon {
    const SIZE: usize = 36;
    let mut px = vec![0u8; SIZE * SIZE * 4];

    // Draw a simple glyph. Each pixel is [R, G, B, A].
    // For template images, RGB should be 0 and alpha carries the shape.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let dist = ((fx - 15.0).powi(2) + (fy - 15.0).powi(2)).sqrt();
            let alpha = if dist < 12.0 { 255 } else { 0 };
            let i = (y * SIZE + x) * 4;
            px[i] = 0;
            px[i + 1] = 0;
            px[i + 2] = 0;
            px[i + 3] = alpha;
        }
    }

    tray_icon::Icon::from_rgba(px, SIZE as u32, SIZE as u32)
        .expect("icon pixel data is valid")
}
```

This draws a filled circle. For a real app you'd draw something more distinctive: a magnifying glass, a bell, a gear. The constraint is that template images must be black with variable alpha. The macOS menu bar renders white icons on dark backgrounds and black icons on light backgrounds automatically.

You can also load a `.png` file at runtime instead:

```rust
let png_bytes = include_bytes!("../assets/tray-icon.png");
let icon = tray_icon::Icon::from_rgba(
    png_bytes.to_vec(),
    36,
    36,
)?;
```

Either way works. Generating from code avoids bundling assets but limits you to simple shapes. PNG files give you full control over the glyph at the cost of an extra file to ship.

## Creating the tray icon

The `TrayIconBuilder` constructs the icon and registers it with the OS:

```rust
use tray_icon::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};

let tray = TrayIconBuilder::new()
    .with_icon(icon)
    .with_icon_as_template(true)
    .with_tooltip("Open Launcher")
    .with_menu_on_left_click(false)
    .build()?;
```

The `with_icon_as_template(true)` call is macOS-specific. It tells AppKit to treat the image as a template, which enables automatic dark/light mode adaptation. Without this flag, macOS renders your icon exactly as provided, which will look wrong in one mode or the other.

`with_menu_on_left_click(false)` means left-clicking the icon fires a click event instead of opening a context menu. This is the behavior most users expect: click to show the window, right-click for options.

There's a subtlety here. The `TrayIcon` struct must live for the entire lifetime of your application. If it gets dropped, the icon disappears from the menu bar. In practice, you either store it in a long-lived struct or leak it:

```rust
Box::leak(Box::new(tray));
```

Leaking a few hundred bytes for the tray handle is fine. The OS process owns it, and it gets cleaned up when the process exits. If leaking bothers you, store the `TrayIcon` in your app state struct and keep a reference alive through the main loop.

## Handling click events

The tray icon emits events through a channel. You poll this channel on a timer to detect clicks:

```rust
cx.spawn(async move |cx| {
    let bg = cx.background_executor();
    loop {
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                cx.update(|cx| {
                    // Show your main window here
                    bring_window_to_front(cx);
                });
            }
        }
        bg.timer(Duration::from_millis(50)).await;
    }
}).detach();
```

The 50ms poll interval is fast enough to feel instant while keeping CPU usage negligible. The `try_recv()` call is non-blocking, so each iteration is cheap.

Filtering on `MouseButton::Left` with `MouseButtonState::Up` means the action fires when the user releases the mouse button, which matches standard macOS click behavior. You can handle right-click separately to show a context menu if you need one.

## Adding a context menu

The `tray-icon` crate supports native context menus through the `muda` crate. This gives you a real native menu with proper styling, keyboard navigation, and accessibility:

```rust
use muda::{Menu, Submenu, MenuItem, PredefinedMenuItem};

let menu = Menu::new();
let submenu = Submenu::new("Actions", true);
submenu.append_items(&[
    &MenuItem::new("Show Window", true, None),
    &PredefinedMenuItem::separator(),
    &MenuItem::new("Quit", true, None),
]);
menu.append(&submenu);

let tray = TrayIconBuilder::new()
    .with_icon(icon)
    .with_icon_as_template(true)
    .with_menu(&menu)
    .build()?;
```

Each `MenuItem` fires an event through `muda`'s own event receiver. You handle those the same way you handle tray clicks: poll the channel and dispatch actions.

Native menus integrate with macOS accessibility features automatically. VoiceOver reads the menu items. Keyboard navigation works. You get all of this without writing platform-specific code.

## Platform differences

The code above works on all three platforms, but there are behavioral differences worth knowing about.

On macOS, the tray area lives in the system menu bar. Icons are typically 22x22 points. The system enforces a monochrome template style for third-party icons, even if you don't mark yours as a template. This changed in recent macOS versions: the menu bar is always dark on dark mode, always light on light mode, and your icon adapts or it looks broken.

On Windows, the tray area (called the notification area) supports color icons. Template mode has no effect. Your icon appears exactly as provided. The notification area can be hidden by the user on a per-app basis, so your icon might not be visible at all. Windows also has a "overflow" area where less-frequently-used icons get shuffled. There's no way to prevent this from the app side.

On Linux, the tray area depends on the desktop environment. GNOME 42+ removed tray icon support by default and requires an extension. KDE Plasma, XFCE, and most other desktops support it through `libappindicator` or `libayatana-appindicator`. If no compatible indicator library is installed, the `tray-icon` crate's `build()` call will fail. Handle this gracefully.

## Keeping the icon alive during background work

One common pattern for background apps is to close the main window but keep the process running. The tray icon becomes the primary way users interact with your app. This requires two things: preventing the app from exiting when the last window closes, and re-creating the window when the tray icon is clicked.

In GPUI, you control this through the application lifecycle. When the window closes, don't exit the run loop. The tray icon's event loop keeps the process alive, and clicking the icon spawns a new window or shows an existing one.

For apps using `winit` or similar frameworks, the same principle applies. Don't call `exit()` on `CloseRequested`. Hide the window instead and rely on the tray event to show it again.

## A note on icon size

macOS Retina displays double the pixel density. A 22-point icon on a Retina display needs 44 actual pixels to look sharp. The `tray-icon` crate handles this internally when you provide a high-enough-resolution image. A 36x36 pixel source (like the generated icon above) works fine on both standard and Retina displays because macOS scales template images smoothly.

If you're loading a PNG, make it at least 44x44 pixels. Anything smaller will look blurry on Retina. Anything much larger wastes memory for no visible improvement.

## Where this fits in a real app

The system tray is one piece of the desktop app puzzle. It works alongside [single-instance enforcement](/blog/single-instance-rust-app/) to make sure only one copy of your app is running, and it pairs with [desktop notifications](/blog/desktop-notifications-rust/) for background event alerts. Together, these three features cover the basics of a macOS background app: the user can find it (tray icon), they won't accidentally run two copies (single instance), and they'll see important events even when the window is hidden (notifications).

If you want to see all of this wired together, check out [gpui-starter](https://github.com/freeoxide/gpui-starter). It ships a working tray icon with programmatic glyph generation, click handling wired through GPUI's async runtime, and the context menu you'd expect from a production macOS app. The [getting started guide](/docs/getting-started/) walks through the full setup.
