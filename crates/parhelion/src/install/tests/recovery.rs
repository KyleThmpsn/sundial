use super::*;

#[test]
fn rollback_refuses_to_overwrite_a_concurrently_changed_target() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.backups).unwrap();
    let file_name = CANONICAL_ARTIFACT_FILE_NAMES[0];
    let target_path = fixture.target.join(file_name);
    let backup_path = fixture.backups.join(file_name);
    let original_bytes = package_bytes(
        CANONICAL_PACKAGE_IDS[0],
        AUTHORED_PATCH_ID,
        SUNDIAL_BUILD_SIGNATURE,
        b"original",
    );
    fs::write(&backup_path, &original_bytes).unwrap();
    let original = OriginalArtifact {
        file_name: file_name.to_owned(),
        target_path: target_path.clone(),
        backup_path: Some(backup_path),
        digest: Some(digest_file(&fixture.backups.join(file_name)).unwrap()),
    };
    let staged_digest = digest_file(&fixture.staging.join(file_name)).unwrap();
    let prepared = PreparedArtifact {
        remove_target: false,
        manifest: ArtifactMetadata {
            file_name: file_name.to_owned(),
            byte_length: staged_digest.byte_length,
            sha256: staged_digest.sha256,
        },
        target_path: target_path.clone(),
        temporary_path: fixture.staging.join("unused.tmp"),
    };
    fs::write(&target_path, b"concurrent external change").unwrap();

    // Exercise the shared digest reconciliation with a single, already-prepared target.
    let transaction = InstallTransactionRecord {
        schema: INSTALL_TRANSACTION_SCHEMA,
        state: InstallTransactionState::Pending,
        target_packages_directory: fixture.target.clone(),
        backup_directory: fixture.backups.clone(),
        account_cleanup: None,
        client_settings: None,
        artifacts: vec![InstallTransactionArtifact {
            remove_target: false,
            file_name: original.file_name.clone(),
            authored: TransactionDigest::from(&prepared.manifest),
            original: original.digest.as_ref().map(TransactionDigest::from),
            backup_file_name: Some(original.file_name),
            temporary_file_name: ".parhelion-install-unused.tmp".to_owned(),
        }],
    };
    let rollback = rollback_transaction(&transaction, game_stopped);

    assert!(!rollback.succeeded());
    assert!(rollback.errors[0].contains("matches neither"));
    assert_eq!(
        fs::read(target_path).unwrap(),
        b"concurrent external change"
    );
}

#[test]
fn recovery_before_first_replace_marks_the_transaction_recovered() {
    let fixture = Fixture::new();
    let (transaction, originals) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    assert_eq!(
        outcome,
        RecoveryOutcome::Recovered {
            restored_files: Vec::new(),
            removed_new_files: Vec::new(),
        }
    );
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            originals[&artifact.file_name]
        );
        assert!(!fixture.target.join(&artifact.temporary_file_name).exists());
    }
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn recovery_mid_set_restores_every_authored_target_from_verified_backups() {
    let fixture = Fixture::new();
    let (transaction, originals) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    for artifact in transaction.artifacts.iter().take(3) {
        fs::write(
            fixture.target.join(&artifact.file_name),
            &fixture.staged_bytes[&artifact.file_name],
        )
        .unwrap();
    }

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    let RecoveryOutcome::Recovered { restored_files, .. } = outcome else {
        panic!("pending transaction should be recovered");
    };
    assert_eq!(restored_files.len(), 3);
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            originals[&artifact.file_name]
        );
    }
}

#[test]
fn recovery_removes_a_new_package_that_had_no_original() {
    let fixture = Fixture::new();
    let (mut transaction, _) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let artifact = transaction.artifacts.last_mut().unwrap();
    fs::remove_file(
        transaction
            .backup_directory
            .join(artifact.backup_file_name.as_ref().unwrap()),
    )
    .unwrap();
    artifact.original = None;
    artifact.backup_file_name = None;
    let new_file_name = artifact.file_name.clone();
    fs::write(
        fixture.target.join(&new_file_name),
        &fixture.staged_bytes[&new_file_name],
    )
    .unwrap();
    write_install_transaction(
        &fixture.target.join(INSTALL_TRANSACTION_FILE_NAME),
        &transaction,
    )
    .unwrap();

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    let RecoveryOutcome::Recovered {
        removed_new_files, ..
    } = outcome
    else {
        panic!("pending transaction should be recovered");
    };
    assert_eq!(removed_new_files.len(), 1);
    assert!(!fixture.target.join(new_file_name).exists());
}

#[test]
fn recovery_after_all_replaces_but_before_commit_marker_restores_the_old_set() {
    let fixture = Fixture::new();
    let (transaction, originals) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    for artifact in &transaction.artifacts {
        fs::write(
            fixture.target.join(&artifact.file_name),
            &fixture.staged_bytes[&artifact.file_name],
        )
        .unwrap();
    }

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    let RecoveryOutcome::Recovered { restored_files, .. } = outcome else {
        panic!("pending transaction should be recovered");
    };
    assert_eq!(restored_files.len(), transaction.artifacts.len());
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            originals[&artifact.file_name]
        );
    }
}

#[test]
fn committed_transaction_is_never_rolled_back_and_only_its_temps_are_cleaned() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Committed);
    let unrelated = fixture.target.join(".parhelion-install-unrelated.tmp");
    fs::write(&unrelated, b"not owned by this transaction").unwrap();
    for artifact in &transaction.artifacts {
        fs::write(
            fixture.target.join(&artifact.file_name),
            &fixture.staged_bytes[&artifact.file_name],
        )
        .unwrap();
    }
    fs::remove_dir_all(&fixture.backups).unwrap();

    let mut recovery_request = fixture.recovery_request();
    recovery_request.game_running_check = game_running;
    let outcome = recover_interrupted_install(&recovery_request).unwrap();

    assert_eq!(outcome, RecoveryOutcome::CommittedTransaction);
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
        assert!(!fixture.target.join(&artifact.temporary_file_name).exists());
    }
    assert!(unrelated.exists());
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn recovered_transaction_is_cleaned_without_querying_the_game() {
    let fixture = Fixture::new();
    fixture.write_synthetic_transaction(InstallTransactionState::Recovered);
    let mut request = fixture.recovery_request();
    request.game_running_check = game_check_failed;

    let outcome = recover_interrupted_install(&request).unwrap();

    assert_eq!(
        outcome,
        RecoveryOutcome::Recovered {
            restored_files: Vec::new(),
            removed_new_files: Vec::new(),
        }
    );
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn external_target_change_refuses_recovery_before_any_file_is_restored() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let authored = &transaction.artifacts[0];
    fs::write(
        fixture.target.join(&authored.file_name),
        &fixture.staged_bytes[&authored.file_name],
    )
    .unwrap();
    let externally_changed = &transaction.artifacts[1];
    fs::write(
        fixture.target.join(&externally_changed.file_name),
        b"external package replacement",
    )
    .unwrap();

    let error = recover_interrupted_install(&fixture.recovery_request()).unwrap_err();

    assert!(
        error
            .message
            .contains("neither the recorded original nor authored")
    );
    assert_eq!(
        fs::read(fixture.target.join(&authored.file_name)).unwrap(),
        fixture.staged_bytes[&authored.file_name]
    );
    assert_eq!(
        fs::read(fixture.target.join(&externally_changed.file_name)).unwrap(),
        b"external package replacement"
    );
    assert_eq!(
        read_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME))
            .unwrap()
            .state,
        InstallTransactionState::Pending
    );
}

#[test]
fn pending_recovery_is_blocked_while_the_game_is_running() {
    let fixture = Fixture::new();
    fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let mut request = fixture.recovery_request();
    request.game_running_check = game_running;

    let error = recover_interrupted_install(&request).unwrap_err();

    assert!(error.message.contains("game is running"));
}

#[test]
fn recovery_rechecks_the_game_after_reconciliation_before_restoring() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let authored = &transaction.artifacts[0];
    fs::write(
        fixture.target.join(&authored.file_name),
        &fixture.staged_bytes[&authored.file_name],
    )
    .unwrap();
    GAME_STARTS_DURING_RECOVERY_CALLS.store(0, Ordering::SeqCst);
    let mut request = fixture.recovery_request();
    request.game_running_check = game_starts_during_recovery;

    let error = recover_interrupted_install(&request).unwrap_err();

    assert!(error.message.contains("game is running"));
    assert_eq!(
        fs::read(fixture.target.join(&authored.file_name)).unwrap(),
        fixture.staged_bytes[&authored.file_name]
    );
    assert_eq!(
        read_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME))
            .unwrap()
            .state,
        InstallTransactionState::Pending
    );
}
