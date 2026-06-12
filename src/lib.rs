#![recursion_limit = "512"]

pub mod app;
pub mod features;
pub mod foundation;
pub mod persistence;
pub mod platform;
pub mod runtime;
pub mod services;
pub mod shell;
pub mod state;
#[cfg(test)]
pub mod testing;
pub mod ui;

pub use app::lifecycle;
pub use features::command_palette as launcher;
pub use features::pages as views;
pub use foundation::validation as input_validation;
pub use foundation::{errors, ids, time};
pub use persistence::sqlite::db_migrations;
#[cfg(target_os = "macos")]
pub use platform::desktop_shell::tray;
pub use platform::filesystem::paths;
pub use platform::input::shortcuts;
pub use platform::ipc;
pub use platform::network::websocket;
pub use platform::process::single_instance;
pub use runtime::{capabilities, events};
pub use services::{
    accessibility, commands, connectivity, crash_report, desktop_actions, error_surface, first_run,
    http_lab, i18n, logging, notifications, secure_storage, session, storage, tasks, telemetry,
    undo_stack, updater,
};
pub use shell::route as routes;
pub use shell::{app_menu, menus, root, sidebar, status_bar, title_bar};
pub use state::config_store as app_state;
pub use state::migrations as config_migrations;
