---
question: "How do notifications work in a Rust desktop app?"
description: "gpui-starter sends desktop notifications through native OS APIs with automatic fallback: macOS UNUserNotificationCenter, then notify-rust for Linux and Windows, then in-app toast. Covers permission handling, the persistent inbox, interactive actions, and how to send your own notifications."
category: "Features"
order: 14
---

gpui-starter uses a three-tier fallback chain for notifications. The app tries the native backend for the platform, falls back to a portable backend, and drops to an in-app toast when neither works. Every send attempt is recorded in a persistent inbox.

## Which backend gets used

On macOS, the primary backend talks directly to `UNUserNotificationCenter` through the `user-notify` crate. This produces native banners with actions and reply fields. It only works in a bundled app with a valid bundle identifier in `Info.plist`. If you are running from `cargo run` without a bundle, the primary backend stays unavailable and the app skips straight to the fallback.

The secondary backend is `notify-rust`. It uses D-Bus and libnotify on Linux, and the WinRT toast API on Windows. This backend has no permission model and no interactive features, but it works without a packaged app identity.

When both backends fail (missing daemon, denied permission, wrong platform), the app renders an in-app toast inside the window via `gpui-component`.

## Permissions

macOS is the only platform that requires explicit user consent. The app checks `UNAuthorizationStatus` at startup. If the status is `NotDetermined`, calling `request_permission_from_window` triggers the native permission dialog. If the user denies permission, the app silently switches to in-app toasts with no crash or error modal.

The settings page includes a button that opens `System Settings > Notifications` so users can re-enable delivery without hunting through menus. On Linux and Windows, `notify-rust` sends without a permission prompt.

## Sending a notification

Build a `NotificationRequest` and call `send_from_window`:

```rust
use crate::notifications::{NotificationRequest, send_from_window};

let request = NotificationRequest::foreground("Export finished", "report.csv saved to ~/Desktop");
send_from_window(request, window, cx);
```

The function returns immediately. Delivery happens asynchronously via `cx.spawn`, and the result is recorded in the inbox when the async work completes.

Four builder methods are available:

- `foreground(title, body)` is the standard path. Plays a sound, shows a native banner, falls back to in-app toast on failure.
- `action_buttons(title, body)` adds Open/Snooze buttons. macOS only through `UserNotifyBackend`.
- `reply(title, body)` adds a text input for replies. Also macOS only.
- `background_worthy(title, body)` skips the in-app fallback entirely. Use this for update checks and sync status where a missed notification is acceptable.

## The notification inbox

Every send attempt, permission change, and settings toggle is recorded in a persistent inbox backed by `NotificationInboxState` (a GPUI global stored in `state.json`). Each entry tracks the backend used, whether it delivered natively, any errors, and a read/unread state. The inbox is capped at 200 items. Older entries are truncated on insert to keep the config file from growing without bound.

The inbox page in the sidebar (`src/features/pages/notifications.rs`) shows unread count, per-item metadata, and Mark all read / Clear all actions. The data survives app restarts.

## Checking capabilities at runtime

Interactive features (action buttons, reply fields) only exist on macOS with the `UserNotifyBackend`. Before offering interactive notifications, check the snapshot:

```rust
let snap = notifications::snapshot(cx);
if snap.capabilities.can_send_interactive {
    let request = NotificationRequest::action_buttons("New message", "From: Alice");
    send_from_window(request, window, cx);
} else {
    let request = NotificationRequest::foreground("New message", "From: Alice");
    send_from_window(request, window, cx);
}
```

The snapshot also exposes `permission`, `enabled_by_user`, `degraded_reason`, and `last_backend_error` so your settings UI can show the right state.

## Toggling notifications off

Users can disable native notifications from the settings page. This updates the persisted config, mutates the runtime snapshot, records a `SettingsUpdate` in the inbox, and updates the capability system. No native backend is contacted while notifications are disabled, which avoids pointless IPC and permission warnings.

## Common failure modes

Every platform has a silent failure mode where the API returns success but nothing appears on screen. On macOS this happens when the bundle identifier is missing or the entitlements are wrong. On Linux it happens when no notification daemon is running. On Windows it happens without app identity registration. The inbox records these as `degraded: true` with the error summary, so your UI can surface a warning instead of assuming everything worked.

## Related

- [Notifications documentation](/docs/notifications/) for the backend trait, full API reference, custom backend integration, and initialization details
- [Desktop notifications in Rust: native backends and toast fallbacks](/blog/notifications-rust-desktop-tutorial/) for the design rationale and what breaks on each platform
- [Architecture overview](/docs/architecture/) for how the capability system and GPUI globals fit together
- [Debugging techniques for Rust desktop apps](/blog/rust-desktop-debugging-techniques/) for reading structured log output from the notification module
- [Getting started](/docs/getting-started/) for app initialization including `notifications::initialize`
