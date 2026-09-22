use super::*;

#[test]
fn runtime_support_lost_during_preparation_stops_before_any_replacement() {
    let fixture = Fixture::new();
    let marker = fixture.target.join("test-runtime-supported");
    fs::write(&marker, b"supported").unwrap();
    let settings = fixture.target.parent().unwrap().join("settings.json");
    fs::write(&settings, br#"{"version":6,"keep":"unchanged"}"#).unwrap();
    let settings_before = fs::read(&settings).unwrap();
    let originals: Vec<_> = fs::read_dir(&fixture.target)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pkg"))
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    let mut request = fixture.request();
    request.runtime_feature_check = |packages| {
        if packages.join("test-runtime-supported").is_file() {
            Ok(())
        } else {
            Err("Runtime no longer supports generated packages".into())
        }
    };
    let mut replaced = false;
    let error = install_staged_packages_with_progress(&request, |event| {
        if event.phase == InstallPhase::Rechecking && marker.exists() {
            fs::remove_file(&marker).unwrap();
        }
        replaced |= matches!(
            event.phase,
            InstallPhase::UpdatingAccount | InstallPhase::Installing
        );
    })
    .unwrap_err();
    assert!(
        error
            .message
            .contains("Runtime check failed before installation"),
        "{error}"
    );
    assert!(!replaced);
    assert_eq!(fs::read(settings).unwrap(), settings_before);
    for (path, bytes) in originals {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    assert!(!fs::read_dir(&fixture.target).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".parhelion-install-")
    }));
}

#[test]
fn runtime_identity_change_during_preparation_stops_before_any_replacement() {
    let fixture = Fixture::new();
    let module = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/steam_api64.dll");
    let originals = CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| {
            let path = fixture.target.join(name);
            (path.clone(), fs::read(path).ok())
        })
        .collect::<Vec<_>>();
    let mut changed = false;

    let error = install_staged_packages_with_progress(&fixture.request(), |event| {
        if !changed && event.phase == InstallPhase::Rechecking {
            fs::write(&module, b"switched runtime").unwrap();
            changed = true;
        }
    })
    .unwrap_err();

    assert!(
        error.message.contains("runtime changed after preflight"),
        "{error}"
    );
    for (path, original) in originals {
        assert_eq!(fs::read(path).ok(), original);
    }
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn linked_packages_cannot_redirect_install_recovery_or_uninstall_to_another_game() {
    let fixture = Fixture::new();
    let selected = tempfile::tempdir().unwrap();
    let packages = selected.path().join("packages");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(&fixture.target, &packages) {
        if error.raw_os_error() == Some(1314) {
            return;
        }
        panic!("Could not create test link: {error}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&fixture.target, &packages).unwrap();
    let alias = selected.path().join("game-alias");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(fixture.target.parent().unwrap(), &alias).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.target.parent().unwrap(), &alias).unwrap();
    assert_eq!(
        sundial::package_authoring::validate_shadowkeep_packages_directory(&alias.join("packages"))
            .unwrap(),
        fs::canonicalize(&fixture.target).unwrap()
    );
    let mut request = fixture.request();
    request.target_packages_directory = packages.clone();
    let error = install_staged_packages(&request).unwrap_err();
    assert!(error.message.contains("outside its game folder"), "{error}");
    let error = recover_interrupted_install(&RecoveryRequest {
        target_packages_directory: packages.clone(),
        backup_root: fixture.backups.clone(),
        game_running_check: game_stopped,
        runtime_snapshot_check: test_runtime_snapshot,
    })
    .unwrap_err();
    assert!(error.message.contains("outside its game folder"), "{error}");
    let error = preview_uninstall(&packages).unwrap_err();
    assert!(error.message.contains("outside its game folder"), "{error}");
    assert!(!fixture.target.join(".parhelion-transaction.lock").exists());
    assert!(!fixture.backups.exists());
}
