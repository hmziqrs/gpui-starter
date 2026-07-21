use es_fluent::EsFluent;
use es_fluent_lang::es_fluent_language;
use gpui::{Action, SharedString, actions};
use gpui_component::ThemeMode;
use strum::EnumIter;

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
    [
        About,
        Quit,
        ToggleSearch,
        OpenDiagnostics,
        TriggerTestPanic,
        Restart
    ]
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
// Re-exported action types used by menus and title_bar
// ---------------------------------------------------------------------------

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SwitchTheme(pub SharedString);

#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct SwitchThemeMode(pub ThemeMode);
