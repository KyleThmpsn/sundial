//! Filesystem lifecycle beside packages emitted by SunriseStudio 007dd7a6.
//! Failure inventory recorded before the implementation:
//! - Different entries in one package are incorrectly treated as a conflict.
//! - Shared entries replace complete tables or disable Studio's row merge.
//! - A sidecar is mistaken for a guarantee that both editors' rows survive.
//! - A mod arrives after review and bypasses package ownership or overlap checks.
//! - Recovery publishes the same unsafe combination after an interrupted upgrade.
//! - A mod declaring Parhelion incompatible is disabled by our install.
//! - Unreadable directories or linked metadata hide occupied package IDs.
//! - Update or removal changes another editor's packages, scripts, configuration or account.
//! - Folder limits or another mod claiming the managed ID block reviewed removal or rollback.
//! - Readers lose disjoint entries, omit standalone mod packages or keep stale content after removal.
//! - A refused or hidden mod contributes content that the native resolver would not load.
//! - Startup config reads the unconfigured payload, stale saved values or an invalid scalar site.
//!
//! Failure inventory recorded before readers followed the loader's admission of other mods:
//! - A mod the loader refuses, an entry two mods change, or a folder inside a mod's `patches`
//!   stops every reader instead of affecting only what the loader leaves out.
//! - An entry two mods change is read from the one that loads first.
//! - A mod whose configuration doesn't fit its content is read with or without its values,
//!   where the loader leaves the whole mod out.
//! - What readers leave out is never reported.
//!
//! Failure inventory recorded before the merge cases were written:
//! - A build ships merge data for some packages and not others, so Sunrise ignores all of it.
//! - An entry the merge reads sits in a compressed block, so Sunrise ignores all merge data.
//! - Merge data binds to another file's header or isn't installed beside its package.
//! - Installation accepts a shared entry Sunrise serves from one mod, or refuses one it merges.
//! - Two mods' edits to one stock word are accepted although Sunrise keeps only one.
//! - Readers show one mod's version of an entry Sunrise merges, or keep it after removal.
//! - Removal leaves merge data behind or changes the other mod's files.
//!
//! The checked-in synthetic binaries are an independent format oracle. Their upstream revision,
//! generation recipe and hashes are in fixtures/studio/provenance.json. No game data is used.
use super::*;

const STOCK: &str = "w64_investment_globals_client_058c_3.pkg";
const PATCH: &str = "w64_investment_globals_client_058c_4.pkg";
const SIDECAR: &str = "w64_investment_globals_client_058c_4.merge";
const FOREIGN: &str = "dev.studio.fixture";

fn studio_fixture() -> Fixture {
    let mut fixture = fixture();
    fs::write(
        fixture.target.join(STOCK),
        include_bytes!("fixtures/studio/w64_investment_globals_client_058c_3.pkg"),
    )
    .unwrap();
    // This fixture's placeholder codec must not be loaded by the package writer.
    let decoder = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/oo2core_3_win64.dll");
    fs::remove_file(&decoder).unwrap();
    let authored = crate::extend::build_extended_overlay(
        &fixture.target,
        0x058c,
        &[crate::ReplacementSpec {
            tag: tiger_pkg::TagHash::new(0x058c, 0),
            payload: b"authored payload".to_vec(),
        }],
        &[],
    )
    .unwrap();
    fs::write(decoder, b"fixture, never loaded").unwrap();
    fs::write(fixture.staging.join(PATCH), authored.bytes()).unwrap();
    fixture
        .staged_bytes
        .insert(PATCH.into(), authored.bytes().to_vec());
    write_manifest(&fixture);
    write_studio(&fixture, false);
    fixture
}

fn studio_directory(fixture: &Fixture) -> PathBuf {
    mod_content::runtime_directory(fixture.target.parent().unwrap())
        .join("mods")
        .join(FOREIGN)
}

fn write_studio(fixture: &Fixture, overlap: bool) {
    let directory = studio_directory(fixture);
    for folder in ["packages", "patches", "scripts", "sdk"] {
        fs::create_dir_all(directory.join(folder)).unwrap();
    }
    fs::write(
        directory.join("mod.json"),
        include_bytes!("fixtures/studio/mod.json"),
    )
    .unwrap();
    fs::write(
        directory.join("packages/w64_sunrise_0aaa_0.pkg"),
        include_bytes!("fixtures/studio/w64_sunrise_0aaa_0.pkg"),
    )
    .unwrap();
    let (patch, sidecar): (&[u8], &[u8]) = if overlap {
        (
            include_bytes!("fixtures/studio/overlap/w64_investment_globals_client_058c_4.pkg"),
            include_bytes!("fixtures/studio/overlap/w64_investment_globals_client_058c_4.merge"),
        )
    } else {
        (
            include_bytes!("fixtures/studio/disjoint/w64_investment_globals_client_058c_4.pkg"),
            include_bytes!("fixtures/studio/disjoint/w64_investment_globals_client_058c_4.merge"),
        )
    };
    fs::write(directory.join("patches").join(PATCH), patch).unwrap();
    fs::write(directory.join("patches").join(SIDECAR), sidecar).unwrap();
    fs::write(
        directory.join("scripts/main.lua"),
        b"return { studio = true }\n",
    )
    .unwrap();
    fs::write(directory.join("sdk/helper.lua"), b"return {}\n").unwrap();
    fs::write(
        directory.join("studio-sources.json"),
        b"{\"owner\":\"studio\"}",
    )
    .unwrap();
}

#[test]
fn studio_disjoint_package_edits_survive_install_update_recovery_and_removal() {
    let mut evidence = Vec::new();
    for schema in [8, 18] {
        let mut fixture = studio_fixture();
        let root = fixture.target.parent().unwrap().to_path_buf();
        let runtime = mod_content::runtime_directory(&root);
        let config = runtime.join("mod_config.json");
        let cache = runtime.join("cache/mods/studio-cache.bin");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&config, b"{\"studio\":{\"enabled\":true}}").unwrap();
        fs::write(&cache, b"loader-owned cache").unwrap();
        let (account, original_account) = account(&fixture, schema);
        let before = tree(&studio_directory(&fixture));
        let stock = compliance::inventory(&fixture.target);
        let report = install_staged_packages(&request(&fixture)).unwrap();
        assert!(report.mod_overlaps.is_empty());
        let readback = assert_mod_readback(&fixture);
        let reader =
            sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
        assert_eq!(
            reader.read_tag(tiger_pkg::TagHash::new(0x058c, 0)).unwrap(),
            b"authored payload"
        );
        assert_eq!(
            reader.read_tag(tiger_pkg::TagHash::new(0x058c, 1)).unwrap(),
            b"Studio edit disjoint"
        );
        let standalone = reader.read_tag(tiger_pkg::TagHash::new(0x0aaa, 0)).unwrap();
        assert!(!standalone.is_empty());
        drop(reader);
        let studio_patch = studio_directory(&fixture).join("patches").join(PATCH);
        let package = PackageD2PreBL::open(studio_patch.to_str().unwrap()).unwrap();
        assert_eq!(package.read_entry(1).unwrap(), b"Studio edit disjoint");
        drop(package);
        change_assets(&mut fixture);
        interrupt_install(&request(&fixture), "published");
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        install_staged_packages(&request(&fixture)).unwrap();
        let removal = preview_uninstall(&fixture.target).unwrap();
        uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
        assert!(!mod_content::directory(&root).exists());
        assert_eq!(tree(&studio_directory(&fixture)), before);
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert_eq!(fs::read(&account).unwrap(), original_account);
        assert_eq!(
            fs::read(config).unwrap(),
            b"{\"studio\":{\"enabled\":true}}"
        );
        assert_eq!(fs::read(cache).unwrap(), b"loader-owned cache");
        evidence.push(
            json!({"schema":schema,"foreign_before_and_after":before,"stock":stock,
            "parhelion_readback":readback,"studio_entry_one":"Studio edit disjoint",
            "account_sha256":hex::encode(Sha256::digest(original_account)),
            "operations":["install","interrupted update","recovery","update","uninstall"]}),
        );
    }
    crate::test_support::artifact(
        "sunrise-studio-coexistence.json",
        &json!({
            "studio_revision":"007dd7a6f10164243369350e63c42168229c35ab",
            "fixture_provenance":serde_json::from_str::<serde_json::Value>(include_str!("fixtures/studio/provenance.json")).unwrap(),
            "cases":evidence,"native_loader":"not executed","combined_gameplay":"not executed"
        }),
    );
}

#[test]
fn studio_overlap_is_refused_with_or_without_merge_data_and_removal_still_works() {
    let mut evidence = Vec::new();
    for sidecar in [false, true] {
        let mut fixture = studio_fixture();
        install_staged_packages(&request(&fixture)).unwrap();
        let own = mod_content::directory(fixture.target.parent().unwrap());
        let previous = tree(&own);
        change_assets(&mut fixture);
        write_studio(&fixture, true);
        if !sidecar {
            fs::remove_file(studio_directory(&fixture).join("patches").join(SIDECAR)).unwrap();
        }
        let foreign = tree(&studio_directory(&fixture));
        let error = install_staged_packages(&request(&fixture)).unwrap_err();
        assert!(error.message.contains(FOREIGN), "{error}");
        assert!(error.message.contains("058C"), "{error}");
        assert_eq!(tree(&own), previous);
        assert_eq!(tree(&studio_directory(&fixture)), foreign);
        let removal = preview_uninstall(&fixture.target).unwrap();
        uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
        assert_eq!(tree(&studio_directory(&fixture)), foreign);
        evidence.push(json!({"merge_sidecar":sidecar,"refusal":error.message,"foreign":foreign}));
    }
    crate::test_support::artifact("sunrise-studio-overlap-refusal.json", &evidence);
}

#[test]
fn studio_changes_during_commit_are_preserved_and_prevent_publication() {
    let mut evidence = Vec::new();
    for conflict in ["overlap", "entry flags", "package id", "incompatible"] {
        let fixture = studio_fixture();
        let stock = compliance::inventory(&fixture.target);
        let mut reached = false;
        let error = backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut |phase| {
            if phase != "journaled" {
                return;
            }
            reached = true;
            match conflict {
                "overlap" => write_studio(&fixture, true),
                "entry flags" => {
                    let path = studio_directory(&fixture).join("patches").join(PATCH);
                    let mut bytes = fs::read(&path).unwrap();
                    let layout = PackageLayout::parse(&bytes).unwrap();
                    // The native loader compares all 16 bytes, including bits omitted by
                    // tiger-pkg's decoded reference/type/placement summary.
                    bytes[layout.entry_table_offset + 4] ^= 1;
                    layout.update_package_tables_hash(&mut bytes).unwrap();
                    fs::write(path, bytes).unwrap();
                }
                "package id" => {
                    fs::write(
                        studio_directory(&fixture).join("packages/w64_sunrise_0aa1_0.pkg"),
                        build_test_package_with_physical_payload(0x0aa1, 0, b"late Studio asset")
                            .unwrap(),
                    )
                    .unwrap();
                }
                _ => {
                    let path = studio_directory(&fixture).join("mod.json");
                    let mut manifest: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    manifest["incompatible"] = json!([mod_content::ID]);
                    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
                }
            }
        })
        .unwrap_err();
        assert!(reached);
        assert!(error.message.contains(FOREIGN), "{error}");
        assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
        assert_eq!(compliance::inventory(&fixture.target), stock);
        let foreign = tree(&studio_directory(&fixture));
        assert_eq!(
            recover_interrupted_install(&recovery(&fixture)).unwrap(),
            RecoveryOutcome::NoTransaction
        );
        assert_eq!(tree(&studio_directory(&fixture)), foreign);
        evidence.push(json!({"conflict":conflict,"refusal":error.message,"foreign":foreign}));
    }
    crate::test_support::artifact("sunrise-studio-concurrent-install.json", &evidence);
}

#[test]
fn interrupted_upgrade_does_not_publish_over_a_late_studio_conflict() {
    let fixture = studio_fixture();
    let retired = legacy(&fixture);
    interrupt_install(&request(&fixture), "journaled");
    write_studio(&fixture, true);
    let foreign = tree(&studio_directory(&fixture));
    let error = recover_interrupted_install(&recovery(&fixture)).unwrap_err();
    assert!(error.message.contains(FOREIGN), "{error}");
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    for (name, bytes) in &retired {
        assert_eq!(&fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
    assert_eq!(tree(&studio_directory(&fixture)), foreign);
    write_studio(&fixture, false);
    recover_interrupted_install(&recovery(&fixture)).unwrap();
    assert_mod_readback(&fixture);
    crate::test_support::artifact(
        "sunrise-studio-upgrade-recovery.json",
        &json!({
            "refusal":error.message,"foreign_preserved":foreign,"resumed_after_conflict_removed":true
        }),
    );
}

#[test]
fn unreadable_studio_package_listing_does_not_look_empty() {
    let fixture = fixture();
    let directory = studio_directory(&fixture);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("mod.json"),
        include_bytes!("fixtures/studio/mod.json"),
    )
    .unwrap();
    // A file in place of the package directory is a deterministic read_dir failure on every OS.
    fs::write(
        directory.join("packages"),
        b"unexpected directory replacement",
    )
    .unwrap();
    let before = tree(&directory);
    let error = install_staged_packages(&request(&fixture)).unwrap_err();
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    assert_eq!(tree(&directory), before);
    crate::test_support::artifact(
        "sunrise-studio-unreadable-listing.json",
        &json!({
            "refusal":error.message,"foreign_preserved":before,"published":false
        }),
    );
}

fn obstruct_publication(fixture: &Fixture, conflict: &str) -> BTreeMap<String, String> {
    let mods = mod_content::runtime_directory(fixture.target.parent().unwrap()).join("mods");
    for index in 0..if conflict == "folder limit" { 64 } else { 1 } {
        let directory = mods.join(format!("dev.external.{index:02}"));
        fs::create_dir_all(&directory).unwrap();
        let id = if conflict == "duplicate id" {
            mod_content::ID.to_owned()
        } else {
            format!("dev.external.{index:02}")
        };
        fs::write(
            directory.join("mod.json"),
            serde_json::to_vec(&json!({
                "manifestVersion":3,"id":id,"version":"1.0.0","name":"External Mod"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            directory.join("external.txt"),
            format!("preserve external {index}"),
        )
        .unwrap();
    }
    tree(&mods)
        .into_iter()
        .filter(|(path, _)| path.starts_with("dev.external."))
        .collect()
}

fn external_files(fixture: &Fixture) -> BTreeMap<String, String> {
    let mods = mod_content::runtime_directory(fixture.target.parent().unwrap()).join("mods");
    tree(&mods)
        .into_iter()
        .filter(|(path, _)| path.starts_with("dev.external."))
        .collect()
}

#[test]
fn external_folder_conflicts_do_not_trap_reviewed_removal_or_rollback() {
    let mut evidence = Vec::new();
    for conflict in ["folder limit", "duplicate id"] {
        let fixture = super::fixture();
        let stock = compliance::inventory(&fixture.target);
        install_staged_packages(&request(&fixture)).unwrap();
        let removal = preview_uninstall(&fixture.target).unwrap();
        let external = obstruct_publication(&fixture, conflict);
        preview_uninstall(&fixture.target).unwrap();
        uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
        assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert_eq!(external_files(&fixture), external);
        assert_eq!(
            recover_interrupted_install(&recovery(&fixture)).unwrap(),
            RecoveryOutcome::NoTransaction
        );
        // The same environment must still prevent a fresh publication.
        assert!(install_staged_packages(&request(&fixture)).is_err());
        assert_eq!(external_files(&fixture), external);
        evidence.push(
            json!({"operation":"removal","conflict":conflict,"external":external,"stock":stock}),
        );

        let fixture = super::fixture();
        let stock = compliance::inventory(&fixture.target);
        let mut external = None;
        let error = backend::install_with_observer(&request(&fixture), &mut |_| {}, &mut |phase| {
            if phase == "journaled" {
                external = Some(obstruct_publication(&fixture, conflict));
            }
        })
        .unwrap_err();
        let external = external.expect("the install must reach its journal boundary");
        assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
        assert_eq!(
            recover_interrupted_install(&recovery(&fixture)).unwrap(),
            RecoveryOutcome::NoTransaction
        );
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert_eq!(external_files(&fixture), external);
        evidence.push(json!({"operation":"rollback","conflict":conflict,"refusal":error.message,"external":external,"stock":stock}));
    }
    crate::test_support::artifact("sunrise-studio-removal-environment.json", &evidence);
}

#[test]
fn independent_studio_content_refreshes_without_a_managed_install() {
    let fixture = studio_fixture();
    let stock = compliance::inventory(&fixture.target);
    let foreign = studio_directory(&fixture);
    let first =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    let stock_entry = first.read_tag(tiger_pkg::TagHash::new(0x058c, 0)).unwrap();
    assert_eq!(
        first.read_tag(tiger_pkg::TagHash::new(0x058c, 1)).unwrap(),
        b"Studio edit disjoint"
    );
    let added = first.read_tag(tiger_pkg::TagHash::new(0x0aaa, 0)).unwrap();
    assert_eq!(first.source_dir(), fixture.target);
    fs::rename(&foreign, foreign.with_file_name(".studio-disabled")).unwrap();
    let second =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert_eq!(
        second.read_tag(tiger_pkg::TagHash::new(0x058c, 0)).unwrap(),
        stock_entry
    );
    assert_ne!(
        second.read_tag(tiger_pkg::TagHash::new(0x058c, 1)).unwrap(),
        b"Studio edit disjoint"
    );
    assert!(
        second
            .get_entry(tiger_pkg::TagHash::new(0x0aaa, 0))
            .is_none()
    );
    assert_eq!(
        first.read_tag(tiger_pkg::TagHash::new(0x058c, 1)).unwrap(),
        b"Studio edit disjoint"
    );
    fs::rename(foreign.with_file_name(".studio-disabled"), &foreign).unwrap();
    let mut manifest: serde_json::Value =
        mod_content::read_json(&foreign.join("mod.json")).unwrap();
    manifest["dependencies"] = json!([{"id":"dev.missing.mod","minVersion":"1.0.0"}]);
    fs::write(
        foreign.join("mod.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let refused =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert!(
        refused
            .get_entry(tiger_pkg::TagHash::new(0x0aaa, 0))
            .is_none()
    );
    assert_eq!(compliance::inventory(&fixture.target), stock);
    crate::test_support::artifact(
        "sunrise-studio-reader.json",
        &json!({
            "studio_revision":"007dd7a6f10164243369350e63c42168229c35ab",
            "entry_one":"Studio edit disjoint", "standalone_sha256":hex::encode(Sha256::digest(added)),
            "stock_preserved":stock,"hidden_and_missing_dependency_excluded":true,"older_reader_retains_content":true
        }),
    );
}

#[test]
fn readers_leave_out_what_the_loader_refuses_and_read_shared_entries_from_the_later_mod() {
    let fixture = studio_fixture();
    install_staged_packages(&request(&fixture)).unwrap();
    let root = fixture.target.parent().unwrap().to_path_buf();
    let mods = mod_content::runtime_directory(&root).join("mods");
    let manifest = |id: &str| {
        serde_json::to_vec(&json!({"manifestVersion":2,"id":id,"version":"1.0.0","name":id}))
            .unwrap()
    };
    // Loads after Parhelion and changes the entry Parhelion changes. The loader skips the folder
    // inside its patches.
    let late = mods.join("org.studio.late");
    fs::create_dir_all(late.join("patches/notes")).unwrap();
    fs::write(late.join("mod.json"), manifest("org.studio.late")).unwrap();
    fs::write(
        late.join("patches").join(PATCH),
        include_bytes!("fixtures/studio/overlap/w64_investment_globals_client_058c_4.pkg"),
    )
    .unwrap();
    // A package the loader can't read refuses only its own mod.
    let broken = mods.join("dev.studio.broken");
    fs::create_dir_all(broken.join("packages")).unwrap();
    fs::write(broken.join("mod.json"), manifest("dev.studio.broken")).unwrap();
    fs::write(
        broken.join("packages/w64_broken_0abc_0.pkg"),
        b"not a package",
    )
    .unwrap();
    let entry = |index| tiger_pkg::TagHash::new(0x058c, index);
    let reader =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert_eq!(reader.read_tag(entry(0)).unwrap(), b"Studio edit overlap");
    assert_eq!(reader.read_tag(entry(1)).unwrap(), b"Studio edit disjoint");
    assert!(
        reader
            .get_entry(tiger_pkg::TagHash::new(0x0aaa, 0))
            .is_some()
    );
    assert!(
        reader
            .get_entry(tiger_pkg::TagHash::new(0x0abc, 0))
            .is_none()
    );
    drop(reader);
    let notices = mod_content::notices(&root).unwrap().unwrap();
    assert!(
        notices.iter().any(|notice| {
            notice.starts_with("dev.studio.broken was left out")
                && notice.contains("w64_broken_0abc_0.pkg")
        }),
        "{notices:?}"
    );
    // Studio's own sidecar for the patch reads, so only its package without one is noted.
    assert!(
        !notices
            .iter()
            .any(|notice| notice.contains("merge data for package 058C was ignored")),
        "{notices:?}"
    );
    let overlap = format!(
        "org.studio.late changes 1 entry in package 058C that {} also changes",
        mod_content::ID
    );
    assert!(
        notices.iter().any(|notice| notice.starts_with(&overlap)),
        "{notices:?}"
    );
    // Without the later mod, Parhelion's version is read again.
    fs::remove_dir_all(&late).unwrap();
    let reader =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert_eq!(reader.read_tag(entry(0)).unwrap(), b"authored payload");
    drop(reader);
    crate::test_support::artifact(
        "sunrise-mod-admission.json",
        &json!({
            "load_order": [mod_content::ID, "dev.studio.broken", "dev.studio.fixture", "org.studio.late"],
            "notices": notices, "entry_zero_with_later_mod": "Studio edit overlap",
            "entry_zero_without_it": "authored payload", "native_loader": "not executed"
        }),
    );
}

#[test]
fn studio_startup_config_reads_saved_values_and_refreshes_without_writing_inputs() {
    let fixture = studio_fixture();
    let root = fixture.target.parent().unwrap();
    let foreign = studio_directory(&fixture);
    let package = foreign.join("packages/w64_sunrise_0aaa_0.pkg");
    let source = include_bytes!("fixtures/studio/configured/w64_sunrise_0aaa_0.pkg");
    fs::write(&package, source).unwrap();
    fs::write(
        foreign.join("mod.json"),
        include_bytes!("fixtures/studio/configured/mod.json"),
    )
    .unwrap();
    let before = tree(&foreign);
    let stock = compliance::inventory(&fixture.target);
    let config = mod_content::runtime_directory(root).join("mod_config.json");
    let tag = tiger_pkg::TagHash::new(0x0aaa, 0);
    let mut evidence = Vec::new();
    let mut previous = None;
    for (saved, expected) in [(json!(17), 17i32), (json!(19), 19), (json!(true), 11)] {
        let bytes = serde_json::to_vec(
            &json!({"version":1,"mods":{"dev.studio.configured":{"limit":saved}}}),
        )
        .unwrap();
        fs::write(&config, &bytes).unwrap();
        let reader =
            sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
        let payload = reader.read_tag(tag).unwrap();
        assert_eq!(
            i32::from_le_bytes(payload[428..432].try_into().unwrap()),
            expected
        );
        if let Some((old, old_expected)) = previous {
            let old: sundial::package_authoring::PackageManager = old;
            assert_eq!(
                i32::from_le_bytes(old.read_tag(tag).unwrap()[428..432].try_into().unwrap()),
                old_expected
            );
        }
        assert_eq!(fs::read(&config).unwrap(), bytes);
        assert_eq!(tree(&foreign), before);
        evidence.push(json!({"saved":saved,"observed":expected,"payload_sha256":hex::encode(Sha256::digest(&payload))}));
        previous = Some((reader, expected));
    }
    assert_eq!(fs::read(package).unwrap(), source);
    assert_eq!(compliance::inventory(&fixture.target), stock);
    let manifest_path = foreign.join("mod.json");
    let mut invalid: serde_json::Value = mod_content::read_json(&manifest_path).unwrap();
    invalid["configBindings"][0]["bindingOffset"] = json!(365);
    fs::write(&manifest_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    // The loader refuses a mod whose binding doesn't fit its content, and readers leave the whole
    // mod out the same way rather than fail.
    let reader =
        sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target).unwrap();
    assert!(reader.get_entry(tag).is_none());
    assert_ne!(
        reader.read_tag(tiger_pkg::TagHash::new(0x058c, 1)).unwrap(),
        b"Studio edit disjoint"
    );
    drop(reader);
    let notices = mod_content::notices(root).unwrap().unwrap();
    let refusal = notices
        .iter()
        .find(|notice| notice.starts_with("dev.studio.configured was left out"))
        .cloned()
        .unwrap_or_else(|| panic!("{notices:?}"));
    assert!(refusal.contains("comparison binding"), "{refusal}");
    fs::write(
        &manifest_path,
        include_bytes!("fixtures/studio/configured/mod.json"),
    )
    .unwrap();
    assert_eq!(tree(&foreign), before);
    crate::test_support::artifact(
        "sunrise-studio-config-readback.json",
        &json!({
            "cases":evidence,"invalid_consumer_refusal":refusal,"foreign_unchanged":before,"stock_unchanged":stock,
            "fixture":serde_json::from_str::<serde_json::Value>(include_str!("fixtures/studio/configured/provenance.json")).unwrap()
        }),
    );
}

/// Another editor's mod that loads after Parhelion and changes the entry Parhelion changes.
const WORDS: &str = "org.studio.words";

/// Overlays entry 0 of the Studio fixture's stock package with `payload`, without its placeholder
/// codec.
fn overlay(fixture: &Fixture, payload: &[u8]) -> Vec<u8> {
    let decoder = fixture
        .target
        .parent()
        .unwrap()
        .join("bin/x64/oo2core_3_win64.dll");
    fs::remove_file(&decoder).unwrap();
    let authored = crate::extend::build_extended_overlay(
        &fixture.target,
        0x058c,
        &[crate::ReplacementSpec {
            tag: tiger_pkg::TagHash::new(0x058c, 0),
            payload: payload.to_vec(),
        }],
        &[],
    )
    .unwrap();
    fs::write(decoder, b"fixture, never loaded").unwrap();
    authored.bytes().to_vec()
}

/// Merge data for every staged package, read back from the staged bytes by the build step, and a
/// manifest that lists it.
fn stage_merge_data(fixture: &Fixture) -> serde_json::Value {
    let files = fixture
        .staged_bytes
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let data = crate::merge_data::describe(&fixture.target, &files);
    let report = serde_json::from_slice::<serde_json::Value>(&data.report).unwrap();
    assert_eq!(data.sidecars.len(), files.len(), "{report}");
    let listed = |name: &str| {
        let digest = digest_file(&fixture.staging.join(name)).unwrap();
        json!({"file_name": name, "byte_length": digest.byte_length, "sha256": digest.sha256})
    };
    for (name, bytes) in &data.sidecars {
        fs::write(fixture.staging.join(name), bytes).unwrap();
    }
    let mut manifest = fixture.manifest_json(Some(
        fixture
            .staged_bytes
            .keys()
            .map(|name| listed(name))
            .collect(),
    ));
    manifest["merge_data"] = data.sidecars.iter().map(|(name, _)| listed(name)).collect();
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    report
}

/// Parhelion's build changes the first eight bytes of entry 0 of package 058C, and another
/// editor's mod, `theirs`, also changes that entry. Both carry merge data from the same generator.
fn merge_fixture(theirs: &[u8]) -> (Fixture, serde_json::Value) {
    let mut fixture = fixture();
    fs::write(
        fixture.target.join(STOCK),
        include_bytes!("fixtures/studio/w64_investment_globals_client_058c_3.pkg"),
    )
    .unwrap();
    let ours = overlay(&fixture, b"PARHELIOtry zero");
    fs::write(fixture.staging.join(PATCH), &ours).unwrap();
    fixture.staged_bytes.insert(PATCH.into(), ours);
    // Merge data reads stock content, so it is written before another mod changes the view.
    let report = stage_merge_data(&fixture);
    let patch = overlay(&fixture, theirs);
    let data = crate::merge_data::describe(&fixture.target, &[(PATCH, patch.as_slice())]);
    let (_, sidecar) = data.sidecars.into_iter().next().unwrap_or_else(|| {
        panic!("{}", String::from_utf8_lossy(&data.report));
    });
    let directory = mod_content::runtime_directory(fixture.target.parent().unwrap())
        .join("mods")
        .join(WORDS);
    fs::create_dir_all(directory.join("patches")).unwrap();
    fs::write(
        directory.join("mod.json"),
        serde_json::to_vec(
            &json!({"manifestVersion":2,"id":WORDS,"version":"1.0.0","name":"Words"}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(directory.join("patches").join(PATCH), patch).unwrap();
    fs::write(directory.join("patches").join(SIDECAR), sidecar).unwrap();
    (fixture, report)
}

fn entry_zero(fixture: &Fixture) -> Vec<u8> {
    sundial::package_authoring::open_shadowkeep_package_manager(&fixture.target)
        .unwrap()
        .read_tag(tiger_pkg::TagHash::new(0x058c, 0))
        .unwrap()
}

#[test]
fn edits_two_mods_make_to_different_words_of_one_entry_are_merged() {
    let (fixture, report) = merge_fixture(b"stock enTRY ZERO");
    let root = fixture.target.parent().unwrap().to_path_buf();
    let words = mod_content::runtime_directory(&root)
        .join("mods")
        .join(WORDS);
    let foreign = tree(&words);
    let mut incoming = mod_content::Incoming::default();
    for name in fixture.staged_bytes.keys() {
        let profile = authored_package_for_file_name(name).unwrap();
        if canonical_package(profile.package_id).is_some() {
            incoming.patches.push(fixture.staging.join(name));
        } else {
            incoming.packages.push(fixture.staging.join(name));
        }
    }
    let previewed = sundial::package_authoring::open_incoming_shadowkeep_package_manager(
        &fixture.target,
        &incoming,
    )
    .unwrap()
    .read_tag(tiger_pkg::TagHash::new(0x058c, 0))
    .unwrap();
    assert_eq!(previewed, b"PARHELIOTRY ZERO");
    assert!(!mod_content::directory(&root).exists());
    assert_eq!(tree(&words), foreign);
    let install = install_staged_packages(&request(&fixture)).unwrap();
    assert_eq!(
        install.mod_overlaps,
        [format!(
            "Sunrise merges 1 entry that Parhelion and {WORDS} both change, in package 058C."
        )]
    );
    let own = mod_content::directory(&root);
    let installed = fs::read(own.join("patches").join(SIDECAR)).unwrap();
    assert_eq!(installed, fs::read(fixture.staging.join(SIDECAR)).unwrap());
    let receipt: mod_content::Receipt =
        mod_content::read_json(&own.join(mod_content::RECEIPT)).unwrap();
    assert!(
        receipt
            .artifacts
            .iter()
            .any(|artifact| artifact.path == format!("patches/{SIDECAR}"))
    );
    // Parhelion loads first, so the merge keeps its first word and takes the later mod's second.
    let merged = entry_zero(&fixture);
    assert_eq!(merged, b"PARHELIOTRY ZERO");
    let notices = mod_content::notices(&root).unwrap().unwrap();
    assert!(
        !notices
            .iter()
            .any(|notice| notice.contains("058C") || notice.contains("merge data")),
        "{notices:?}"
    );
    let removal = preview_uninstall(&fixture.target).unwrap();
    uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
    assert!(!own.exists());
    assert_eq!(tree(&words), foreign);
    let after = entry_zero(&fixture);
    assert_eq!(after, b"stock enTRY ZERO");
    crate::test_support::artifact(
        "sunrise-table-merge.json",
        &json!({
            "sunrise_revision": "f1e7c66baf655b1b0be46607d1054a8a367f02d7",
            "studio_generator_revision": "007dd7a6f10164243369350e63c42168229c35ab",
            "stock_entry_zero": "stock entry zero",
            "parhelion_entry_zero": "PARHELIOtry zero",
            "other_entry_zero": "stock enTRY ZERO",
            "merged_read": String::from_utf8_lossy(&merged),
            "incoming_read_before_install": String::from_utf8_lossy(&previewed),
            "read_after_removal": String::from_utf8_lossy(&after),
            "install_report": install.mod_overlaps,
            "sidecar_sha256": hex::encode(Sha256::digest(&installed)),
            "merge_data_report": report,
            "notices": notices,
            "native_loader": "not executed"
        }),
    );
}

#[test]
fn edits_two_mods_make_to_the_same_word_are_refused() {
    let (fixture, _) = merge_fixture(b"STOCK entry zero");
    let root = fixture.target.parent().unwrap().to_path_buf();
    let error = install_staged_packages(&request(&fixture)).unwrap_err();
    assert!(error.message.contains(WORDS), "{error}");
    assert!(error.message.contains("0x81318000"), "{error}");
    assert!(!mod_content::directory(&root).exists());
    assert_eq!(entry_zero(&fixture), b"STOCK entry zero");
    crate::test_support::artifact(
        "sunrise-table-merge-conflict.json",
        &json!({"refusal": error.message, "native_loader": "not executed"}),
    );
}
