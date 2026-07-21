use super::*;
use std::fs;

#[test]
fn valid_deep_link_urls() {
    assert!(validate_deep_link_url("gpui-starter://home").is_ok());
    assert!(validate_deep_link_url("gpui-starter://settings").is_ok());
    // "http" was an orphan host — its feature (http_lab) was removed in the
    // gpui-query migration, so it is correctly rejected now.
    assert!(validate_deep_link_url("gpui-starter://http").is_err());
    assert!(validate_deep_link_url("gpui-starter://settings/notifications").is_ok());
    assert!(validate_deep_link_url("gpui-starter://diagnostics").is_ok());
    assert!(validate_deep_link_url("gpui-starter://notifications").is_ok());
    assert!(validate_deep_link_url("gpui-starter://about").is_ok());
    assert!(validate_deep_link_url("gpui-starter://form").is_ok());
}

#[test]
fn rejects_wrong_scheme() {
    let err = validate_deep_link_url("https://example.com").unwrap_err();
    assert!(
        matches!(err, AppError::InvalidDeepLink { ref reason, .. } if reason.contains("unsupported scheme"))
    );
}

#[test]
fn rejects_unexpected_host() {
    let err = validate_deep_link_url("gpui-starter://evil-host").unwrap_err();
    assert!(
        matches!(err, AppError::InvalidDeepLink { ref reason, .. } if reason.contains("unexpected host"))
    );
}

#[test]
fn rejects_malformed_url() {
    assert!(validate_deep_link_url("://missing-scheme").is_err());
}

#[test]
fn file_path_rejects_traversal() {
    let err = validate_file_path("../../etc/passwd", &[]).unwrap_err();
    assert!(matches!(err, ValidationError::PathTraversal { .. }));
}

#[test]
fn file_path_rejects_escape_from_allowed_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("inner.txt");
    fs::write(&file_path, "hello").unwrap();

    // Using a sibling temp dir as the allowed dir should fail.
    let other_tmp = tempfile::tempdir().unwrap();
    let err = validate_file_path(
        file_path.to_str().unwrap(),
        &[other_tmp.path().to_path_buf()],
    )
    .unwrap_err();
    assert!(matches!(err, ValidationError::EscapesAllowedDir { .. }));
}

#[test]
fn file_path_accepts_within_allowed_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("good.txt");
    fs::write(&file_path, "data").unwrap();

    let result = validate_file_path(file_path.to_str().unwrap(), &[tmp.path().to_path_buf()]);
    assert!(result.is_ok());
}

#[test]
fn file_path_rejects_nonexistent() {
    let err = validate_file_path("/no/such/file/ever", &[PathBuf::from("/")]).unwrap_err();
    assert!(matches!(err, ValidationError::NotFound { .. }));
}

#[test]
fn sanitize_strips_control_chars() {
    let dirty = "  hello\x00\x01\x02 world\t\n  ";
    assert_eq!(sanitize_string(dirty), "hello world");
}

#[test]
fn sanitize_trims_and_limits_length() {
    let long: String = "a".repeat(5000);
    let result = sanitize_string(&long);
    assert_eq!(result.len(), MAX_SANITIZED_LENGTH);
    // Result is trimmed so no surrounding whitespace.
    assert!(result.starts_with('a'));
}

#[test]
fn sanitize_keeps_newline_and_tab() {
    assert_eq!(sanitize_string("line1\nline2\ttab"), "line1\nline2\ttab");
}

#[test]
fn valid_notification_ids() {
    assert!(validate_notification_id("abc123"));
    assert!(validate_notification_id("a-b-c"));
    assert!(validate_notification_id("ABC-123-xyz"));
    assert!(validate_notification_id(
        "550e8400-e29b-41d4-a716-446655440000"
    ));
}

#[test]
fn invalid_notification_ids() {
    assert!(!validate_notification_id(""));
    assert!(!validate_notification_id("has space"));
    assert!(!validate_notification_id("under_score"));
    assert!(!validate_notification_id("special!char"));
    assert!(!validate_notification_id("dot.name"));
}
