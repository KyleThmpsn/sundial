//! Family lifecycle failures were inventoried before implementation in the task's investigation
//! record. These checks cross staging validation, publication, readers and recovery. They use
//! real package files and persisted accounts, and retain independent entry readback.
use super::*;

const REACH: &str = "dev.kylethmpsn.parhelion-halo_reach";
const D2: &str = "dev.kylethmpsn.parhelion-d2_mot";
const REACH_FILE: &str = "w64_parhelion_assets_0aa1_0.pkg";
const D2_FILE: &str = "w64_parhelion_assets_0cff_0.pkg";

fn directory(fixture: &Fixture, id: &str) -> PathBuf {
    mod_content::runtime_directory(fixture.target.parent().unwrap())
        .join("mods")
        .join(id)
}

fn split(fixture: &Fixture, kept: &[&str], omit: Option<&str>) {
    let path = fixture.staging.join(MANIFEST_FILE_NAME);
    let mut build: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut members = Vec::new();
    let mut dependencies = Vec::new();
    for (id, name, file) in [
        (D2, "Parhelion Destiny 2: Monument of Triumph", D2_FILE),
        (REACH, "Parhelion Halo: Reach", REACH_FILE),
    ] {
        if omit == Some(id) {
            build["artifacts"]
                .as_array_mut()
                .unwrap()
                .retain(|artifact| artifact["file_name"] != file);
            continue;
        }
        let artifact = build["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["file_name"] == file)
            .unwrap();
        let mut manifest = serde_json::to_value(mod_content::Manifest::default()).unwrap();
        manifest["id"] = id.into();
        manifest["name"] = name.into();
        let digest = artifact["sha256"].as_str().unwrap();
        let receipt = json!({
            "schema": 1,
            "mod_id": id,
            "artifacts": [{
                "path": format!("packages/{file}"),
                "byte_length": artifact["byte_length"],
                "sha256": digest,
            }],
            "content": {
                "fingerprint": digest,
                "inputs": digest,
                "allocation": [if id == REACH { 0x0aa1 } else { 0x0cff }],
                "state": {}
            }
        });
        dependencies.push(json!({
            "id": id, "minVersion": manifest["version"], "maxVersion": manifest["version"],
        }));
        members.push(json!({"manifest":manifest,"receipt":receipt,"kept":kept.contains(&id)}));
    }
    let mut manifest = serde_json::to_value(mod_content::Manifest::default()).unwrap();
    manifest["dependencies"] = dependencies.into();
    let artifacts = build["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|artifact| {
            let file = artifact["file_name"].as_str().unwrap();
            file != REACH_FILE && file != D2_FILE
        })
        .map(|artifact| {
            let file = artifact["file_name"].as_str().unwrap();
            let profile = authored_package_for_file_name(file).unwrap();
            let folder = if profile.stock_overlay {
                "patches"
            } else {
                "packages"
            };
            json!({
                "path": format!("{folder}/{file}"),
                "byte_length": artifact["byte_length"],
                "sha256": artifact["sha256"],
            })
        })
        .collect::<Vec<_>>();
    members.insert(
        0,
        json!({
            "manifest": manifest,
            "receipt": {"schema":1,"mod_id":mod_content::ID,"artifacts":artifacts},
            "kept": false,
        }),
    );
    build["mods"] = members.into();
    fs::write(path, serde_json::to_vec_pretty(&build).unwrap()).unwrap();
}

fn entries(fixture: &Fixture) -> serde_json::Value {
    let root = fixture.target.parent().unwrap();
    let files = mod_content::files(root).unwrap();
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    let mut readback = Vec::new();
    for (id, file) in [(0x0aa1, REACH_FILE), (0x0cff, D2_FILE)] {
        if !files
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == file))
        {
            continue;
        }
        let payload = manager.read_tag(tiger_pkg::TagHash::new(id, 0)).unwrap();
        let path = files.iter().find(|path| path.ends_with(file)).unwrap();
        assert_eq!(
            payload,
            PackageD2PreBL::open(path.to_str().unwrap())
                .unwrap()
                .read_entry(0)
                .unwrap()
        );
        readback.push(json!({
            "file": file, "id": id, "entry_zero": hex::encode(payload),
            "sha256": digest_file(path).unwrap().sha256
        }));
    }
    let notices = mod_content::notices(root).unwrap().unwrap_or_default();
    assert!(
        !notices.iter().any(|notice| notice.contains("was left out")),
        "{notices:?}"
    );
    json!({"entries":readback,"loader_notices":notices})
}

#[test]
#[allow(
    clippy::cognitive_complexity,
    reason = "One ordered install lifecycle verifies persisted JSON and SQLite accounts"
)]
fn family_migration_keep_change_retire_and_remove_preserve_both_account_backends() {
    let mut results = Vec::new();
    for schema in [8, 18] {
        let mut fixture = fixture();
        let (account, original_account) = account(&fixture, schema);
        let stock = compliance::inventory(&fixture.target);
        // The first installation is the shipped single-mod layout.
        install_staged_packages(&request(&fixture)).unwrap();
        let monolith = tree(&directory(&fixture, mod_content::ID));
        split(&fixture, &[], None);
        let migrated = install_staged_packages(&request(&fixture)).unwrap();
        assert_eq!(
            tree(&migrated.backup_directory.join("mods").join(mod_content::ID)),
            monolith
        );
        for (id, file) in [(REACH, REACH_FILE), (D2, D2_FILE)] {
            assert!(
                directory(&fixture, id)
                    .join("packages")
                    .join(file)
                    .is_file()
            );
            assert!(
                !directory(&fixture, mod_content::ID)
                    .join("packages")
                    .join(file)
                    .exists()
            );
            assert!(!directory(&fixture, id).join("patches").exists());
        }
        let main: serde_json::Value = mod_content::read_json(
            &directory(&fixture, mod_content::ID).join(mod_content::MANIFEST),
        )
        .unwrap();
        assert_eq!(main["dependencies"].as_array().unwrap().len(), 2);
        let initial = entries(&fixture);
        let kept_file = directory(&fixture, D2).join("packages").join(D2_FILE);
        let handle = same_file::Handle::from_path(&kept_file).unwrap();
        let modified = fs::metadata(&kept_file).unwrap().modified().unwrap();
        let d2_before = tree(&directory(&fixture, D2));
        split(&fixture, &[REACH, D2], None);
        fs::remove_file(fixture.staging.join(REACH_FILE)).unwrap();
        fs::remove_file(fixture.staging.join(D2_FILE)).unwrap();
        let repeated = install_staged_packages(&request(&fixture)).unwrap();
        assert_eq!(handle, same_file::Handle::from_path(&kept_file).unwrap());
        assert_eq!(
            modified,
            fs::metadata(&kept_file).unwrap().modified().unwrap()
        );
        assert_eq!(d2_before, tree(&directory(&fixture, D2)));
        assert!(!repeated.backup_directory.join("mods").join(D2).exists());

        let changed =
            build_test_package_with_physical_payload(0x0aa1, 0, b"changed Reach").unwrap();
        fs::write(fixture.staging.join(REACH_FILE), &changed).unwrap();
        fixture.staged_bytes.insert(REACH_FILE.into(), changed);
        // Metadata is recorded from the generation, including the kept D2 member.
        let path = fixture.staging.join(MANIFEST_FILE_NAME);
        let mut build: serde_json::Value = mod_content::read_json(&path).unwrap();
        let digest = digest_file(&fixture.staging.join(REACH_FILE)).unwrap();
        let artifact = build["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["file_name"] == REACH_FILE)
            .unwrap();
        artifact["byte_length"] = digest.byte_length.into();
        artifact["sha256"] = digest.sha256.into();
        fs::write(&path, serde_json::to_vec_pretty(&build).unwrap()).unwrap();
        split(&fixture, &[D2], None);
        install_staged_packages(&request(&fixture)).unwrap();
        assert_eq!(handle, same_file::Handle::from_path(&kept_file).unwrap());
        assert_eq!(d2_before, tree(&directory(&fixture, D2)));
        let changed = entries(&fixture);
        let retiring = tree(&directory(&fixture, REACH));
        split(&fixture, &[D2], Some(REACH));
        fs::remove_file(fixture.staging.join(REACH_FILE)).unwrap();
        let removed = install_staged_packages(&request(&fixture)).unwrap();
        assert!(!directory(&fixture, REACH).exists());
        assert_eq!(
            tree(&removed.backup_directory.join("mods").join(REACH)),
            retiring
        );
        assert_eq!(handle, same_file::Handle::from_path(&kept_file).unwrap());
        assert_eq!(fs::read(&account).unwrap(), original_account);
        assert_eq!(compliance::inventory(&fixture.target), stock);
        let final_readback = entries(&fixture);
        // Windows keeps a file open for identity comparison. Release it before removal.
        drop(handle);
        let plan = preview_uninstall(&fixture.target)
            .unwrap()
            .without_account_cleanup();
        uninstall_custom_packages(&plan, &fixture.backups, game_stopped).unwrap();
        assert!(!directory(&fixture, mod_content::ID).exists());
        assert!(!directory(&fixture, D2).exists());
        results.push(json!({
            "account_schema":schema,"initial":initial,"changed":changed,"retired":final_readback,
            "kept_file_identity":true,"kept_file_write_time":true,
            "stock_unchanged":true,"account_unchanged":true
        }));
    }
    crate::test_support::artifact(
        "importer-mod-family-lifecycle.json",
        &json!({
            "cases":results,
            "native_loader":"not executed",
            "recipe":"install::tests::loader::family"
        }),
    );
}

#[test]
fn interrupted_family_publication_recovers_every_member_and_keeps_other_mods() {
    let boundaries = [
        "journaled".to_owned(),
        "verified".into(),
        "committed".into(),
    ]
    .into_iter()
    .chain(
        [mod_content::ID, D2, REACH]
            .into_iter()
            .flat_map(|id| [format!("retired:{id}"), format!("published:{id}")]),
    );
    let mut evidence = Vec::new();
    for boundary in boundaries {
        let mut fixture = fixture();
        account(&fixture, 18);
        split(&fixture, &[], None);
        install_staged_packages(&request(&fixture)).unwrap();
        let root = directory(&fixture, mod_content::ID)
            .parent()
            .unwrap()
            .to_path_buf();
        let foreign = root.join("org.example.fixture");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("mod.json"), br#"{"manifestVersion":3,"id":"org.example.fixture","version":"1.0.0","name":"Fixture"}"#).unwrap();
        fs::write(foreign.join("editor-note.txt"), b"owned by another editor").unwrap();
        let previous = tree(&root);
        change_assets(&mut fixture);
        for (id, file) in [(0x0aa1, REACH_FILE), (0x0cff, D2_FILE)] {
            let bytes =
                build_test_package_with_physical_payload(id, 0, b"replacement importer payload")
                    .unwrap();
            fs::write(fixture.staging.join(file), &bytes).unwrap();
            fixture.staged_bytes.insert(file.into(), bytes);
        }
        write_manifest(&fixture);
        split(&fixture, &[], None);
        let mut seen = 0;
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut |step| {
                if step == boundary {
                    seen += 1;
                    panic!("fixture interruption");
                }
            })
        }));
        assert!(failure.is_err(), "boundary {boundary} was not reached");
        assert_eq!(seen, 1);
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        if boundary != "committed" {
            assert_eq!(
                tree(&root),
                previous,
                "{boundary} restored the whole family"
            );
        }
        evidence.push(json!({"boundary":boundary, "readback":entries(&fixture), "rolled_back":boundary != "committed"}));
        assert!(
            !mod_content::runtime_directory(fixture.target.parent().unwrap())
                .join(mod_content::JOURNAL)
                .exists()
        );
    }
    crate::test_support::artifact(
        "importer-mod-family-recovery.json",
        &json!({"boundaries":evidence}),
    );
}

#[test]
fn a_kept_importer_changed_after_journaling_is_preserved_and_blocks_recovery() {
    let fixture = fixture();
    account(&fixture, 8);
    split(&fixture, &[], None);
    install_staged_packages(&request(&fixture)).unwrap();
    split(&fixture, &[D2, REACH], None);
    fs::remove_file(fixture.staging.join(REACH_FILE)).unwrap();
    fs::remove_file(fixture.staging.join(D2_FILE)).unwrap();
    let path = directory(&fixture, D2).join("packages").join(D2_FILE);
    let failure = backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut |step| {
        if step == "journaled" {
            fs::write(&path, b"concurrent edit").unwrap();
        }
    });
    assert!(failure.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"concurrent edit");
    assert!(recover_interrupted_install(&recovery(&fixture)).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"concurrent edit");
}

#[test]
fn missing_importer_refuses_the_main_mod_and_switching_runtime_retires_the_family() {
    let mut fixture = fixture();
    account(&fixture, 18);
    split(&fixture, &[], None);
    install_staged_packages(&request(&fixture)).unwrap();
    let importer = directory(&fixture, D2);
    let hidden = importer.with_file_name(".fixture-hidden-importer");
    fs::rename(&importer, &hidden).unwrap();
    let view = mod_content::View::open(fixture.target.parent().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        !view
            .packages()
            .join(crate::package_profile::PARHELION_ASSET_FILE_NAME)
            .exists()
    );
    drop(view);
    let notices = mod_content::notices(fixture.target.parent().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        notices
            .iter()
            .any(|notice| notice.starts_with(mod_content::ID) && notice.contains(D2))
    );
    fs::rename(&hidden, &importer).unwrap();
    switch_runtime(&fixture, "Dawn", [1, 7, 2, 0]);
    let replacement = legacy(&fixture);
    for (name, bytes) in &replacement {
        // This helper also installs its legacy fixture. Runtime switching starts with only
        // the loader family installed, so retain the replacement in staging alone.
        fs::remove_file(fixture.target.join(name)).unwrap();
        fs::write(fixture.staging.join(name), bytes).unwrap();
    }
    fixture.staged_bytes = replacement;
    write_manifest(&fixture);
    let installed = install_staged_packages(&request(&fixture)).unwrap();
    let backup = installed
        .retired_mod_backup
        .expect("the inactive family was backed up");
    for id in [mod_content::ID, D2, REACH] {
        assert!(!directory(&fixture, id).exists());
        assert!(backup.join("mods").join(id).is_dir());
    }
}

#[test]
fn both_backup_layouts_restore_the_complete_previous_family() {
    let mut evidence = Vec::new();
    for old_layout in [false, true] {
        let mut fixture = fixture();
        account(&fixture, 18);
        split(&fixture, &[], None);
        install_staged_packages(&request(&fixture)).unwrap();
        let mods = directory(&fixture, mod_content::ID)
            .parent()
            .unwrap()
            .to_owned();
        let previous = tree(&mods);
        let previous_reach = tree(&directory(&fixture, REACH));
        change_assets(&mut fixture);
        let reach = build_test_package_with_physical_payload(0x0aa1, 0, b"changed Reach").unwrap();
        fs::write(fixture.staging.join(REACH_FILE), &reach).unwrap();
        fixture.staged_bytes.insert(REACH_FILE.into(), reach);
        write_manifest(&fixture);
        split(&fixture, &[], None);
        let journal = mod_content::runtime_directory(fixture.target.parent().unwrap())
            .join(mod_content::JOURNAL);
        let mut interrupted = false;
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut |step| {
                if step == "journaled" {
                    let mut record: serde_json::Value = mod_content::read_json(&journal).unwrap();
                    let backup =
                        PathBuf::from(record["context"]["backup_directory"].as_str().unwrap());
                    for id in [mod_content::ID, D2, REACH] {
                        assert_eq!(
                            tree(&backup.join("mods").join(id)),
                            tree(&directory(&fixture, id))
                        );
                    }
                    if old_layout {
                        fs::rename(
                            backup.join("mods").join(mod_content::ID),
                            backup.join("mod"),
                        )
                        .unwrap();
                        record["schema"] = 1.into();
                        let bytes = serde_json::to_vec_pretty(&record).unwrap();
                        fs::write(&journal, &bytes).unwrap();
                        fs::write(backup.join("mod-recovery.json"), &bytes).unwrap();
                    }
                }
                if step == format!("published:{REACH}") {
                    assert_ne!(tree(&directory(&fixture, REACH)), previous_reach);
                    let record: serde_json::Value = mod_content::read_json(&journal).unwrap();
                    let token = record["token"].as_str().unwrap();
                    // Force recovery to read the saved backup instead of the retired folder.
                    fs::remove_dir_all(mods.join(format!(".{REACH}-retired-{token}"))).unwrap();
                    interrupted = true;
                    panic!("interrupt after replacing the family");
                }
            })
        }));
        assert!(failure.is_err());
        assert!(interrupted, "publication boundary was not reached");
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert_eq!(tree(&mods), previous);
        evidence.push(json!({"legacy_layout":old_layout,"readback":entries(&fixture)}));
    }
    crate::test_support::artifact(
        "mod-backup-layout-recovery.json",
        &json!({"cases":evidence}),
    );
}
