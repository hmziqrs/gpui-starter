---
title: "Notifications"
description: "Desktop notification system in gpui-starter with native OS backends, automatic fallback, and a persistent inbox for tracking delivery status."
---

The notification module (`src/services/notifications/`) handles sending desktop notifications through native OS APIs, with an automatic two-tier fallback chain. When the primary backend fails, it tries a secondary backend. When both fail, it shows an in-app toast. Every send attempt is recorded in a persistent inbox you can surface in your UI.

This page covers the backend trait, the fallback chain, permission handling, the inbox store, and how to wire up a custom backend. For a broader tutorial on why desktop notifications are hard and how this system was designed, see the [notifications in Rust desktop apps](/blog/notifications-rust-desktop-tutorial/) blog post. For app-wide architecture patterns, see [Architecture](/docs/architecture/).

## Module structure

```
src/services/notifications/
├── mod.rs              # Public API re-exports
├── service.rs          # NotificationService, globals, send logic
├── inbox.rs            # Persistent inbox store (NotificationInboxState)
└── backend/
    ├── mod.rs          # NotificationBackend trait
    ├── user_notify.rs  # Primary backend (macOS UserNotifications)
    └── notify_rust.rs  # Fallback backend (libnotify/DBus/Windows)
```

The inbox page rendered in the app sidebar lives at `src/features/pages/notifications.rs`.

## NotificationBackend trait

Every native backend implements this async trait:

```rust
#[async_trait]
pub trait NotificationBackend: Send + Sync {
    fn kind(&self) -> NotificationBackendKind;
    fn capabilities(&self) -> NotificationCapabilities;
    async fn refresh_permission_state(&self) -> NotificationPermissionState;
    async fn request_permission(&self) -> NotificationPermissionState;
    async fn send(&self, request: &NotificationRequest) -> anyhow::Result<()>;
}
```

The `NotificationService` holds two backend slots. `primary` is `Option<Arc<dyn NotificationBackend>>` because the primary might not be available on the current platform. `secondary` is always present and wraps `NotifyRustBackend`. On send, the service tries the primary first, falls back to the secondary on failure, and degrades to in-app only when both fail.

## Backend selection

| Backend | Crate | When used | Interactive | Permission API |
|---------|-------|-----------|-------------|----------------|
| `UserNotifyBackend` | `user-notify` | Primary on macOS bundled app | Yes (actions, reply) | Yes |
| `NotifyRustBackend` | `notify-rust` | Fallback: Linux, Windows, unbundled macOS | No | No |
| In-app toast | `gpui-component` | Degraded mode when native delivery fails | No | No |

`NotificationService::new()` calls `UserNotifyBackend::new()`. If that fails (wrong platform, no bundle identifier, missing entitlements), the primary slot stays `None` and the error is logged. `NotifyRustBackend` is always initialized as the secondary.

### Backend capabilities

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotificationCapabilities {
    pub can_request_permission: bool,
    pub can_read_permission_state: bool,
    pub can_send_immediate_native: bool,
    pub can_send_interactive: bool,
    pub requires_packaged_runtime: bool,
}
```

| Capability | UserNotify (macOS) | NotifyRust (Linux) | NotifyRust (Windows) |
|------------|-------------------|--------------------|---------------------|
| `can_request_permission` | Yes | No | No |
| `can_read_permission_state` | Yes | No | No |
| `can_send_immediate_native` | Yes | Yes | Yes |
| `can_send_interactive` | Yes | No | No |
| `requires_packaged_runtime` | Yes | No | Yes |

Check capabilities at runtime with `snapshot(cx).capabilities` to conditionally show or hide UI elements like the "Request Permission" button.

## Permission handling

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationPermissionState {
    Unknown,
    Unsupported,
    Unavailable(String),
    NotDetermined,
    Denied,
    Authorized,
}
```

Each variant maps to a real platform condition:

| Platform | How permissions work |
|----------|---------------------|
| macOS (bundled) | Uses `UNUserNotificationCenter`. Reads state via `getNotificationSettingsWithCompletionHandler`, requests via `first_time_ask_for_notification_permission`. Requires a valid bundle identifier in `Info.plist`. |
| macOS (unbundled) | Falls back to `NotifyRustBackend`. Permission state is `Unavailable` with the error reason. |
| Linux | `notify-rust` sends via libnotify/DBus. No permission model exists. State is `Unsupported`. |
| Windows | `notify-rust` sends via Windows toast XML. State is `Unsupported`. |

To request permission from a window context (triggers the native macOS permission dialog on first call):

```rust
notifications::request_permission_from_window(window, cx);
```

To open the system notification settings panel (macOS only):

```rust
notifications::open_system_settings(cx);
```

This opens `x-apple.systempreferences:com.apple.Notifications-Settings.extension`. On non-macOS platforms, it logs a warning and sets `last_backend_error` on the snapshot.

## Notification inbox

Every send attempt, permission change, and settings toggle is recorded in a persistent inbox backed by `NotificationInboxState` (a GPUI global). The inbox is loaded from `target/state.json` on startup via `app_state::config()` and written back on every mutation.

### InboxEntry fields

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationInboxItem {
    pub id: NotificationId,
    pub created_at: AppTimestamp,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub backend: String,           // "user-notify", "notify-rust", "in-app only"
    pub delivered_natively: bool,
    pub degraded: bool,
    pub error_summary: Option<String>,
    pub kind: NotificationInboxKind,
}
```

### Inbox kinds

| Kind | Recorded when |
|------|--------------|
| `Attempt` | A notification send completes (success or failure) |
| `PermissionUpdate` | Permission state changes after a request |
| `SettingsUpdate` | User toggles native notifications on or off |

### Inbox operations

| Function | Description |
|----------|-------------|
| `inbox::initialize(cx)` | Load inbox from persisted config, install global |
| `inbox::snapshot(cx)` | Read current items as a cloned `Vec` |
| `inbox::unread_count(cx)` | Count unread items without cloning the full list |
| `inbox::record(item, cx)` | Prepend an item, cap at 200 entries, persist |
| `inbox::mark_all_read(cx)` | Mark every item as read, persist |
| `inbox::clear_all(cx)` | Remove all items, persist |

The max inbox size is 200 items (`MAX_INBOX_ITEMS`). Older items are truncated on insert. This keeps the config file from growing unbounded.

### Sidebar integration

`src/features/pages/notifications.rs` renders the inbox page in the sidebar. It observes `NotificationInboxState` and displays unread count, per-item metadata (backend used, timestamp, error if any), and "Mark all read" / "Clear all" action buttons. See [Getting Started](/docs/getting-started/) for how pages are registered in the sidebar router.

## Sending a notification

Build a `NotificationRequest` and call `send_from_window`:

```rust
use crate::notifications::{NotificationRequest, send_from_window};

// Standard notification with sound
let request = NotificationRequest::foreground("Download complete", "save.zip is ready");
send_from_window(request, window, cx);
```

`send_from_window` is async internally (it uses `cx.spawn`). The function returns immediately; the result is applied via snapshot mutation and inbox recording when the async work completes.

### NotificationRequest builders

| Method | Use case |
|--------|----------|
| `foreground(title, body)` | Standard notification with sound, `ForegroundOnly` importance |
| `action_buttons(title, body)` | Includes Open/Snooze action buttons (macOS `UNNotificationCategory`) |
| `reply(title, body)` | Includes a text reply input (macOS) |
| `background_worthy(title, body)` | Higher importance, no in-app fallback on native failure |

### NotificationRequest fields

```rust
pub struct NotificationRequest {
    pub title: SharedString,
    pub body: SharedString,
    pub play_sound: bool,          // default: true
    pub thread_id: Option<String>, // groups notifications on macOS
    pub category: Option<String>,  // maps to interactive action sets
    pub prefer_native: bool,       // default: true; set false to force in-app only
    pub importance: NotificationImportance,
}
```

### Importance levels

| Level | Behavior |
|-------|----------|
| `ForegroundOnly` | If native delivery fails, an in-app toast appears via `window.push_notification` |
| `BackgroundWorthy` | No in-app fallback on native failure. Marked `degraded: true` in the inbox. Use for update checks, sync status, and other non-urgent background events. |

### Send flow step by step

1. `send_from_window` checks `enabled_by_user` and `permission != Denied`.
2. If disabled or denied, returns a `UiOnly` result immediately. No native call is made.
3. If enabled and permitted, attempts primary backend (`UserNotifyBackend`).
4. On primary failure, attempts secondary backend (`NotifyRustBackend`).
5. Records the attempt in the inbox with backend used, delivery status, and any errors.
6. If native delivery failed and importance is `ForegroundOnly`, pushes an in-app toast via `window.push_notification`.

One detail worth knowing: the send function checks `enabled_by_user && permission != Denied` before calling any backend. If the user has disabled notifications or denied permission, the native backends are never contacted. This avoids pointless IPC and permission warnings.

### Send result

```rust
pub struct NotificationSendResult {
    pub backend_used: NotificationBackendKind,
    pub degraded: bool,
    pub delivered_natively: bool,
    pub error_summary: Option<SharedString>,
    pub importance: NotificationImportance,
}
```

`degraded` is `true` when the primary backend was available but the send fell through to the secondary, or when both backends failed and delivery fell back to in-app. Check this field to show warning indicators in your settings UI.

## Initialization

Call `notifications::initialize(cx)` during app startup. This does five things:

1. Creates `NotificationService` (selects backends based on platform).
2. Builds a `NotificationRuntimeSnapshot` from the service state.
3. Installs `NativeNotificationState` as a GPUI global.
4. Registers the `native_notifications` capability with the app's capability system (see [Architecture](/docs/architecture/) for how capabilities work).
5. Starts an async permission state refresh.

```rust
// In your app init function
notifications::initialize(cx);
notifications::inbox::initialize(cx);
```

### Runtime snapshot

```rust
pub fn snapshot(cx: &App) -> NotificationRuntimeSnapshot {
    cx.global::<NativeNotificationState>().snapshot.clone()
}
```

The snapshot contains `active_backend`, `permission`, `capabilities`, `enabled_by_user`, `degraded_reason`, and `last_backend_error`. Use it to drive notification status UI in settings pages. See the [Testing](/docs/testing/) page for how to mock globals like `NativeNotificationState` in test contexts.

## Toggling notifications at runtime

```rust
// Disable native notifications (user toggled a switch)
notifications::set_native_notifications_enabled(false, cx);

// Re-enable
notifications::set_native_notifications_enabled(true, cx);
```

This does three things: updates the persisted config (`native_notifications_enabled` field), mutates the runtime snapshot, and records a `SettingsUpdate` entry in the inbox. It also updates the capability system so the rest of the app can react to the change.

## Adding a custom backend

You might need a custom backend for a platform not covered by `user-notify` or `notify-rust`, or to integrate with a push notification service. Here is how to add one.

### Step 1: Create the backend file

Create `src/services/notifications/backend/my_backend.rs`:

```rust
use async_trait::async_trait;
use super::NotificationBackend;
use crate::notifications::{
    NotificationBackendKind, NotificationCapabilities,
    NotificationPermissionState, NotificationRequest,
};

pub struct MyBackend;

#[async_trait]
impl NotificationBackend for MyBackend {
    fn kind(&self) -> NotificationBackendKind {
        NotificationBackendKind::UiOnly // or add a new variant to the enum
    }

    fn capabilities(&self) -> NotificationCapabilities {
        NotificationCapabilities {
            can_request_permission: false,
            can_read_permission_state: false,
            can_send_immediate_native: true,
            can_send_interactive: false,
            requires_packaged_runtime: false,
        }
    }

    async fn refresh_permission_state(&self) -> NotificationPermissionState {
        NotificationPermissionState::Unsupported
    }

    async fn request_permission(&self) -> NotificationPermissionState {
        NotificationPermissionState::Unsupported
    }

    async fn send(&self, request: &NotificationRequest) -> anyhow::Result<()> {
        // Your delivery logic here
        Ok(())
    }
}
```

### Step 2: Register the module

In `backend/mod.rs`:

```rust
mod my_backend;
pub use my_backend::MyBackend;
```

### Step 3: Wire into NotificationService

In `service.rs`, modify `NotificationService::new()` to use your backend as the primary, secondary, or an intermediate fallback before `NotifyRustBackend`:

```rust
// Example: use MyBackend as primary on a specific platform
let primary = if cfg!(target_os = "my_platform") {
    Some(Arc::new(MyBackend) as Arc<dyn NotificationBackend>)
} else {
    // existing UserNotifyBackend logic
};
```

The fallback chain in `NotificationService::send()` tries `primary` then `secondary`. If you need more than two tiers, refactor `send()` to iterate over a `Vec<Arc<dyn NotificationBackend>>` instead.

## Testing notifications

Notification backends can be tested without firing real OS notifications. See [Testing](/docs/testing/) for the general GPUI test harness setup. The pattern for notifications:

1. Replace `NativeNotificationState` global with a test instance that uses a mock backend.
2. Call `send_from_window` and assert on the inbox state.
3. Use `inbox::snapshot(cx)` to verify the recorded attempt.

For debugging delivery issues in production, the [debugging techniques](/blog/rust-desktop-debugging-techniques/) post covers how to read the structured log output from the notification module (all log events use the `gpui_starter::notifications` target).

## Common patterns

### Show a notification after a long-running task

```rust
cx.spawn(async move |cx| {
    some_async_work().await;
    cx.update(|cx| {
        // Note: you need a window handle here; use the one captured before the spawn
        let request = NotificationRequest::foreground("Task done", "Your file has been exported");
        // send_from_window requires a Window reference, so use push_in_app_feedback
        // or dispatch through your window management layer
    });
}).detach();
```

### Conditionally show interactive buttons

Interactive notifications (actions, reply) only work on macOS with `UserNotifyBackend`. Check capabilities before offering the option:

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

### Display inbox unread count in the sidebar

```rust
let unread = notifications::inbox::unread_count(cx);
// Use `unread` to render a badge next to the Notifications nav item
```
