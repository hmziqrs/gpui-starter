use es_fluent::EsFluent;
use es_fluent_lang::es_fluent_language;
use gpui::{
    Action, App, AppContext as _, Bounds, Focusable as _, Global, KeyBinding, SharedString,
    WindowBounds, WindowKind, WindowOptions, actions, px, size,
};
use gpui_component::{ActiveTheme, Root, TitleBar, WindowExt, text::markdown};
use strum::EnumIter;

pub mod lifecycle;

// ---------------------------------------------------------------------------
// Languages (es-fluent)
// ---------------------------------------------------------------------------

#[es_fluent_language]
#[derive(Clone, Copy, Debug, EnumIter, EsFluent, PartialEq)]
pub enum Languages {}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

actions!(
    app,
    [About, Quit, ToggleSearch, OpenDiagnostics, TriggerTestPanic]
);

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SelectLocale(pub SharedString);

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SelectFont(pub usize);

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SelectRadius(pub usize);

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct ExecuteCommand(pub crate::commands::CommandId);

// ---------------------------------------------------------------------------
// Locale state (reactive global for settings page)
// ---------------------------------------------------------------------------

pub const LOCALE_EN: &str = "en";
pub const LOCALE_ZH_CN: &str = "zh-CN";

#[derive(Clone, Debug)]
pub struct LocaleState(pub SharedString);

impl Global for LocaleState {}

pub fn current_locale(cx: &App) -> SharedString {
    cx.global::<LocaleState>().0.clone()
}

pub fn set_locale(locale: &str, cx: &mut App) {
    rust_i18n::set_locale(locale);
    let _ = crate::i18n::i18n().select_language(
        locale
            .parse()
            .unwrap_or_else(|_| es_fluent::unic_langid::langid!("en")),
    );
    cx.set_global::<LocaleState>(LocaleState(SharedString::from(locale.to_string())));
    crate::app_state::update_config(cx, |config| {
        config.locale = locale.to_string();
    });
    cx.refresh_windows();
}

pub fn set_theme_mode(mode: gpui_component::ThemeMode, cx: &mut App) {
    set_theme_mode_with_record(mode, true, cx);
}

pub fn set_theme_mode_with_record(mode: gpui_component::ThemeMode, record: bool, cx: &mut App) {
    let before = cx.theme().mode;
    gpui_component::Theme::change(mode, None, cx);
    if record {
        crate::undo_stack::record_theme_mode_change(before, mode, cx);
    }
    cx.refresh_windows();
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

pub fn init(cx: &mut App) {
    let startup_start = std::time::Instant::now();

    crate::lifecycle::install_panic_hook();

    // Crash marker: write on startup, detect previous crash
    crate::lifecycle::write_crash_marker();
    if let Some(marker) = crate::lifecycle::check_previous_crash() {
        tracing::warn!(
            target: "gpui_starter::lifecycle",
            marker = %marker,
            "previous crash detected"
        );
    }

    crate::lifecycle::set_startup_step("component_init", cx);

    // Must be called before using any gpui-component features
    let step_t = std::time::Instant::now();
    gpui_component::init(cx);
    tracing::info!(target: "gpui_starter::startup", elapsed_ms = step_t.elapsed().as_millis() as u64, "component_init done");

    crate::lifecycle::set_stage(crate::lifecycle::LifecycleStage::Starting, cx);
    crate::lifecycle::set_startup_step("app_state_init", cx);
    let step_t = std::time::Instant::now();
    crate::app_state::initialize(cx);
    tracing::info!(target: "gpui_starter::startup", elapsed_ms = step_t.elapsed().as_millis() as u64, "app_state_init done");

    crate::lifecycle::set_startup_step("logging_init", cx);
    let step_t = std::time::Instant::now();
    crate::logging::initialize(cx);
    tracing::info!(target: "gpui_starter::startup", elapsed_ms = step_t.elapsed().as_millis() as u64, "logging_init done");

    crate::lifecycle::set_startup_step("capabilities_init", cx);
    let step_t = std::time::Instant::now();
    crate::capabilities::initialize(cx);
    tracing::info!(target: "gpui_starter::startup", elapsed_ms = step_t.elapsed().as_millis() as u64, "capabilities_init done");
    crate::capabilities::set(
        "app_state",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "deep_links",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "diagnostics",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "notification_inbox",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "background_tasks",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "status_bar",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "connectivity",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "session",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "first_run",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "launcher",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "app_menu",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "command_registry",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );

    // Initialize es-fluent i18n for app and form text
    let system_locale = crate::i18n::detect_system_locale();
    tracing::info!(
        target: "gpui_starter::startup",
        system_locale = %system_locale,
        "detected system locale"
    );
    if let Err(err) = crate::i18n::init_i18n(<_ as Into<
        es_fluent::unic_langid::LanguageIdentifier,
    >>::into(Languages::default()))
    {
        tracing::error!("i18n initialization failed: {err}, using fallback locale");
    }

    let persisted = crate::app_state::config(cx);
    let locale_to_use = if persisted.locale.is_empty() {
        system_locale
    } else {
        persisted.locale.clone()
    };
    set_locale(&locale_to_use, cx);

    // Load extra themes from the themes/ directory (with hot-reload)
    let persisted_theme = persisted.theme.clone();
    if let Err(err) = gpui_component::ThemeRegistry::watch_dir(
        std::path::PathBuf::from(format!("{}/themes", env!("CARGO_MANIFEST_DIR"))),
        cx,
        move |cx| {
            if let Some(theme) = gpui_component::ThemeRegistry::global(cx)
                .themes()
                .get(persisted_theme.as_str())
                .cloned()
            {
                gpui_component::Theme::global_mut(cx).apply_config(&theme);
            }
        },
    ) {
        tracing::error!("Failed to watch themes directory: {}", err);
        crate::lifecycle::set_startup_error(format!("theme watch failed: {err}"), cx);
    }

    if let Some(show) = persisted.scrollbar_show {
        gpui_component::Theme::global_mut(cx).scrollbar_show = show;
    }
    cx.refresh_windows();

    cx.observe_global::<gpui_component::Theme>(move |cx| {
        let theme_name = cx.theme().theme_name().to_string();
        let scrollbar_show = cx.theme().scrollbar_show;
        crate::app_state::update_config(cx, |config| {
            config.theme = theme_name;
            config.scrollbar_show = Some(scrollbar_show);
        });
    })
    .detach();

    // Theme switching actions
    cx.on_action(|switch: &SwitchTheme, cx| {
        if let Some(config) = gpui_component::ThemeRegistry::global(cx)
            .themes()
            .get(&switch.0)
            .cloned()
        {
            gpui_component::Theme::global_mut(cx).apply_config(&config);
        }
        cx.refresh_windows();
    });
    cx.on_action(|switch: &SwitchThemeMode, cx| {
        set_theme_mode(switch.0, cx);
    });
    cx.on_action(|locale: &SelectLocale, cx| {
        set_locale(&locale.0, cx);
    });

    crate::launcher::init(cx);
    cx.set_global(crate::events::AppEventQueue::default());
    cx.set_global(crate::launcher::LauncherOpen(false));
    crate::lifecycle::set_startup_step("runtime_services_init", cx);
    let step_t = std::time::Instant::now();
    crate::tasks::initialize(cx);
    crate::error_surface::initialize(cx);
    crate::undo_stack::initialize(cx);
    crate::shortcuts::initialize(cx);
    cx.set_global(crate::services::tokio_runtime::TokioRuntimeGlobal(
        crate::services::tokio_runtime::TokioRuntime::new(),
    ));
    crate::connectivity::initialize(cx);
    crate::http_lab::initialize(cx);
    crate::desktop_actions::initialize(cx);
    crate::accessibility::initialize(cx);
    crate::secure_storage::initialize(cx);
    crate::session::initialize(cx);
    crate::storage::initialize(cx);

    // Run database migrations after storage is initialized
    crate::lifecycle::set_startup_step("db_migrations", cx);
    let migrations_t = std::time::Instant::now();
    if let Some(snapshot) = cx.try_global::<crate::storage::StorageSnapshot>()
        && snapshot.available
    {
        let db_path = std::path::PathBuf::from(snapshot.db_path.clone());
        match rusqlite::Connection::open(&db_path) {
            Ok(conn) => match crate::db_migrations::run_migrations(&conn) {
                Ok(version) => {
                    tracing::info!(
                        target: "gpui_starter::startup",
                        version,
                        elapsed_ms = migrations_t.elapsed().as_millis() as u64,
                        "db_migrations complete"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        target: "gpui_starter::startup",
                        error = %err,
                        "db_migrations failed"
                    );
                    crate::lifecycle::set_startup_error(format!("migration failed: {err}"), cx);
                }
            },
            Err(err) => {
                tracing::error!(
                    target: "gpui_starter::startup",
                    error = %err,
                    "failed to open db for migrations"
                );
            }
        }
    }

    crate::telemetry::initialize(cx);
    tracing::info!(target: "gpui_starter::startup", elapsed_ms = step_t.elapsed().as_millis() as u64, "runtime_services_init done");
    crate::telemetry::record_event("app_runtime_initialized", cx);
    crate::notifications::inbox::initialize(cx);
    crate::notifications::initialize(cx);
    crate::notifications::set_native_notifications_enabled(
        persisted.native_notifications_enabled,
        cx,
    );
    crate::services::updater::initialize(cx);

    // Key bindings
    cx.bind_keys([
        KeyBinding::new("cmd-k", ToggleSearch, None),
        KeyBinding::new("/", ToggleSearch, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-q", Quit, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-f4", Quit, None),
    ]);

    cx.on_action(|_: &Quit, cx| {
        crate::lifecycle::set_shutdown_step("begin_shutdown", cx);
        crate::lifecycle::set_stage(crate::lifecycle::LifecycleStage::ShuttingDown, cx);

        // Flush any debounced window bounds before shutdown so the final
        // position is persisted even if the 500 ms timer has not fired yet.
        crate::lifecycle::set_shutdown_step("flush_window_bounds", cx);
        crate::root::flush_window_bounds(cx);

        crate::lifecycle::set_shutdown_step("drain_tasks", cx);

        let drain = crate::tasks::drain_with_timeout(std::time::Duration::from_secs(5), cx);

        cx.spawn(async move |cx| {
            drain.await;

            cx.update(|cx| {
                crate::lifecycle::set_shutdown_step("stop_ipc", cx);
                crate::single_instance::shutdown(cx);
                crate::lifecycle::set_shutdown_step("stop_watchers", cx);
                crate::desktop_actions::shutdown(cx);
                crate::lifecycle::set_shutdown_step("unregister_shortcuts", cx);
                crate::shortcuts::shutdown(cx);
                // Flush any debounced config changes before continuing shutdown.
                crate::lifecycle::set_shutdown_step("flush_config", cx);
                crate::app_state::force_save(cx);
                crate::lifecycle::set_shutdown_step("flush_storage", cx);
                crate::storage::shutdown(cx);
                crate::lifecycle::set_shutdown_step("flush_telemetry", cx);
                crate::telemetry::record_event("app_shutdown_requested", cx);
                crate::telemetry::shutdown(cx);
                crate::lifecycle::set_shutdown_step("flush_logs", cx);
                crate::logging::shutdown(cx);
                crate::lifecycle::set_shutdown_step("remove_crash_marker", cx);
                crate::lifecycle::remove_crash_marker();
                crate::lifecycle::set_shutdown_step("quit", cx);
                cx.quit();
            });
        })
        .detach();
    });

    cx.on_action(|_: &About, cx| {
        if let Some(window) = cx.active_window().and_then(|w| w.downcast::<Root>()) {
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, cx| {
                        window.defer(cx, |window, cx| {
                            window.open_alert_dialog(cx, |alert, _, _| {
                                alert.title("About").description(markdown(
                                    "GPUI Starter\n\n\
                                    Version 0.1.0\n\n\
                                    A boilerplate for GPUI desktop apps.",
                                ))
                            });
                        });
                    })
                    .ok();
            });
        }
    });
    cx.on_action(|_: &OpenDiagnostics, cx| {
        crate::events::emit(
            crate::events::AppEventKind::Navigate(crate::routes::AppRoute::page(
                crate::sidebar::Page::Diagnostics,
            )),
            cx,
        );
    });
    #[cfg(debug_assertions)]
    cx.on_action(|_: &TriggerTestPanic, _cx| {
        panic!("gpui-starter test panic action");
    });
    cx.on_action(|command: &ExecuteCommand, cx| {
        let availability = crate::commands::availability(command.0, cx);
        if !availability.enabled {
            let reason = availability
                .disabled_reason
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "command disabled".to_string());
            tracing::warn!(
                target: "gpui_starter::commands",
                command = ?command.0,
                reason = %reason,
                "command ignored"
            );
            return;
        }
        crate::commands::execute(command.0, cx);
    });

    cx.activate(true);
    crate::lifecycle::set_startup_step("running", cx);
    crate::lifecycle::set_stage(crate::lifecycle::LifecycleStage::Running, cx);

    tracing::info!(
        target: "gpui_starter::startup",
        total_elapsed_ms = startup_start.elapsed().as_millis() as u64,
        "startup complete"
    );
}

// ---------------------------------------------------------------------------
// Window creation
// ---------------------------------------------------------------------------

pub fn create_new_window(title: &str, cx: &mut App) {
    let mut window_size = size(px(1400.0), px(900.0));
    if let Some(display) = cx.primary_display() {
        let display_size = display.bounds().size;
        window_size.width = window_size.width.min(display_size.width * 0.85);
        window_size.height = window_size.height.min(display_size.height * 0.85);
    }
    let persisted_bounds = crate::app_state::config(cx).window_bounds;
    let window_bounds = if let Some(bounds) = persisted_bounds {
        Bounds {
            origin: gpui::point(px(bounds.x), px(bounds.y)),
            size: gpui::size(px(bounds.width), px(bounds.height)),
        }
    } else {
        Bounds::centered(None, window_size, cx)
    };
    let title: SharedString = title.into();

    cx.spawn(async move |cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(window_bounds)),
            titlebar: Some(TitleBar::title_bar_options()),
            window_min_size: Some(gpui::Size {
                width: px(480.),
                height: px(320.),
            }),
            kind: WindowKind::Normal,
            #[cfg(target_os = "linux")]
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            #[cfg(target_os = "linux")]
            window_decorations: Some(gpui::WindowDecorations::Client),
            ..Default::default()
        };

        let Some(window) = cx
            .open_window(options, |window, cx| {
                let root_view = cx.new(|cx| crate::root::AppRoot::new(title.clone(), window, cx));

                let focus_handle = root_view.focus_handle(cx);
                window.defer(cx, move |window, cx| {
                    focus_handle.focus(window, cx);
                });

                cx.new(|cx| Root::new(root_view, window, cx))
            })
            .ok()
        else {
            tracing::error!("failed to open window");
            return Ok::<_, anyhow::Error>(());
        };

        window.update(cx, |_, window, _| {
            window.activate_window();
            window.set_window_title(&title);
        })?;

        Ok::<_, anyhow::Error>(())
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Re-exported action types used by menus and title_bar
// ---------------------------------------------------------------------------

use gpui_component::ThemeMode;

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SwitchTheme(pub SharedString);

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SwitchThemeMode(pub ThemeMode);
