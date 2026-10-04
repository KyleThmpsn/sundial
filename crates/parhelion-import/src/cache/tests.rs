use super::*;

fn key(index: u32) -> String {
    format!("{index:064x}")
}

fn receipt(data: &serde_json::Value) {
    if let Some(output) = std::env::var_os("SUNDIAL_CACHE_VERIFICATION_DIRECTORY") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        write_json(&output.join("generations.json"), &serde_json::json!({"active_readback": data, "spare_preserved": true, "unrelated_preserved": true, "failed_write_preserved": true, "legacy_grace": true})).unwrap();
    }
}

#[test]
fn generations_survive_concurrent_readers_and_unwritable_publication() {
    let storage = tempfile::tempdir().unwrap();
    let root = storage.path().join("rigs");
    let first = Generation::directory(&root, &key(1)).unwrap();
    let data = serde_json::json!({"item": 7, "bones": [3, 5, 8]});
    write_json(&first.path().join("rig.json"), &data).unwrap();
    let second = Generation::directory(&root, &key(2)).unwrap();
    write_json(&second.path().join("rig.json"), &data).unwrap();
    drop(second);
    let third = Generation::directory(&root, &key(3)).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(first.path().join("rig.json")).unwrap()
        )
        .unwrap(),
        data
    );
    drop(third);
    drop(first);
    let fourth = Generation::directory(&root, &key(4)).unwrap();
    assert!(
        !root.join(key(1)).exists(),
        "Released oldest generation stayed"
    );
    assert!(root.join(key(3)).exists(), "Spare generation disappeared");
    std::fs::write(root.join("notes.txt"), b"unrelated").unwrap();
    std::fs::create_dir(root.join("not-a-generation")).unwrap();
    drop(fourth);
    let fifth = Generation::directory(&root, &key(5)).unwrap();
    assert!(root.join("notes.txt").is_file());
    assert!(root.join("not-a-generation").is_dir());
    let blocked = storage.path().join("blocked");
    std::fs::write(&blocked, b"keep").unwrap();
    assert!(Generation::directory(&blocked, &key(1)).is_none());
    assert!(write_json(&blocked.join("data.json"), &data).is_err());
    assert_eq!(std::fs::read(&blocked).unwrap(), b"keep");

    let files = storage.path().join("contracts");
    for index in 1..=4 {
        let generation = Generation::file(&files, &format!("5-{}", key(index))).unwrap();
        write_json(generation.path(), &data).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(generation.path()).unwrap())
                .unwrap(),
            data
        );
    }
    assert!(!files.join(format!("5-{}.json", key(1))).exists());
    assert!(files.join(format!("5-{}.json", key(3))).exists());
    let legacy = files.join(format!("5-{}.json", key(0)));
    std::fs::write(&legacy, b"{}").unwrap();
    let current = Generation::file(&files, &format!("5-{}", key(5))).unwrap();
    assert!(
        legacy.exists(),
        "Unleased legacy writer lost its grace period"
    );
    assert!(Generation::directory(&root, "../escape").is_none());
    receipt(&data);
    drop((current, fifth));
}
