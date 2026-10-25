//! Sidecar ownership through the real stage, install, recovery and removal boundaries.
//! Failure inventory is recorded before implementation in the map diagnostics investigation.
use super::*;

const FILE: &str = "map-diagnostics.json";

fn stage(fixture: &Fixture, label: &str) -> Vec<u8> {
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema": 1,
        "maps": [{
            "activity": "mission_example",
            "bubble_hash": sundial::package_authoring::fnv1_name_hash("mission_example"),
            "source_cache": "example.map",
            "source_build": "Jun 21 2023 15:35:31",
            "zones": [{ "index": 0, "name": label, "bsps": ["levels\\solo\\example\\example_000"] }]
        }]
    }))
    .unwrap();
    fs::write(fixture.staging.join(FILE), &bytes).unwrap();
    let manifest_path = fixture.staging.join(MANIFEST_FILE_NAME);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["map_diagnostics"] = json!({
        "file_name": FILE, "byte_length": bytes.len(),
        "sha256": hex::encode_upper(Sha256::digest(&bytes))
    });
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    bytes
}

#[test]
fn source_labels_follow_install_update_recovery_and_removal() {
    let mut evidence = Vec::new();
    for schema in [8, 18] {
        let fixture = fixture();
        let (account_path, original_account) = account(&fixture, schema);
        let stock = compliance::inventory(&fixture.target);
        let first = stage(&fixture, "set_hill");
        install_staged_packages(&request(&fixture)).unwrap();
        let directory = mod_content::directory(fixture.target.parent().unwrap());
        assert_eq!(fs::read(directory.join(FILE)).unwrap(), first);
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join(mod_content::RECEIPT)).unwrap())
                .unwrap();
        let row = receipt["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["path"] == FILE)
            .unwrap();
        assert!(
            row["sha256"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case(&hex::encode(Sha256::digest(&first)))
        );
        assert!(
            mod_content::files(fixture.target.parent().unwrap())
                .unwrap()
                .iter()
                .any(|path| path.ends_with(FILE))
        );

        let second = stage(&fixture, "set_transition");
        let changed = String::from_utf8(first.clone())
            .unwrap()
            .replace("set_hill", "set_bad!");
        fs::write(directory.join(FILE), changed.as_bytes()).unwrap();
        assert!(install_staged_packages(&request(&fixture)).is_err());
        assert_eq!(fs::read(directory.join(FILE)).unwrap(), changed.as_bytes());
        fs::write(directory.join(FILE), &first).unwrap();
        interrupt_install(&request(&fixture), "published");
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert_eq!(fs::read(directory.join(FILE)).unwrap(), first);
        install_staged_packages(&request(&fixture)).unwrap();
        assert_eq!(fs::read(directory.join(FILE)).unwrap(), second);

        // An old stage without diagnostics removes the previous generation's labels.
        write_manifest(&fixture);
        install_staged_packages(&request(&fixture)).unwrap();
        assert!(!directory.join(FILE).exists());
        let removal = preview_uninstall(&fixture.target).unwrap();
        uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
        assert!(!directory.exists());
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert_eq!(fs::read(&account_path).unwrap(), original_account);
        evidence.push(json!({
            "schema": schema, "first_sha256": hex::encode(Sha256::digest(&first)),
            "second_sha256": hex::encode(Sha256::digest(&second)),
            "receipt": row, "recovery": "old labels restored with packages",
            "older_stage": "labels removed", "uninstall": "managed folder removed"
        }));
    }
    crate::test_support::artifact("map-diagnostics-install.json", &evidence);
}

#[test]
fn changed_or_invalid_source_labels_never_reach_the_mod_folder() {
    let fixture = fixture();
    let original = stage(&fixture, "set_hill");
    fs::write(fixture.staging.join(FILE), b"changed after staging").unwrap();
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    fs::write(fixture.staging.join(FILE), original).unwrap();
    stage(&fixture, "line\nbreak");
    assert!(install_staged_packages(&request(&fixture)).is_err());
    assert!(!mod_content::directory(fixture.target.parent().unwrap()).exists());
    assert!(!fixture.backups.exists());
}
