use super::*;
use serde_json::json;

fn dawn() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap()
}

fn sunrise() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
    ))
    .unwrap()
}

fn restore(
    receipt: &storage::Receipt,
    folder: &Path,
    check: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    recovery::Plan::prepare(&receipt.install, folder)?
        .apply(check)
        .map(|_| ())
}

fn forward(existing_database: bool) -> (tempfile::TempDir, plan::Plan) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("Sunrise")).unwrap();
    fs::write(root.join("steam_api64.dll"), b"test runtime").unwrap();
    let path = root.join("Sunrise/settings.json");
    fs::write(&path, serde_json::to_vec(&dawn()).unwrap()).unwrap();
    if existing_database {
        native::tests::create_fixture(&crate::persistence::investment_path(&path), 3);
    }
    let document = WorkspaceDocument::load(dawn(), &path);
    let mut target = RuntimeInspection::inspect(root).copies.remove(0);
    target.bundled_schema = Some(18);
    let plan = plan::Plan::prepare(
        root,
        &path,
        &document,
        &document,
        target,
        sunrise(),
        Some(&native::tests::default_resources()),
    )
    .unwrap();
    (directory, plan)
}

#[test]
fn dawn_fixture_is_valid_v6() {
    let source = dawn();
    assert_eq!(source["version"], 6);
    assert_eq!(source["experiments"]["omega"]["coo_executor"], false);
    settings::validate_document(&source).unwrap();
    assert!(crate::game_settings::dawn::settings_issues(&source).is_empty());
}

#[test]
fn conversion_preview_leaves_settings_and_existing_database_unchanged() {
    let (_directory, plan) = forward(true);
    let target = plan.receipt.install.join(&plan.receipt.target_path);
    assert_eq!(
        fs::read(&target).unwrap(),
        fs::read(plan.staged.path().join("source-settings.json")).unwrap()
    );
    assert_eq!(
        native::snapshot::read(&crate::persistence::investment_path(&target)).unwrap(),
        native::snapshot::read(&plan.staged.path().join("account.before.sqlite3")).unwrap()
    );
    assert!(!plan.receipt.install.join(".sunrise").exists());
}

#[test]
fn explicit_conversion_and_restore_handle_new_and_existing_sqlite_accounts() {
    for exists in [false, true] {
        let (_directory, plan) = forward(exists);
        let target = plan.receipt.install.join(&plan.receipt.target_path);
        let database = crate::persistence::investment_path(&target);
        let original = fs::read(&target).unwrap();
        let backup = plan.apply(|| Ok(())).unwrap();
        let after = settings::load_workspace_json(&target).unwrap();
        assert_eq!(after["version"], 18);
        let loaded = WorkspaceDocument::load(after, &target);
        assert!(loaded.native_account().is_some());
        settings::validate_workspace_document(&loaded).unwrap();
        assert_eq!(
            storage::load_backup(&plan.receipt.install, &backup)
                .unwrap()
                .files,
            plan.receipt.files
        );
        restore(&plan.receipt, &backup, || Ok(())).unwrap();
        restore(&plan.receipt, &backup, || Ok(())).unwrap();
        assert_eq!(fs::read(&target).unwrap(), original);
        assert_eq!(database.exists(), exists);
        if exists {
            assert_eq!(
                native::snapshot::read(&database).unwrap(),
                native::snapshot::read(&backup.join("account.before.sqlite3")).unwrap()
            );
        }
    }
}

#[test]
fn conversion_refuses_changed_settings_database_runtime_or_stage() {
    for kind in 0..4 {
        let (_directory, plan) = forward(true);
        let root = &plan.receipt.install;
        let target = root.join(&plan.receipt.target_path);
        let db_path = crate::persistence::investment_path(&target);
        match kind {
            0 => fs::write(&target, b"outside edit").unwrap(),
            1 => {
                rusqlite::Connection::open(&db_path)
                    .unwrap()
                    .execute("UPDATE account_display SET show_fps=0", [])
                    .unwrap();
            }
            2 => fs::write(root.join("steam_api64.dll"), b"swapped runtime").unwrap(),
            _ => fs::write(
                plan.staged.path().join("settings.after.json"),
                b"tampered preview",
            )
            .unwrap(),
        }
        let before = fs::read(&target).unwrap();
        let database = native::snapshot::read(&db_path).unwrap();
        assert!(plan.apply(|| Ok(())).is_err());
        assert_eq!(fs::read(&target).unwrap(), before);
        assert_eq!(native::snapshot::read(&db_path).unwrap(), database);
        assert!(!root.join(".sunrise").exists());
    }
}

#[test]
fn failed_json_conversion_rolls_back_database_without_overwriting_outside_edits() {
    let (_directory, plan) = forward(true);
    let target = plan.receipt.install.join(&plan.receipt.target_path);
    let backup = storage::save_backup(&plan.receipt, plan.staged.path()).unwrap();
    let mut checks = 0;
    let error = storage::install(&plan.receipt, &backup, &mut || {
        checks += 1;
        if checks == 2 {
            fs::write(&target, b"outside edit").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(!error.is_empty());
    assert_eq!(fs::read(&target).unwrap(), b"outside edit");
    assert_eq!(
        native::snapshot::read(&crate::persistence::investment_path(&target)).unwrap(),
        native::snapshot::read(&backup.join("account.before.sqlite3")).unwrap()
    );
}

#[test]
fn conversion_to_dawn_preserves_native_source_and_validates_generated_json() {
    let (directory, forward) = forward(false);
    forward.apply(|| Ok(())).unwrap();
    let root = directory.path();
    let path = root.join("Sunrise/settings.json");
    let database = crate::persistence::investment_path(&path);
    let before = native::snapshot::read(&database).unwrap();
    let document = WorkspaceDocument::load(settings::load_workspace_json(&path).unwrap(), &path);
    let mut target = RuntimeInspection::inspect(root).copies.remove(0);
    target.bundled_schema = Some(6);
    target.dawn = true;
    target.dawn_runtime = Some(crate::game_settings::dawn::Runtime::inspect(
        &target.dll_path,
    ));
    let plan =
        plan::Plan::prepare(root, &path, &document, &document, target, dawn(), None).unwrap();
    let backup = plan.apply(|| Ok(())).unwrap();
    let result = settings::load_workspace_json(&path).unwrap();
    assert_eq!(result["version"], 6);
    settings::validate_document(&result).unwrap();
    assert!(crate::game_settings::dawn::settings_issues(&result).is_empty());
    assert_eq!(native::snapshot::read(&database).unwrap(), before);
    assert_eq!(
        native::snapshot::read(&backup.join("source-draft.before.sqlite3")).unwrap(),
        before
    );
    restore(&plan.receipt, &backup, || Ok(())).unwrap();
    assert_eq!(settings::load_workspace_json(&path).unwrap()["version"], 18);
}

#[test]
fn conversion_includes_unsaved_changes_and_preserves_unknown_source_fields_in_backup() {
    let (_directory, mut plan) = forward(false);
    let root = &plan.receipt.install;
    let path = root.join(&plan.receipt.source_path);
    let persisted = plan.document.clone();
    plan.document.json_mut()["steam"]["user"]["persona_name"] = json!("Draft Name");
    plan.document.json_mut()["future_extension"] = json!({"opaque": [1, 2, 3]});
    let mut target = plan.target.clone();
    target.settings_path = root.join(&plan.receipt.target_path);
    let plan = plan::Plan::prepare(
        root,
        &path,
        &plan.document,
        &persisted,
        target,
        sunrise(),
        Some(&native::tests::default_resources()),
    )
    .unwrap();
    let backup = plan.apply(|| Ok(())).unwrap();
    let saved = settings::load_workspace_json(&path).unwrap();
    assert_eq!(saved["steam"]["user"]["persona_name"], "Draft Name");
    let draft: Value =
        serde_json::from_slice(&fs::read(backup.join("source-draft.json")).unwrap()).unwrap();
    assert_eq!(draft["future_extension"], json!({"opaque": [1, 2, 3]}));
}

#[test]
fn restore_after_play_keeps_newer_data_and_rejects_edits_after_review() {
    let (_directory, plan) = forward(true);
    let backup = plan.apply(|| Ok(())).unwrap();
    let target = plan.receipt.install.join(&plan.receipt.target_path);
    let database = crate::persistence::investment_path(&target);
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE future_data(value TEXT); INSERT INTO future_data VALUES('gameplay progress');").unwrap();
    let played = native::snapshot::read(&database).unwrap();
    let restore = recovery::Plan::prepare(&plan.receipt.install, &backup).unwrap();
    db.execute("INSERT INTO future_data VALUES('changed after review')", [])
        .unwrap();
    assert!(
        restore
            .apply(|| Ok(()))
            .unwrap_err()
            .contains("changed after review")
    );
    db.execute(
        "DELETE FROM future_data WHERE value='changed after review'",
        [],
    )
    .unwrap();
    let safety = restore.apply(|| Ok(())).unwrap().unwrap();
    assert_eq!(
        native::snapshot::read(&safety.join("investment.sqlite3")).unwrap(),
        played
    );
    assert_eq!(
        native::snapshot::read(&database).unwrap(),
        native::snapshot::read(&backup.join("account.before.sqlite3")).unwrap()
    );
    assert_eq!(
        settings::load_workspace_json(&target).unwrap()["version"],
        6
    );
}

#[test]
fn restore_refuses_a_changed_or_moved_backup() {
    let (directory, plan) = forward(false);
    let backup = plan.apply(|| Ok(())).unwrap();
    let foreign = tempfile::tempdir().unwrap();
    assert!(recovery::Plan::prepare(foreign.path(), &backup).is_err());
    fs::write(backup.join("settings.before.json"), b"changed").unwrap();
    assert!(recovery::Plan::prepare(directory.path(), &backup).is_err());
    assert_eq!(
        settings::load_workspace_json(&directory.path().join("Sunrise/settings.json")).unwrap()["version"],
        18
    );
}

#[test]
fn conversion_cannot_write_while_the_game_is_running() {
    let (_directory, plan) = forward(false);
    assert!(plan.apply(|| Err("Close Destiny 2".into())).is_err());
    assert!(!plan.receipt.install.join(".sunrise").exists());
    assert!(
        !crate::persistence::investment_path(&plan.receipt.install.join(&plan.receipt.target_path))
            .exists()
    );
}

#[test]
fn conversion_uses_the_dll_settings_folder_and_restores_a_previously_missing_target() {
    for location in RuntimeLocation::ALL {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let runtime_dir = location.directory(root);
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(runtime_dir.join("steam_api64.dll"), b"test runtime").unwrap();
        let source = root.join("settings.json");
        let bytes = serde_json::to_vec(&dawn()).unwrap();
        fs::write(&source, &bytes).unwrap();
        let document = WorkspaceDocument::load(dawn(), &source);
        let mut target = RuntimeInspection::inspect(root).copies.remove(0);
        target.bundled_schema = Some(18);
        let plan = plan::Plan::prepare(
            root,
            &source,
            &document,
            &document,
            target,
            sunrise(),
            Some(&native::tests::default_resources()),
        )
        .unwrap();
        let backup = plan.apply(|| Ok(())).unwrap();
        let target = runtime_dir.join("Sunrise/settings.json");
        assert_eq!(
            settings::load_workspace_json(&target).unwrap()["version"],
            18
        );
        assert_eq!(fs::read(&source).unwrap(), bytes);
        restore(&plan.receipt, &backup, || Ok(())).unwrap();
        assert!(!target.exists());
        assert!(!crate::persistence::investment_path(&target).exists());
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }
}

#[test]
fn conversion_dialog_requires_consent_and_fits_small_windows() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for size in [egui::vec2(640.0, 480.0), egui::vec2(1240.0, 900.0)] {
            let (directory, plan) = forward(false);
            let mut app = crate::app::tests::regressions::app(directory.path().to_owned());
            app.document = plan.document.clone();
            app.persisted_document = app.document.clone();
            app.runtime_choice.pending_conversion = Some(Dialog::Convert {
                plan: Box::new(plan),
                accepted: false,
            });
            let context = egui::Context::default();
            context.set_theme(theme);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for frame in 0..3 {
                let output = context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| app.draw_account_conversion(ctx),
                );
                if frame == 0 {
                    continue;
                }
                for expected in [
                    "Experimental Conversion",
                    "I Understand This Is Experimental",
                    "Back Up and Convert",
                    "Cancel",
                ] {
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape
                                && text.galley.text().contains(expected)
                            {
                                Some(text)
                            } else {
                                None
                            }
                        })
                        .expect("conversion controls must be visible");
                    assert!(
                        screen.contains_rect(text.galley.rect.translate(text.pos.to_vec2())),
                        "{expected}"
                    );
                }
                crate::app::tests::capture::write(
                    &context,
                    &output,
                    &format!("account-conversion-{theme:?}-{}", size.x),
                );
            }
            assert!(matches!(
                app.runtime_choice.pending_conversion,
                Some(Dialog::Convert {
                    accepted: false,
                    ..
                })
            ));
            assert!(!directory.path().join(".sunrise").exists());
        }
    }
}
