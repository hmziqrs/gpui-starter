use gpui::{App, BorrowAppContext as _, Global};
#[cfg(target_os = "macos")]
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Default)]
pub struct ShortcutState {
    pub enabled: bool,
    pub registered: bool,
    pub accelerator: String,
    pub last_error: Option<String>,
}

impl Global for ShortcutState {}

#[cfg(target_os = "macos")]
static HOTKEY_MANAGER: OnceLock<Mutex<Option<global_hotkey::GlobalHotKeyManager>>> =
    OnceLock::new();

#[cfg(target_os = "macos")]
pub fn initialize(cx: &mut App) {
    let enabled = crate::app_state::config(cx).global_shortcut_enabled;
    let state = ShortcutState {
        enabled,
        registered: false,
        accelerator: "Alt+Space".to_string(),
        last_error: None,
    };
    cx.set_global(state);
    apply_enabled(enabled, cx);
}

#[cfg(target_os = "macos")]
pub fn apply_enabled(enabled: bool, cx: &mut App) {
    use global_hotkey::{
        GlobalHotKeyManager,
        hotkey::{Code, HotKey, Modifiers},
    };

    let mut state = snapshot(cx);
    state.enabled = enabled;
    state.registered = false;
    state.last_error = None;

    let slot = HOTKEY_MANAGER.get_or_init(|| Mutex::new(None));
    if let Ok(mut manager_slot) = slot.lock() {
        *manager_slot = None;
        if enabled {
            match GlobalHotKeyManager::new() {
                Ok(manager) => {
                    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
                    match manager.register(hotkey) {
                        Ok(()) => {
                            *manager_slot = Some(manager);
                            state.registered = true;
                        }
                        Err(err) => state.last_error = Some(err.to_string()),
                    }
                }
                Err(err) => state.last_error = Some(err.to_string()),
            }
        }
    } else {
        state.last_error = Some("failed to lock hotkey manager".to_string());
    }

    commit(state, cx);
}

#[cfg(not(target_os = "macos"))]
pub fn apply_enabled(enabled: bool, cx: &mut App) {
    let mut state = snapshot(cx);
    state.enabled = enabled;
    state.registered = false;
    state.last_error = if enabled {
        Some("global shortcut service currently configured for macOS".to_string())
    } else {
        None
    };
    commit(state, cx);
}

#[cfg(not(target_os = "macos"))]
pub fn initialize(cx: &mut App) {
    let state = ShortcutState {
        enabled: false,
        registered: false,
        accelerator: "Alt+Space".to_string(),
        last_error: Some("global shortcut service currently configured for macOS".to_string()),
    };
    set_capability(&state, cx);
    cx.set_global(state);
}

pub fn snapshot(cx: &App) -> ShortcutState {
    cx.try_global::<ShortcutState>()
        .cloned()
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
pub fn shutdown(cx: &mut App) {
    let slot = HOTKEY_MANAGER.get_or_init(|| Mutex::new(None));
    if let Ok(mut manager_slot) = slot.lock() {
        *manager_slot = None;
    }
    let mut state = snapshot(cx);
    state.registered = false;
    if state.enabled {
        state.last_error = Some("global shortcuts unregistered during shutdown".to_string());
    }
    commit(state, cx);
}

#[cfg(not(target_os = "macos"))]
pub fn shutdown(_cx: &mut App) {}

fn set_capability(state: &ShortcutState, cx: &mut App) {
    crate::capabilities::set(
        "global_shortcuts",
        crate::capabilities::CapabilityStatus {
            supported: cfg!(target_os = "macos"),
            enabled: state.registered,
            degraded: state.enabled && !state.registered,
            reason: state
                .last_error
                .as_ref()
                .map(|error| format!("shortcut unavailable: {error}").into())
                .or_else(|| {
                    if !cfg!(target_os = "macos") {
                        Some("global shortcut service currently configured for macOS".into())
                    } else if !state.enabled {
                        Some("disabled by user setting".into())
                    } else {
                        None
                    }
                }),
            last_error: state.last_error.clone().map(Into::into),
        },
        cx,
    );
}

fn commit(state: ShortcutState, cx: &mut App) {
    set_capability(&state, cx);
    cx.update_global::<ShortcutState, _>(|s, _cx| {
        *s = state;
    });
}
