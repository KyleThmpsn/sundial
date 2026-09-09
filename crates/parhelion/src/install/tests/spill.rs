use super::*;

const SPILL_NAME: &str = "w64_parhelion_assets_0aa1_0.pkg";

fn fixture_with_spill() -> Fixture {
    let mut fixture = Fixture::new();
    let bytes =
        build_test_package_with_physical_payload(0x0AA1, 0, b"second asset package").unwrap();
    fs::write(fixture.staging.join(SPILL_NAME), &bytes).unwrap();
    fixture.staged_bytes.insert(SPILL_NAME.to_owned(), bytes);
    let mut artifacts = fixture.artifact_json();
    let digest = digest_file(&fixture.staging.join(SPILL_NAME)).unwrap();
    artifacts.push(json!({
        "file_name": SPILL_NAME,
        "byte_length": digest.byte_length,
        "sha256": digest.sha256,
    }));
    fixture.write_manifest(Some(artifacts));
    fixture
}

#[test]
fn spill_packages_install_repeat_and_uninstall_with_backups() {
    let fixture = fixture_with_spill();
    let stock = fixture.source_artifact_json();
    for _ in 0..2 {
        let report = install_staged_packages(&fixture.request()).unwrap();
        assert_eq!(report.artifacts.len(), AUTHORED_PACKAGES.len() + 1);
        assert_eq!(
            fs::read(fixture.target.join(SPILL_NAME)).unwrap(),
            fixture.staged_bytes[SPILL_NAME]
        );
    }
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert_eq!(plan.artifacts().len(), AUTHORED_PACKAGES.len() + 1);
    uninstall_custom_packages(&plan, &fixture.backups, game_stopped).unwrap();
    assert!(!fixture.target.join(SPILL_NAME).exists());
    assert_eq!(stock, fixture.source_artifact_json());
}

#[test]
fn shrinking_a_generation_restores_spill_on_interruption_then_retires_it() {
    let fixture = fixture_with_spill();
    install_staged_packages(&fixture.request()).unwrap();
    let stock = fixture.source_artifact_json();
    fs::remove_file(fixture.staging.join(SPILL_NAME)).unwrap();
    fixture.write_manifest(None);

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
    assert!(!fixture.target.join(SPILL_NAME).exists());
    recover_interrupted_install(&fixture.recovery_request()).unwrap();
    assert_eq!(
        fs::read(fixture.target.join(SPILL_NAME)).unwrap(),
        fixture.staged_bytes[SPILL_NAME]
    );

    let report = install_staged_packages(&fixture.request()).unwrap();
    assert_eq!(
        report.removed_obsolete_packages,
        vec![fs::canonicalize(&fixture.target).unwrap().join(SPILL_NAME)]
    );
    assert!(!fixture.target.join(SPILL_NAME).exists());
    assert_eq!(
        fs::read(report.backup_directory.join(SPILL_NAME)).unwrap(),
        fixture.staged_bytes[SPILL_NAME]
    );
    assert_eq!(stock, fixture.source_artifact_json());
    assert!(
        install_staged_packages(&fixture.request())
            .unwrap()
            .removed_obsolete_packages
            .is_empty()
    );
}

#[test]
fn foreign_spill_packages_are_never_overwritten_or_removed() {
    for name in [SPILL_NAME, "w64_foreign_0aa1_0.pkg"] {
        let fixture = fixture_with_spill();
        let bytes = package_bytes(0x0AA1, 0, 0xAABB_CCDD_EEFF_0011, b"foreign assets");
        fs::write(fixture.target.join(name), &bytes).unwrap();
        assert!(install_staged_packages(&fixture.request()).is_err());
        assert!(preview_uninstall(&fixture.target).is_err());
        assert_eq!(fs::read(fixture.target.join(name)).unwrap(), bytes);
        assert!(!fixture.backups.exists());
    }
}
