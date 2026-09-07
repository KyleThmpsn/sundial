use super::*;
use crate::test_support::TestDirectory;

#[test]
fn activity_entries_have_utc_time_and_severity() {
    let entry = Entry::error("Build failed");
    let timestamp = entry.formatted();
    let (timestamp, message) = timestamp.split_once(' ').unwrap();
    assert!(timestamp.contains('T') && timestamp.ends_with('Z'));
    assert_eq!(message, "[Error] Build failed");
    assert!(Entry::info("Ready").formatted().ends_with("[Info] Ready"));
}

#[test]
fn file_log_appends_across_sessions_and_keeps_two_archives() {
    let directory = TestDirectory::new("activity-rotation");
    let path = directory.0.join("sundial.log");
    for session in 0..6 {
        let entry = Entry::info(format!("Session {session} {}", "x".repeat(80)));
        append_at(&path, &entry, 200).unwrap();
    }
    assert!(fs::read_to_string(&path).unwrap().contains("Session 5"));
    assert!(
        fs::read_to_string(path.with_extension("log.1"))
            .unwrap()
            .contains("Session 4")
    );
    assert!(
        fs::read_to_string(path.with_extension("log.2"))
            .unwrap()
            .contains("Session 3")
    );
    assert!(!path.with_extension("log.3").exists());
    for name in ["sundial.log", "sundial.log.1", "sundial.log.2"] {
        assert!(fs::metadata(directory.0.join(name)).unwrap().len() <= 200);
    }
}

#[test]
fn sessions_append_without_truncating_existing_events() {
    let directory = TestDirectory::new("activity-session");
    let path = directory.0.join("parhelion.log");
    for message in ["First session", "Second session"] {
        let mut logger = FileLog {
            path: Some(path.clone()),
            error: None,
        };
        logger.append(&Entry::info(message));
        assert!(logger.error().is_none());
    }
    let text = fs::read_to_string(path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.lines().next().unwrap().contains("First session"));
    assert!(text.lines().nth(1).unwrap().contains("Second session"));
}

#[test]
fn huge_unicode_entries_are_bounded_and_remain_valid_utf8() {
    let directory = TestDirectory::new("activity-large-entry");
    let path = directory.0.join("parhelion.log");
    append_at(
        &path,
        &Entry::info("💡".repeat(MAX_ENTRY_BYTES)),
        MAX_FILE_BYTES,
    )
    .unwrap();
    let text = fs::read_to_string(path).unwrap();
    assert!(text.len() <= MAX_ENTRY_BYTES);
    assert!(text.ends_with(" [truncated]\n"));
}

#[test]
fn write_failures_are_reported_and_recover_without_touching_other_files() {
    let directory = TestDirectory::new("activity-failure");
    let parent = directory.0.join("blocked");
    fs::write(&parent, "keep me").unwrap();
    let mut logger = FileLog {
        path: Some(parent.join("sundial.log")),
        error: None,
    };
    logger.append(&Entry::info("Not saved"));
    assert!(logger.error().is_some());
    assert_eq!(fs::read_to_string(&parent).unwrap(), "keep me");
    logger.path = Some(directory.0.join("sundial.log"));
    logger.append(&Entry::info("Recovered"));
    assert!(logger.error().is_none());
}

#[test]
fn another_writer_cannot_rotate_or_append_while_locked() {
    let directory = TestDirectory::new("activity-lock");
    let path = directory.0.join("sundial.log");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("log.lock"))
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    assert!(append_at(&path, &Entry::info("Blocked"), 200).is_err());
    assert!(!path.exists());
    drop(lock);
    append_at(&path, &Entry::info("Saved"), 200).unwrap();
}

#[test]
fn disabled_file_log_is_memory_only() {
    let mut logger = FileLog::default();
    logger.append(&Entry::info("Preview"));
    assert!(logger.directory().is_none());
    assert!(logger.error().is_none());
}

#[test]
fn rotation_only_touches_the_selected_products_files() {
    let directory = TestDirectory::new("activity-products");
    let sundial = directory.0.join("sundial.log");
    let parhelion = directory.0.join("parhelion.log");
    append_at(&sundial, &Entry::info("Keep Sundial history"), 200).unwrap();
    let original = fs::read(&sundial).unwrap();
    for _ in 0..4 {
        append_at(&parhelion, &Entry::info("x".repeat(120)), 200).unwrap();
    }
    assert_eq!(fs::read(sundial).unwrap(), original);
    assert!(!directory.0.join("sundial.log.1").exists());
}

#[test]
fn exact_size_limit_rotates_only_when_the_next_entry_would_overflow() {
    let directory = TestDirectory::new("activity-boundary");
    let path = directory.0.join("sundial.log");
    let entry = Entry::info("x".repeat(80));
    let limit = entry.formatted().len() + 1;
    append_at(&path, &entry, limit).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), limit as u64);
    assert!(!path.with_extension("log.1").exists());
    append_at(&path, &Entry::info("Next event"), limit).unwrap();
    assert_eq!(
        fs::metadata(path.with_extension("log.1")).unwrap().len(),
        limit as u64
    );
}
