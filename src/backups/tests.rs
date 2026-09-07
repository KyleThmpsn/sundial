use super::*;
use crate::test_support::TestDirectory;

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
