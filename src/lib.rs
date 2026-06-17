// 8192 (was 512): the test target expands several thousand macro levels
// (the gpui proc-macros recurse through syn). On CI this is paired with
// RUST_MIN_STACK=512 MiB (see .github/workflows/ci.yml) so the deeper
// expansion does not overflow the proc-macro thread stack — which on the
// macOS runner surfaces as SIGBUS (signal 10) and was the real cause of the
// long-standing "Test" job failure (not heap OOM, as the limit build jobs
// suggested).
#![recursion_limit = "8192"]
#![allow(
    clippy::map_unwrap_or,
    clippy::let_unit_value,
    clippy::explicit_auto_deref,
    clippy::unnecessary_sort_by,
    clippy::type_complexity,
    clippy::derivable_impls,
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::too_many_arguments,
    clippy::let_and_return,
    clippy::map_flatten,
    clippy::new_ret_no_self,
    clippy::enum_variant_names,
    clippy::unnecessary_map_or,
    clippy::needless_borrow,
    clippy::new_without_default,
    clippy::unneeded_wildcard_pattern,
    clippy::redundant_closure_call
)]

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
