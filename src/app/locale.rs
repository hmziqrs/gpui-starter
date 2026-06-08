use gpui::{App, Global, SharedString};

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
