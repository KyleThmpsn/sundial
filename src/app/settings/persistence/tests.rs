use super::*;
use crate::test_support::TestDirectory;
use serde_json::json;

// These tests use disposable files and injected process state. Never depend on
// whether a developer happens to have Destiny running alongside the suite.
fn save_json_with_writer(
    path: &Path,
    document: &Value,
    expected: &Value,
    normalize: bool,
    backups: &Path,
    replace: impl FnOnce(&Path, &[u8], &[u8]) -> io::Result<()>,
) -> Result<SaveJsonResult, String> {
    super::save_json_with_writer(
        path,
        document,
        expected,
        normalize,
        backups,
        replace,
        || Ok(()),
    )
    .map_err(Into::into)
}

fn save_json_checked_with_backup_root(
    path: &Path,
    document: &Value,
    expected: &Value,
    normalize: bool,
    backups: &Path,
) -> Result<SaveJsonResult, String> {
    save_json_with_writer(
        path,
        document,
        expected,
        normalize,
        backups,
        storage::replace_file_if_unchanged,
    )
}

fn fixture(name: &str) -> (TestDirectory, PathBuf, Value, Value) {
    let directory = TestDirectory::new(name);
    let path = directory.0.join("settings.json");
    let before = json!({"version": 8, "test_value": "original"});
    let after = json!({"version": 8, "test_value": "edited"});
    fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
    (directory, path, before, after)
}

#[test]
fn every_settings_write_requires_a_closed_game_and_known_process_state() {
    assert!(
        require_game_closed(Ok(true))
            .unwrap_err()
            .contains("Close Destiny 2")
    );
    assert!(require_game_closed(Ok(false)).is_ok());
    assert_eq!(
        require_game_closed(Err("process check failed".into())),
        Err("process check failed".into())
    );
}

#[test]
fn settings_only_save_is_blocked_if_the_game_starts_during_preparation() {
    let (directory, path, before, after) = fixture("settings-game-started");
    let checks = std::cell::Cell::new(0);
    let error = super::save_json_with_writer(
        &path,
        &after,
        &before,
        false,
        &directory.0.join("backups"),
        |_, _, _| panic!("must not replace with Destiny running"),
        || {
            checks.set(checks.get() + 1);
            require_game_closed(Ok(checks.get() > 1))
        },
    )
    .err()
    .unwrap();
    assert!(error.message.contains("Close Destiny 2"));
    assert_eq!(load_workspace_json(&path).unwrap(), before);
}

#[test]
fn stale_source_is_rejected_under_the_writer_lock() {
    let (directory, path, before, after) = fixture("stale-settings-save");
    let external = json!({"version": 8, "test_value": "external"});
    fs::write(&path, serde_json::to_vec(&external).unwrap()).unwrap();
    let error = save_json_checked_with_backup_root(
        &path,
        &after,
        &before,
        false,
        &directory.0.join("backups"),
    )
    .err()
    .unwrap();
    assert!(error.contains("changed outside Sundial"));
    assert_eq!(load_workspace_json(&path).unwrap(), external);
    assert!(!directory.0.join("backups").exists());
}

#[test]
fn late_external_write_is_preserved_before_replacement() {
    let (directory, path, before, after) = fixture("late-settings-writer");
    let external = b"{\"version\":8,\"external\":true}";
    let result = save_json_with_writer(
        &path,
        &after,
        &before,
        false,
        &directory.0.join("backups"),
        |path, bytes, original| {
            fs::write(path, external)?;
            storage::replace_file_if_unchanged(path, bytes, original)
        },
    );
    assert!(
        result
            .err()
            .unwrap()
            .contains("current contents were preserved")
    );
    assert_eq!(fs::read(&path).unwrap(), external);
}

#[test]
fn verification_conflict_preserves_the_newer_save_and_original_backup() {
    let (directory, path, before, after) = fixture("settings-verification-conflict");
    let backups = directory.0.join("backups");
    let external = b"{\"version\":8,\"external\":true}";
    let result = save_json_with_writer(
        &path,
        &after,
        &before,
        false,
        &backups,
        |path, bytes, original| {
            storage::replace_file_if_unchanged(path, bytes, original)?;
            fs::write(path, external)
        },
    );
    assert!(
        result
            .err()
            .unwrap()
            .contains("current contents were preserved")
    );
    assert_eq!(fs::read(&path).unwrap(), external);
    let backup = fs::read_dir(&backups)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("settings-v")
        })
        .unwrap()
        .path();
    assert_eq!(load_workspace_json(&backup).unwrap(), before);
}

#[test]
fn error_after_replacement_is_a_verified_save_with_a_durability_warning() {
    let (directory, path, before, after) = fixture("settings-post-rename-error");
    let receipt = save_json_with_writer(
        &path,
        &after,
        &before,
        false,
        &directory.0.join("backups"),
        |path, bytes, original| {
            storage::replace_file_if_unchanged(path, bytes, original)?;
            Err(io::Error::other("injected parent directory sync failure"))
        },
    )
    .unwrap();
    assert!(
        receipt
            .durability_warning
            .as_ref()
            .unwrap()
            .contains("parent directory sync failure")
    );
    assert_eq!(load_workspace_json(&path).unwrap(), after);
    assert_eq!(load_workspace_json(&receipt.backup).unwrap(), before);
}

#[test]
fn error_before_replacement_keeps_the_original_document() {
    let (directory, path, before, after) = fixture("settings-pre-rename-error");
    let result = save_json_with_writer(
        &path,
        &after,
        &before,
        false,
        &directory.0.join("backups"),
        |_, _, _| Err(io::Error::other("injected rename failure")),
    );
    assert!(result.err().unwrap().contains("injected rename failure"));
    assert_eq!(load_workspace_json(&path).unwrap(), before);
}

#[test]
fn unreadable_verification_never_blindly_restores_the_backup() {
    let (directory, path, before, after) = fixture("settings-missing-after-write");
    let result = save_json_with_writer(
        &path,
        &after,
        &before,
        false,
        &directory.0.join("backups"),
        |path, bytes, original| {
            storage::replace_file_if_unchanged(path, bytes, original)?;
            fs::remove_file(path)
        },
    );
    assert!(result.err().unwrap().contains("state is uncertain"));
    assert!(
        !path.exists(),
        "an external removal must not be silently undone"
    );
}

#[test]
fn another_writer_blocks_the_save_until_its_handle_is_closed() {
    let (directory, path, before, after) = fixture("settings-writer-lock");
    let canonical = fs::canonicalize(&path).unwrap();
    let lock = lock_settings(&canonical).unwrap();
    let backups = directory.0.join("backups");
    let error = save_json_checked_with_backup_root(&path, &after, &before, false, &backups)
        .err()
        .unwrap();
    assert!(error.contains("Another Sundial operation"));
    assert_eq!(load_workspace_json(&path).unwrap(), before);
    drop(lock);
    assert!(save_json_checked_with_backup_root(&path, &after, &before, false, &backups).is_ok());
}

#[test]
fn normalized_v8_baseline_and_unknown_fields_survive_a_checked_save() {
    let (directory, path, before, _) = fixture("normalized-settings-save");
    let mut expected = before;
    ensure_schema_v8_preferences(&mut expected);
    let mut after = expected.clone();
    after["test_value"] = json!("edited");
    save_json_checked_with_backup_root(
        &path,
        &after,
        &expected,
        true,
        &directory.0.join("backups"),
    )
    .unwrap();
    assert_eq!(load_workspace_json(&path).unwrap(), after);
}
