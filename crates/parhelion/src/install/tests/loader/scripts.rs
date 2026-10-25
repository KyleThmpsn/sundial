//! Mission sidecars use the package transaction. Failures: modified staged or installed bytes,
//! publication interrupted before account commit, an older stage leaving a stale controller,
//! path traversal, or scripts appearing in stock packages. Both account formats share the path.
use super::*;

const FILE: &str = "mission_example.lua";
const PATH: &str = "scripts/mission_example/mission_example.lua";

fn stage(fixture: &Fixture, text: &[u8]) {
    fs::write(fixture.staging.join(FILE), text).unwrap();
    let path = fixture.staging.join(MANIFEST_FILE_NAME);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["mission_scripts"] = json!([{
        "file_name": FILE, "byte_length": text.len(),
        "sha256": hex::encode_upper(Sha256::digest(text))
    }]);
    fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

#[test]
fn mission_scripts_follow_package_publication_recovery_and_removal() {
    let mut evidence = Vec::new();
    for schema in [8, 18] {
        let fixture = fixture();
        let (account_path, original_account) = account(&fixture, schema);
        let stock = compliance::inventory(&fixture.target);
        let first = b"return { on_start = function(context) end }\n";
        let second = b"return { on_start = function(context) local n = 2 end }\n";
        stage(&fixture, first);
        fs::write(fixture.staging.join(FILE), second).unwrap();
        assert!(install_staged_packages(&request(&fixture)).is_err());
        let directory = mod_content::directory(fixture.target.parent().unwrap());
        assert!(!directory.exists());
        stage(&fixture, first);
        install_staged_packages(&request(&fixture)).unwrap();
        assert_eq!(fs::read(directory.join(PATH)).unwrap(), first);
        let receipt: mod_content::Receipt =
            mod_content::read_json(&directory.join(mod_content::RECEIPT)).unwrap();
        let recorded = receipt
            .artifacts
            .iter()
            .find(|row| row.path == PATH)
            .unwrap();
        assert!(
            recorded
                .sha256
                .eq_ignore_ascii_case(&hex::encode(Sha256::digest(first)))
        );
        assert!(
            mod_content::files(fixture.target.parent().unwrap())
                .unwrap()
                .iter()
                .any(|path| path.ends_with(PATH))
        );
        stage(&fixture, second);
        fs::write(directory.join(PATH), b"concurrent edit").unwrap();
        assert!(install_staged_packages(&request(&fixture)).is_err());
        assert_eq!(fs::read(directory.join(PATH)).unwrap(), b"concurrent edit");
        fs::write(directory.join(PATH), first).unwrap();
        interrupt_install(&request(&fixture), "published");
        recover_interrupted_install(&recovery(&fixture)).unwrap();
        assert_eq!(fs::read(directory.join(PATH)).unwrap(), first);
        install_staged_packages(&request(&fixture)).unwrap();
        assert_eq!(fs::read(directory.join(PATH)).unwrap(), second);
        write_manifest(&fixture);
        install_staged_packages(&request(&fixture)).unwrap();
        assert!(!directory.join(PATH).exists());
        let removal = preview_uninstall(&fixture.target).unwrap();
        uninstall_custom_packages(&removal, &fixture.backups, game_stopped).unwrap();
        assert!(!directory.exists());
        assert_eq!(compliance::inventory(&fixture.target), stock);
        assert_eq!(fs::read(account_path).unwrap(), original_account);
        evidence.push(json!({"schema":schema,"receipt":recorded,"recovery":"controller restored with packages","stale_controller":"removed"}));
    }
    crate::test_support::artifact("mission-script-install.json", &evidence);
}
