use super::*;

fn complete_generation(fixture: &Fixture) -> PathBuf {
    let validated = validate_request(&fixture.request()).unwrap();
    let backup = create_backup_directory(&fixture.backups).unwrap();
    let originals = backup_originals(&validated, &backup).unwrap();
    let prepared = prepare_temporary_files(&validated).unwrap();
    let mut record = build_install_transaction(&validated, &backup, &originals, &prepared).unwrap();
    cleanup_prepared_files(&prepared);
    record.state = InstallTransactionState::Committed;
    mark_backup_complete(&record).unwrap();
    backup
}

#[test]
fn retention_preserves_pending_and_legacy_backups_across_installations() {
    let a = Fixture::new();
    let (mut record, _) = a.write_synthetic_transaction(InstallTransactionState::Pending);
    let pending = create_backup_directory(&a.backups).unwrap();
    for artifact in &record.artifacts {
        fs::copy(
            record.backup_directory.join(&artifact.file_name),
            pending.join(&artifact.file_name),
        )
        .unwrap();
    }
    record.backup_directory = pending.clone();
    write_install_transaction(&a.target.join(INSTALL_TRANSACTION_FILE_NAME), &record).unwrap();
    let first = &record.artifacts[0].file_name;
    fs::write(a.target.join(first), &a.staged_bytes[first]).unwrap();
    let legacy = create_backup_directory(&a.backups).unwrap();
    fs::write(legacy.join("old-package.pkg"), b"unclassified backup").unwrap();

    let mut b = Fixture::new();
    b.backups = a.backups.clone();
    for _ in 0..4 {
        complete_generation(&b);
    }
    let report = prune_package_backups(&a.backups, 3).unwrap();
    assert_eq!(report.removed_directories.len(), 1);
    assert!(pending.exists());
    assert!(legacy.exists());
    assert!(a.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    assert!(matches!(
        recover_interrupted_install(&a.recovery_request()).unwrap(),
        RecoveryOutcome::Recovered { .. }
    ));
    assert_eq!(
        fs::read(a.target.join(first)).unwrap(),
        fs::read(pending.join(first)).unwrap()
    );
}

#[test]
fn retention_counts_completed_generations_per_installation() {
    let a = Fixture::new();
    let mut b = Fixture::new();
    b.backups = a.backups.clone();
    let a_backup = complete_generation(&a);
    for _ in 0..4 {
        complete_generation(&b);
    }
    let report = prune_package_backups(&a.backups, 3).unwrap();
    assert_eq!(report.removed_directories.len(), 1);
    assert_eq!(report.retained_directories.len(), 4);
    assert!(a_backup.exists());
}

#[test]
fn pending_or_malformed_completion_records_are_not_pruned() {
    let fixture = Fixture::new();
    let (record, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    assert!(mark_backup_complete(&record).is_err());
    let malformed = create_backup_directory(&fixture.backups).unwrap();
    fs::write(malformed.join("install-complete.json"), b"broken").unwrap();
    for _ in 0..2 {
        complete_generation(&fixture);
    }
    prune_package_backups(&fixture.backups, 1).unwrap();
    assert!(malformed.exists());
}

#[test]
fn installation_lock_covers_install_uninstall_and_recovery() {
    let fixture = installed_fixture();
    let plan = preview_uninstall(&fixture.target).unwrap();
    let lock = lock_installation(&fixture.target).unwrap();
    assert!(
        install_staged_packages(&fixture.request())
            .unwrap_err()
            .message
            .contains("transaction lock")
    );
    assert!(
        recover_interrupted_install(&fixture.recovery_request())
            .unwrap_err()
            .message
            .contains("transaction lock")
    );
    assert!(
        uninstall_custom_packages(&plan, &fixture.backups, game_stopped)
            .unwrap_err()
            .message
            .contains("transaction lock")
    );
    let other = Fixture::new();
    let _other_lock = lock_installation(&other.target).unwrap();
    drop(lock);
    let _reacquired = lock_installation(&fixture.target).unwrap();
}

#[test]
fn lock_probe_child() {
    let Some(target) = std::env::var_os("PARHELION_LOCK_PROBE_TARGET") else {
        return;
    };
    assert!(lock_installation(Path::new(&target)).is_err());
}

#[test]
fn installation_lock_excludes_a_second_process() {
    let fixture = Fixture::new();
    let _lock = lock_installation(&fixture.target).unwrap();
    let status = process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "install::tests::transaction_safety::lock_probe_child",
            "--nocapture",
        ])
        .env("PARHELION_LOCK_PROBE_TARGET", &fixture.target)
        .status()
        .unwrap();
    assert!(status.success());
}

fn replace_then_fail(source: &Path, target: &Path) -> Result<(), String> {
    sundial::package_authoring::replace_file_from_path_atomically(source, target)?;
    Err("Injected directory synchronization failure after replacement".to_owned())
}

#[test]
fn post_replacement_failure_restores_existing_and_removes_new_packages() {
    for existing in [false, true] {
        let fixture = if existing {
            installed_fixture()
        } else {
            Fixture::new()
        };
        let first = AUTHORED_PACKAGES[0];
        let original = package_bytes(
            first.package_id,
            first.patch_id,
            SUNDIAL_BUILD_SIGNATURE,
            b"previous generation, not the staged payload",
        );
        if existing {
            fs::write(fixture.target.join(first.file_name), &original).unwrap();
        }
        let before = discover_target_source_artifact_names(&fixture.target).unwrap();
        let error = install_with_replacer(
            &fixture.request(),
            None,
            DEFAULT_CACHE_INVALIDATION_OPS,
            replace_then_fail,
        )
        .unwrap_err();
        assert!(error.message.contains("after replacement"));
        assert!(error.rollback.as_ref().unwrap().succeeded(), "{error:?}");
        for (name, bytes) in &fixture.staged_bytes {
            if existing {
                let expected = if name == first.file_name {
                    &original
                } else {
                    bytes
                };
                assert_eq!(fs::read(fixture.target.join(name)).unwrap(), *expected);
            } else {
                assert!(!fixture.target.join(name).exists());
            }
        }
        assert_eq!(
            before,
            discover_target_source_artifact_names(&fixture.target).unwrap()
        );
        assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    }
}

fn replace_then_corrupt_backup(source: &Path, target: &Path) -> Result<(), String> {
    sundial::package_authoring::replace_file_from_path_atomically(source, target)?;
    let journal = target.parent().unwrap().join(INSTALL_TRANSACTION_FILE_NAME);
    let record = read_install_transaction(&journal).unwrap();
    fs::write(
        record.backup_directory.join(target.file_name().unwrap()),
        b"damaged backup",
    )
    .unwrap();
    Err("Injected ambiguous replacement plus backup damage".to_owned())
}

#[test]
fn unverifiable_rollback_keeps_pending_journal_and_protected_backup() {
    let fixture = installed_fixture();
    let error = install_with_replacer(
        &fixture.request(),
        None,
        DEFAULT_CACHE_INVALIDATION_OPS,
        replace_then_corrupt_backup,
    )
    .unwrap_err();
    assert!(!error.rollback.as_ref().unwrap().succeeded());
    let record =
        read_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME)).unwrap();
    assert_eq!(record.state, InstallTransactionState::Pending);
    assert!(
        !record
            .backup_directory
            .join("install-complete.json")
            .exists()
    );
}

fn omit_optional_overlays(fixture: &Fixture) {
    let artifacts = fixture
        .artifact_json()
        .into_iter()
        .filter(|artifact| {
            let name = artifact["file_name"].as_str().unwrap();
            authored_package_for_file_name(name)
                .unwrap()
                .required_output
        })
        .collect();
    for profile in AUTHORED_PACKAGES
        .iter()
        .filter(|profile| !profile.required_output)
    {
        fs::remove_file(fixture.staging.join(profile.file_name)).unwrap();
    }
    fixture.write_manifest(Some(artifacts));
}

#[test]
fn replacement_build_retires_only_obsolete_authored_overlays() {
    let fixture = installed_fixture();
    omit_optional_overlays(&fixture);
    let stock = fixture.source_artifact_json();
    let report = install_staged_packages(&fixture.request()).unwrap();
    assert_eq!(report.removed_obsolete_packages.len(), 3);
    assert!(
        report
            .removed_obsolete_packages
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == "w64_ui_037e_6.pkg"))
    );
    assert_eq!(report.artifacts.len(), AUTHORED_PACKAGES.len() - 3);
    for profile in AUTHORED_PACKAGES {
        assert_eq!(
            fixture.target.join(profile.file_name).exists(),
            profile.required_output
        );
        assert_eq!(
            fs::read(report.backup_directory.join(profile.file_name)).unwrap(),
            fixture.staged_bytes[profile.file_name]
        );
    }
    assert_eq!(stock, fixture.source_artifact_json());
    let again = install_staged_packages(&fixture.request()).unwrap();
    assert!(again.removed_obsolete_packages.is_empty());
}

#[test]
fn failure_after_retiring_an_overlay_restores_the_complete_previous_set() {
    let fixture = installed_fixture();
    omit_optional_overlays(&fixture);
    let required = AUTHORED_PACKAGES
        .iter()
        .filter(|profile| profile.required_output)
        .count();
    let error = install_staged_packages_inner(
        &fixture.request(),
        Some(required + 1),
        DEFAULT_CACHE_INVALIDATION_OPS,
    )
    .unwrap_err();
    assert!(error.rollback.as_ref().unwrap().succeeded(), "{error:?}");
    for (name, bytes) in &fixture.staged_bytes {
        assert_eq!(fs::read(fixture.target.join(name)).unwrap(), *bytes);
    }
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn interrupted_mixed_replacement_and_removal_recovers_after_restart() {
    let fixture = installed_fixture();
    omit_optional_overlays(&fixture);
    let validated = validate_request(&fixture.request()).unwrap();
    let backup = create_backup_directory(&fixture.backups).unwrap();
    let originals = backup_originals(&validated, &backup).unwrap();
    let prepared = prepare_temporary_files(&validated).unwrap();
    let record = build_install_transaction(&validated, &backup, &originals, &prepared).unwrap();
    validate_install_transaction(&record, &validated.target_packages_directory).unwrap();
    write_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME), &record)
        .unwrap();
    commit_prepared_files(
        &prepared,
        None,
        sundial::package_authoring::replace_file_from_path_atomically,
    )
    .unwrap();
    recover_interrupted_install(&fixture.recovery_request()).unwrap();
    for (name, bytes) in &fixture.staged_bytes {
        assert_eq!(fs::read(fixture.target.join(name)).unwrap(), *bytes);
    }
}

#[test]
fn mixed_transaction_cannot_remove_required_packages() {
    let fixture = Fixture::new();
    let (mut record, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let required = record
        .artifacts
        .iter_mut()
        .find(|artifact| {
            authored_package_for_file_name(&artifact.file_name)
                .unwrap()
                .required_output
        })
        .unwrap();
    required.remove_target = true;
    required.authored = required.original.clone().unwrap();
    assert!(validate_install_transaction(&record, &record.target_packages_directory).is_err());
}
