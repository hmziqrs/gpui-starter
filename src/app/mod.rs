pub mod actions;
pub mod init;
pub mod lifecycle;
pub mod locale;
pub mod theme;
pub mod window;

// ---------------------------------------------------------------------------
// Re-exports — preserve the public API
// ---------------------------------------------------------------------------

pub use actions::{
    About, ExecuteCommand, Languages, OpenDiagnostics, Quit, SelectFont, SelectLocale,
    SelectRadius, SwitchTheme, SwitchThemeMode, ToggleSearch, TriggerTestPanic,
};

pub use locale::{LOCALE_EN, LOCALE_ZH_CN, LocaleState, current_locale, set_locale};

pub use theme::{set_theme_mode, set_theme_mode_with_record};

pub use init::init;

pub use window::create_new_window;
