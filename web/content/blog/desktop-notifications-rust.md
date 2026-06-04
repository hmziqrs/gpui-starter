---
title: "Desktop notifications in Rust: a fallback strategy"
description: "How gpui-starter handles desktop notifications in Rust with automatic backend selection and graceful fallback when OS notifications fail."
date: 2026-05-25
tags: [Rust, desktop, notifications]
draft: false
---

Notifications seem simple until you ship them across platforms. macOS has its own notification center. Linux has libnotify, except when it doesn't. Windows has its notification API, which behaves differently depending on packaging format. Getting a banner to appear reliably on all three is harder than it should be.

gpui-starter solves this with a fallback chain for desktop notifications in Rust. It tries the native OS notification first. If the OS rejects the request (permission denied, no notification daemon, missing app identity), it falls back to an in-app toast rendered by GPUI. Everything gets persisted to a notification inbox regardless of which backend delivered it.

## The cross-platform mess

Each platform has its own constraints.

On macOS, you work with NSUserNotificationCenter (deprecated) or the newer UNUserNotificationCenter. Your app needs to request permission. If the user denies it, `notify()` silently does nothing. There is no error callback.

On Linux, libnotify is the standard, but it depends on a notification daemon running. Headless servers, minimal window managers, and some distros do not ship one. The `notify-rust` crate wraps this, but you still need to handle the case where no daemon is listening.

On Windows, the notification API works through COM. It requires an app identity (either a packaged app or a properly registered unpackaged app). Without that identity, notifications go nowhere.

Every platform can fail silently. A notification call that returns success might still produce no visible output. This is why fallback matters.

## The notification service trait

gpui-starter defines a trait that every backend must implement:

```rust
pub trait NotificationBackend: Send + Sync + 'static {
    fn send(&self, notification: Notification) -> NotificationResult;
    fn request_permission(&self) -> PermissionStatus;
    fn permission_status(&self) -> PermissionStatus;
    fn is_available(&self) -> bool;
}

pub struct Notification {
    pub title: String,
    pub body: String,
    pub icon: Option<Icon>,
    pub category: NotificationCategory,
    pub action: Option<NotificationAction>,
}

pub enum NotificationResult {
    Delivered,
    FallbackNeeded(String),
    Failed(Error),
}
```

The `is_available()` check runs at startup. The `request_permission()` call runs before the first notification is sent. And `FallbackNeeded` tells the dispatcher to try the next backend in the chain.

This trait-based approach is the same pattern we use for other platform services. The [architecture guide](/docs/architecture/) covers how these abstractions fit together across the codebase.

## Automatic backend selection

The dispatcher probes backends at runtime and picks the best one available:

```rust
pub struct NotificationDispatcher {
    backends: Vec<Box<dyn NotificationBackend>>,
    inbox: Entity<NotificationInbox>,
}

impl NotificationDispatcher {
    pub fn new(cx: &mut AppContext) -> Self {
        let mut backends: Vec<Box<dyn NotificationBackend>> = Vec::new();

        // Primary: native OS notifications
        #[cfg(target_os = "macos")]
        if let Some(backend) = MacOSBackend::new() {
            backends.push(Box::new(backend));
        }

        #[cfg(target_os = "linux")]
        if let Some(backend) = LinuxBackend::new() {
            backends.push(Box::new(backend));
        }

        #[cfg(target_os = "windows")]
        if let Some(backend) = WindowsBackend::new() {
            backends.push(Box::new(backend));
        }

        // Fallback: GPUI in-app toast
        backends.push(Box::new(InAppToastBackend::new()));

        let inbox = cx.new_model(|_| NotificationInbox::default());

        Self { backends, inbox }
    }

    pub fn send(&self, notification: Notification, cx: &AppContext) {
        for backend in &self.backends {
            if !backend.is_available() {
                continue;
            }

            match backend.send(notification.clone()) {
                NotificationResult::Delivered => {
                    self.persist(&notification, cx);
                    return;
                }
                NotificationResult::FallbackNeeded(reason) => {
                    log::info!(
                        "Backend failed ({}), trying next",
                        reason
                    );
                    continue;
                }
                NotificationResult::Failed(e) => {
                    log::warn!("Notification error: {}", e);
                    continue;
                }
            }
        }

        log::warn!("All notification backends exhausted");
    }
}
```

The dispatcher iterates through backends in priority order. If the native backend reports `FallbackNeeded` (which happens when permission was denied or no notification daemon exists), it moves to the next one. The in-app toast backend is always last and always succeeds because it renders inside the GPUI window.

The calling code never needs to know which backend actually delivered the notification. This is the same principle behind the [state management with GPUI entities](/blog/state-management-gpui-entities/) approach: hide platform details behind a clean interface.

## Permission handling

Permissions are the worst part of notification implementation. Each platform handles them differently.

macOS requires an explicit permission request before any notification can appear. The user gets a system dialog with "Allow" and "Deny". If they deny it, there is no programmatic way to re-ask. The app can only direct the user to System Settings.

Linux does not have a unified permission model. Whether notifications appear depends on the notification daemon and desktop environment configuration. There is no permission dialog to request.

Windows needs app identity registration. Without it, the permission request fails silently.

gpui-starter handles this by making `request_permission()` idempotent and safe to call multiple times:

```rust
fn ensure_permission(&self, cx: &AppContext) -> bool {
    for backend in &self.backends {
        if !backend.is_available() {
            continue;
        }

        match backend.permission_status() {
            PermissionStatus::Granted => return true,
            PermissionStatus::Denied => continue,
            PermissionStatus::NotDetermined => {
                match backend.request_permission() {
                    PermissionStatus::Granted => return true,
                    _ => continue,
                }
            }
        }
    }

    // All backends failed permission check. The in-app toast
    // backend doesn't need OS permission, so it will still work.
    false
}
```

If every OS-level backend fails the permission check, the in-app toast backend catches the notification. The user still sees it. It just stays inside the app window instead of appearing in the system notification center.

### A note on testing this

Testing permission flows is tricky because you cannot easily automate the OS permission dialog. We handle this in the test suite by injecting a mock backend that always reports `Granted`. The [testing GPUI applications](/blog/testing-gpui-applications/) post covers the mock injection pattern in more detail.

## The notification inbox

Notifications are ephemeral by nature. A user might miss one while looking at another window. gpui-starter solves this with a persistent notification inbox.

Every notification, regardless of which backend delivered it, gets stored:

```rust
#[derive(Clone, Debug)]
pub struct InboxEntry {
    pub id: Uuid,
    pub notification: Notification,
    pub delivered_via: String,
    pub read: bool,
    pub timestamp: DateTime<Utc>,
}

pub struct NotificationInbox {
    entries: Vec<InboxEntry>,
    unread_count: usize,
}
```

The inbox renders as a panel accessible from the sidebar. Unread count appears as a badge on the bell icon. Clicking an entry marks it as read and performs the notification's associated action if one exists.

Most notification libraries ignore this part. They fire the notification and forget about it. For a desktop app where notifications might carry important state changes, losing them to the OS notification history is unacceptable. The inbox persists to SQLite for the same reason, alongside other app data covered in the [notifications documentation](/docs/notifications/).

## Why this matters

A notification system that silently fails is worse than no notification system at all. Users assume the app told them something. They did not see it. They miss the event.

The fallback chain fixes this. Native OS notifications give the best experience when they work: they appear in the system notification center, respect Do Not Disturb settings, and follow the user's notification preferences. When they do not work, the in-app toast guarantees the notification is still visible. The inbox guarantees it is never permanently lost.

This pattern, wrapping platform APIs in a trait and falling back to a controlled in-app alternative, works for more than just notifications. File dialogs and deep links benefit from the same approach.

If you are building a desktop app with GPUI, check out the [getting started guide](/docs/getting-started/) to set up the notification service in your own project. The full notification module is also documented in the [notifications reference](/docs/notifications/) with configuration options for custom icons, categories, and action handlers.
