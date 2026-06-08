mod app_sections;
mod event_sections;
mod runtime_sections;
mod telemetry_sections;

pub(crate) use app_sections::{
    render_developer_section,
    render_shortcuts_section,
    render_storage_section,
};
pub(crate) use event_sections::render_event_emitter_section;
pub(crate) use runtime_sections::{
    render_desktop_actions_section,
    render_runtime_boundaries_section,
};
pub(crate) use telemetry_sections::{
    render_telemetry_runtime_section,
    render_telemetry_section,
};
