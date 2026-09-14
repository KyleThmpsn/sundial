use super::*;

#[test]
fn hash_mismatch_is_rejected_before_any_target_or_backup_mutation() {
    let fixture = Fixture::new();
    let target_path = fixture.target.join(CANONICAL_ARTIFACT_FILE_NAMES[0]);
    let original = package_bytes(
        CANONICAL_PACKAGE_IDS[0],
        AUTHORED_PATCH_ID,
        SUNDIAL_BUILD_SIGNATURE,
        b"original",
    );
    fs::write(&target_path, &original).unwrap();
    fs::write(
        fixture.staging.join(CANONICAL_ARTIFACT_FILE_NAMES[0]),
        b"tampered-after-manifest",
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("does not match its manifest"));
    assert_eq!(fs::read(target_path).unwrap(), original);
    assert!(!fixture.backups.exists());
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        name == ".parhelion-transaction.lock" || !name.starts_with(".parhelion-")
    }));
}

#[test]
fn forged_manifest_digest_cannot_hide_tampered_block_bytes() {
    let fixture = Fixture::new();
    let profile = AUTHORED_PACKAGES[0];
    let artifact_path = fixture.staging.join(profile.file_name);
    let mut bytes = fs::read(&artifact_path).unwrap();
    let payload = profile.file_name.as_bytes();
    let payload_offset = bytes
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("fixture payload should be stored in its physical block");
    bytes[payload_offset] ^= 1;
    fs::write(&artifact_path, bytes).unwrap();
    fixture.write_manifest(None);

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("failed structural validation"));
    assert!(error.message.contains("failed SHA-1"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn forged_manifest_digest_cannot_hide_a_truncated_package() {
    let fixture = Fixture::new();
    let profile = AUTHORED_PACKAGES[0];
    let artifact_path = fixture.staging.join(profile.file_name);
    let mut bytes = fs::read(&artifact_path).unwrap();
    bytes.pop();
    fs::write(&artifact_path, bytes).unwrap();
    fixture.write_manifest(None);

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("failed structural validation"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn changed_stock_input_is_rejected_before_any_target_or_backup_mutation() {
    let fixture = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let source_name = profile.stock_file_name(profile.stock_patch_id);
    fs::write(
        fixture.target.join(&source_name),
        package_bytes(
            profile.package_id,
            profile.stock_patch_id,
            0xAABB_CCDD_EEFF_0011,
            b"stock changed after staging",
        ),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("changed since this staged build"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn sparse_affected_stock_chains_are_fingerprinted_and_set_changes_are_rejected() {
    let fixture = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let sparse_name = profile.stock_file_name(1);
    fs::write(
        fixture.target.join(&sparse_name),
        package_bytes(
            profile.package_id,
            1,
            0xAABB_CCDD_EEFF_0011,
            b"sparse stock generation",
        ),
    )
    .unwrap();
    fixture.write_manifest(None);

    let report = install_staged_packages(&fixture.request()).unwrap();
    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());

    let changed_set = Fixture::new();
    fs::write(
        changed_set.target.join(&sparse_name),
        package_bytes(
            profile.package_id,
            1,
            0xAABB_CCDD_EEFF_0011,
            b"added after staging",
        ),
    )
    .unwrap();
    let error = install_staged_packages(&changed_set.request()).unwrap_err();
    assert!(error.message.contains("package-chain set changed"));
    assert!(!changed_set.backups.exists());
}

#[test]
fn manifest_rejects_duplicate_missing_extra_and_traversal_names() {
    for mutation in ["duplicate", "missing", "extra", "traversal"] {
        let fixture = Fixture::new();
        let mut artifacts = fixture.artifact_json();
        match mutation {
            "duplicate" => artifacts[1]["file_name"] = artifacts[0]["file_name"].clone(),
            "missing" => {
                artifacts.pop();
            }
            "extra" => artifacts.push(json!({
                "file_name": "extra.pkg",
                "byte_length": 1,
                "sha256": "0".repeat(64),
            })),
            "traversal" => {
                artifacts[0]["file_name"] = json!("../outside.pkg");
            }
            _ => unreachable!(),
        }
        fixture.write_manifest(Some(artifacts));

        let error = install_staged_packages(&fixture.request()).unwrap_err();

        assert!(
            error.message.contains("duplicate")
                || error.message.contains("exactly")
                || error.message.contains("Missing required authored packages")
                || error.message.contains("Unrecognized authored package")
                || error.message.contains("plain file name"),
            "unexpected error for {mutation}: {error}"
        );
        assert!(!fixture.backups.exists());
    }
}

#[test]
fn unmanifested_direct_package_is_rejected() {
    let fixture = Fixture::new();
    fs::write(fixture.staging.join("unexpected.pkg"), b"unexpected").unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(
        error.message.contains("canonical package files")
            || error.message.contains("Unrecognized authored package")
            || error
                .message
                .contains("package files do not match the recipe-selected manifest set")
    );
    assert!(!fixture.backups.exists());
}

#[test]
fn target_inside_staging_and_backup_inside_target_are_rejected() {
    let fixture = Fixture::new();
    let nested_target = fixture.staging.join("nested-target");
    fs::create_dir(&nested_target).unwrap();
    let mut nested_target_request = InstallRequest::new(
        fixture.staging.clone(),
        nested_target,
        fixture.backups.clone(),
    );
    nested_target_request.game_running_check = game_stopped;
    nested_target_request.runtime_feature_check = runtime_supported;
    assert!(
        install_staged_packages(&nested_target_request)
            .unwrap_err()
            .message
            .contains("nested")
    );

    let nested_staging = fixture.target.join("nested-staging");
    fs::create_dir(&nested_staging).unwrap();
    let mut nested_staging_request = InstallRequest::new(
        nested_staging,
        fixture.target.clone(),
        fixture.backups.clone(),
    );
    nested_staging_request.game_running_check = game_stopped;
    nested_staging_request.runtime_feature_check = runtime_supported;
    assert!(
        install_staged_packages(&nested_staging_request)
            .unwrap_err()
            .message
            .contains("nested")
    );

    let backup_inside_target = fixture.target.join("backups");
    let mut invalid_backup_request = InstallRequest::new(
        fixture.staging.clone(),
        fixture.target.clone(),
        backup_inside_target,
    );
    invalid_backup_request.game_running_check = game_stopped;
    invalid_backup_request.runtime_feature_check = runtime_supported;
    assert!(
        install_staged_packages(&invalid_backup_request)
            .unwrap_err()
            .message
            .contains("outside")
    );
}

#[test]
fn strict_package_headers_and_stock_chain_are_checked_before_backup() {
    let wrong_staged = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let staged_path = wrong_staged.staging.join(CANONICAL_ARTIFACT_FILE_NAMES[0]);
    fs::write(
        staged_path,
        package_bytes(
            profile.package_id,
            profile.authored_patch_id,
            0xDEAD_BEEF_DEAD_BEEF,
            b"foreign",
        ),
    )
    .unwrap();
    wrong_staged.write_manifest(None);
    assert!(
        install_staged_packages(&wrong_staged.request())
            .unwrap_err()
            .message
            .contains(&format!(
                "expected 38/{:04X}/patch {}",
                profile.package_id, profile.authored_patch_id
            ))
    );
    assert!(!wrong_staged.backups.exists());

    let missing_stock = Fixture::new();
    fs::remove_file(
        missing_stock
            .target
            .join(profile.stock_file_name(profile.stock_patch_id)),
    )
    .unwrap();
    assert!(
        install_staged_packages(&missing_stock.request())
            .unwrap_err()
            .message
            .contains("stock chain package")
    );
    assert!(!missing_stock.backups.exists());
}

#[test]
fn foreign_or_aliased_authored_targets_are_never_overwritten() {
    let foreign = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let canonical_target = foreign.target.join(CANONICAL_ARTIFACT_FILE_NAMES[0]);
    let foreign_bytes = package_bytes(
        profile.package_id,
        profile.authored_patch_id,
        0x1111_2222_3333_4444,
        b"foreign",
    );
    fs::write(&canonical_target, &foreign_bytes).unwrap();
    assert!(
        install_staged_packages(&foreign.request())
            .unwrap_err()
            .message
            .contains(&format!(
                "expected 38/{:04X}/patch {}",
                profile.package_id, profile.authored_patch_id
            ))
    );
    assert_eq!(fs::read(canonical_target).unwrap(), foreign_bytes);
    assert!(!foreign.backups.exists());

    let alias = Fixture::new();
    let alias_profile = CANONICAL_PACKAGES[1];
    let alias_path = alias.target.join(format!(
        "foreign_alias_{}.pkg",
        alias_profile.authored_patch_id
    ));
    fs::write(
        &alias_path,
        package_bytes(
            alias_profile.package_id,
            alias_profile.authored_patch_id,
            0x1111_2222_3333_4444,
            b"alias",
        ),
    )
    .unwrap();
    assert!(
        install_staged_packages(&alias.request())
            .unwrap_err()
            .message
            .contains("aliases")
    );
    assert!(alias_path.exists());
    assert!(!alias.backups.exists());

    let higher_patch = Fixture::new();
    let higher_profile = CANONICAL_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.package_id == 0x0709)
        .unwrap();
    let unsupported_patch_id = higher_profile.authored_patch_id + 1;
    let higher_path = higher_patch
        .target
        .join(higher_profile.stock_file_name(unsupported_patch_id));
    fs::write(
        &higher_path,
        package_bytes(
            higher_profile.package_id,
            unsupported_patch_id,
            0x1111_2222_3333_4444,
            b"higher",
        ),
    )
    .unwrap();
    assert!(
        install_staged_packages(&higher_patch.request())
            .unwrap_err()
            .message
            .contains(&format!("unsupported patch {unsupported_patch_id}"))
    );
    assert!(higher_path.exists());
    assert!(!higher_patch.backups.exists());
}

#[test]
fn manifest_source_directory_must_exist_and_match_target() {
    let fixture = Fixture::new();
    let other = fixture._temporary.path().join("other-packages");
    fs::create_dir(&other).unwrap();
    let mut manifest = fixture.manifest_json(None);
    manifest["source_package_directory"] = json!(other.display().to_string());
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("not the selected target"));
    assert!(!fixture.backups.exists());
}

#[test]
fn non_current_manifest_schema_is_rejected_before_mutation() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest_json(None);
    manifest["schema"] = json!(MANIFEST_SCHEMA + 1);
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(
        error
            .message
            .contains("Unsupported Parhelion manifest schema")
    );
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn manifest_unknown_fields_are_rejected_before_mutation() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest_json(None);
    manifest["project"]["sunrise"]["unexpected"] = json!(true);
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("unknown field"));
    assert!(!fixture.backups.exists());
}

#[test]
fn manifest_recipe_fingerprint_is_consumed_before_mutation() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest_json(None);
    manifest["selection_fingerprint"] = json!("0".repeat(64));
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(
        error
            .message
            .contains("does not match the manifest fingerprint")
    );
    assert!(!fixture.backups.exists());
}

#[test]
fn manifest_unlock_rows_must_have_unique_definitions_and_account_slots() {
    let duplicate_definition = test_manifest_project_with_unlocks(&[
        (21_613, ACCOUNT_UNLOCK_BANK, 11_923),
        (21_613, ACCOUNT_UNLOCK_BANK, 11_924),
    ]);
    assert!(
        validate_manifest_unlocks(&duplicate_definition)
            .unwrap_err()
            .message
            .contains("repeats authored unlock definition")
    );

    let duplicate_slot = test_manifest_project_with_unlocks(&[
        (21_613, ACCOUNT_UNLOCK_BANK, 11_923),
        (21_614, ACCOUNT_UNLOCK_BANK, 11_923),
    ]);
    assert!(
        validate_manifest_unlocks(&duplicate_slot)
            .unwrap_err()
            .message
            .contains("repeats authored unlock bank 1, slot 11923")
    );
}
