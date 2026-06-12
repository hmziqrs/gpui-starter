use super::*;
use tempfile::tempdir;

#[test]
fn crash_report_new_populates_fields() {
    let report = CrashReport::new(
        "panic message".into(),
        "backtrace".into(),
        true,
        vec!["err1".into()],
    );
    assert_eq!(report.panic_message, "panic message");
    assert_eq!(report.backtrace, "backtrace");
    assert!(report.render_path);
    assert_eq!(report.recent_errors, vec!["err1"]);
    assert!(!report.id.is_empty());
    assert!(!report.app_version.is_empty());
    assert!(!report.os.is_empty());
    assert!(!report.arch.is_empty());
    assert!(!report.timestamp.is_empty());
}

#[test]
fn write_and_detect_crash_report() {
    let dir = tempdir().expect("tempdir");
    let report = CrashReport::new("test panic".into(), "fake backtrace".into(), false, vec![]);

    write_crash_report(&report, dir.path()).expect("write");

    let detected = detect_pending_reports(dir.path());
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].id, report.id);
    assert_eq!(detected[0].panic_message, "test panic");
}

#[test]
fn detect_pending_ignores_malformed_files() {
    let dir = tempdir().expect("tempdir");
    let reports_dir = dir.path().join("crash_reports");
    std::fs::create_dir_all(&reports_dir).expect("mkdir");

    // Write a valid report.
    let report = CrashReport::new("good".into(), "bt".into(), false, vec![]);
    write_crash_report(&report, dir.path()).expect("write");

    // Write a malformed JSON file.
    let bad_path = reports_dir.join("bad.json");
    std::fs::write(&bad_path, "not json").expect("write bad");

    let detected = detect_pending_reports(dir.path());
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].panic_message, "good");
}

#[test]
fn detect_pending_returns_empty_when_no_dir() {
    let dir = tempdir().expect("tempdir");
    let detected = detect_pending_reports(dir.path());
    assert!(detected.is_empty());
}

#[test]
fn snapshot_default_values() {
    let snap = CrashReportSnapshot::default();
    assert_eq!(snap.pending_count, 0);
    assert!(snap.last_crash_timestamp.is_none());
    assert!(snap.upload_endpoint.is_empty());
    assert!(snap.last_upload_error.is_none());
}

#[test]
fn write_crash_report_creates_directory() {
    let dir = tempdir().expect("tempdir");
    let nested = dir.path().join("does_not_exist");
    let report = CrashReport::new("msg".into(), "bt".into(), false, vec![]);
    write_crash_report(&report, &nested).expect("write should create dir");
    assert!(nested.join("crash_reports").exists());
}

#[test]
fn detect_pending_sorts_newest_first() {
    let dir = tempdir().expect("tempdir");

    // Write two reports with a small delay so timestamps differ.
    let r1 = CrashReport::new("first".into(), "bt".into(), false, vec![]);
    write_crash_report(&r1, dir.path()).expect("write");
    std::thread::sleep(std::time::Duration::from_millis(10));
    let r2 = CrashReport::new("second".into(), "bt".into(), false, vec![]);
    write_crash_report(&r2, dir.path()).expect("write");

    let detected = detect_pending_reports(dir.path());
    assert_eq!(detected.len(), 2);
    // Newest first, so the second report should be first.
    assert_eq!(detected[0].panic_message, "second");
    assert_eq!(detected[1].panic_message, "first");
}
