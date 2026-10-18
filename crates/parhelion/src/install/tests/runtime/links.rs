//! Package lifecycle with real filesystem links and generated runtime version resources.
//! Failure cases were recorded in the runtime-link investigation before implementation.
use super::*;

fn link_file(target: &Path, link: &Path) {
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}

pub(in crate::install::tests) fn external(fixture: &Fixture, root: bool) -> TempDir {
    let outside = TempDir::new().unwrap();
    let bin = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/steam_api64.dll");
    let target = outside.path().join("steam_api64.dll");
    fs::write(&target, fs::read(&bin).unwrap()).unwrap();
    fs::remove_file(&bin).unwrap();
    let selected = if root {
        fixture.target.parent().unwrap().join("steam_api64.dll")
    } else {
        bin
    };
    link_file(&target, &selected);
    outside
}

fn request(fixture: &Fixture) -> InstallRequest {
    let mut request = fixture.request();
    request.runtime_snapshot_check = sundial::package_authoring::installed_runtime;
    request.runtime_feature_check = sundial::package_authoring::validate_package_authoring_runtime;
    request
}

fn recovery(fixture: &Fixture) -> RecoveryRequest {
    RecoveryRequest {
        runtime_snapshot_check: sundial::package_authoring::installed_runtime,
        ..fixture.recovery_request()
    }
}

fn inventory(directory: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn retain(case: &str, name: &str, source: &Path) {
    let Some(directory) = crate::test_support::artifacts("runtime-links") else {
        return;
    };
    let directory = directory.join(case);
    fs::create_dir_all(&directory).unwrap();
    fs::copy(source, directory.join(name)).unwrap();
}

fn readback(fixture: &Fixture, case: &str) -> Vec<serde_json::Value> {
    fixture.staged_bytes.iter().map(|(name, expected)| {
        let path = fixture.target.join(name);
        assert_eq!(fs::read(&path).unwrap(), *expected);
        let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
        let payload = package.read_entry(0).unwrap();
        if CANONICAL_PACKAGES.iter().any(|profile| profile.authored_file_name == name.as_str()) {
            assert_eq!(payload, b"authored payload");
        }
        retain(case, name, &path);
        json!({"file": name, "sha256": hex::encode(Sha256::digest(expected)), "entry_zero": hex::encode(payload)})
    }).collect()
}

fn interrupted_replacement(
    fixture: &Fixture,
    request: &InstallRequest,
) -> InstallTransactionRecord {
    let validated = validate_request(request).unwrap();
    let backup = create_backup_directory(&validated.backup_root).unwrap();
    let originals = backup_originals(&validated, &backup).unwrap();
    let prepared = prepare_temporary_files(&validated).unwrap();
    let record = build_install_transaction(&validated, &backup, &originals, &prepared).unwrap();
    write_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME), &record)
        .unwrap();
    commit_prepared_files(&prepared, Some(prepared.len() - 1), publish_package).unwrap_err();
    record
}

fn reject_bad_metadata(fixture: &Fixture, record: &InstallTransactionRecord) {
    let journal = fixture.target.join(INSTALL_TRANSACTION_FILE_NAME);
    let before = fixture
        .staged_bytes
        .keys()
        .map(|name| (name.clone(), fs::read(fixture.target.join(name)).unwrap()))
        .collect::<BTreeMap<_, _>>();
    for (field, invalid) in [
        ("module_path", "bin/x64/steam_api64.dll"),
        ("dll_sha256", "bad hash"),
    ] {
        let mut value = serde_json::to_value(record).unwrap();
        value["runtime"][field] = json!(invalid);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&journal, &bytes).unwrap();
        let error = recover_interrupted_install(&recovery(fixture)).unwrap_err();
        assert!(
            error.message.contains("invalid runtime identity metadata"),
            "{error}"
        );
        assert_eq!(fs::read(&journal).unwrap(), bytes);
        for (name, original) in &before {
            assert_eq!(fs::read(fixture.target.join(name)).unwrap(), *original);
        }
    }
    write_install_transaction(&journal, record).unwrap();
}

fn verify_removal(
    fixture: &Fixture,
    stock: &BTreeMap<String, Vec<u8>>,
    account: (&Path, &[u8]),
    runtime: (&Path, &[u8]),
    selected: &Path,
) {
    assert_eq!(&inventory(&fixture.target), stock);
    assert_eq!(fs::read(account.0).unwrap(), account.1);
    assert_eq!(fs::read(runtime.0).unwrap(), runtime.1);
    assert!(
        fs::symlink_metadata(selected)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

fn lifecycle(schema: u64, root: bool) -> serde_json::Value {
    let mut fixture = super::super::loader::fixture();
    let (mut account, account_before) = super::super::loader::account(&fixture, schema);
    if root {
        let game = fixture.target.parent().unwrap();
        fs::rename(game.join("bin/x64/Sunrise"), game.join("Sunrise")).unwrap();
        account = game.join("Sunrise").join(if schema == 18 {
            "data/investment.sqlite3"
        } else {
            "settings.json"
        });
    }
    let bin = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/steam_api64.dll");
    fs::write(
        &bin,
        sundial::test_support::runtime::module("Sunrise", [0, 5, 1, 0], schema),
    )
    .unwrap();
    let outside = external(&fixture, root);
    let dll = outside.path().join("steam_api64.dll");
    let dll_before = fs::read(&dll).unwrap();
    let stock = inventory(&fixture.target);
    let case = format!("schema-{schema}-{}", if root { "root" } else { "bin" });
    let request = request(&fixture);
    let cache = fixture.write_sunrise_cache(b"old runtime build cache");
    let header = fixture
        .target
        .parent()
        .unwrap()
        .join("cache_phr_0000e2e9.dat");
    fs::write(&header, b"old package headers").unwrap();
    let installed = install_staged_packages(&request).unwrap();
    assert!(!cache.exists());
    assert!(!header.exists());
    let packages = readback(&fixture, &case);
    retain(
        &case,
        MANIFEST_FILE_NAME,
        &fixture.staging.join(MANIFEST_FILE_NAME),
    );
    install_staged_packages(&request).unwrap();
    readback(&fixture, &case);

    // Persist the same schema-1 record an older interrupted install would leave.
    let previous = inventory(&fixture.target);
    super::super::loader::change_assets(&mut fixture);
    let record = interrupted_replacement(&fixture, &request);
    let assets = crate::package_profile::PARHELION_ASSET_FILE_NAME;
    assert_ne!(
        fs::read(fixture.target.join(assets)).unwrap(),
        previous[assets]
    );
    let journal = fixture.target.join(INSTALL_TRANSACTION_FILE_NAME);
    retain(&case, "pending-transaction.json", &journal);
    retain(
        &case,
        "replacement-manifest.json",
        &fixture.staging.join(MANIFEST_FILE_NAME),
    );
    reject_bad_metadata(&fixture, &record);
    let recovered = recover_interrupted_install(&recovery(&fixture)).unwrap();
    assert!(matches!(recovered, RecoveryOutcome::Recovered { .. }));
    assert_eq!(inventory(&fixture.target), previous);
    assert!(!journal.exists());

    let plan = preview_uninstall(&fixture.target).unwrap();
    uninstall_custom_packages(&plan, &fixture.backups, game_stopped).unwrap();
    let selected = fixture.target.parent().unwrap().join(if root {
        "steam_api64.dll"
    } else {
        "bin/x64/steam_api64.dll"
    });
    verify_removal(
        &fixture,
        &stock,
        (&account, &account_before),
        (&dll, &dll_before),
        &selected,
    );
    retain(&case, "runtime.dll", &dll);
    retain(&case, "account.bin", &account);
    retain(
        &case,
        "install-complete.json",
        &installed.backup_directory.join("install-complete.json"),
    );
    json!({"case": case, "schema": schema, "runtime_sha256": hex::encode(Sha256::digest(dll_before)), "account_sha256": hex::encode(Sha256::digest(account_before)), "packages": packages, "recovery": "restored", "uninstall": "stock and account preserved", "runtime_link": "preserved"})
}

#[test]
fn external_runtime_links_support_install_recovery_and_uninstall() {
    let evidence = [8, 18]
        .map(|schema| [false, true].map(|root| lifecycle(schema, root)))
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    crate::test_support::artifact("runtime-link-lifecycle.json", &evidence);
}

#[test]
fn linked_runtime_changes_are_refused_before_package_replacement() {
    let mut evidence = Vec::new();
    for change in ["same-byte-retarget", "target-rebuild", "dangling-link"] {
        let fixture = super::super::loader::fixture();
        let (account, account_before) = super::super::loader::account(&fixture, 18);
        let bin = fixture
            .target
            .parent()
            .unwrap()
            .join("bin/x64/steam_api64.dll");
        fs::write(
            &bin,
            sundial::test_support::runtime::module("Sunrise", [0, 5, 1, 0], 18),
        )
        .unwrap();
        let outside = external(&fixture, false);
        let dll = outside.path().join("steam_api64.dll");
        let alternate = outside.path().join("alternate.dll");
        fs::copy(&dll, &alternate).unwrap();
        let before = inventory(&fixture.target);
        let mut changed = false;
        let error = install_staged_packages_with_progress(&request(&fixture), |event| {
            if !changed && event.phase == InstallPhase::Rechecking {
                match change {
                    "same-byte-retarget" => {
                        fs::remove_file(&bin).unwrap();
                        link_file(&alternate, &bin);
                    }
                    "target-rebuild" => {
                        let mut bytes = fs::read(&dll).unwrap();
                        bytes.extend_from_slice(b"rebuilt target");
                        fs::write(&dll, bytes).unwrap();
                    }
                    "dangling-link" => fs::remove_file(&dll).unwrap(),
                    _ => unreachable!(),
                }
                changed = true;
            }
        })
        .unwrap_err();
        assert!(changed);
        assert_eq!(inventory(&fixture.target), before);
        assert_eq!(fs::read(&account).unwrap(), account_before);
        assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
        evidence.push(json!({"change": change, "error": error.message, "packages": "unchanged", "account_sha256": hex::encode(Sha256::digest(account_before))}));
    }
    crate::test_support::artifact("runtime-link-refusals.json", &evidence);
}
