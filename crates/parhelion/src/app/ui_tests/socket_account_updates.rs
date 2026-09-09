use super::*;

#[test]
fn socket_only_install_review_shows_saved_instance_updates_without_writing_accounts() {
    let directory = tempfile::tempdir().unwrap();
    let packages = directory.path().join("packages");
    std::fs::create_dir(&packages).unwrap();
    let path = directory.path().join("settings.json");
    let original = serde_json::to_vec(&serde_json::json!({"version": 16, "state": {
        "account": {"primary_soid": "0x0000000000000001"},
        "characters": [{"soid": "0x0000000000000002", "class": 0, "equipment": {
            "kinetic": {"instance_soid": "0x0000000000000010", "definition_hash": 100,
                "level": 100, "quantity": 1, "plugs": vec![301; 8]}
        }}]
    }}))
    .unwrap();
    std::fs::write(&path, &original).unwrap();
    let review = crate::install::test_review_with_sockets(
        &packages,
        BTreeSet::new(),
        vec![sundial::investment::AuthoredSocketChange {
            definition_hash: 100,
            previous_socket_count: 8,
            default_plugs: vec![Some(400); 9],
        }],
    );
    let mut app = PackageAuthoringApp {
        replacement_review: Some(Ok(review)),
        latest_build: Some(Ok(BuildReport {
            weapons: vec![],
            run_directory: PathBuf::from("staged"),
            manifest_path: PathBuf::from("staged/manifest.json"),
            artifacts: vec![],
            selection_fingerprint: "test".into(),
            staged_recipe_paths: vec![],
        })),
        ..Default::default()
    };
    let before = app.recipe.clone();
    let (output, _) = render(900.0, |ui| app.draw_install_confirmation(ui));
    let labels = text(&output);
    assert!(labels.contains("Account Changes"));
    assert!(labels.contains("Update 1 saved Item 0x00000064 socket lists from 8 to 9 sockets."));
    assert!(labels.contains("Added sockets use the new definition's defaults."));
    assert!(labels.contains("Back Up, Update & Install"));
    assert!(!labels.contains("Back Up, Remove & Install"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(app.recipe, before);
    assert!(app.install_receiver.is_none());
}
