//! Account references the complete incoming item table lacks, removed through the native package
//! transaction only after review.
use super::*;
use rusqlite::Connection;
use sundial::package_authoring::account::read_authored_account_source;

const CHILD: &str = "PARHELION_COMPATIBILITY_CHILD";
const MISSING: [u32; 4] = [0xFEE10001, 0xFEE10002, 0xFEE10003, 0xFEE10004];

#[test]
#[ignore = "requires SUNDIAL_INSTALL, PARHELION_TEST_STAGED_RUN and SUNDIAL_TEST_ARTIFACTS"]
fn missing_account_definitions_are_cleaned_on_first_install_and_unchanged_reinstall() {
    if let Ok(backend) = std::env::var(CHILD) {
        exercise(&backend);
        return;
    }
    let root = TempDir::new().unwrap();
    for backend in ["json8", "json16", "sqlite18", "dawn"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["install::tests::native_lifecycle::account_compatibility::missing_account_definitions_are_cleaned_on_first_install_and_unchanged_reinstall", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CHILD, backend).env(CHILD_ROOT, root.path()).output().unwrap();
        assert!(
            output.status.success(),
            "{backend}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[allow(clippy::cognitive_complexity)]
fn exercise(backend: &str) {
    let mut fixture = NativeFixture::copy_from_environment(16);
    assert!(!fixture.manifest.source_artifacts.is_empty());
    assert!(!fixture.manifest.project.weapons.is_empty());
    let root = fixture
        .request
        .target_packages_directory
        .parent()
        .unwrap()
        .to_path_buf();
    fs::write(root.join("bin/x64/steam_api64.dll"), b"test runtime").unwrap();
    fixture.request.runtime_snapshot_check = if backend == "dawn" {
        test_dawn_runtime_snapshot
    } else {
        test_runtime_snapshot
    };
    // Installation snapshots the runtime from the canonical game root and compares its review.
    let runtime =
        (fixture.request.runtime_snapshot_check)(&fs::canonicalize(&root).unwrap()).unwrap();
    let manifest = serde_json::to_value(&fixture.manifest).unwrap();
    let donor = manifest["project"]["weapons"][0]["donor"]["item_hash"]
        .as_str()
        .unwrap();
    let valid = u32::from_str_radix(donor.trim_start_matches("0x"), 16).unwrap();
    assert!(!MISSING.contains(&valid));
    let output = crate::test_support::artifact_dir("incoming-account-compatibility").join(backend);
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let mut receipts = Vec::new();
    for installed in [false, true] {
        if installed {
            // The first pass installed this build. The next install has no identity differences.
            fixture.assert_authored_set();
        }
        // A valid account reaches package mutation, and a failure there restores everything.
        let account = seed(&root, backend, valid, false);
        let valid_before = read_authored_account_source(&account).unwrap();
        let packages = package_digests(&fixture);
        let mut progress = Vec::new();
        let failure = install_with_progress(
            &fixture.request,
            Some(1),
            DEFAULT_CACHE_INVALIDATION_OPS,
            publish_package,
            &mut |event| progress.push(event),
        )
        .unwrap_err();
        assert!(
            failure.rollback.as_ref().is_some_and(|r| r.succeeded()),
            "valid {backend} account did not reach package mutation: {failure}"
        );
        let reviewed = progress
            .iter()
            .rposition(|event| event.phase == InstallPhase::ReviewingAccount)
            .unwrap();
        let backed_up = progress
            .iter()
            .position(|event| event.phase == InstallPhase::BackingUp)
            .unwrap();
        assert!(reviewed < backed_up);
        assert_eq!(progress[reviewed].completed, progress[reviewed].total);
        assert_eq!(progress.last().unwrap().phase, InstallPhase::RollingBack);
        fs::write(output.join(format!("progress-{installed}.json")), serde_json::to_vec_pretty(&progress.iter().map(|event| json!({"phase": format!("{:?}", event.phase), "completed": event.completed, "total": event.total})).collect::<Vec<_>>()).unwrap()).unwrap();
        assert_eq!(
            read_authored_account_source(&account).unwrap(),
            valid_before
        );
        assert_eq!(package_digests(&fixture), packages);
        fs::write(
            output.join(format!("account-valid-{installed}.json")),
            &valid_before,
        )
        .unwrap();

        // Saved items and a saved plug the incoming table lacks are removed after review.
        let account = seed(&root, backend, valid, true);
        let before = read_authored_account_source(&account).unwrap();
        let refusal = install_staged_packages(&fixture.request)
            .unwrap_err()
            .to_string();
        assert!(
            refusal.contains("Review Account Changes"),
            "{backend}: {refusal}"
        );
        assert_eq!(read_authored_account_source(&account).unwrap(), before);
        assert_eq!(package_digests(&fixture), packages);
        let proposal = review(&fixture, &runtime);
        let cleanup = proposal.account_cleanup().unwrap();
        assert_eq!(
            cleanup.removed_items,
            BTreeMap::from([(MISSING[0], 1), (MISSING[1], 1), (MISSING[2], 1)]),
            "{backend}"
        );
        assert_eq!(cleanup.cleared_plugs, 1, "{backend}");
        let removed = cleanup
            .removed_items
            .iter()
            .map(|(hash, count)| (format!("0x{hash:08X}"), *count))
            .collect::<BTreeMap<_, _>>();
        let cleared_plugs = cleanup.cleared_plugs;
        assert_eq!(read_authored_account_source(&account).unwrap(), before);
        let mut confirmed = fixture.request.clone();
        confirmed.confirmed_replacement = Some(proposal);
        install_staged_packages(&confirmed).unwrap();
        fixture.assert_authored_set();
        let after = read_authored_account_source(&account).unwrap();
        let text = String::from_utf8_lossy(&after);
        let remaining = MISSING
            .iter()
            .filter(|hash| text.contains(&hash.to_string()))
            .map(|hash| format!("0x{hash:08X}"))
            .collect::<Vec<_>>();
        assert!(remaining.is_empty(), "{backend}: {remaining:?} remain");
        assert!(
            text.contains(&valid.to_string()),
            "{backend}: the valid item was removed"
        );
        let second = review(&fixture, &runtime);
        assert!(!second.removes_account_data(), "{backend}");
        fs::write(
            output.join(format!("account-before-{installed}.json")),
            &before,
        )
        .unwrap();
        fs::write(
            output.join(format!("account-after-{installed}.json")),
            &after,
        )
        .unwrap();
        receipts.push(json!({
            "previous_generation": installed,
            "unconfirmed_refusal": refusal,
            "removed_items": removed,
            "cleared_plugs": cleared_plugs,
            "package_sha256_before": packages,
            "package_sha256_after": package_digests(&fixture),
            "missing_hashes_after_install": remaining,
            "second_review_removes_account_data": second.removes_account_data(),
        }));
    }
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&json!({"backend": backend, "cases": receipts, "valid_account_package_rollback_verified": true})).unwrap()).unwrap();
}

/// The account review installation computes for this build and runtime.
fn review(fixture: &NativeFixture, runtime: &RuntimeSnapshot) -> ReplacementReview {
    crate::install::replacement::test_review_for_runtime(
        &fixture.request.target_packages_directory,
        &fixture.request.staged_run_directory,
        runtime,
    )
    .unwrap()
}

fn package_digests(fixture: &NativeFixture) -> BTreeMap<String, String> {
    fs::read_dir(&fixture.request.target_packages_directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pkg"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                digest_file(&path).unwrap().sha256,
            )
        })
        .collect()
}

fn seed(root: &Path, backend: &str, valid: u32, broken: bool) -> PathBuf {
    let hashes = if broken { MISSING } else { [valid; 4] };
    if backend.starts_with("json") {
        let version = if backend == "json8" { 8 } else { 16 };
        let item = |hash, soid, plugs| json!({"definition_hash": hash, "instance_soid": soid, "quantity": 1, "level": 100, "plugs": plugs});
        let document = json!({"version":version,"client":{"reveal_lore_books":false},"opaque":{"keep":true},"state":{"account":{"primary_soid":"0x0000000000000001","profile_items":[{"definition_hash":hashes[2],"quantity":1}]},"characters":[{"soid":"0x0000000000000002","class":0,"equipment":{"energy":item(hashes[0],"0x4000000000000001",json!(null)),"kinetic":item(valid,"0x4000000000000002",json!([hashes[3]]))},"inventory":[item(hashes[1],"0x4000000000000003",json!(null))]}]}});
        let path = root.join("settings.json");
        fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        return path;
    }
    let dawn = backend == "dawn";
    let path = if dawn {
        root.join("bin/x64/Dawn/player-state.db")
    } else {
        root.join("data/investment.sqlite3")
    };
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    if path.exists() {
        fs::remove_file(&path).unwrap();
    }
    let db = Connection::open(&path).unwrap();
    if dawn {
        fs::write(
            path.parent().unwrap().join("settings.json"),
            br#"{"version":6}"#,
        )
        .unwrap();
        db.execute_batch(include_str!(
            "../../../../../../src/persistence/dawn_account/fixtures/player_state_schema.sql"
        ))
        .unwrap();
        db.execute_batch("INSERT INTO account VALUES(1,'9EAA300100100100'); INSERT INTO allocators VALUES('item_instance','4000000000001000'),('profile_item_instance','5000000000001000'); INSERT INTO characters VALUES(0,'9EAA300100100101',0,0,0,1,50,1,1,1.0,308080871,1,6,7,10,15,2,93,0);").unwrap();
        for (location, position, soid, hash, policy) in [
            (0, 3, "4000000000000001", hashes[0], 0),
            (1, 0, "4000000000000002", hashes[1], 0),
            (0, 0, "4000000000000003", valid, 1),
        ] {
            db.execute(
                "INSERT INTO character_items VALUES('9EAA300100100101',?,?,?,?,100,1,0,0,?,0)",
                rusqlite::params![location, position, soid, hash, policy],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO item_sockets VALUES('4000000000000003',0,?)",
            [hashes[3]],
        )
        .unwrap();
        db.execute(
            "INSERT INTO profile_items VALUES(0,'5000000000000001',?,1,0)",
            [hashes[2]],
        )
        .unwrap();
    } else {
        fs::write(
            root.join("settings.json"),
            br#"{"version":18,"state":{"characters":[{"definition_hash":1}]}}"#,
        )
        .unwrap();
        db.execute_batch(include_str!(
            "../../../../../../src/persistence/sqlite_account/fixtures/investment_schema.sql"
        ))
        .unwrap();
        db.execute_batch(include_str!(
            "../../../../../../src/persistence/sqlite_account/fixtures/account_settings_schema.sql"
        ))
        .unwrap();
        db.execute_batch(include_str!("../../../../../../src/persistence/sqlite_account/fixtures/account_settings_defaults.sql")).unwrap();
        db.execute_batch("INSERT INTO account VALUES(1,1,1); INSERT INTO characters VALUES(0,2,0,0,0,40,1,0.0,0,0,65535,-1,4);").unwrap();
        for (location, position, soid, hash, policy, count) in [
            (0, 3, 101, hashes[0], 0, 0),
            (1, 0, 102, hashes[1], 0, 0),
            (0, 0, 103, valid, 1, 1),
        ] {
            db.execute(
                "INSERT INTO items VALUES(0,?,?,?,?,100,1,0,0,?,?,0,0,0,0,0,1)",
                rusqlite::params![location, position, soid, hash, policy, count],
            )
            .unwrap();
        }
        db.execute("INSERT INTO sockets VALUES(103,0,?)", [hashes[3]])
            .unwrap();
        db.execute(
            "INSERT INTO profile_items VALUES(0,201,?,1,0,1)",
            [hashes[2]],
        )
        .unwrap();
    }
    path
}
