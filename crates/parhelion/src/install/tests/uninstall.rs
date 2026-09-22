use super::*;

#[test]
#[ignore = "read-only; requires PARHELION_UNINSTALL_REVIEW_PACKAGES with installed custom packages"]
fn native_uninstall_review_identifies_the_complete_installed_set_without_mutation() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_UNINSTALL_REVIEW_PACKAGES").unwrap());
    let first = preview_uninstall(&packages).unwrap();
    assert!(!first.artifacts().is_empty());
    assert_eq!(preview_uninstall(&packages).unwrap(), first);
    eprintln!(
        "Reviewed {} recognized custom packages without changing any files",
        first.artifacts().len()
    );
}

#[test]
fn uninstall_fails_closed_when_runtime_identity_is_unrecognized() {
    let fixture = installed_fixture();
    let plan = preview_uninstall(&fixture.target).unwrap();

    let error = uninstall_custom_packages(&plan, &fixture.backups, game_stopped).unwrap_err();

    assert!(
        error.message.contains("no recognized Sunrise or Dawn"),
        "{error}"
    );
    for artifact in plan.artifacts() {
        assert!(fixture.target.join(&artifact.file_name).is_file());
    }
}

#[test]
fn uninstall_backs_up_complete_set_preserves_stock_accounts_and_invalidates_caches() {
    let fixture = installed_fixture();
    let cache = fixture.write_sunrise_cache(b"stale build cache");
    let header = fixture.write_package_header_cache("0000e2e9", b"stale header");
    let unrelated_cache =
        fixture.write_package_header_cache("uninstall-test", b"not a native cache name");
    let settings = fixture._temporary.path().join("settings.json");
    fs::write(&settings, b"keep saved account data").unwrap();
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert_eq!(plan.artifacts().len(), AUTHORED_PACKAGES.len());
    let report = uninstall_fixture(&plan, &fixture.backups, game_stopped).unwrap();
    assert_eq!(report.removed_files.len(), AUTHORED_PACKAGES.len());
    assert!(!cache.exists());
    assert!(!header.exists());
    assert!(unrelated_cache.exists());
    assert_eq!(fs::read(&settings).unwrap(), b"keep saved account data");
    for (name, bytes) in &fixture.staged_bytes {
        assert!(!fixture.target.join(name).exists());
        assert_eq!(
            fs::read(report.backup_directory.join(name)).unwrap(),
            *bytes
        );
    }
    assert!(
        preview_uninstall(&fixture.target)
            .unwrap()
            .artifacts()
            .is_empty()
    );
    prune_package_backups(&fixture.backups, 1).unwrap();
    assert!(report.backup_directory.exists());
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn uninstall_rejects_running_game_stale_reviews_unsigned_targets_and_nested_backups() {
    let fixture = installed_fixture();
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert!(uninstall_fixture(&plan, &fixture.backups, game_running).is_err());
    assert!(uninstall_fixture(&plan, &fixture.target.join("backups"), game_stopped).is_err());
    let name = AUTHORED_PACKAGES[0].file_name;
    let mut changed = fixture.staged_bytes[name].clone();
    changed.push(123);
    fs::write(fixture.target.join(name), changed).unwrap();
    assert!(uninstall_fixture(&plan, &fixture.backups, game_stopped).is_err());
    for profile in AUTHORED_PACKAGES {
        assert!(fixture.target.join(profile.file_name).exists());
    }
    fs::write(fixture.target.join(name), b"not a recognized package").unwrap();
    assert!(preview_uninstall(&fixture.target).is_err());
}

#[test]
#[ignore = "copies real packages into a disposable directory; requires PARHELION_UNINSTALL_TEST_PACKAGES and PARHELION_UNINSTALL_TEST_ITEM_HASH"]
fn account_cleanup_and_package_removal_commit_or_rollback_together() {
    let source = PathBuf::from(std::env::var_os("PARHELION_UNINSTALL_TEST_PACKAGES").unwrap());
    let hash = u32::from_str_radix(
        &std::env::var("PARHELION_UNINSTALL_TEST_ITEM_HASH").unwrap(),
        16,
    )
    .unwrap();
    let temporary = TempDir::new().unwrap();
    let target = temporary.path().join("packages");
    let backups = temporary.path().join("backups");
    fs::create_dir(&target).unwrap();
    #[cfg(windows)]
    {
        let runtime_directory = temporary.path().join("bin/x64");
        fs::create_dir_all(&runtime_directory).unwrap();
        fs::copy(
            source.parent().unwrap().join("bin/x64/oo2core_3_win64.dll"),
            runtime_directory.join("oo2core_3_win64.dll"),
        )
        .unwrap();
    }
    // Copy, never link or mutate the live installation. Native ownership checks stay enabled.
    let names = discover_target_source_artifact_names(&source).unwrap();
    for name in &names {
        fs::copy(source.join(name), target.join(name)).unwrap();
    }
    let authored = preview_uninstall(&source).unwrap();
    assert!(!authored.artifacts().is_empty());
    let settings = temporary.path().join("settings.json");
    for version in [6, 8, 16] {
        for fail_after in [Some(1), None] {
            for artifact in authored.artifacts() {
                fs::copy(
                    source.join(&artifact.file_name),
                    target.join(&artifact.file_name),
                )
                .unwrap();
            }
            let original = serde_json::to_vec_pretty(&json!({
                "version": version, "unknown_setting": {"keep": true},
                "state": {"account": {"primary_soid": "0x0000000000000001"},
                    "characters": [{"soid": "0x0000000000000002", "class": 0,
                    "equipment": {}, "inventory": [{"instance_soid": "0x0000000000000101",
                        "definition_hash": hash, "level": 106, "quantity": 1, "plugs": null}]}]}
            }))
            .unwrap();
            fs::write(&settings, &original).unwrap();
            let plan = preview_uninstall_with_account_cleanup(&target).unwrap();
            let cleanup = plan
                .account_cleanup()
                .unwrap_or_else(|| panic!("{:?}", plan.account_cleanup_error()));
            assert_eq!(cleanup.removed_items.get(&hash), Some(&1));
            // A concurrent account edit must be rejected before any package removal.
            let external = [original.as_slice(), b" "].concat();
            fs::write(&settings, &external).unwrap();
            assert!(uninstall_fixture(&plan, &backups, game_stopped).is_err());
            assert_eq!(fs::read(&settings).unwrap(), external);
            assert_eq!(
                preview_uninstall(&target).unwrap(),
                plan.clone().without_account_cleanup()
            );
            fs::write(&settings, &original).unwrap();
            assert_account_uninstall_outcome(&plan, &backups, &settings, &original, fail_after);
            eprintln!(
                "v{version}: account/package {}",
                if fail_after.is_some() {
                    "rollback"
                } else {
                    "commit"
                }
            );
        }
    }
}

#[test]
fn uninstall_failures_restore_every_removed_package() {
    for count in [1, AUTHORED_PACKAGES.len()] {
        let fixture = installed_fixture();
        let plan = preview_uninstall(&fixture.target).unwrap();
        let error = crate::install::uninstall::uninstall_inner(
            &plan,
            &fixture.backups,
            game_stopped,
            Some(count),
            DEFAULT_CACHE_INVALIDATION_OPS,
            test_runtime_snapshot,
        )
        .unwrap_err();
        assert!(error.to_string().contains("restored"), "{error}");
        assert_eq!(preview_uninstall(&fixture.target).unwrap(), plan);
    }
}

#[test]
fn uninstall_cache_failure_restores_packages_and_keeps_a_recovery_backup() {
    let fixture = installed_fixture();
    fixture.write_sunrise_cache(b"cache");
    let plan = preview_uninstall(&fixture.target).unwrap();
    let ops = CacheInvalidationOps {
        rename: fail_cache_quarantine_rename,
        cleanup: cleanup_cache_quarantine,
    };
    let error = crate::install::uninstall::uninstall_inner(
        &plan,
        &fixture.backups,
        game_stopped,
        None,
        ops,
        test_runtime_snapshot,
    )
    .unwrap_err();
    assert!(error.backup_directory.unwrap().exists());
    assert_eq!(preview_uninstall(&fixture.target).unwrap(), plan);
}

#[test]
fn interrupted_uninstall_is_recoverable_and_refuses_changed_survivors() {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    fn starts_during_rollback() -> Result<bool, String> {
        Ok(CALLS.fetch_add(1, Ordering::SeqCst) >= 2)
    }
    let fixture = installed_fixture();
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert!(
        crate::install::uninstall::uninstall_inner(
            &plan,
            &fixture.backups,
            starts_during_rollback,
            Some(1),
            DEFAULT_CACHE_INVALIDATION_OPS,
            test_runtime_snapshot,
        )
        .is_err()
    );
    assert!(fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    let survivor = AUTHORED_PACKAGES[0].file_name;
    fs::write(fixture.target.join(survivor), b"external change").unwrap();
    let request = RecoveryRequest {
        target_packages_directory: fixture.target.clone(),
        backup_root: fixture.backups.clone(),
        game_running_check: game_stopped,
        runtime_snapshot_check: test_runtime_snapshot,
    };
    assert!(recover_interrupted_install(&request).is_err());
    assert_eq!(
        fs::read(fixture.target.join(survivor)).unwrap(),
        b"external change"
    );
    fs::write(
        fixture.target.join(survivor),
        &fixture.staged_bytes[survivor],
    )
    .unwrap();
    recover_interrupted_install(&request).unwrap();
    assert_eq!(preview_uninstall(&fixture.target).unwrap(), plan);
}

#[test]
fn interrupted_uninstall_restores_account_and_packages_without_overwriting_newer_account_edits() {
    let fixture = installed_fixture();
    let (mut record, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    for artifact in &mut record.artifacts {
        artifact.remove_target = true;
        let original = &fixture.staged_bytes[&artifact.file_name];
        fs::write(record.backup_directory.join(&artifact.file_name), original).unwrap();
        fs::write(fixture.target.join(&artifact.file_name), original).unwrap();
        artifact.original = Some(artifact.authored.clone());
    }
    let settings = fixture._temporary.path().join("settings.json");
    let backup = record.backup_directory.join("account-settings.json");
    fs::write(&backup, br#"{"version":8,"value":"original account"}"#).unwrap();
    fs::write(&settings, br#"{"version":8,"value":"cleaned account"}"#).unwrap();
    record.account_cleanup = Some(
        serde_json::from_value(json!({
            "relative_path": "settings.json",
            "original": TransactionDigest::from(&digest_file(&backup).unwrap()),
            "cleaned": TransactionDigest::from(&digest_file(&settings).unwrap())
        }))
        .unwrap(),
    );
    write_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME), &record)
        .unwrap();
    let removed = fixture.target.join(&record.artifacts[0].file_name);
    fs::remove_file(&removed).unwrap();
    fs::write(
        &settings,
        br#"{"version":8,"value":"external account edit"}"#,
    )
    .unwrap();
    assert!(recover_interrupted_install(&fixture.recovery_request()).is_err());
    assert!(
        !removed.exists(),
        "must reconcile the account before restoring packages"
    );
    assert_eq!(
        fs::read(&settings).unwrap(),
        br#"{"version":8,"value":"external account edit"}"#
    );
    fs::write(&settings, br#"{"version":8,"value":"cleaned account"}"#).unwrap();
    recover_interrupted_install(&fixture.recovery_request()).unwrap();
    assert_eq!(
        fs::read(&settings).unwrap(),
        br#"{"version":8,"value":"original account"}"#
    );
    for (name, bytes) in fixture.staged_bytes {
        assert_eq!(fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
}
