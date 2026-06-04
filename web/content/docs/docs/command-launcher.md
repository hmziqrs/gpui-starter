---
title: "Command launcher"
description: "The Cmd+K command palette with fuzzy search, action registry, and popup window management in gpui-starter"
---

The command launcher is a Spotlight-style search overlay activated with `Cmd+K` (macOS) or `Ctrl+K` (Linux/Windows). Pressing `/` also opens it. It provides filtered search across all registered commands and dispatches the selected action.

The launcher is implemented across two modules:

| Module | Responsibility |
|--------|---------------|
| `src/features/command_palette.rs` | Popup window, search input, filtered results list, keyboard navigation |
| `src/services/commands.rs` | Command registry, availability checks, action execution |

If you are setting up the project for the first time, the [getting started guide](/docs/getting-started) covers the build and run steps. For a broader look at how the pieces fit together, see the [architecture overview](/docs/architecture).

## How it works

The launcher opens as a `WindowKind::PopUp` window centered on the primary display. A `LauncherOpen` global tracks whether it is already open. The `Launcher` entity manages the search state and result list. When the user selects a command, the `LauncherRoot` parent handles the `LauncherEvent::Act` event, calls `commands::execute()`, and closes the popup window.

```
User presses Cmd+K
  -> ToggleSearch action
  -> open_launcher()
  -> PopUp window with LauncherRoot entity
  -> Launcher entity (search input + result list)
  -> User selects a command
  -> LauncherEvent::Act(LauncherActionKind::Execute(CommandId))
  -> commands::execute(command_id, cx)
  -> Window closes
```

`LauncherRoot` also listens for window deactivation events. When the user clicks outside the popup, macOS changes the blur treatment on the deactivated window, which causes a visible flash. To avoid this, `LauncherRoot` calls `remove_window()` directly on deactivation instead of waiting a frame.

## CommandId enum

Every command has a unique variant in the `CommandId` enum. This is the stable identifier used throughout the system for registry lookups, availability checks, and dispatch.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandId {
    OpenHome,
    OpenForm,
    OpenHttpLab,
    OpenSettings,
    OpenNotifications,
    OpenDiagnostics,
    OpenAbout,
    ThemeLight,
    ThemeDark,
    StartDemoTask,
    CheckConnectivity,
    CopyDiagnostics,
    OpenLogsFolder,
    OpenConfigFolder,
    Undo,
    Redo,
}
```

Navigation commands like `OpenHome` and `OpenForm` emit `AppEventKind::Navigate(AppRoute)` through the app event bus. The routing system picks up the event and changes the active page. See [routing and navigation](/docs/routing) for how that dispatch works end to end.

## CommandSpec struct

Each command is described by a `CommandSpec`, which provides the data shown in the launcher UI.

```rust
pub struct CommandSpec {
    pub id: CommandId,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub icon: IconName,
}
```

| Field | Purpose |
|-------|---------|
| `id` | The `CommandId` variant used for dispatch |
| `title` | Primary label shown in the launcher |
| `subtitle` | Secondary description below the title |
| `icon` | Icon rendered next to the title |

## Command registry

The `registry()` function returns the full list of commands available in the launcher. The order of entries determines the display order when the search query is empty.

```rust
pub fn registry() -> Vec<CommandSpec> {
    vec![
        command(CommandId::OpenHome, "Home", "Open the Home page", IconName::Inbox),
        command(CommandId::OpenForm, "Form", "Open the Form page", IconName::File),
        command(CommandId::OpenHttpLab, "HTTP Lab", "Explore httpbin requests", IconName::Globe),
        command(CommandId::OpenSettings, "Settings", "Open the Settings page", IconName::Settings2),
        command(CommandId::ThemeLight, "Light Mode", "Switch to light theme", IconName::Sun),
        command(CommandId::ThemeDark, "Dark Mode", "Switch to dark theme", IconName::Moon),
        // ...
    ]
}

fn command(id: CommandId, title: &str, subtitle: &str, icon: IconName) -> CommandSpec {
    CommandSpec {
        id,
        title: title.into(),
        subtitle: subtitle.into(),
        icon,
    }
}
```

### Built-in commands

| CommandId | Title | Description |
|-----------|-------|-------------|
| `OpenHome` | Home | Navigate to the home page |
| `OpenForm` | Form | Navigate to the form page |
| `OpenHttpLab` | HTTP Lab | Navigate to the HTTP lab page |
| `OpenSettings` | Settings | Navigate to settings |
| `OpenNotifications` | Notifications | Navigate to notifications |
| `OpenDiagnostics` | Diagnostics | Navigate to diagnostics |
| `OpenAbout` | About | Navigate to about page |
| `ThemeLight` | Light Mode | Switch to light theme |
| `ThemeDark` | Dark Mode | Switch to dark theme |
| `StartDemoTask` | Start Demo Task | Start a background task |
| `CheckConnectivity` | Check Connectivity | Run a network probe |
| `CopyDiagnostics` | Copy Diagnostics | Copy diagnostics to clipboard |
| `OpenLogsFolder` | Open Logs Folder | Open logs in file manager |
| `OpenConfigFolder` | Open Config Folder | Open config in file manager |
| `Undo` | Undo | Undo last reversible command |
| `Redo` | Redo | Redo last reversed command |

The `ThemeLight` and `ThemeDark` commands call `set_theme_mode`, which swaps the active theme at runtime. The [themes guide](/docs/themes) covers how theme switching works and how to add custom themes.

## Command availability

Some commands depend on runtime conditions. The `availability()` function checks whether a command can run and returns a reason when it cannot.

```rust
pub fn availability(id: CommandId, cx: &App) -> CommandAvailability {
    let desktop = crate::desktop_actions::snapshot(cx);
    match id {
        CommandId::OpenHome
        | CommandId::OpenForm
        | CommandId::OpenHttpLab
        | CommandId::OpenSettings
        | CommandId::ThemeLight
        | CommandId::ThemeDark
        | CommandId::StartDemoTask => CommandAvailability {
            enabled: true,
            disabled_reason: None,
        },
        CommandId::CopyDiagnostics => CommandAvailability {
            enabled: desktop.clipboard_available,
            disabled_reason: (!desktop.clipboard_available)
                .then_some("Clipboard backend unavailable".into()),
        },
        CommandId::Undo => CommandAvailability {
            enabled: crate::undo_stack::can_undo(cx).is_some(),
            disabled_reason: crate::undo_stack::can_undo(cx)
                .is_none()
                .then_some("No undo available".into()),
        },
        // ... Redo follows the same pattern
    }
}
```

| Condition | Commands affected |
|-----------|------------------|
| Clipboard unavailable | `CopyDiagnostics` |
| System opener unavailable | `OpenLogsFolder`, `OpenConfigFolder` |
| No undo history | `Undo` |
| No redo history | `Redo` |

The `desktop_actions::snapshot` call queries the OS for clipboard and file opener support. On platforms where these backends are missing (some Linux window managers, for example), the commands appear in the list but are grayed out with an explanation.

## Command execution

The `execute()` function dispatches a `CommandId` to its handler. Each arm handles errors through the error surface, which reports them to the user with actionable buttons.

```rust
pub fn execute(id: CommandId, cx: &mut App) {
    match id {
        CommandId::OpenHome => navigate(Page::Home, cx),
        CommandId::OpenHttpLab => navigate(Page::HttpLab, cx),
        CommandId::ThemeLight => crate::app::set_theme_mode(ThemeMode::Light, cx),
        CommandId::StartDemoTask => crate::tasks::start_demo_task(cx),
        CommandId::CopyDiagnostics => {
            if let Err(error) = crate::desktop_actions::copy_diagnostics(cx) {
                tracing::warn!(%error, "copy diagnostics failed");
                crate::error_surface::report(
                    format!("Copy diagnostics failed: {error}"),
                    crate::errors::AppErrorSeverity::Error,
                    crate::error_surface::ErrorCategory::System,
                    vec![ErrorAction::Retry, ErrorAction::Dismiss],
                    cx,
                );
            }
        }
        // ...
    }
}
```

Navigation commands emit `AppEventKind::Navigate(AppRoute)` through the app event bus. The error surface gives the user a retry button for transient failures (clipboard backend not ready, for example) and a dismiss button for permanent ones.

## Registering a new command

Adding a command to the launcher requires changes in `src/services/commands.rs`. The launcher picks up new registry entries automatically. No changes to `command_palette.rs` are needed.

### Step 1: Add a CommandId variant

```rust
pub enum CommandId {
    // ... existing variants
    MyNewCommand,
}
```

The `Serialize, Deserialize` derive macros on the enum mean this variant can be persisted in the undo stack or config file. If you change variant names later, add a serde alias to maintain backward compatibility with saved data.

### Step 2: Add a CommandSpec to the registry

```rust
command(CommandId::MyNewCommand, "My Command", "Does something useful", IconName::Star),
```

The position in the `vec![]` determines the display order when no search query is active. Group related commands together so users can find them quickly.

### Step 3: Add an availability check

In `availability()`, either match the variant in the default enabled arm (the `|` chain at the top of the match) or add a dedicated arm with a condition. Commands that always work go in the default arm.

### Step 4: Add an execution arm

```rust
CommandId::MyNewCommand => {
    // your logic here
}
```

If the action can fail, wrap it in `if let Err(error)` and report through `error_surface::report`. This gives the user a visible error with retry/dismiss buttons instead of a silent failure.

## Popup window configuration

The launcher opens as a `WindowKind::PopUp` window. The window bounds and appearance are set in `open_launcher()`:

```rust
let window_w = px(620.);
let window_h = px(460.);

let options = WindowOptions {
    window_bounds: Some(WindowBounds::Windowed(bounds)),
    titlebar: None,
    focus: true,
    show: true,
    kind: WindowKind::PopUp,
    is_movable: true,
    is_resizable: false,
    window_background: WindowBackgroundAppearance::Blurred,
    window_min_size: Some(gpui::Size {
        width: window_w,
        height: window_h,
    }),
    ..Default::default()
};
```

| Property | Value | Notes |
|----------|-------|-------|
| Size | 620 x 460 px | Fixed, non-resizable |
| Position | Centered on primary display, offset 12% from top | Falls back to fixed coordinates if no display is detected |
| Background | Blurred | Uses `WindowBackgroundAppearance::Blurred` for a translucent backdrop |
| Title bar | None | Frameless popup |
| Movable | Yes | User can drag the window |

On macOS, `LauncherRoot::new` also installs a `LiquidGlass` effect, which creates an `NSGlassEffectView` directly in the native view hierarchy. This gives the popup window a system-native translucency effect without patching GPUI source.

## Search and filtering

The `Launcher` entity performs case-insensitive substring matching against both `title` and `subtitle`:

```rust
fn refilter(&mut self, cx: &mut Context<Self>) {
    let q = self.input.read(cx).value().to_lowercase();
    self.filtered = if q.is_empty() {
        (0..self.items.len()).collect()
    } else {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.title.to_lowercase().contains(&q)
                    || item.subtitle.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    };
    self.selected_index = 0;
    cx.notify();
}
```

When the query is empty, all commands are shown. Results reset the selection to the first item on every keystroke. The current implementation uses substring matching, which is sufficient for the built-in command set. If you need ranked fuzzy matching (where "ldm" matches "Light Mode"), replace the `contains` check with a fuzzy scorer and sort by score.

## Keyboard navigation

The launcher binds three actions within its `CONTEXT` focus scope:

| Key | Action | Behavior |
|-----|--------|----------|
| Up | `SelectPrev` | Move selection up, wrapping to the last item |
| Down | `SelectNext` | Move selection down, wrapping to the first item |
| Escape | `Dismiss` | Close the launcher without executing |

Enter is handled through the `InputEvent::PressEnter` subscription on the search input, which calls `act()` to dispatch the selected command and close the window. The mouse also works: hovering an item updates the selection, and clicking dispatches it immediately.

The footer bar renders a static hint showing the three available keys. This is pure UI text, not bound to anything. The actual keybindings are defined in `init()`:

```rust
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
    ]);
}
```

## Customizing launcher behavior

To change how the launcher appears or behaves, modify these areas.

**Window size and position:** edit the `window_w`, `window_h`, and the `0.12` vertical offset in `open_launcher()`.

**Search behavior:** replace the substring filter in `refilter()` with a different matching algorithm. A fuzzy scorer with ranked results is a common upgrade. You could also add category grouping or recently-used sorting.

**Item rendering:** modify the `Render` implementation for `Launcher` to change how results display. The current layout renders an icon in a rounded square next to the title and subtitle. You could add keyboard shortcut hints, section dividers, or badge indicators.

**Action kinds:** extend `LauncherActionKind` to support action types beyond `Execute`, then handle them in `LauncherRoot`'s subscription callback. For example, you could add a `Preview` variant that shows a tooltip without closing the popup.

## See also

- [Getting started](/docs/getting-started) for project structure and setup
- [Architecture](/docs/architecture) for GPUI patterns used throughout the app
- [Routing and navigation](/docs/routing) for how navigation commands change the active page
- [Themes](/docs/themes) for theme switching commands and custom theme setup
- [Building a command launcher in GPUI](/blog/building-command-launcher-gpui) for a tutorial walkthrough
