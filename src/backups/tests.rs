use super::*;
use crate::test_support::TestDirectory;
use std::io::Write;

fn flat_backup(root: &Path, source: &Path, automatic: bool) -> PathBuf {
    create(
        root,
        source,
        "settings-v16",
        "json",
        automatic,
        |_, file| {
            file.write_all(b"{\"version\":16}")
                .map_err(|error| error.to_string())
        },
    )
    .unwrap()
}

#[test]
fn readable_backups_are_flat_unique_and_source_is_only_in_the_index() {
    let directory = TestDirectory::new("flat-backups");
    let root = directory.0.join("backups");
    let first = directory.0.join("first/settings.json");
    let second = directory.0.join("second/settings.json");
    let one = flat_backup(&root, &first, true);
    let two = flat_backup(&root, &first, true);
    let three = flat_backup(&root, &second, true);
    let resolved = paths::resolve_path_for_comparison(&root).unwrap();
    assert_eq!(one.parent(), Some(resolved.as_path()));
    assert_eq!(two.parent(), one.parent());
    assert_eq!(three.parent(), one.parent());
    assert_ne!(one, two);
    assert_ne!(two, three);
    assert!(!root.join("sources").exists());
    assert_eq!(
        index::timestamp(time::OffsetDateTime::from_unix_timestamp(1_788_804_000).unwrap()),
        "2026-09-07_18-00-00Z"
    );
    assert!(
        one.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings-v16-20")
    );
    let store = index::Store::open(&root).unwrap();
    assert_eq!(store.records().len(), 3);
    assert_eq!(
        store.records()[one.file_name().unwrap().to_str().unwrap()].source,
        index::source_identity(&first).unwrap()
    );
}

#[test]
fn flat_retention_preserves_other_sources_manual_files_modified_backups_and_recovery() {
    let directory = TestDirectory::new("flat-retention");
    let root = directory.0.join("backups");
    let source = directory.0.join("a/settings.json");
    let other = directory.0.join("b/settings.json");
    let first = flat_backup(&root, &source, true);
    let second = flat_backup(&root, &source, true);
    let changed = flat_backup(&root, &source, true);
    let recovery = flat_backup(&root, &source, false);
    let unrelated = flat_backup(&root, &other, true);
    let manual = root.join("settings-v16-123.json");
    fs::write(&manual, b"manual").unwrap();
    fs::write(&changed, b"user edited backup").unwrap();
    assert_eq!(prune_automatic_backups(&root, &source, 0).unwrap(), 2);
    assert!(!first.exists());
    assert!(!second.exists());
    for path in [changed, recovery, unrelated, manual] {
        assert!(path.exists());
    }
}

#[test]
fn legacy_and_flat_backups_share_one_retention_limit() {
    let directory = TestDirectory::new("mixed-backup-retention");
    let root = directory.0.join("backups");
    let source = directory.0.join("settings.json");
    let legacy = create_source_directory(&root, &source).unwrap();
    fs::write(legacy.join("settings-v8-1.json"), b"legacy").unwrap();
    flat_backup(&root, &source, true);
    assert_eq!(prune_automatic_backups(&root, &source, 1).unwrap(), 1);
    assert_eq!(prune_automatic_backups(&root, &source, 1).unwrap(), 0);
}

#[test]
fn failed_backup_writer_does_not_register_an_automatic_backup() {
    let directory = TestDirectory::new("failed-flat-backup");
    let root = directory.0.join("backups");
    let source = directory.0.join("settings.json");
    let result = create(&root, &source, "settings-v16", "json", true, |_, file| {
        file.write_all(b"partial").unwrap();
        Err("injected copy failure".into())
    });
    assert!(result.unwrap_err().contains("injected copy failure"));
    assert!(index::Store::open(&root).unwrap().records().is_empty());
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("settings-"))
            .count(),
        0
    );
}

#[cfg(feature = "sqlite-account")]
#[test]
fn sqlite_backup_can_write_and_restore_a_reserved_flat_file() {
    let directory = TestDirectory::new("flat-sqlite-backup");
    let root = directory.0.join("backups");
    let source = directory.0.join("state.sqlite3");
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute_batch("CREATE TABLE fixture (value INTEGER); INSERT INTO fixture VALUES (42);")
        .unwrap();
    let backup = create(&root, &source, "state-v1", "sqlite3", true, |path, _| {
        db.backup(rusqlite::MAIN_DB, path, None)
            .map_err(|error| error.to_string())
    })
    .unwrap();
    let saved = rusqlite::Connection::open(&backup).unwrap();
    assert_eq!(
        saved
            .query_row("SELECT value FROM fixture", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        42
    );
    drop(saved);
    assert_eq!(prune_automatic_backups(&root, &source, 0).unwrap(), 1);
}

#[test]
fn automatic_backup_retention_is_per_source_and_preserves_recovery_files() {
    let directory = TestDirectory::new("backup-retention");
    let root = directory.0.join("backups");
    let source = directory.0.join("installation-a").join("settings.json");
    let other_source = directory.0.join("installation-b").join("settings.json");
    let backups = create_source_directory(&root, &source).unwrap();
    let other_backups = create_source_directory(&root, &other_source).unwrap();
    let legacy = root.join("settings-v8-0.json");
    let other = other_backups.join("settings-v8-0.json");
    fs::write(&legacy, b"keep").unwrap();
    fs::write(&other, b"keep").unwrap();
    fs::create_dir_all(&backups).unwrap();
    for index in 1..=4 {
        fs::write(backups.join(format!("settings-v8-{index}.json")), b"{}").unwrap();
        fs::write(
            backups.join(format!("state-sqlite-v1-{index}.sqlite3")),
            b"sqlite",
        )
        .unwrap();
    }
    for untouched in [
        "state-sqlite-recovery-1.sqlite3",
        "settings.json.bak",
        "settings-not-an-automatic-backup.json",
    ] {
        fs::write(backups.join(untouched), b"keep").unwrap();
    }

    assert_eq!(prune_automatic_backups(&root, &source, 2).unwrap(), 4);
    assert!(legacy.exists());
    assert!(other.exists());
    let names = fs::read_dir(&backups)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("settings-v8-") && name.ends_with(".json"))
            .count(),
        2
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| { name.starts_with("state-sqlite-v1-") && name.ends_with(".sqlite3") })
            .count(),
        2
    );
    assert!(
        names
            .iter()
            .any(|name| name == "state-sqlite-recovery-1.sqlite3")
    );
    assert!(names.iter().any(|name| name == "settings.json.bak"));
    assert!(
        names
            .iter()
            .any(|name| name == "settings-not-an-automatic-backup.json")
    );
}

#[test]
fn source_identity_is_stable_across_creation_and_normalized_paths() {
    let directory = TestDirectory::new("backup-identity");
    let root = directory.0.join("backups");
    let source = directory.0.join("settings.json");
    let before = source_directory(&root, &source).unwrap();
    fs::write(&source, b"{}").unwrap();
    assert_eq!(before, source_directory(&root, &source).unwrap());
    assert_eq!(
        before,
        source_directory(&root, &directory.0.join(".").join("settings.json")).unwrap()
    );
    assert_ne!(
        before,
        source_directory(&root, &directory.0.join("state.sqlite3")).unwrap()
    );
    #[cfg(windows)]
    assert_eq!(
        before,
        source_directory(&root, &source.with_file_name("SETTINGS.JSON")).unwrap()
    );
}

#[test]
fn pruning_a_missing_source_creates_nothing() {
    let directory = TestDirectory::new("missing-backups");
    let root = directory.0.join("backups");
    assert_eq!(
        prune_automatic_backups(&root, &directory.0.join("settings.json"), 2).unwrap(),
        0
    );
    assert!(!root.exists());
}

#[cfg(unix)]
#[test]
fn redirected_source_directory_is_rejected() {
    let directory = TestDirectory::new("redirected-backups");
    let root = directory.0.join("backups");
    let source = directory.0.join("settings.json");
    let scoped = source_directory(&root, &source).unwrap();
    fs::create_dir_all(scoped.parent().unwrap()).unwrap();
    let other = directory.0.join("other");
    fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&other, &scoped).unwrap();
    assert!(prune_automatic_backups(&root, &source, 2).is_err());
}
