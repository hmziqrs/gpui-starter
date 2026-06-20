use super::*;

#[test]
fn parses_supported_deep_links() {
    let home = AppRoute::parse_deep_link("gpui-starter://home").unwrap();
    assert_eq!(home, AppRoute::Page(Page::Home));
    assert_eq!(home.to_url(), "gpui-starter://home");
    assert_eq!(
        AppRoute::parse_deep_link("gpui-starter://settings/notifications").unwrap(),
        AppRoute::SettingsNotifications
    );
    assert_eq!(
        AppRoute::parse_deep_link("gpui-starter://diagnostics").unwrap(),
        AppRoute::Page(Page::Diagnostics)
    );
    assert_eq!(
        AppRoute::parse_deep_link("gpui-starter://notifications").unwrap(),
        AppRoute::Page(Page::Notifications)
    );
}

#[test]
fn rejects_unknown_deep_links() {
    assert!(AppRoute::parse_deep_link("https://example.com").is_err());
    assert!(AppRoute::parse_deep_link("gpui-starter://missing").is_err());
}

// --- URL validation tests -----------------------------------------------

#[test]
fn rejects_wrong_scheme() {
    let err = AppRoute::parse_deep_link("https://home").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unsupported scheme"),
        "expected scheme rejection, got: {msg}"
    );
}

#[test]
fn rejects_unexpected_host() {
    let err = AppRoute::parse_deep_link("gpui-starter://evil-host").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unexpected host"),
        "expected host rejection, got: {msg}"
    );
}

#[test]
fn rejects_path_traversal_in_segment() {
    let err = AppRoute::parse_deep_link("gpui-starter://settings/..%2Fetc").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("path traversal") || msg.contains("forbidden character"),
        "expected path-traversal rejection, got: {msg}"
    );
}

#[test]
fn rejects_null_byte_in_segment() {
    let err = AppRoute::parse_deep_link("gpui-starter://settings/\0notifications").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("forbidden character") || msg.contains("invalid deep link"),
        "expected null-byte rejection, got: {msg}"
    );
}

#[test]
fn accepts_deep_link_with_clean_query_params() {
    let result = AppRoute::parse_deep_link("gpui-starter://home?ref=test&source=menu");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AppRoute::Page(Page::Home));
}

#[test]
fn sanitize_control_chars_helper() {
    assert_eq!(sanitize_control_chars("hello\tworld\n"), "helloworld");
    assert_eq!(sanitize_control_chars("clean"), "clean");
    assert_eq!(sanitize_control_chars("\x07bell\x1besc"), "bellesc");
}

#[test]
fn validate_path_segment_helper() {
    assert!(validate_path_segment("notifications").is_ok());
    assert!(validate_path_segment("..").is_err());
    assert!(validate_path_segment("foo/../bar").is_err());
    assert!(validate_path_segment("seg\x00ment").is_err());
}
