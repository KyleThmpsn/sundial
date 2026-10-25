//! Filesystem lifecycle through the public installation, review and recovery boundaries.
//!
//! Failure inventory, written before the change to retire an earlier layout:
//! - an earlier layout's packages patch stock packages with added entries, which the loader
//!   refuses, so moving them into the mod folder publishes a mod that never loads
//! - an install refuses to run while they remain, and removal or recovery that validates them as
//!   mod content stops, leaving the installation unusable
//! - a build whose patch adds entries reaches the mod folder
//! - retiring them loses their bytes, or loses them when an operation is interrupted
//! - an interrupted operation completes forward before its account change was written, or rolls
//!   back after the earlier layout's packages were removed
//! - a retired package's id is taken as a conflict for the build replacing it
//!
//! Failure inventory, written before the change to support switching between Sunrise 0.6 and
//! Dawn or an earlier Sunrise on one installation:
//! - reading fails, or shows the mod folder, on a runtime that doesn't load Sunrise mods
//! - removal there refuses the mod folder, or leaves it for Sunrise 0.6 to load again later
//! - an install there leaves the mod folder beside the new packages, so Sunrise 0.6 then loads
//!   both layouts, or retires the folder without a backup or before the build was validated
//! - a build made for the mod folder installs where its records package isn't known to load
//! - the other runtime keeps a build-data cache describing packages that were replaced
use super::*;
use crate::install::loader as backend;
use sundial::package_authoring::mod_content;
mod coexistence;
mod compliance;
mod diagnostics;
mod family;
mod scripts;

pub(super) fn fixture() -> Fixture {
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

pub(super) fn change_assets(fixture: &mut Fixture) {
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

/// Writes an earlier layout's output into stock `packages`, as a build for Dawn or Sunrise
/// before 0.6 installed it: each canonical patch replaces an entry and appends one, and every
/// standalone package the build stages has a copy of its own. The patches are built beside copies
/// of the stock packages, away from the fixture's placeholder encoder. Returns the written bytes.
fn legacy(fixture: &Fixture) -> BTreeMap<String, Vec<u8>> {
    let view = tempfile::tempdir().unwrap();
    let packages = view.path().join("packages");
    fs::create_dir(&packages).unwrap();
    let mut written = BTreeMap::new();
    for profile in CANONICAL_PACKAGES {
        let stock = profile.stock_file_name(profile.stock_patch_id);
        fs::copy(fixture.target.join(&stock), packages.join(&stock)).unwrap();
        let overlay = crate::extend::build_extended_overlay(
            &packages,
            profile.package_id,
            &[crate::ReplacementSpec {
                tag: tiger_pkg::TagHash::new(profile.package_id, 0),
                payload: b"legacy payload".to_vec(),
            }],
            &[crate::NewTagSpec {
                template_tag: tiger_pkg::TagHash::new(profile.package_id, 0),
                payload: b"legacy record".to_vec(),
                storage: crate::NewTagStorageMode::InheritTemplate,
            }],
        )
        .unwrap();
        written.insert(
            profile.authored_file_name.to_owned(),
            overlay.bytes().to_vec(),
        );
    }
    for name in fixture.staged_bytes.keys() {
        if let Some(profile) = crate::package_profile::authored_package_for_file_name(name)
            && canonical_package(profile.package_id).is_none()
        {
            let bytes =
                build_test_package_with_physical_payload(profile.package_id, 0, b"legacy assets")
                    .unwrap();
            written.insert(name.clone(), bytes);
        }
    }
    for (name, bytes) in &written {
        fs::write(fixture.target.join(name), bytes).unwrap();
    }
    written
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

pub(super) fn account(fixture: &Fixture, schema: u64) -> (PathBuf, Vec<u8>) {
    let root = fixture.target.parent().unwrap().join("bin/x64/Sunrise");
    fs::create_dir_all(root.join("data")).unwrap();
    let settings = root.join("settings.json");
    fs::write(
        &settings,
        serde_json::to_vec(&json!({
            "version": if schema == 3 { 18 } else { schema }, "unknown": {"keep": true},
            "state": {"characters": [{"inventory": [{"hash": 1234, "sockets": [77, 88]}]}]}
        }))
        .unwrap(),
    )
    .unwrap();
    let path = if schema == 18 || schema == 3 {
        let path = root.join("data/investment.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        // Initialize the complete resource set in one transaction to avoid a disk sync per row.
        db.execute_batch("BEGIN").unwrap();
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
        if schema == 3 {
            db.execute_batch(include_str!(
                "../../../../../src/persistence/sqlite_account/fixtures/v3/migration.sql"
            ))
            .unwrap();
            db.execute(
                "UPDATE characters SET appearance_header=?,appearance_header_valid=1",
                [vec![0x5a_u8; 36]],
            )
            .unwrap();
            db.execute_batch("UPDATE starter_character SET level=42")
                .unwrap();
        }
        db.execute_batch("CREATE TABLE migration_extension(value TEXT); INSERT INTO migration_extension VALUES('retain');").unwrap();
        db.execute_batch("COMMIT").unwrap();
        drop(db);
        path
    } else {
        settings
    };
    let bytes = fs::read(&path).unwrap();
    (path, bytes)
}

/// How many items of `definition` a SQLite account holds.
fn item_count(account: &Path, definition: u32) -> u64 {
    rusqlite::Connection::open(account)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM items WHERE definition_hash=?",
            [i64::from(definition)],
            |row| row.get(0),
        )
        .unwrap()
}

/// The mod folder holds exactly the staged build, read back through the loader's projection,
/// and no output of either layout remains in stock `packages`.
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

/// Every file in `directory` and its subfolders by relative path, with its SHA-256.
fn tree(directory: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, path: &Path, found: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    digest_file(&path).unwrap().sha256,
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(directory, directory, &mut found);
    found
}

/// Makes the installation's runtime `brand` at `version`.
fn switch_runtime(fixture: &Fixture, brand: &str, version: [u16; 4]) {
    fs::write(
        fixture
            .target
            .parent()
            .unwrap()
            .join("bin/x64/steam_api64.dll"),
        sundial::test_support::runtime::module(brand, version, 18),
    )
    .unwrap();
}

/// Every retired package is in exactly one recovery backup, byte for byte.
fn assert_retired_backup(fixture: &Fixture, retired: &BTreeMap<String, Vec<u8>>) -> PathBuf {
    let backups = fs::read_dir(&fixture.backups)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("legacy").is_dir())
        .collect::<Vec<_>>();
    let [backup] = backups.as_slice() else {
        panic!("one backup holds the retired packages: {backups:?}");
    };
    for (name, bytes) in retired {
        assert_eq!(&fs::read(backup.join("legacy").join(name)).unwrap(), bytes);
    }
    backup.clone()
}

#[test]
fn install_retires_an_earlier_layout_and_preserves_persisted_accounts() {
    let results = [8, 18, 3]
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
    let outside = super::runtime::links::external(&fixture, false);
    let external_dll = outside.path().join("steam_api64.dll");
    let runtime_before = fs::read(&external_dll).unwrap();
    let (account, original_account) = account(&fixture, schema);
    let stock = fixture.source_artifact_json();
    let directory_before = compliance::inventory(&fixture.target);
    fs::write(fixture.target.join(".parhelion-transaction.lock"), []).unwrap();
    let retired = legacy(&fixture);
    let review = preview_upgrade(&fixture.target).unwrap().unwrap();
    assert_eq!(
        review
            .artifacts()
            .iter()
            .map(|artifact| artifact.file_name.clone())
            .collect::<BTreeSet<_>>(),
        retired.keys().cloned().collect()
    );
    assert_eq!(fs::read(&account).unwrap(), original_account);
    let report = install_staged_packages(&request(&fixture)).unwrap();
    assert!(
        report.backup_prune_warning.is_none(),
        "Recovery backup retention is successful installation policy"
    );
    let readback = assert_mod_readback(&fixture);
    let backup = assert_retired_backup(&fixture, &retired);
    // Reports name the canonical packages folder.
    let target = fs::canonicalize(&fixture.target).unwrap();
    for name in retired.keys() {
        assert!(
            report
                .removed_obsolete_packages
                .contains(&target.join(name)),
            "{name} is reported as retired"
        );
    }
    assert_eq!(compliance::inventory(&fixture.target), directory_before);
    assert_eq!(fs::read(&account).unwrap(), original_account);
    assert_eq!(fixture.source_artifact_json(), stock);
    assert!(preview_upgrade(&fixture.target).unwrap().is_none());
    assert_loader_manifest(&report.mod_directory.clone().unwrap());
    verify_update_and_removal(&mut fixture, &account, &original_account, &stock);
    assert_eq!(fs::read(external_dll).unwrap(), runtime_before);
    assert_eq!(compliance::inventory(&fixture.target), directory_before);
    if schema == 18 || schema == 3 {
        assert_sqlite_account_kept(&account);
    }
    json!({"account_schema": schema, "stock": stock, "readback": readback,
        "retired": retired.iter().map(|(name, bytes)| json!({"file": name, "sha256": hex::encode(Sha256::digest(bytes))})).collect::<Vec<_>>(),
        "retired_backup": backup.file_name().unwrap().to_string_lossy(),
        "runtime_sha256": hex::encode(Sha256::digest(runtime_before)), "runtime_link": "external and preserved",
        "account_sha256": hex::encode(Sha256::digest(&original_account)), "repeat_review": "no action", "update": "mod folder", "uninstall": "stock and account preserved"})
}

/// The published manifest holds only loader fields: Parhelion's ID, this release as the loader's
/// `MAJOR.MINOR.PATCH`, and the author and source the Mods page shows.
fn assert_loader_manifest(directory: &Path) {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("mod.json")).unwrap()).unwrap();
    let release = [
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
    ]
    .join(".");
    assert_eq!(
        manifest,
        json!({
            "manifestVersion": 3,
            "id": mod_content::ID,
            "version": release,
            "name": "Parhelion",
            "authors": ["KyleThmpsn"],
            "src": "https://github.com/KyleThmpsn/sundial",
        })
    );
}

/// A SQLite account keeps its migration extension and its items.
fn assert_sqlite_account_kept(account: &Path) {
    let db = rusqlite::Connection::open(account).unwrap();
    if db
        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
        .unwrap()
        == 3
    {
        assert_eq!(
            db.query_row(
                "SELECT appearance_header FROM characters WHERE slot=0",
                [],
                |r| r.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
            vec![0x5a; 36]
        );
        assert_eq!(
            db.query_row("SELECT level FROM starter_character", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            42
        );
    }

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

/// Runs `request`'s install until it reaches `stop`, where the process stops.
fn interrupt_install(request: &InstallRequest, stop: &str) {
    let reached = std::cell::Cell::new(false);
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend::install_with_observer(request, &mut |_| {}, &mut |phase| {
            if phase == stop {
                reached.set(true);
                panic!("simulated process stop at {phase}");
            }
        })
        .unwrap();
    }));
    assert!(stopped.is_err());
    assert!(reached.get(), "the operation did not reach {stop}");
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
fn review_gates_on_the_active_runtime_and_install_stops_before_mutation() {
    let fixture = fixture();
    let retired = legacy(&fixture);
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
    assert!(preview_upgrade(&fixture.target).unwrap().is_some());
    let mut running = request(&fixture);
    running.game_running_check = game_running;
    assert!(install_staged_packages(&running).is_err());
    assert!(!fixture.backups.exists());
    fs::write(root.join("steam_api64.dll"), b"unrecognized root DLL").unwrap();
    assert!(preview_upgrade(&fixture.target).is_err());
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert!(!mod_content::directory(root).exists());
    fs::remove_file(root.join("steam_api64.dll")).unwrap();
    for (name, bytes) in &retired {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
}

#[test]
fn a_build_whose_patch_adds_entries_never_reaches_the_mod_folder() {
    let mut fixture = fixture();
    let retired = legacy(&fixture);
    let profile = CANONICAL_PACKAGES[0];
    let stock = fixture
        .target
        .join(profile.stock_file_name(profile.stock_patch_id));
    let view = tempfile::tempdir().unwrap();
    fs::copy(&stock, view.path().join(stock.file_name().unwrap())).unwrap();
    let appended = crate::extend::build_extended_overlay(
        view.path(),
        profile.package_id,
        &[],
        &[crate::NewTagSpec {
            template_tag: tiger_pkg::TagHash::new(profile.package_id, 0),
            payload: b"appended record".to_vec(),
            storage: crate::NewTagStorageMode::InheritTemplate,
        }],
    )
    .unwrap();
    fs::write(
        fixture.staging.join(profile.authored_file_name),
        appended.bytes(),
    )
    .unwrap();
    fixture
        .staged_bytes
        .insert(profile.authored_file_name.into(), appended.bytes().to_vec());
    write_manifest(&fixture);
    let error = install_staged_packages(&request(&fixture)).unwrap_err();
    assert!(
        error.message.contains("refuses a patch that adds entries"),
        "{}",
        error.message
    );
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    assert!(!fixture.backups.exists());
    for (name, bytes) in &retired {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
}

#[test]
fn a_staged_patch_with_changed_inherited_blocks_is_refused_before_install() {
    let fixture = fixture();
    let retired = legacy(&fixture);
    let profile = CANONICAL_PACKAGES[0];
    let path = fixture.staging.join(profile.authored_file_name);
    let mut bytes = fs::read(&path).unwrap();
    let layout = PackageLayout::parse(&bytes).unwrap();
    bytes[layout.block_table_offset] ^= 1;
    layout.update_package_tables_hash(&mut bytes).unwrap();
    fs::write(&path, bytes).unwrap();
    let mut fixture = fixture;
    fixture
        .staged_bytes
        .insert(profile.authored_file_name.into(), fs::read(&path).unwrap());
    write_manifest(&fixture);
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert!(!fixture.backups.exists());
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    for (name, bytes) in &retired {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
}

#[test]
fn interrupted_install_retiring_an_earlier_layout_recovers_every_boundary() {
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
        let retired = legacy(&fixture);
        interrupt_install(&request(&fixture), stop);
        // Without an account change, every boundary finishes forward.
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert_mod_readback(&fixture);
        assert_retired_backup(&fixture, &retired);
        assert!(preview_upgrade(&fixture.target).unwrap().is_none());
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert!(matches!(
            recover_interrupted_install(&recovery(&fixture)).unwrap(),
            RecoveryOutcome::NoTransaction
        ));
        evidence.push(
            json!({"interruption": stop, "recovery": "finished forward", "repeat": "verified"}),
        );
    }
    crate::test_support::artifact("sunrise-mod-upgrade-recovery.json", &evidence);
}

/// An account change is written before any package moves. Interrupted before it, recovery
/// restores the account and keeps the earlier layout. Interrupted after it, recovery publishes
/// the build and retires the earlier layout with the account change kept. The account is the
/// SQLite source that Sunrise 0.6 uses, holding an item whose definition the build removes.
#[test]
fn interrupted_install_with_an_account_change_recovers_on_the_side_it_reached() {
    // The first equipped definition of the SQLite defaults, which every character holds.
    const REMOVED: u32 = 3_843_477_312;
    let mut evidence = Vec::new();
    for (schema, stop, forward) in [
        (18, "journaled", false),
        (18, "published", true),
        (3, "journaled", false),
        (3, "published", true),
    ] {
        let fixture = fixture();
        let stock = compliance::inventory(&fixture.target);
        let (path, _) = account(&fixture, schema);
        let held = item_count(&path, REMOVED);
        assert!(held > 0);
        let review =
            crate::install::replacement::test_review(&fixture.target, BTreeSet::from([REMOVED]));
        assert!(
            review
                .account_cleanup()
                .unwrap()
                .removed_items
                .contains_key(&REMOVED)
        );
        let retired = legacy(&fixture);
        let mut confirmed = request(&fixture);
        confirmed.confirmed_replacement = Some(review);
        interrupt_install(&confirmed, stop);
        let outcome = recover_interrupted_install(&recovery(&fixture)).unwrap();
        let settled = if forward {
            assert!(matches!(outcome, RecoveryOutcome::CommittedTransaction));
            assert_eq!(item_count(&path, REMOVED), 0);
            assert_mod_readback(&fixture);
            assert_eq!(compliance::inventory(&fixture.target), stock);
            "finished forward"
        } else {
            assert!(matches!(outcome, RecoveryOutcome::Recovered { .. }));
            assert_eq!(item_count(&path, REMOVED), held);
            assert_earlier_layout_kept(&fixture, &retired);
            "account restored, earlier layout kept"
        };
        assert!(matches!(
            recover_interrupted_install(&recovery(&fixture)).unwrap(),
            RecoveryOutcome::NoTransaction
        ));
        assert_sqlite_account_kept(&path);
        evidence.push(
            json!({"schema":schema,"interruption": stop, "recovery": settled,
            "account_sha256": hex::encode(Sha256::digest(fs::read(&path).unwrap()))}),
        );
    }
    crate::test_support::artifact("sunrise-mod-upgrade-account-recovery.json", &evidence);
}

/// Nothing reached the mod folder, every earlier package is in place and the review offers the
/// upgrade again.
fn assert_earlier_layout_kept(fixture: &Fixture, retired: &BTreeMap<String, Vec<u8>>) {
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    for (name, bytes) in retired {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
    assert!(preview_upgrade(&fixture.target).unwrap().is_some());
}

#[test]
fn recovery_keeps_concurrent_files_and_install_refuses_conflicting_mods() {
    let fixture = fixture();
    let retired = legacy(&fixture);
    let root = fixture.target.parent().unwrap();
    let foreign = root.join("bin/x64/Sunrise/mods/dev.other.content/packages");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join(crate::package_profile::PARHELION_ASSET_FILE_NAME),
        b"foreign package",
    )
    .unwrap();
    assert!(install_staged_packages(&request(&fixture)).is_err());
    fs::remove_file(foreign.join(crate::package_profile::PARHELION_ASSET_FILE_NAME)).unwrap();
    let reached = std::cell::Cell::new(false);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut |phase| {
            if phase == "published" {
                reached.set(true);
                panic!("simulated process stop");
            }
        })
        .unwrap();
    }));
    assert!(reached.get(), "the install did not publish its folder");
    let changed = mod_content::directory(root)
        .join("packages")
        .join(crate::package_profile::PARHELION_ASSET_FILE_NAME);
    fs::write(&changed, b"concurrent content").unwrap();
    assert!(recover_interrupted_install(&recovery(&fixture)).is_err());
    assert_eq!(fs::read(changed).unwrap(), b"concurrent content");
    for (name, bytes) in &retired {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
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
            install_staged_packages(&request(&fixture)).unwrap();
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

/// Fails an install on a runtime without the mod loader after it retired the mod folder, at the
/// last runtime check, or after `fail_after_commits` packages with a rollback, and returns the mod
/// folder it left. Stock `packages` must be unchanged and no working folder left beside the mods.
fn failed_install_restores(
    fixture: &Fixture,
    fail_after_commits: Option<usize>,
    stock: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let root = fixture.target.parent().unwrap();
    let mut failing = request(fixture);
    if fail_after_commits.is_none() {
        failing.runtime_feature_check = |packages| {
            let mods = mod_content::directory(packages.parent().unwrap());
            if mods.exists() {
                Ok(())
            } else {
                Err("the mod folder is gone".into())
            }
        };
    }
    let error =
        install_staged_packages_inner(&failing, fail_after_commits, DEFAULT_CACHE_INVALIDATION_OPS)
            .unwrap_err();
    let expected = if fail_after_commits.is_some() {
        assert!(error.rollback.as_ref().unwrap().succeeded(), "{error}");
        "Injected installation failure"
    } else {
        "Runtime check failed"
    };
    assert!(error.message.contains(expected), "{}", error.message);
    let mods = mod_content::directory(root);
    let leftovers: Vec<_> = fs::read_dir(mods.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".parhelion-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(&compliance::inventory(&fixture.target), stock);
    tree(&mods)
}

/// On `brand` `version`, which doesn't load mods, readers don't see `added` from the installed
/// mod folder, nothing offers an upgrade, and removal reviews the folder's packages.
fn inactive_runtime_ignores_the_mod_folder(
    fixture: &Fixture,
    brand: &str,
    version: [u16; 4],
    added: tiger_pkg::TagHash,
) {
    switch_runtime(fixture, brand, version);
    let reader =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert!(reader.get_entry(added).is_none(), "{brand}");
    assert!(preview_upgrade(&fixture.target).unwrap().is_none());
    let removal = preview_uninstall(&fixture.target).unwrap();
    assert_eq!(removal.artifacts().len(), fixture.staged_bytes.len());
}

#[test]
fn switching_runtimes_keeps_reading_installing_and_removing_working() {
    let fixture = fixture();
    let root = fixture.target.parent().unwrap().to_path_buf();
    let stock = compliance::inventory(&fixture.target);
    let added = tiger_pkg::TagHash::new(0x0AA1, 0);
    install_staged_packages(&request(&fixture)).unwrap();
    let installed = tree(&mod_content::directory(&root));
    // Dawn and Sunrise before 0.6 load stock `packages` alone, so readers see no mod content,
    // there is nothing to upgrade, and removal reviews the mod folder.
    for (brand, version) in [("Dawn", [0, 1, 3, 0]), ("Sunrise", [0, 5, 1, 0])] {
        inactive_runtime_ignores_the_mod_folder(&fixture, brand, version, added);
    }
    // An install there that fails after retiring the mod folder, at its last runtime check or by
    // rolling back after publishing a package, puts the folder back and leaves no working copies
    // beside it.
    switch_runtime(&fixture, "Dawn", [0, 1, 3, 0]);
    let restored = [None, Some(1)]
        .map(|fail_after_commits| failed_install_restores(&fixture, fail_after_commits, &stock));
    assert!(restored.iter().all(|folder| *folder == installed));
    let [_, restored] = restored;
    // There an install goes to `packages` and retires the mod folder, with a backup, so it can't
    // load beside the new packages after a switch back. The other runtime's cache is cleared.
    let sunrise_cache = root.join("bin/x64/Sunrise/cache/build_data.bin");
    fs::create_dir_all(sunrise_cache.parent().unwrap()).unwrap();
    fs::write(&sunrise_cache, b"built while the mod folder loaded").unwrap();
    switch_runtime(&fixture, "Dawn", [0, 1, 3, 0]);
    let report = install_staged_packages(&request(&fixture)).unwrap();
    assert!(!mod_content::directory(&root).exists());
    for (name, bytes) in &fixture.staged_bytes {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
    let backup = report
        .retired_mod_backup
        .expect("the install names the retired mod folder's backup");
    assert_eq!(tree(&backup.join("mods").join(mod_content::ID)), installed);
    assert!(!sunrise_cache.exists());
    let reader =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert!(reader.get_entry(added).is_some());
    drop(reader);
    // Back on Sunrise 0.6, the review offers the upgrade, and an install returns the build to the
    // mod folder.
    switch_runtime(&fixture, "Sunrise", [0, 6, 0, 0]);
    assert!(preview_upgrade(&fixture.target).unwrap().is_some());
    install_staged_packages(&request(&fixture)).unwrap();
    let readback = assert_mod_readback(&fixture);
    // Removal on Dawn takes the mod folder out too, so it doesn't return on Sunrise 0.6.
    switch_runtime(&fixture, "Dawn", [0, 1, 3, 0]);
    let removal = preview_uninstall(&fixture.target).unwrap();
    uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
    assert!(!mod_content::directory(&root).exists());
    assert_eq!(compliance::inventory(&fixture.target), stock);
    switch_runtime(&fixture, "Sunrise", [0, 6, 0, 0]);
    assert!(mod_content::View::open(&root).unwrap().is_none());
    crate::test_support::artifact(
        "sunrise-runtime-switching.json",
        &json!({
            "runtimes": ["Sunrise 0.6.0.0", "Dawn 0.1.3.0", "Sunrise 0.5.1.0"],
            "mod_folder": installed, "mod_folder_after_failed_install": restored,
            "retired_mod_backup": backup, "mod_readback": readback,
            "stock_after_removal": stock, "native_loader": "not executed"
        }),
    );
}

#[test]
fn a_build_for_the_mod_folder_does_not_install_where_mods_do_not_load() {
    let mut fixture = fixture();
    let records = "w64_parhelion_investment_0aa2_0.pkg";
    let bytes = build_test_package_with_physical_payload(0x0AA2, 0, b"records").unwrap();
    fs::write(fixture.staging.join(records), &bytes).unwrap();
    fixture.staged_bytes.insert(records.into(), bytes);
    write_manifest(&fixture);
    let stock = compliance::inventory(&fixture.target);
    for (brand, version) in [("Dawn", [0, 1, 3, 0]), ("Sunrise", [0, 5, 1, 0])] {
        switch_runtime(&fixture, brand, version);
        let error = install_staged_packages(&request(&fixture)).unwrap_err();
        assert!(error.message.contains("mod folder"), "{error}");
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert!(!fixture.backups.exists());
    }
    switch_runtime(&fixture, "Sunrise", [0, 6, 0, 0]);
    install_staged_packages(&request(&fixture)).unwrap();
    let root = fixture.target.parent().unwrap();
    let installed = mod_content::directory(root).join("packages").join(records);
    assert_eq!(fs::read(installed).unwrap(), fixture.staged_bytes[records]);
}
