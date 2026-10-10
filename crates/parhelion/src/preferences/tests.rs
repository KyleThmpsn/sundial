use super::*;
use crate::test_support::artifact;

#[test]
fn preference_edits_preserve_unknown_fields_across_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    assert_eq!(
        ParhelionPreferences::load_from(&path).unwrap(),
        ParhelionPreferences::default()
    );
    let unknown = serde_json::json!({"nested":[null,{"enabled":true}],"version":8});
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({"schema":1,"future_option":unknown})).unwrap(),
    )
    .unwrap();
    let mut preferences = ParhelionPreferences::load_from(&path).unwrap();
    assert!(!preferences.show_preview_fps);
    assert!(preferences.play_preview_animations);
    preferences.package_backup_retention = 7;
    preferences.backup_recipe_snapshots = false;
    preferences.show_preview_fps = true;
    preferences.play_preview_animations = false;
    preferences.save_to(&path).unwrap();
    assert_eq!(ParhelionPreferences::load_from(&path).unwrap(), preferences);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future_option"], unknown);
    assert_eq!(saved["show_preview_fps"], true);
    assert_eq!(saved["play_preview_animations"], false);
    artifact("preferences-preservation.json", &saved);
}

#[test]
fn preference_saves_refuse_unreadable_invalid_or_newer_documents() {
    let directory = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("malformed", "{broken"),
        ("future", "{\"schema\":2,\"future\":true}"),
        ("invalid", "{\"schema\":1,\"package_backup_retention\":0}"),
        ("duplicate", "{\"schema\":1,\"schema\":2}"),
        ("array", "[]"),
    ] {
        let path = directory.path().join(format!("{name}.json"));
        fs::write(&path, bytes).unwrap();
        assert!(ParhelionPreferences::load_from(&path).is_err());
        let error = ParhelionPreferences::default().save_to(&path).unwrap_err();
        assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        artifact(
            &format!("preferences-refused-{name}.json"),
            &serde_json::json!({"error":error,"preserved":bytes}),
        );
    }
    let path = directory.path().join("unreadable.json");
    fs::create_dir(&path).unwrap();
    assert!(ParhelionPreferences::default().save_to(&path).is_err());
    assert!(path.is_dir());
}

#[test]
fn preference_write_contention_preserves_the_last_saved_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let mut preferences = ParhelionPreferences::default();
    preferences.save_to(&path).unwrap();
    let before = fs::read(&path).unwrap();
    let lock = sundial::storage::try_lock_file(&path.with_extension("lock")).unwrap();
    preferences.backup_recipe_snapshots = false;
    assert!(preferences.save_to(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(lock);
    preferences.save_to(&path).unwrap();
    assert!(
        !ParhelionPreferences::load_from(&path)
            .unwrap()
            .backup_recipe_snapshots
    );
}
