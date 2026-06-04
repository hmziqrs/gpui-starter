---
title: "Desktop notifications in Rust: native backends and toast fallbacks"
description: "How to implement notifications in a Rust desktop app using native OS backends on macOS with in-app toast fallback when permissions are denied."
date: 2026-06-05
tags: [Rust, notifications, tutorial]
draft: false
---

Notifications seem easy. Call an API, show a banner. Then you actually try to ship it. macOS wants a bundle identifier and explicit permission. Linux might not have a notification daemon running. Windows wants you registered as a packaged app. Every platform can accept your call, return success, and produce zero visible output.

I spent a few weeks building the notification system for gpui-starter. This post covers what I learned: talking to the native macOS notification center from Rust, using `notify-rust` as a portable fallback, and why an in-app toast is the safety net you can't skip.

## The platform situation

Here is what actually breaks on each platform.

On macOS, the modern API is `UNUserNotificationCenter`. It replaced the deprecated `NSUserNotificationCenter` years ago. You need two things for it to work: a valid bundle identifier in your app's `Info.plist`, and explicit user permission. If either is missing, your notification call succeeds silently. No error. No banner. The user never knows.

On Linux, `notify-rust` wraps libnotify/D-Bus. It works great when a notification daemon is running. On minimal window managers, headless setups, or some container environments, there is no daemon. The `show()` call on a `notify_rust::Notification` will fail with a D-Bus error.

On Windows, notifications require app identity through either MSIX packaging or the `ums-notification` COM registration for unpackaged apps. Without that identity, nothing renders.

The common thread: every platform has a silent failure mode. You can't assume the notification appeared just because the API returned `Ok(())`.

## Defining a backend trait

The first thing I did was define a trait that every notification backend must implement:

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

The `capabilities()` method matters more than you'd think. Different backends can do different things. The macOS native backend can request permission and show interactive notifications with action buttons. The `notify-rust` backend can't do either of those. The in-app toast backend always succeeds but lives inside the window. Code that sends notifications needs to know what's possible so it can adapt.

The `NotificationRequest` struct carries everything a backend might need:

```rust
pub struct NotificationRequest {
    pub title: SharedString,
    pub body: SharedString,
    pub play_sound: bool,
    pub thread_id: Option<String>,
    pub category: Option<String>,
    pub prefer_native: bool,
    pub importance: NotificationImportance,
}
```

The `importance` field matters. `ForegroundOnly` notifications are fine as in-app toasts. `BackgroundWorthy` notifications really should hit the OS notification center because the user might not have the app focused. When the fallback chain degrades and a background-worthy notification ends up as a toast, that's a problem worth surfacing in diagnostics.

## The macOS native backend

For macOS, I used the `user-notify` crate. It wraps `UNUserNotificationCenter` and handles the Objective-C interop. The initialization needs a bundle identifier:

```rust
const APP_ID: &str = "com.gpui-starter.app";

pub struct UserNotifyBackend {
    manager: Arc<dyn user_notify::NotificationManager>,
}

impl UserNotifyBackend {
    pub fn new() -> anyhow::Result<Self> {
        // Check that the app was launched with a proper bundle.
        // Raw `cargo run` won't have one.
        if let Err(reason) = platform_primary_runtime_status() {
            anyhow::bail!("{reason}");
        }

        let manager = user_notify::get_notification_manager(
            APP_ID.to_string(),
            None,
        );

        manager.register(
            Box::new(|response| {
                // Handle notification actions (button taps, replies).
            }),
            categories(),
        )?;

        Ok(Self { manager })
    }
}
```

One gotcha: if you launch the app with `cargo run` instead of the proper bundled app, `NSBundle::mainBundle().bundleIdentifier()` returns `None`. The notification center silently ignores everything. This trips up every new contributor. gpui-starter handles it by checking the bundle identifier at initialization and returning an error that points to the correct launch script.

### Permission handling on macOS

macOS shows the user a one-time dialog: "Allow" or "Deny". If they deny, you cannot re-ask programmatically. Your only option is to send them to System Settings.

The permission state is queried asynchronously through `UNUserNotificationCenter`:

```rust
async fn platform_permission_state() -> NotificationPermissionState {
    let (tx, rx) = tokio::sync::oneshot::channel();

    unsafe {
        let tx = RefCell::new(Some(tx));
        let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let status = settings.as_ref().authorizationStatus();
            let state = match status {
                UNAuthorizationStatus::Authorized
                | UNAuthorizationStatus::Provisional
                | UNAuthorizationStatus::Ephemeral => {
                    NotificationPermissionState::Authorized
                }
                UNAuthorizationStatus::Denied => NotificationPermissionState::Denied,
                UNAuthorizationStatus::NotDetermined => {
                    NotificationPermissionState::NotDetermined
                }
                status => NotificationPermissionState::Unavailable(
                    format!("unknown status: {status:?}")
                ),
            };
            if let Some(tx) = tx.take() {
                let _ = tx.send(state);
            }
        });

        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&block);
    }

    rx.await.unwrap_or_else(|err| {
        NotificationPermissionState::Unavailable(err.to_string())
    })
}
```

The unsafe block is necessary because Apple's API uses Objective-C blocks. The `RcBlock` from the `block2` crate handles the lifetime management. The `tx.take()` pattern ensures the oneshot channel fires exactly once.

### Sending with categories

macOS supports interactive notifications: action buttons and text input replies. You register categories with the notification center at startup, then reference them when sending:

```rust
fn categories() -> Vec<user_notify::NotificationCategory> {
    vec![
        user_notify::NotificationCategory {
            identifier: CATEGORY_ACTIONS.to_string(),
            actions: vec![
                user_notify::NotificationCategoryAction::Action {
                    identifier: "settings.open".to_string(),
                    title: "Open".to_string(),
                },
                user_notify::NotificationCategoryAction::Action {
                    identifier: "settings.snooze".to_string(),
                    title: "Snooze".to_string(),
                },
            ],
        },
        user_notify::NotificationCategory {
            identifier: CATEGORY_REPLY.to_string(),
            actions: vec![
                user_notify::NotificationCategoryAction::TextInputAction {
                    identifier: "settings.reply".to_string(),
                    title: "Reply".to_string(),
                    input_button_title: "Send".to_string(),
                    input_placeholder: "Type a reply".to_string(),
                },
            ],
        },
    ]
}
```

When the user taps an action button or sends a reply, the callback registered in `manager.register()` fires with the response. You route that to the right handler.

## The notify-rust fallback

When the native macOS backend isn't available (no bundle ID, permission denied, running on a non-macOS platform), the system falls back to `notify-rust`:

```rust
pub struct NotifyRustBackend;

#[async_trait]
impl NotificationBackend for NotifyRustBackend {
    fn kind(&self) -> NotificationBackendKind {
        NotificationBackendKind::NotifyRust
    }

    fn capabilities(&self) -> NotificationCapabilities {
        NotificationCapabilities {
            can_request_permission: false,
            can_read_permission_state: false,
            can_send_immediate_native: true,
            can_send_interactive: false,
            requires_packaged_runtime: cfg!(target_os = "windows"),
        }
    }

    async fn send(&self, request: &NotificationRequest) -> anyhow::Result<()> {
        let mut notification = notify_rust::Notification::new();
        notification
            .appname("GPUI Starter")
            .summary(&request.title)
            .body(&request.body);

        if request.play_sound {
            notification.sound_name("default");
        }

        notification.show()?;
        Ok(())
    }
}
```

This is simpler. No permission dance. No interactive categories. It fires a D-Bus notification on Linux and uses whatever the platform provides elsewhere. If no notification daemon is listening, `show()` returns an error that gets caught by the dispatcher.

## The dispatcher: trying backends in order

The `NotificationService` holds a primary and a secondary backend. It tries the primary first. On failure, it falls through to the secondary. If both fail, the notification becomes an in-app toast:

```rust
pub struct NotificationService {
    primary: Option<Arc<dyn NotificationBackend>>,
    secondary: Arc<dyn NotificationBackend>,
    initial_error: Option<String>,
}

impl NotificationService {
    pub fn new() -> Self {
        let mut initial_error = None;
        let primary = match UserNotifyBackend::new() {
            Ok(backend) => Some(Arc::new(backend) as Arc<dyn NotificationBackend>),
            Err(err) => {
                initial_error = Some(err.to_string());
                None
            }
        };

        Self {
            primary,
            secondary: Arc::new(NotifyRustBackend::new()),
            initial_error,
        }
    }

    pub async fn send(
        &self,
        request: NotificationRequest,
        enabled_by_user: bool,
    ) -> NotificationSendResult {
        if !enabled_by_user || !request.prefer_native {
            return NotificationSendResult {
                backend_used: NotificationBackendKind::UiOnly,
                degraded: false,
                delivered_natively: false,
                error_summary: Some("native notifications are disabled".into()),
                importance: request.importance,
            };
        }

        let mut errors = Vec::new();

        // Try primary backend first.
        if let Some(primary) = &self.primary {
            match primary.send(&request).await {
                Ok(()) => {
                    return NotificationSendResult {
                        backend_used: primary.kind(),
                        degraded: false,
                        delivered_natively: true,
                        error_summary: None,
                        importance: request.importance,
                    };
                }
                Err(err) => {
                    errors.push(format!("{}: {err:#}", primary.kind()));
                }
            }
        }

        // Try secondary fallback.
        match self.secondary.send(&request).await {
            Ok(()) => NotificationSendResult {
                backend_used: self.secondary.kind(),
                degraded: self.primary.is_some(),
                delivered_natively: true,
                error_summary: if errors.is_empty() {
                    None
                } else {
                    Some(errors.join("; ").into())
                },
                importance: request.importance,
            },
            Err(err) => {
                errors.push(format!("{}: {err:#}", self.secondary.kind()));
                // Both failed. Return as in-app only.
                NotificationSendResult {
                    backend_used: NotificationBackendKind::UiOnly,
                    degraded: true,
                    delivered_natively: false,
                    error_summary: Some(errors.join("; ").into()),
                    importance: request.importance,
                }
            }
        }
    }
}
```

The `degraded` flag on the result is `true` when we expected the primary backend to work but had to fall back. That distinction matters for diagnostics. Going through `notify-rust` because the user isn't on macOS is expected. Going through it because the macOS backend threw an error means something went wrong.

## The in-app toast: always works

When both native backends fail, the notification gets displayed as an in-app toast using GPUI's `window.push_notification()`. This is the guarantee: the user will see the notification if they have the app focused. It stays inside the window, so it won't appear in the system notification center and disappears when the window closes. But it's visible.

The caller never picks a backend. It calls `send_from_window()` with a `NotificationRequest`. The service decides which backend to use based on availability, permissions, and the user's settings. The result gets recorded in a persistent inbox so nothing is lost.

## Keeping a persistent inbox

Notifications are ephemeral. A banner appears for five seconds and vanishes. If the user was looking at another monitor, they missed it. gpui-starter solves this with a notification inbox backed by the config store:

```rust
pub struct NotificationInboxItem {
    pub id: NotificationId,
    pub created_at: AppTimestamp,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub backend: String,
    pub delivered_natively: bool,
    pub degraded: bool,
    pub error_summary: Option<String>,
    pub kind: NotificationInboxKind,
}
```

Every notification, regardless of which backend delivered it, gets recorded. The inbox caps at 200 items. A badge on the bell icon in the sidebar shows the unread count. This is the part most notification libraries skip. They fire and forget. For a desktop app where notifications carry real state (task completion, update availability, permission changes), losing them to the OS notification history is not acceptable.

## What I'd do differently

A few things I learned the hard way.

Check the bundle identifier at build time, not runtime. The runtime check works, but a compile-time error pointing to the right launch script would save debugging time.

The permission state cache goes stale. If the user changes notification settings in System Settings while your app is running, your cached state is wrong. gpui-starter refreshes on app activation, but I should also refresh before every send for critical notifications.

`notify-rust` on macOS uses the legacy notification API. It works, but you don't get the interactive categories or the proper grouping that `UNUserNotificationCenter` provides. If you're only targeting macOS, skip `notify-rust` entirely and use `user-notify` with an in-app fallback.

The fallback chain pattern, wrapping platform APIs in a trait and degrading gracefully, applies beyond notifications. File dialogs and deep links benefit from the same approach. The [architecture guide](/docs/architecture/) covers how gpui-starter structures these service abstractions across the codebase. The [notifications documentation](/docs/notifications/) has the full API reference for the notification service and inbox.

gpui-starter ships this notification system out of the box. If you're building a Rust desktop app with GPUI and want native notifications with automatic fallback, grab the [getting started guide](/docs/getting-started/) or check the source on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).
