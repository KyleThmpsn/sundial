use super::*;
use crate::game_settings::dawn::settings_issues as dawn_settings_issues;

pub(super) fn fixture(root: &Path) {
    for location in RuntimeLocation::ALL {
        let directory = location.directory(root);
        fs::create_dir_all(directory.join("Sunrise/scripts")).unwrap();
        fs::write(directory.join("steam_api64.dll"), b"runtime fixture").unwrap();
        fs::write(directory.join("Sunrise/settings.json"), br#"{"version":8}"#).unwrap();
        fs::write(
            directory.join("Sunrise/scripts/mission.lua"),
            b"return 'preserve me'",
        )
        .unwrap();
        fs::write(
            directory.join("Sunrise/investment.sqlite3"),
            b"saved account",
        )
        .unwrap();
    }
}

#[test]
fn archive_preserves_both_copies_in_either_direction() {
    for keep in RuntimeLocation::ALL {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let inspection = RuntimeInspection::inspect(dir.path());
        assert!(inspection.duplicates());
        assert!(inspection.copies.iter().all(|copy| !copy.dawn));
        let backup = archive_other_runtime(dir.path(), &inspection, keep, || Ok(())).unwrap();
        let other = RuntimeLocation::ALL
            .into_iter()
            .find(|l| *l != keep)
            .unwrap();
        assert!(keep.directory(dir.path()).join("steam_api64.dll").is_file());
        assert!(!other.directory(dir.path()).join("steam_api64.dll").exists());
        assert!(!other.directory(dir.path()).join("Sunrise").exists());
        let archived = other.directory(&backup);
        assert_eq!(
            fs::read(archived.join("Sunrise/scripts/mission.lua")).unwrap(),
            b"return 'preserve me'"
        );
        assert_eq!(
            fs::read(archived.join("Sunrise/investment.sqlite3")).unwrap(),
            b"saved account"
        );
        assert!(!backup.join("Restore-Runtime.ps1").exists());
        let receipt: Value =
            serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(receipt["state"], "complete");
        assert_eq!(RuntimeInspection::inspect(dir.path()).copies.len(), 1);
        let plan = preview_runtime_restore(dir.path(), &backup).unwrap();
        restore_runtime(&plan, || Ok(())).unwrap();
        assert_eq!(RuntimeInspection::inspect(dir.path()), inspection);
        assert_eq!(
            fs::read(
                other
                    .directory(dir.path())
                    .join("Sunrise/investment.sqlite3")
            )
            .unwrap(),
            b"saved account"
        );
    }
}

#[test]
fn restore_never_overwrites_an_existing_runtime() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let inspection = RuntimeInspection::inspect(dir.path());
    let backup =
        archive_other_runtime(dir.path(), &inspection, RuntimeLocation::BinX64, || Ok(())).unwrap();
    let plan = preview_runtime_restore(dir.path(), &backup).unwrap();
    fs::write(dir.path().join("steam_api64.dll"), b"newer runtime").unwrap();
    assert!(
        restore_runtime(&plan, || Ok(()))
            .unwrap_err()
            .contains("already exists")
    );
    assert_eq!(
        fs::read(dir.path().join("steam_api64.dll")).unwrap(),
        b"newer runtime"
    );
    assert!(backup.join("Sunrise/settings.json").exists());
}

#[test]
fn restore_rolls_back_on_a_process_check_failure() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let inspection = RuntimeInspection::inspect(dir.path());
    let backup =
        archive_other_runtime(dir.path(), &inspection, RuntimeLocation::BinX64, || Ok(())).unwrap();
    let plan = preview_runtime_restore(dir.path(), &backup).unwrap();
    let mut checks = 0;
    let error = restore_runtime(&plan, || {
        checks += 1;
        if checks == 3 {
            Err("Process scan failed".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(error.contains("Files remain in the backup"));
    assert!(!dir.path().join("steam_api64.dll").exists());
    assert!(backup.join("steam_api64.dll").exists());
    assert!(backup.join("Sunrise/settings.json").exists());
}

#[test]
fn restore_rejects_a_manifest_path_outside_its_runtime() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let inspection = RuntimeInspection::inspect(dir.path());
    let backup =
        archive_other_runtime(dir.path(), &inspection, RuntimeLocation::BinX64, || Ok(())).unwrap();
    let manifest_path = backup.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["paths"] = serde_json::json!(["../outside"]);
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        preview_runtime_restore(dir.path(), &backup)
            .err()
            .unwrap()
            .contains("unsupported")
    );
}

#[test]
fn archive_rejects_changed_settings_or_dll() {
    for name in ["Sunrise/settings.json", "steam_api64.dll"] {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let inspection = RuntimeInspection::inspect(dir.path());
        fs::write(dir.path().join(name), b"changed after review").unwrap();
        assert!(
            archive_other_runtime(dir.path(), &inspection, RuntimeLocation::BinX64, || Ok(()))
                .unwrap_err()
                .contains("changed")
        );
        assert!(dir.path().join("steam_api64.dll").exists());
        assert!(!dir.path().join(".sunrise").exists());
    }
}

#[test]
fn archive_rolls_back_when_game_starts_between_moves() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let inspection = RuntimeInspection::inspect(dir.path());
    let mut checks = 0;
    let error = archive_other_runtime(dir.path(), &inspection, RuntimeLocation::BinX64, || {
        checks += 1;
        if checks == 3 {
            Err("Game started".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(error.contains("Moved files were restored"), "{error}");
    assert_eq!(RuntimeInspection::inspect(dir.path()), inspection);
    assert!(dir.path().join("Sunrise/scripts/mission.lua").exists());
}

#[test]
fn archive_requires_closed_game_before_preparing_backup() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let inspection = RuntimeInspection::inspect(dir.path());
    assert!(
        archive_other_runtime(dir.path(), &inspection, RuntimeLocation::Root, || Err(
            "Game is running".into()
        ))
        .is_err()
    );
    assert!(!dir.path().join(".sunrise").exists());
}

#[test]
fn inspection_accepts_bom_settings_and_rejects_duplicate_keys() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    fs::write(
        dir.path().join("Sunrise/settings.json"),
        b"\xef\xbb\xbf{\"version\":8}",
    )
    .unwrap();
    fs::write(
        dir.path().join("bin/x64/Sunrise/settings.json"),
        br#"{"version":8,"version":18}"#,
    )
    .unwrap();
    let inspection = RuntimeInspection::inspect(dir.path());
    assert_eq!(inspection.copies[0].schema, Some(8));
    assert!(inspection.copies[0].selection_problem.is_none());
    assert!(inspection.copies[1].selection_problem.is_some());
}

#[test]
fn missing_or_invalid_settings_cannot_be_selected() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    fs::remove_file(dir.path().join("bin/x64/Sunrise/settings.json")).unwrap();
    fs::write(dir.path().join("Sunrise/settings.json"), b"invalid").unwrap();
    let inspection = RuntimeInspection::inspect(dir.path());
    for keep in RuntimeLocation::ALL {
        assert!(archive_other_runtime(dir.path(), &inspection, keep, || Ok(())).is_err());
    }
    assert!(!dir.path().join(".sunrise").exists());
}

#[test]
fn generic_settings_and_build_paths_do_not_identify_dawn() {
    let defaults = serde_json::json!({"version":6,"experiments":{"omega":{"coo_executor":false}}});
    assert!(!dawn_signature(
        b"C:/dev/dawn/steam_api64.pdb coo_executor",
        Some(&defaults)
    ));
    let markers = b"ev=coo_script mission=omega result=loaded format=lua\0ev=coo_executor mission=omega mode=composition\0Sunrise/scripts/omega.lua\0coo_executor";
    assert!(dawn_signature(markers, Some(&defaults)));
    assert!(!dawn_signature(markers, None));
    let current = serde_json::json!({"version":18,"experiments":{"omega":{"coo_executor":false}}});
    assert!(!dawn_signature(markers, Some(&current)));
}

#[test]
fn dawn_compatibility_accepts_negative_previous_activity_index() {
    let settings = serde_json::json!({"version":6,"state":{"activity":{"default_destination":{"previous_activity_index":-1}},"account":{},"characters":[{}]}});
    assert!(dawn_settings_issues(&settings).is_empty());
    assert!(!dawn_settings_issues(&serde_json::json!({"version":18})).is_empty());
}

#[test]
fn runtime_discovery_supports_root_and_bin_locations() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    assert_eq!(
        super::super::sunrise_module_path(dir.path()),
        dir.path().join("steam_api64.dll")
    );
    fs::remove_file(dir.path().join("steam_api64.dll")).unwrap();
    assert_eq!(
        super::super::sunrise_module_path(dir.path()),
        dir.path().join("bin/x64/steam_api64.dll")
    );
}

#[test]
fn backup_rejects_a_link_outside_the_installation() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(dir.path());
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(outside.path(), dir.path().join(".sunrise"));
    #[cfg(not(windows))]
    let linked = std::os::unix::fs::symlink(outside.path(), dir.path().join(".sunrise"));
    if let Err(error) = linked {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("{error}");
    }
    let inspection = RuntimeInspection::inspect(dir.path());
    assert!(
        archive_other_runtime(dir.path(), &inspection, RuntimeLocation::Root, || Ok(()))
            .unwrap_err()
            .contains("leaves the installation")
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}
