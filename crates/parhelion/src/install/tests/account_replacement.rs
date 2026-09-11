use super::*;

#[test]
fn slot_replacement_moves_or_deletes_with_package_commit_and_rolls_back_on_failure() {
    use sundial::investment::{AuthoredMoveOutcome, AuthoredSlotChange, AuthoredSlotReplacement};
    for capacity in [1, 2] {
        for fail in [false, true] {
            let fixture = Fixture::new();
            let (path, original, _) = account_fixture(&fixture);
            let slots = AuthoredSlotReplacement {
                changes: vec![AuthoredSlotChange {
                    definition_hash: 100,
                    previous_bucket: 0,
                    incoming_bucket: 1,
                }],
                incoming_buckets: BTreeMap::from([(100, 1), (200, 1)]),
                weapon_capacities: [10, capacity, 10],
            };
            let review = crate::install::test_review_with_slots(
                &fixture.target,
                BTreeSet::new(),
                vec![],
                Some(slots),
            );
            assert!(review.changes_account());
            assert_eq!(review.removes_account_data(), capacity == 1);
            let proposal = review.account_cleanup().unwrap();
            assert_eq!(
                proposal.slot_moves[0].outcome,
                if capacity == 1 {
                    AuthoredMoveOutcome::DeletedInventoryFull
                } else {
                    AuthoredMoveOutcome::MovedToInventory
                }
            );
            assert_eq!(fs::read(&path).unwrap(), original);
            let expected = proposal.cleaned_bytes.clone();
            let mut request = fixture.request();
            request.confirmed_replacement = Some(review);
            let result = install_staged_packages_inner(
                &request,
                fail.then_some(1),
                DEFAULT_CACHE_INVALIDATION_OPS,
            );
            if fail {
                let error = result.unwrap_err();
                assert!(error.rollback.as_ref().unwrap().succeeded(), "{error}");
                assert_eq!(fs::read(&path).unwrap(), original);
            } else {
                let report = result.unwrap();
                assert_eq!(fs::read(&path).unwrap(), expected);
                assert_eq!(
                    fs::read(report.backup_directory.join("account-settings.json")).unwrap(),
                    original
                );
            }
        }
    }
}

fn socket_fixture(
    fixture: &Fixture,
    previous: usize,
    incoming: usize,
) -> (PathBuf, Vec<u8>, ReplacementReview) {
    let path = fixture.target.parent().unwrap().join("settings.json");
    let original = serde_json::to_vec(&json!({"version": 16, "unknown": {"keep": true}, "state": {
        "account": {"primary_soid": "0x0000000000000001"},
        "characters": [{"soid": "0x0000000000000002", "class": 0, "equipment": {
            "kinetic": {"instance_soid": "0x0000000000000010", "definition_hash": 100, "level": 100, "quantity": 1, "plugs": vec![301; previous]},
            "energy": {"instance_soid": "0x0000000000000011", "definition_hash": 100, "level": 100, "quantity": 1, "plugs": null}
        }}]
    }})).unwrap();
    fs::write(&path, &original).unwrap();
    let review = crate::install::replacement::test_review_with_sockets(
        &fixture.target,
        BTreeSet::new(),
        vec![sundial::investment::AuthoredSocketChange {
            definition_hash: 100,
            previous_socket_count: previous,
            default_plugs: vec![Some(400); incoming],
        }],
    );
    (path, original, review)
}

#[test]
fn retained_socket_lists_resize_with_packages_and_failed_installs_restore_them() {
    for (previous, incoming) in [(8, 9), (8, 12), (12, 8)] {
        for fail in [false, true] {
            let fixture = Fixture::new();
            let (path, original, review) = socket_fixture(&fixture, previous, incoming);
            let proposal = review.account_cleanup().unwrap();
            assert_eq!(proposal.resized_items, BTreeMap::from([(100, 1)]));
            let expected = proposal.cleaned_bytes.clone();
            let mut request = fixture.request();
            request.confirmed_replacement = Some(review);
            let result = install_staged_packages_inner(
                &request,
                fail.then_some(1),
                DEFAULT_CACHE_INVALIDATION_OPS,
            );
            if fail {
                let error = result.unwrap_err();
                assert!(error.rollback.as_ref().unwrap().succeeded(), "{error}");
                assert_eq!(fs::read(path).unwrap(), original);
            } else {
                let report = result.unwrap();
                assert_eq!(fs::read(&path).unwrap(), expected);
                assert_eq!(
                    fs::read(report.backup_directory.join("account-settings.json")).unwrap(),
                    original
                );
                let saved: serde_json::Value = serde_json::from_slice(&expected).unwrap();
                assert_eq!(
                    saved["state"]["characters"][0]["equipment"]["kinetic"]["plugs"]
                        .as_array()
                        .unwrap()
                        .len(),
                    incoming
                );
                assert!(saved["state"]["characters"][0]["equipment"]["energy"]["plugs"].is_null());
            }
        }
    }
}

fn account_fixture(fixture: &Fixture) -> (PathBuf, Vec<u8>, ReplacementReview) {
    let path = fixture.target.parent().unwrap().join("settings.json");
    let original = serde_json::to_vec(&json!({"version": 8, "unknown": {"keep": true}, "state": {
        "account": {"primary_soid": "0x0000000000000001"},
        "characters": [{"soid": "0x0000000000000002", "class": 0, "equipment": {
            "kinetic": {"instance_soid": "0x0000000000000010", "definition_hash": 100, "level": 100, "quantity": 1, "plugs": null},
            "energy": {"instance_soid": "0x0000000000000011", "definition_hash": 200, "level": 100, "quantity": 1, "plugs": [300, 301]}
        }}]
    }})).unwrap();
    fs::write(&path, &original).unwrap();
    let review =
        crate::install::replacement::test_review(&fixture.target, BTreeSet::from([100, 300]));
    (path, original, review)
}

#[test]
fn failed_replacement_restores_account_and_packages_together() {
    let fixture = Fixture::new();
    let (path, original, review) = account_fixture(&fixture);
    let mut request = fixture.request();
    request.confirmed_replacement = Some(review);
    let error = install_staged_packages_inner(&request, Some(1), DEFAULT_CACHE_INVALIDATION_OPS)
        .unwrap_err();
    assert!(error.rollback.as_ref().unwrap().succeeded(), "{error}");
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(
        preview_uninstall(&fixture.target)
            .unwrap()
            .artifacts()
            .is_empty()
    );
}

#[test]
fn confirmed_replacement_keeps_account_backup_out_of_retention() {
    let fixture = Fixture::new();
    let (path, original, review) = account_fixture(&fixture);
    let expected = review.account_cleanup().unwrap().cleaned_bytes.clone();
    let mut request = fixture.request();
    request.confirmed_replacement = Some(review);
    let report = install_staged_packages(&request).unwrap();
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(
        fs::read(report.backup_directory.join("account-settings.json")).unwrap(),
        original
    );
    for _ in 0..3 {
        install_staged_packages(&fixture.request()).unwrap();
    }
    prune_package_backups(&fixture.backups, 1).unwrap();
    assert!(report.backup_directory.is_dir());
}

#[test]
fn stale_account_consent_fails_before_package_replacement() {
    let fixture = Fixture::new();
    let (path, _, review) = account_fixture(&fixture);
    let mut request = fixture.request();
    request.confirmed_replacement = Some(review);
    let changed = b"{\"version\":8,\"state\":{},\"concurrent\":true}";
    fs::write(&path, changed).unwrap();
    assert!(install_staged_packages(&request).is_err());
    assert_eq!(fs::read(path).unwrap(), changed);
    assert!(
        preview_uninstall(&fixture.target)
            .unwrap()
            .artifacts()
            .is_empty()
    );
}

#[test]
fn interrupted_replacement_recovers_cleaned_account_and_partial_packages() {
    let fixture = Fixture::new();
    let (path, original, review) = account_fixture(&fixture);
    let request = fixture.request();
    let validated = validate_request(&request).unwrap();
    let backup = create_backup_directory(&fixture.backups).unwrap();
    let originals = backup_originals(&validated, &backup).unwrap();
    let prepared = prepare_temporary_files(&validated).unwrap();
    let mut transaction =
        build_install_transaction(&validated, &backup, &originals, &prepared).unwrap();
    let cleanup = review.account_cleanup().unwrap();
    let record = account::prepare(cleanup, &validated.target_packages_directory, &backup).unwrap();
    transaction.account_cleanup = Some(record.clone());
    write_install_transaction(
        &fixture.target.join(INSTALL_TRANSACTION_FILE_NAME),
        &transaction,
    )
    .unwrap();
    account::commit(
        cleanup,
        &record,
        &validated.target_packages_directory,
        &backup,
    )
    .unwrap();
    fs::write(
        &prepared[0].target_path,
        &fixture.staged_bytes[&prepared[0].manifest.file_name],
    )
    .unwrap();
    recover_interrupted_install(&RecoveryRequest {
        target_packages_directory: fixture.target.clone(),
        backup_root: fixture.backups.clone(),
        game_running_check: game_stopped,
    })
    .unwrap();
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(
        preview_uninstall(&fixture.target)
            .unwrap()
            .artifacts()
            .is_empty()
    );
}
