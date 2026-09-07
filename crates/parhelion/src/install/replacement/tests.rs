use super::*;
use serde_json::json;

fn item(id: u64, hash: u32, plugs: serde_json::Value) -> serde_json::Value {
    json!({"instance_soid": format!("0x{id:016X}"), "definition_hash": hash,
        "level": 100, "quantity": 1, "plugs": plugs})
}

#[test]
fn removed_items_plugs_and_rewards_require_exact_consent_without_writing_during_review() {
    for version in [6, 8, 16] {
        for location in [
            "equipped",
            "inventory",
            "plug",
            "inventory_plug",
            "profile",
            "reward",
        ] {
            let root = tempfile::tempdir().unwrap();
            let packages = root.path().join("packages");
            fs::create_dir(&packages).unwrap();
            let sentinel = packages.join("stock.pkg");
            fs::write(&sentinel, b"unchanged stock").unwrap();
            let mut value = json!({"version": version, "unknown": {"keep": true}, "state": {
                "account": {"primary_soid": "0x0000000000000001"},
                "characters": [{"soid": "0x0000000000000002", "class": 0, "equipment": {}, "inventory": []}]
            }});
            match location {
                "equipped" => {
                    value["state"]["characters"][0]["equipment"]["kinetic"] =
                        item(20, 100, json!(null))
                }
                "inventory" => {
                    value["state"]["characters"][0]["inventory"] =
                        json!([item(20, 100, json!(null))])
                }
                "plug" => {
                    value["state"]["characters"][0]["equipment"]["kinetic"] =
                        item(20, 200, json!([300, 301]))
                }
                "inventory_plug" => {
                    value["state"]["characters"][0]["inventory"] =
                        json!([item(20, 200, json!([300, 301]))])
                }
                "profile" => {
                    value["state"]["account"]["profile_items"] =
                        json!([{"definition_hash": 100, "quantity": 1}])
                }
                "reward" => {
                    value["state"]["account"]["dismantle_rewards"] =
                        json!([{"definition_hash": 100, "quantity": 1}])
                }
                _ => unreachable!(),
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            let path = root.path().join("settings.json");
            fs::write(&path, &bytes).unwrap();
            let review = test_review(&packages, BTreeSet::from([100, 300]));
            assert!(review.changes_account(), "v{version} {location}");
            assert!(validate_consent(&review, None).is_err());
            validate_consent(&review, Some(&review)).unwrap();
            let mut stale = review.clone();
            stale.cleanup.as_mut().unwrap().original_bytes.push(b' ');
            assert!(validate_consent(&review, Some(&stale)).is_err());
            assert_eq!(fs::read(path).unwrap(), bytes);
            assert_eq!(fs::read(sentinel).unwrap(), b"unchanged stock");
        }
    }
}

#[test]
fn unchanged_definitions_need_no_account_and_unreferenced_removals_guard_concurrent_edits() {
    let root = tempfile::tempdir().unwrap();
    let packages = root.path().join("packages");
    fs::create_dir(&packages).unwrap();
    let path = root.path().join("settings.json");
    let bytes = serde_json::to_vec(&json!({"version": 8, "unknown": 42, "state": {
        "account": {"primary_soid": "0x0000000000000001"},
        "characters": [{"soid": "0x0000000000000002", "class": 0,
            "equipment": {"kinetic": item(20, 200, json!([301]))}}],
        "unlocks": {"account_flag_runs": [[100, 2]]}
    }}))
    .unwrap();
    fs::write(&path, &bytes).unwrap();
    let guard = test_review(&packages, BTreeSet::from([100, 300]));
    assert!(!guard.changes_account());
    validate_consent(&guard, None).unwrap();
    verify_account(&packages, Some(&guard)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let mut changed = bytes.clone();
    changed.push(b' ');
    fs::write(&path, changed).unwrap();
    assert!(
        verify_account(&packages, Some(&guard))
            .unwrap_err()
            .contains("changed after replacement review")
    );
    fs::write(&path, b"not valid JSON").unwrap();
    assert!(account_references(&packages, &guard).is_err());
}

#[test]
#[ignore = "read-only native comparison; requires PARHELION_LIFECYCLE_SOURCE_PACKAGES and PARHELION_TEST_STAGED_RUN"]
fn staged_identity_reader_matches_installed_generation() {
    let target = PathBuf::from(std::env::var_os("PARHELION_LIFECYCLE_SOURCE_PACKAGES").unwrap());
    let staged = PathBuf::from(std::env::var_os("PARHELION_TEST_STAGED_RUN").unwrap());
    let (hashes, unlocks) = identities::generation_identities(&target, &staged).unwrap();
    let manifest: ManifestDocument =
        serde_json::from_slice(&fs::read(staged.join(MANIFEST_FILE_NAME)).unwrap()).unwrap();
    for weapon in &manifest.project.weapons {
        assert!(hashes.contains(&weapon.item.hash.get()));
    }
    assert_eq!(
        unlocks,
        validate_manifest_unlocks(&manifest.project).unwrap()
    );
    assert!(hashes.len() >= manifest.project.weapons.len());
}
