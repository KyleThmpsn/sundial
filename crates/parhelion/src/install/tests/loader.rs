//! Filesystem lifecycle through the public upgrade and installation boundaries.
//! The failure inventory was recorded before implementation in the migration evidence.
use super::*;
use crate::install::loader as backend;
use sundial::package_authoring::mod_content;
mod compliance;

fn fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let runtime = fixture.target.parent().unwrap().join("bin/x64");
    fs::remove_file(runtime.join("oo2core_3_win64.dll")).unwrap();
    fs::write(
        runtime.join("steam_api64.dll"),
        sundial::test_support::runtime::module("Sunrise", [0, 6, 0, 0], 18),
    )
    .unwrap();
    for profile in CANONICAL_PACKAGES {
        let mut bytes = build_test_package_with_physical_payload(
            profile.package_id,
            profile.stock_patch_id,
            b"stock payload",
        )
        .unwrap();
        bytes[8..16].copy_from_slice(&0xAABB_CCDD_EEFF_0011_u64.to_le_bytes());
        fs::write(
            fixture
                .target
                .join(profile.stock_file_name(profile.stock_patch_id)),
            bytes,
        )
        .unwrap();
    }
    for profile in CANONICAL_PACKAGES {
        let authored = crate::extend::build_extended_overlay(
            &fixture.target,
            profile.package_id,
            &[crate::ReplacementSpec {
                tag: tiger_pkg::TagHash::new(profile.package_id, 0),
                payload: b"authored payload".to_vec(),
            }],
            &[],
        )
        .unwrap();
        fs::write(
            fixture.staging.join(profile.authored_file_name),
            authored.bytes(),
        )
        .unwrap();
        fixture
            .staged_bytes
            .insert(profile.authored_file_name.into(), authored.bytes().to_vec());
    }
    fs::write(
        runtime.join("oo2core_3_win64.dll"),
        b"fixture, never loaded",
    )
    .unwrap();
    for id in [0x0AA1, 0x0CFF] {
        let profile = authored_package(id).unwrap();
        let bytes = build_test_package_with_physical_payload(id, 0, b"spill payload").unwrap();
        fs::write(fixture.staging.join(profile.file_name), &bytes).unwrap();
        fixture.staged_bytes.insert(profile.file_name.into(), bytes);
    }
    write_manifest(&fixture);
    fixture
}

fn write_manifest(fixture: &Fixture) {
    fixture.write_manifest(Some(fixture.staged_bytes.keys().map(|name| {
        let digest = digest_file(&fixture.staging.join(name)).unwrap();
        json!({"file_name": name, "byte_length": digest.byte_length, "sha256": digest.sha256})
    }).collect()));
}

fn change_assets(fixture: &mut Fixture) {
    let name = crate::package_profile::PARHELION_ASSET_FILE_NAME;
    let bytes = build_test_package_with_physical_payload(
        crate::package_profile::PARHELION_ASSET_PACKAGE_ID,
        0,
        b"updated assets",
    )
    .unwrap();
    fs::write(fixture.staging.join(name), &bytes).unwrap();
    fixture.staged_bytes.insert(name.into(), bytes);
    write_manifest(fixture);
}

fn legacy(fixture: &Fixture) {
    for (name, bytes) in &fixture.staged_bytes {
        fs::write(fixture.target.join(name), bytes).unwrap();
    }
}

fn request(fixture: &Fixture) -> InstallRequest {
    let mut request = fixture.request();
    request.runtime_snapshot_check = sundial::package_authoring::installed_runtime;
    request
}

fn recovery(fixture: &Fixture) -> RecoveryRequest {
    RecoveryRequest {
        target_packages_directory: fixture.target.clone(),
        backup_root: fixture.backups.clone(),
        game_running_check: game_stopped,
        runtime_snapshot_check: sundial::package_authoring::installed_runtime,
    }
}

fn account(fixture: &Fixture, schema: u64) -> (PathBuf, Vec<u8>) {
    let root = fixture.target.parent().unwrap().join("bin/x64/Sunrise");
    fs::create_dir_all(root.join("data")).unwrap();
    let settings = root.join("settings.json");
    fs::write(
        &settings,
        serde_json::to_vec(&json!({
            "version": schema, "unknown": {"keep": true},
            "state": {"characters": [{"inventory": [{"hash": 1234, "sockets": [77, 88]}]}]}
        }))
        .unwrap(),
    )
    .unwrap();
    let path = if schema == 18 {
        let path = root.join("data/investment.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        for sql in [
            include_str!(
                "../../../../../src/persistence/sqlite_account/fixtures/investment_schema.sql"
            ),
            include_str!(
                "../../../../../src/persistence/sqlite_account/fixtures/account_settings_schema.sql"
            ),
            include_str!(
                "../../../../../src/persistence/sqlite_account/fixtures/investment_defaults.sql"
            ),
            include_str!(
                "../../../../../src/persistence/sqlite_account/fixtures/account_settings_defaults.sql"
            ),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch("CREATE TABLE migration_extension(value TEXT); INSERT INTO migration_extension VALUES('retain');").unwrap();
        drop(db);
        path
    } else {
        settings
    };
    let bytes = fs::read(&path).unwrap();
    (path, bytes)
}

fn assert_mod_readback(fixture: &Fixture) -> serde_json::Value {
    let root = fixture.target.parent().unwrap();
    let view = mod_content::View::open(root).unwrap().unwrap();
    let files = fixture.staged_bytes.iter().map(|(name, expected)| {
        assert!(!fixture.target.join(name).exists(), "legacy output remains: {name}");
        let path = view.packages().join(name);
        assert_eq!(fs::read(&path).unwrap(), *expected);
        let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
        let payload = package.read_entry(0).unwrap();
        if !name.starts_with("w64_parhelion_assets_") {
            assert_eq!(payload, b"authored payload");
        } else if name != crate::package_profile::PARHELION_ASSET_FILE_NAME {
            assert_eq!(payload, b"spill payload");
        }
        json!({"file": name, "sha256": digest_file(&path).unwrap().sha256, "entry_zero": hex::encode(payload)})
    }).collect::<Vec<_>>();
    json!({"packages": files})
}

#[test]
fn upgrade_preserves_persisted_accounts_and_supports_repeat_update_and_removal() {
    let results = [8, 18]
        .into_iter()
        .map(verify_lifecycle)
        .collect::<Vec<_>>();
    crate::test_support::artifact(
        "sunrise-mod-upgrade-lifecycle.json",
        &json!({"cases":results, "native_loader": "not executed"}),
    );
}

fn verify_lifecycle(schema: u64) -> serde_json::Value {
    let mut fixture = fixture();
    let (account, original_account) = account(&fixture, schema);
    let stock = fixture.source_artifact_json();
    let directory_before = compliance::inventory(&fixture.target);
    fs::write(fixture.target.join(".parhelion-transaction.lock"), []).unwrap();
    legacy(&fixture);
    let plan = preview_upgrade(&fixture.target).unwrap().unwrap();
    assert_eq!(fs::read(&account).unwrap(), original_account);
    let report = upgrade_installation(&plan, &fixture.backups, game_stopped).unwrap();
    let readback = assert_mod_readback(&fixture);
    assert_eq!(compliance::inventory(&fixture.target), directory_before);
    assert_eq!(fs::read(&account).unwrap(), original_account);
    assert_eq!(fixture.source_artifact_json(), stock);
    assert!(preview_upgrade(&fixture.target).unwrap().is_none());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(report.mod_directory.join("mod.json")).unwrap()).unwrap();
    assert_eq!(manifest["manifestVersion"], 3);
    assert_eq!(manifest.as_object().unwrap().len(), 4);
    assert_eq!(manifest["id"], mod_content::ID);
    verify_update_and_removal(&mut fixture, &account, &original_account, &stock);
    assert_eq!(compliance::inventory(&fixture.target), directory_before);
    if schema == 18 {
        let db = rusqlite::Connection::open(&account).unwrap();
        assert_eq!(
            db.query_row("SELECT value FROM migration_extension", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "retain"
        );
        assert!(
            db.query_row("SELECT count(*) FROM items", [], |row| row.get::<_, u64>(0))
                .unwrap()
                > 0
        );
    }
    json!({"account_schema": schema, "stock": stock, "readback": readback,
        "account_sha256": hex::encode(Sha256::digest(&original_account)), "repeat_upgrade": "no action", "update": "mod folder", "uninstall": "stock and account preserved"})
}

fn verify_update_and_removal(
    fixture: &mut Fixture,
    account: &Path,
    original: &[u8],
    stock: &[serde_json::Value],
) {
    change_assets(fixture);
    let updated = install_staged_packages(&request(fixture)).unwrap();
    assert!(updated.mod_directory.is_some());
    assert_mod_readback(fixture);
    assert_eq!(fs::read(account).unwrap(), original);
    let removal = preview_uninstall(&fixture.target).unwrap();
    uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
    assert!(
        preview_uninstall(&fixture.target)
            .unwrap()
            .artifacts()
            .is_empty()
    );
    assert!(!updated.mod_directory.unwrap().exists());
    assert_eq!(fixture.source_artifact_json(), stock);
    assert_eq!(fs::read(account).unwrap(), original);
    let reinstalled = install_staged_packages(&request(fixture)).unwrap();
    assert!(reinstalled.mod_directory.is_some());
    assert_mod_readback(fixture);
    assert_eq!(fs::read(account).unwrap(), original);
}

#[test]
fn upgrade_gates_on_active_runtime_and_rechecks_review_before_mutation() {
    let fixture = fixture();
    legacy(&fixture);
    let root = fixture.target.parent().unwrap();
    let bin = root.join("bin/x64/steam_api64.dll");
    for (brand, version) in [("Sunrise", [0, 5, 9, 0]), ("Dawn", [0, 6, 0, 0])] {
        fs::write(
            &bin,
            sundial::test_support::runtime::module(brand, version, 18),
        )
        .unwrap();
        assert!(preview_upgrade(&fixture.target).unwrap().is_none());
    }
    fs::write(
        &bin,
        sundial::test_support::runtime::module("Sunrise", [0, 6, 0, 0], 18),
    )
    .unwrap();
    let plan = preview_upgrade(&fixture.target).unwrap().unwrap();
    assert!(upgrade_installation(&plan, &fixture.backups, game_running).is_err());
    assert!(!fixture.backups.exists());
    fs::write(root.join("steam_api64.dll"), b"unrecognized root DLL").unwrap();
    assert!(preview_upgrade(&fixture.target).is_err());
    assert!(upgrade_installation(&plan, &fixture.backups, game_stopped).is_err());
    assert!(!mod_content::directory(root).exists());
    fs::remove_file(root.join("steam_api64.dll")).unwrap();
    let asset = fixture
        .target
        .join(crate::package_profile::PARHELION_ASSET_FILE_NAME);
    fs::write(&asset, b"outside edit").unwrap();
    assert!(upgrade_installation(&plan, &fixture.backups, game_stopped).is_err());
    assert_eq!(fs::read(asset).unwrap(), b"outside edit");
}

#[test]
fn interrupted_upgrade_recovers_every_publication_boundary() {
    let mut evidence = Vec::new();
    for stop in [
        "journaled",
        "published",
        "legacy_file_removed",
        "legacy_removed",
        "verified",
        "committed",
    ] {
        let fixture = fixture();
        let stock = compliance::inventory(&fixture.target);
        legacy(&fixture);
        let plan = preview_upgrade(&fixture.target).unwrap().unwrap();
        let reached = std::cell::Cell::new(false);
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backend::upgrade_with_observer(&plan, &fixture.backups, game_stopped, &mut |phase| {
                if phase == stop {
                    reached.set(true);
                    panic!("simulated process stop at {phase}");
                }
            })
            .unwrap();
        }));
        assert!(stopped.is_err());
        assert!(reached.get(), "the operation did not reach {stop}");
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert_mod_readback(&fixture);
        assert!(preview_upgrade(&fixture.target).unwrap().is_none());
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert!(matches!(
            recover_interrupted_install(&recovery(&fixture)).unwrap(),
            RecoveryOutcome::NoTransaction
        ));
        evidence.push(json!({"interruption": stop, "recovery": "verified", "repeat": "verified"}));
    }
    crate::test_support::artifact("sunrise-mod-upgrade-recovery.json", &evidence);
}

#[test]
fn recovery_keeps_concurrent_files_and_upgrade_refuses_conflicting_mods() {
    let fixture = fixture();
    legacy(&fixture);
    let root = fixture.target.parent().unwrap();
    let foreign = root.join("bin/x64/Sunrise/mods/dev.other.content/packages");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join(crate::package_profile::PARHELION_ASSET_FILE_NAME),
        b"foreign package",
    )
    .unwrap();
    assert!(preview_upgrade(&fixture.target).is_err());
    fs::remove_file(foreign.join(crate::package_profile::PARHELION_ASSET_FILE_NAME)).unwrap();
    let plan = preview_upgrade(&fixture.target).unwrap().unwrap();
    let reached = std::cell::Cell::new(false);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend::upgrade_with_observer(&plan, &fixture.backups, game_stopped, &mut |phase| {
            if phase == "published" {
                reached.set(true);
                panic!("simulated process stop");
            }
        })
        .unwrap();
    }));
    assert!(reached.get(), "the upgrade did not publish its folder");
    let changed = mod_content::directory(root)
        .join("packages")
        .join(crate::package_profile::PARHELION_ASSET_FILE_NAME);
    fs::write(&changed, b"concurrent content").unwrap();
    assert!(recover_interrupted_install(&recovery(&fixture)).is_err());
    assert_eq!(fs::read(changed).unwrap(), b"concurrent content");
    for (name, bytes) in &fixture.staged_bytes {
        assert_eq!(fs::read(fixture.target.join(name)).unwrap(), *bytes);
    }
}

#[test]
fn mod_update_and_uninstall_restore_the_previous_folder_after_interruption() {
    let mut evidence = Vec::new();
    for operation in ["update", "uninstall"] {
        for stop in ["retired", "published", "verified", "committed"] {
            if operation == "uninstall" && stop == "published" {
                continue;
            }
            let mut fixture = fixture();
            legacy(&fixture);
            let plan = preview_upgrade(&fixture.target).unwrap().unwrap();
            upgrade_installation(&plan, &fixture.backups, game_stopped).unwrap();
            let directory = mod_content::directory(fixture.target.parent().unwrap());
            let manifest_before = fs::read(directory.join("mod.json")).unwrap();
            let original_bytes = fixture.staged_bytes.clone();
            if operation == "update" {
                change_assets(&mut fixture);
            }
            let reached = std::cell::Cell::new(false);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut observer = |phase: &str| {
                    if phase == stop {
                        reached.set(true);
                        panic!("simulated process stop");
                    }
                };
                if operation == "update" {
                    backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut observer)
                        .unwrap();
                } else {
                    let plan = preview_uninstall(&fixture.target).unwrap();
                    backend::uninstall_with_observer(
                        &plan,
                        &fixture.backups,
                        game_stopped,
                        &mut observer,
                    )
                    .unwrap();
                }
            }));
            assert!(result.is_err());
            assert!(reached.get(), "{operation} did not reach {stop}");
            recover_interrupted_install(&recovery(&fixture)).unwrap();
            if stop != "committed" {
                fixture.staged_bytes = original_bytes;
            }
            if operation == "uninstall" && stop == "committed" {
                assert!(!directory.exists());
            } else {
                assert_mod_readback(&fixture);
                assert_eq!(
                    fs::read(directory.join("mod.json")).unwrap(),
                    manifest_before
                );
            }
            evidence.push(
                json!({"operation": operation, "interruption": stop, "recovery": "verified"}),
            );
        }
    }
    crate::test_support::artifact("sunrise-mod-update-recovery.json", &evidence);
}

#[test]
fn upgraded_content_is_not_opened_on_a_downgraded_runtime() {
    let fixture = fixture();
    legacy(&fixture);
    let plan = preview_upgrade(&fixture.target).unwrap().unwrap();
    upgrade_installation(&plan, &fixture.backups, game_stopped).unwrap();
    let root = fixture.target.parent().unwrap();
    fs::write(
        root.join("bin/x64/steam_api64.dll"),
        sundial::test_support::runtime::module("Sunrise", [0, 5, 0, 0], 8),
    )
    .unwrap();
    assert!(mod_content::View::open(root).is_err());
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert!(
        fixture
            .staged_bytes
            .keys()
            .all(|name| !fixture.target.join(name).exists())
    );
}

#[test]
fn an_overlay_with_changed_inherited_blocks_is_refused_before_upgrade() {
    let fixture = fixture();
    legacy(&fixture);
    let profile = CANONICAL_PACKAGES[0];
    let path = fixture.target.join(profile.authored_file_name);
    let mut bytes = fs::read(&path).unwrap();
    let layout = PackageLayout::parse(&bytes).unwrap();
    bytes[layout.block_table_offset] ^= 1;
    layout.update_package_tables_hash(&mut bytes).unwrap();
    fs::write(&path, bytes).unwrap();
    assert!(preview_upgrade(&fixture.target).is_err());
    assert!(!fixture.backups.exists());
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
}
