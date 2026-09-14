use super::*;

#[test]
#[ignore = "requires PARHELION_TEST_STAGED_RUN, PARHELION_TEST_TARGET_PACKAGES, and PARHELION_TEST_BACKUP_ROOT"]
fn configured_staged_run_installs_through_the_transaction() {
    let staged_run_directory = std::env::var_os("PARHELION_TEST_STAGED_RUN")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_STAGED_RUN must point to a staged run");
    let target_packages_directory = std::env::var_os("PARHELION_TEST_TARGET_PACKAGES")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_TARGET_PACKAGES must point to Shadowkeep packages");
    let backup_root = std::env::var_os("PARHELION_TEST_BACKUP_ROOT")
        .map(PathBuf::from)
        .expect("PARHELION_TEST_BACKUP_ROOT must be configured");
    let manifest: ManifestDocument = sundial::package_authoring::read_json(
        File::open(staged_run_directory.join(MANIFEST_FILE_NAME))
            .expect("configured staged manifest should open"),
    )
    .expect("configured staged manifest should decode");
    let report = install_staged_packages(&InstallRequest::new(
        staged_run_directory,
        target_packages_directory.clone(),
        backup_root,
    ))
    .unwrap_or_else(|error| panic!("configured install should succeed: {error}"));

    assert_eq!(report.artifacts.len(), manifest.artifacts.len());
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(&target_packages_directory)
            .expect("installed authored package set should reopen");
    let globals =
        sundial::package_authoring::resolve_live_named_tag(&manager, "investment_globals", None)
            .expect("installed package set should expose one live investment-globals tag");
    assert!(
        !manager
            .read_tag(globals)
            .expect("installed investment globals should read")
            .is_empty()
    );
    for weapon in &manifest.project.weapons {
        for (description, tag) in [
            ("definition", weapon.item.definition_tag),
            ("strings", weapon.item.string_tag),
        ] {
            let tag = tiger_pkg::TagHash(tag.get());
            assert!(
                manager.get_entry(tag).is_some(),
                "installed {} {description} entry {tag} is missing",
                weapon.namespace
            );
            assert!(
                !manager
                    .read_tag(tag)
                    .unwrap_or_else(|error| panic!(
                        "installed {} {description} tag {tag} should read: {error}",
                        weapon.namespace
                    ))
                    .is_empty(),
                "installed {} {description} tag {tag} is empty",
                weapon.namespace
            );
        }
    }
    for icon in manifest
        .project
        .sunrise
        .watermarked_icon_containers
        .iter()
        .copied()
        .chain([manifest.project.sunrise.badge_icon_tag])
    {
        crate::watermark::validate_icon_definition_graph(&manager, tiger_pkg::TagHash(icon.get()))
            .unwrap_or_else(|error| {
                panic!(
                    "installed icon definition 0x{:08X} is invalid: {error}",
                    icon.get()
                )
            });
    }
    if let Some(result) = &report.profile_sync {
        let sync = result
            .as_ref()
            .expect("installed Collections unlocks should synchronize");
        println!(
            "PARHELION_PROFILE_SYNC={} total={} newly_set={}",
            sync.settings_path.display(),
            sync.total_unlocks,
            sync.newly_set_unlocks
        );
    }
    println!("PARHELION_BACKUP={}", report.backup_directory.display());
    for artifact in report.artifacts {
        println!(
            "PARHELION_INSTALLED={} {}",
            artifact.file_name, artifact.sha256
        );
    }
}

#[test]
fn installs_exact_verified_set_and_backs_up_only_existing_targets() {
    let fixture = Fixture::new();
    let existing_names = CANONICAL_PACKAGES[..2]
        .iter()
        .map(|profile| profile.authored_file_name)
        .collect::<Vec<_>>();
    for profile in &CANONICAL_PACKAGES[..2] {
        fs::write(
            fixture.target.join(profile.authored_file_name),
            package_bytes(
                profile.package_id,
                profile.authored_patch_id,
                SUNDIAL_BUILD_SIGNATURE,
                format!("original-{}", profile.authored_file_name).as_bytes(),
            ),
        )
        .unwrap();
    }

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());
    assert!(!path_is_within(
        &report.backup_directory,
        &fs::canonicalize(&fixture.target).unwrap()
    ));
    for artifact in &report.artifacts {
        assert_eq!(
            fs::read(&artifact.target_path).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
        let replaced = existing_names.contains(&artifact.file_name.as_str());
        assert_eq!(artifact.replaced_existing_file, replaced);
        assert_eq!(artifact.backup_path.is_some(), replaced);
    }
    let backup_names = fs::read_dir(&report.backup_directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        backup_names,
        existing_names
            .iter()
            .map(|name| (*name).to_owned())
            .chain(["install-complete.json".to_owned()])
            .collect()
    );
    for name in existing_names {
        assert!(
            fs::read(report.backup_directory.join(name))
                .unwrap()
                .ends_with(format!("original-{name}").as_bytes())
        );
    }
}

#[test]
fn target_temp_allocation_failure_cleans_every_earlier_prepared_file() {
    let fixture = Fixture::new();
    let validated = validate_request(&fixture.request()).unwrap();

    let error = prepare_temporary_files_inner(&validated, Some(2)).unwrap_err();

    assert!(error.contains("allocation failure"));
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".parhelion-install-")
    }));
}

#[test]
fn injected_mid_commit_failure_restores_old_files_and_removes_new_files() {
    let fixture = Fixture::new();
    let cache_path = fixture.write_sunrise_cache(b"cache must survive failed install");
    let first = CANONICAL_ARTIFACT_FILE_NAMES[0];
    let first_profile = AUTHORED_PACKAGES
        .iter()
        .find(|profile| profile.file_name == first)
        .unwrap();
    let first_original = package_bytes(
        first_profile.package_id,
        first_profile.patch_id,
        SUNDIAL_BUILD_SIGNATURE,
        b"first-original",
    );
    fs::write(fixture.target.join(first), &first_original).unwrap();

    let error =
        install_staged_packages_inner(&fixture.request(), Some(2), DEFAULT_CACHE_INVALIDATION_OPS)
            .unwrap_err();
    let rollback = error.rollback.unwrap();

    assert!(rollback.succeeded(), "{:?}", rollback.errors);
    assert_eq!(
        fs::read(fixture.target.join(first)).unwrap(),
        first_original
    );
    assert!(
        !fixture
            .target
            .join(CANONICAL_ARTIFACT_FILE_NAMES[1])
            .exists()
    );
    for name in &CANONICAL_ARTIFACT_FILE_NAMES[2..] {
        assert!(!fixture.target.join(name).exists());
    }
    assert_eq!(
        fs::read(cache_path).unwrap(),
        b"cache must survive failed install"
    );
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        !name.starts_with(".parhelion-install-") || name == INSTALL_TRANSACTION_FILE_NAME
    }));
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn game_running_checks_refuse_initial_and_precommit_races() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request.game_running_check = game_running;

    assert!(
        install_staged_packages(&request)
            .unwrap_err()
            .message
            .contains("game is running")
    );

    GAME_STARTS_AFTER_PREFLIGHT_CALLS.store(0, Ordering::SeqCst);
    request.game_running_check = game_starts_after_preflight;
    let error = install_staged_packages(&request).unwrap_err();
    assert!(
        error
            .message
            .contains("started during installation preflight")
    );
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }

    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".parhelion-install-")
    }));
    request.game_running_check = game_stopped;
    let report = install_staged_packages(&request).unwrap();
    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());

    let failed_check = Fixture::new();
    let mut request = failed_check.request();
    request.game_running_check = game_check_failed;
    let error = install_staged_packages(&request).unwrap_err();
    assert!(error.message.contains("Could not check"));
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!failed_check.target.join(name).exists());
    }
}

#[test]
fn a_new_install_recovers_a_pending_transaction_before_preflight() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let first = &transaction.artifacts[0];
    fs::write(
        fixture.target.join(&first.file_name),
        &fixture.staged_bytes[&first.file_name],
    )
    .unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
    }
}
