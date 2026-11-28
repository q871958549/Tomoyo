use super::*;

#[test]
fn rolling_log_rotates_two_generations_strictly_at_budget() {
    let dir = std::env::temp_dir().join(format!("tomoyo-diag-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = RollingLogState::open(&dir, "unit", 32 * 1024, true);
    let chunk = vec![b'X'; 20 * 1024];
    for _ in 0..5 {
        st.write_bytes(&chunk);
    }
    assert!(dir.join("unit.1.log").is_file(), "expected 1st backup");
    assert!(dir.join("unit.2.log").is_file(), "expected 2nd backup");
    let cur = std::fs::metadata(dir.join("unit.log")).unwrap().len();
    assert!(cur <= 32 * 1024, "current log exceeded cap: {cur}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn trace_ids_are_monotonic_and_prefixed() {
    let a = next_trace_id("PLG");
    let b = next_trace_id("PLG");
    assert!(a.starts_with("PLG-"));
    assert_ne!(a, b);
}

#[test]
fn beijing_timestamp_uses_utc_plus_8_format() {
    let ts = format_beijing_now();
    assert!(ts.ends_with(" +08:00"), "unexpected timestamp: {ts}");
    assert_eq!(ts.len(), 30, "expected YYYY-MM-DD HH:MM:SS.mmm +08:00, got: {ts}");
}

#[test]
fn archive_diagnostics_rotates_open_logs_and_crash_snapshot() {
    let dir = std::env::temp_dir().join(format!("tomoyo-arch-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = RollingLogState::open(&dir, "tomoyo", 64 * 1024, false);
    st.write_bytes(b"2026-11-23 10:00:00.000 +08:00  WARN test warning\n");
    std::fs::write(dir.join("last_crash.txt"), b"=== TOMOYO CRASH RECORD ===\n").unwrap();

    let count = archive_diagnostic_issues_in(&dir).expect("archive should succeed");
    assert!(count >= 2);
    assert!(!dir.join("last_crash.txt").exists(), "crash snapshot should be archived away");
    assert_eq!(std::fs::metadata(dir.join("tomoyo.log")).unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
