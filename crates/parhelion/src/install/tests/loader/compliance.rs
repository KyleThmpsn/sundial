//! Public filesystem workflows for the loader document's stock-directory boundary. An earlier
//! layout's packages are retired from stock `packages` with a verified backup, never adopted
//! into the mod folder, since their patches add entries the loader refuses.
use super::*;

pub(super) fn inventory(directory: &Path) -> BTreeMap<String, String> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            (
                entry.file_name().into_string().unwrap(),
                digest_file(&entry.path()).unwrap().sha256,
            )
        })
        .collect()
}

#[test]
fn loader_operations_share_the_external_lock_and_retire_the_legacy_lock() {
    let fixture = fixture();
    let stock = inventory(&fixture.target);
    legacy(&fixture);
    let legacy_path = fixture.target.join(".parhelion-transaction.lock");
    let old = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&legacy_path)
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&old).unwrap();
    assert!(
        install_staged_packages(&request(&fixture))
            .unwrap_err()
            .message
            .contains("transaction lock")
    );
    drop(old);
    let external = fixture
        .target
        .parent()
        .unwrap()
        .join(".parhelion-transaction.lock");
    // A finished operation removes its lock, so the other holder creates it as one would.
    let hold = || {
        let guard = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&external)
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&guard).unwrap();
        guard
    };
    assert!(!external.exists());
    let guard = hold();
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert!(recover_interrupted_install(&recovery(&fixture)).is_err());
    drop(guard);
    install_staged_packages(&request(&fixture)).unwrap();
    assert!(!external.exists());
    let removal = preview_uninstall(&fixture.target).unwrap();
    let guard = hold();
    assert!(uninstall_custom_packages(&removal, &fixture.backups, game_stopped).is_err());
    drop(guard);
    uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
    assert!(!external.exists());
    assert_eq!(inventory(&fixture.target), stock);
    crate::test_support::artifact(
        "sunrise-mod-lock-compliance.json",
        &json!({"stock": stock, "legacy_lock_removed": !legacy_path.exists(), "external_lock": external.file_name().unwrap().to_string_lossy(), "external_lock_removed": !external.exists(), "contending_operations": ["install", "recovery", "uninstall"]}),
    );
}

fn old_transaction(
    fixture: &Fixture,
    state: InstallTransactionState,
    removal: bool,
    had_original: bool,
) -> InstallTransactionRecord {
    let backup = fixture.backups.join("legacy-operation");
    fs::create_dir_all(&backup).unwrap();
    let artifacts = fixture
        .staged_bytes
        .iter()
        .enumerate()
        .map(|(index, (name, bytes))| {
            let incoming = if !removal && name == crate::package_profile::PARHELION_ASSET_FILE_NAME
            {
                build_test_package_with_physical_payload(
                    crate::package_profile::PARHELION_ASSET_PACKAGE_ID,
                    0,
                    b"interrupted assets",
                )
                .unwrap()
            } else {
                bytes.clone()
            };
            let digest = TransactionDigest {
                byte_length: incoming.len() as u64,
                sha256: hex::encode_upper(Sha256::digest(&incoming)),
            };
            let original =
                TransactionDigest::from(&digest_file(&fixture.staging.join(name)).unwrap());
            if had_original {
                fs::write(backup.join(name), bytes).unwrap();
            }
            let temporary_file_name = format!(".parhelion-install-old-{index}.tmp");
            if !removal {
                fs::write(fixture.target.join(&temporary_file_name), &incoming).unwrap();
            }
            if !removal || index % 2 == 0 {
                fs::write(fixture.target.join(name), &incoming).unwrap();
            }
            InstallTransactionArtifact {
                remove_target: removal,
                file_name: name.clone(),
                authored: digest,
                original: had_original.then_some(original),
                backup_file_name: had_original.then_some(name.clone()),
                temporary_file_name,
            }
        })
        .collect();
    let record = InstallTransactionRecord {
        schema: 1,
        state,
        target_packages_directory: fs::canonicalize(&fixture.target).unwrap(),
        backup_directory: fs::canonicalize(backup).unwrap(),
        artifacts,
        runtime: Some(
            sundial::package_authoring::installed_runtime(fixture.target.parent().unwrap())
                .unwrap(),
        ),
        account_cleanup: None,
        client_settings: None,
    };
    write_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME), &record)
        .unwrap();
    record
}

#[test]
fn old_install_and_removal_journals_retire_their_packages_on_recovery() {
    let cases = [(false, true), (true, true), (false, false)];
    let results = cases.into_iter().map(|(removal, had_original)| {
        let fixture = fixture();
        let stock = inventory(&fixture.target);
        let (account, original_account) = account(&fixture, 18);
        old_transaction(&fixture, InstallTransactionState::Pending, removal, had_original);
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
        assert_eq!(inventory(&fixture.target), stock);
        assert_eq!(fs::read(account).unwrap(), original_account);
        assert!(matches!(recover_interrupted_install(&recovery(&fixture)).unwrap(), RecoveryOutcome::NoTransaction));
        json!({"removal":removal, "had_original":had_original, "stock":stock, "account_sha256":hex::encode(Sha256::digest(original_account))})
    }).collect::<Vec<_>>();
    crate::test_support::artifact("sunrise-mod-legacy-recovery.json", &results);
}

#[test]
fn legacy_retirement_survives_another_interruption_and_preserves_conflicting_files() {
    let mut evidence = Vec::new();
    for stop in [
        "journaled",
        "legacy_file_removed",
        "legacy_removed",
        "committed",
    ] {
        let fixture = fixture();
        let stock = inventory(&fixture.target);
        old_transaction(&fixture, InstallTransactionState::Pending, true, true);
        let reached = std::cell::Cell::new(false);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backend::recover_legacy_with_observer(&recovery(&fixture), &mut |phase| {
                if phase == stop {
                    reached.set(true);
                    panic!("simulated legacy retirement stop");
                }
            })
            .unwrap();
        }));
        assert!(reached.get(), "legacy retirement did not reach {stop}");
        let journal = fixture.target.join(INSTALL_TRANSACTION_FILE_NAME);
        let expected = fs::read(&journal).unwrap();
        fs::write(&journal, b"concurrent journal").unwrap();
        assert!(recover_interrupted_install(&recovery(&fixture)).is_err());
        assert_eq!(fs::read(&journal).unwrap(), b"concurrent journal");
        fs::write(&journal, expected).unwrap();
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
        assert_eq!(inventory(&fixture.target), stock);
        evidence.push(json!({"interruption":stop,"stock":stock}));
    }
    crate::test_support::artifact("sunrise-mod-legacy-retirement-recovery.json", &evidence);
}

#[test]
fn committed_legacy_install_is_retired_to_its_backup_and_unknown_temporary_content_is_kept() {
    let mut fixture = fixture();
    let stock = inventory(&fixture.target);
    let record = old_transaction(&fixture, InstallTransactionState::Committed, false, true);
    let temporary = fixture
        .target
        .join(&record.artifacts[0].temporary_file_name);
    let bytes = fs::read(&temporary).unwrap();
    fs::write(&temporary, b"outside content").unwrap();
    assert!(recover_interrupted_install(&recovery(&fixture)).is_err());
    assert_eq!(fs::read(&temporary).unwrap(), b"outside content");
    fs::write(temporary, bytes).unwrap();
    recover_interrupted_install(&recovery(&fixture)).unwrap();
    fixture.staged_bytes.insert(
        crate::package_profile::PARHELION_ASSET_FILE_NAME.into(),
        build_test_package_with_physical_payload(
            crate::package_profile::PARHELION_ASSET_PACKAGE_ID,
            0,
            b"interrupted assets",
        )
        .unwrap(),
    );
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    assert_eq!(inventory(&fixture.target), stock);
    assert_retired_backup(&fixture, &fixture.staged_bytes);
}

#[test]
fn legacy_retirement_restores_the_recorded_json_and_sqlite_account_state() {
    let results = [8, 18]
        .into_iter()
        .map(verify_account_recovery)
        .collect::<Vec<_>>();
    crate::test_support::artifact("sunrise-mod-adopted-accounts.json", &results);
}

fn verify_account_recovery(schema: u64) -> serde_json::Value {
    let fixture = fixture();
    let outside = super::super::runtime::links::external(&fixture, false);
    let external_dll = outside.path().join("steam_api64.dll");
    let runtime_before = fs::read(&external_dll).unwrap();
    let (path, _) = account(&fixture, schema);
    let read = sundial::package_authoring::account::read_authored_account_source;
    let original = read(&path).unwrap();
    let mut record = old_transaction(&fixture, InstallTransactionState::Pending, true, true);
    fs::write(
        record.backup_directory.join("account-settings.json"),
        &original,
    )
    .unwrap();
    if schema == 18 {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("UPDATE items SET quantity = quantity + 1")
            .unwrap();
    } else {
        let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        changed["unknown"]["keep"] = json!(false);
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    }
    let cleaned = read(&path).unwrap();
    assert_ne!(original, cleaned);
    let digest = |bytes: &[u8]| json!({"byte_length":bytes.len(),"sha256":hex::encode_upper(Sha256::digest(bytes))});
    record.account_cleanup = Some(serde_json::from_value(json!({"source":"account","relative_path":path.strip_prefix(fixture.target.parent().unwrap()).unwrap(),"original":digest(&original),"cleaned":digest(&cleaned)})).unwrap());
    write_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME), &record)
        .unwrap();
    recover_interrupted_install(&recovery(&fixture)).unwrap();
    assert_eq!(read(&path).unwrap(), original);
    assert_eq!(fs::read(external_dll).unwrap(), runtime_before);
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    if schema == 18 {
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT value FROM migration_extension", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "retain"
        );
    }
    json!({"schema":schema,"original_sha256":hex::encode(Sha256::digest(&original)),"interrupted_sha256":hex::encode(Sha256::digest(&cleaned)),"recovered_sha256":hex::encode(Sha256::digest(read(&path).unwrap())),"runtime_sha256":hex::encode(Sha256::digest(runtime_before)),"runtime_link":"external and preserved"})
}

#[test]
fn nonempty_legacy_lock_is_preserved_instead_of_claimed_as_editor_metadata() {
    let fixture = fixture();
    legacy(&fixture);
    let path = fixture.target.join(".parhelion-transaction.lock");
    fs::write(&path, b"foreign metadata").unwrap();
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"foreign metadata");
    assert!(!fixture.backups.exists());
}

#[test]
fn legacy_uninstall_on_loader_runtime_leaves_only_the_original_stock_files() {
    let fixture = fixture();
    let stock = inventory(&fixture.target);
    let retired = legacy(&fixture);
    fs::write(fixture.target.join(".parhelion-transaction.lock"), []).unwrap();
    let plan = preview_uninstall(&fixture.target).unwrap();
    let report = uninstall_custom_packages(&plan, &fixture.backups, game_stopped).unwrap();
    assert_eq!(inventory(&fixture.target), stock);
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    assert_retired_backup(&fixture, &retired);
    // Reports name the canonical packages folder.
    let target = fs::canonicalize(&fixture.target).unwrap();
    for name in retired.keys() {
        assert!(report.removed_files.contains(&target.join(name)));
    }
    crate::test_support::artifact(
        "sunrise-mod-legacy-removal.json",
        &json!({"stock":stock,"mod_removed":true}),
    );
}
