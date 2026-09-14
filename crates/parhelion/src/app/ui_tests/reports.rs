use super::*;

#[test]
fn replacement_confirmation_names_removals_without_changing_account_or_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let packages = directory.path().join("packages");
    std::fs::create_dir(&packages).unwrap();
    let path = directory.path().join("settings.json");
    let original = br#"{"version":8,"state":{"account":{"primary_soid":"0x0000000000000001","profile_items":[{"definition_hash":100,"quantity":1}]},"characters":[]}}"#;
    std::fs::write(&path, original).unwrap();
    let review = crate::install::test_review(&packages, BTreeSet::from([100]));
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
    assert!(labels.contains("Remove 1 saved item:"));
    assert!(labels.contains("Back Up, Remove & Install"));
    assert!(labels.contains("Back to Build"));
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert_eq!(app.recipe, before);
    assert!(app.install_receiver.is_none());
}

#[test]
fn successful_install_report_is_compact_and_ends_with_close() {
    let report = InstallReport {
        manifest_schema: 1,
        staged_run_directory: PathBuf::from("staged"),
        target_packages_directory: PathBuf::from(r"\\?\C:\Destiny2\packages"),
        backup_directory: PathBuf::from(r"\\?\C:\Backups\parhelion-backup-v2-abc-123-1-0"),
        artifacts: vec![],
        removed_obsolete_packages: vec![],
        recipe_backup_directory: None,
        pruned_backup_directories: vec![],
        backup_prune_warning: None,
        invalidated_sunrise_cache: None,
        invalidated_package_header_caches: vec![],
        profile_sync: Some(Err("Account is read-only".into())),
        cleaned_account: None,
    };
    let mut app = PackageAuthoringApp {
        latest_install: Some(Ok(report)),
        build_dialog_step: BuildDialogStep::Install,
        ..Default::default()
    };
    for width in [620.0, 960.0] {
        let (output, overflow) = render(width, |ui| app.draw_install_status(ui));
        let labels = text(&output);
        for label in [
            "Packages Installed",
            "Installation Details",
            "Close",
            "Account is read-only",
        ] {
            assert!(labels.contains(label), "Missing {label}");
        }
        assert!(!labels.contains("Review"));
        assert!(!labels.contains("Open Backup Folder"));
        assert!(!labels.contains("Back to Build"));
        assert!(!labels.contains("parhelion-backup-v2"));
        assert!(!labels.contains(r"\\?\"));
        assert!(
            overflow < 1.0,
            "Install report overflow {overflow} at {width}"
        );
        assert!(app.install_receiver.is_none());
    }
}
