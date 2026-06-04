use tempfile::tempdir;

use super::*;
use crate::sidebar::Page;

#[test]
fn save_and_load_config_uses_json_state_file() {
    let dir = tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    let config = AppConfig {
        active_route: AppRoute::page(Page::Settings),
        sidebar_collapsed: true,
        ..AppConfig::default()
    };

    let bytes = serde_json::to_vec(&config).unwrap();
    save_config(&state_file, &bytes).unwrap();
    let (loaded, err) = load_config(&state_file);

    assert_eq!(err, None);
    assert_eq!(loaded.active_route, AppRoute::page(Page::Settings));
    assert!(loaded.sidebar_collapsed);
}

#[test]
fn corrupt_config_is_quarantined() {
    let dir = tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    std::fs::write(&state_file, "{not-json").unwrap();

    let (loaded, err) = load_config(&state_file);

    assert_eq!(loaded, AppConfig::default());
    assert!(err.is_some());
    assert!(!state_file.exists());
    assert!(state_file.with_extension("json.bad").exists());
}
