use super::*;

fn unsaved_frame(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, workbench_style);
            app.draw_discard_confirmation(ui);
        },
    )
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn unsaved_dialog_saves_before_close_or_navigation_and_keeps_build_membership() {
    use crate::test_support::driver::{label, tap};
    for width in [380.0, 900.0] {
        for closing in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
            let path = library.root().join("every-end.parhelion.json");
            let mut app = PackageAuthoringApp {
                recipe_library: Some(library),
                ..Default::default()
            };
            assert!(app.open_recipe_path(&path));
            app.enabled_recipe_paths.clear();
            app.recipe.flavor = "Saved from the unsaved changes dialog".into();
            app.synchronize_recipe_dirty();
            let saved = app.recipe.clone();
            app.request_recipe_action(if closing {
                PendingRecipeAction::Close
            } else {
                PendingRecipeAction::New(ItemKind::Weapon)
            });
            let ctx = egui::Context::default();
            ctx.set_theme(egui::Theme::Dark);
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = unsaved_frame(&mut app, &ctx, width, vec![]);
            }
            let save_label = if closing {
                "Save and Close"
            } else {
                "Save and Continue"
            };
            for name in [
                save_label,
                "Cancel",
                if closing {
                    "Discard and Close"
                } else {
                    "Discard and Continue"
                },
            ] {
                let rect = label(&output, name).unwrap();
                assert!(
                    rect.left() >= 0.0
                        && rect.right() <= width
                        && rect.top() >= 0.0
                        && rect.bottom() <= 700.0,
                    "{name}: {rect:?}"
                );
            }
            crate::app::custom_perks::workbench::tests::capture::write(
                &ctx,
                &output,
                &format!("unsaved-{width}-{closing}"),
            );
            let position = label(&output, save_label).unwrap().center();
            for events in tap(position) {
                unsaved_frame(&mut app, &ctx, width, events);
            }
            assert_eq!(WeaponRecipe::load_json(&path).unwrap(), saved);
            assert!(app.pending_recipe_action.is_none());
            assert_eq!(app.take_close_approved(), closing);
            assert!(!app.recipe_dirty);
            assert!(app.enabled_recipe_paths.is_empty());
        }
    }
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn failed_save_keeps_the_dialog_and_draft_with_cancel_and_discard_available() {
    use crate::test_support::driver::{label, tap};
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    let mut external = app.recipe.clone();
    external.flavor = "External editor's version".into();
    library.save_existing(&path, &external).unwrap();
    app.recipe.flavor = "Preserve this unsaved draft".into();
    app.synchronize_recipe_dirty();
    let draft = app.recipe.clone();
    app.request_recipe_action(PendingRecipeAction::Close);
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = unsaved_frame(&mut app, &ctx, 480.0, vec![]);
    }
    let position = label(&output, "Save and Close").unwrap().center();
    for events in tap(position) {
        unsaved_frame(&mut app, &ctx, 480.0, events);
    }
    for _ in 0..3 {
        output = unsaved_frame(&mut app, &ctx, 480.0, vec![]);
    }
    assert!(text(&output).contains("changed on disk"));
    assert!(app.pending_recipe_action.is_some());
    assert!(!app.take_close_approved());
    assert_eq!(app.recipe, draft);
    assert!(app.recipe_dirty);
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), external);
    crate::app::custom_perks::workbench::tests::capture::write(
        &ctx,
        &output,
        "unsaved-save-conflict",
    );
    let position = label(&output, "Cancel").unwrap().center();
    for events in tap(position) {
        unsaved_frame(&mut app, &ctx, 480.0, events);
    }
    assert!(app.pending_recipe_action.is_none());
    assert_eq!(app.recipe, draft);
    app.request_recipe_action(PendingRecipeAction::Close);
    for _ in 0..3 {
        output = unsaved_frame(&mut app, &ctx, 480.0, vec![]);
    }
    let position = label(&output, "Discard and Close").unwrap().center();
    for events in tap(position) {
        unsaved_frame(&mut app, &ctx, 480.0, events);
    }
    assert!(app.take_close_approved());
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), external);
}

#[test]
fn incomplete_name_edits_are_retained_but_cannot_be_saved_or_built() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.scan().unwrap().entries[0].path.clone();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    app.enabled_recipe_paths.insert(path.clone());
    let original = app.recipe.clone();
    let bytes = std::fs::read(&path).unwrap();
    app.edit_weapon_name(String::new());
    assert_eq!(app.invalid_weapon_name.as_ref().unwrap().0, "");
    assert_eq!(
        app.recipe, original,
        "an intermediate edit must not corrupt identities"
    );
    assert!(app.recipe_dirty);
    let (output, overflow) = render(480.0, |ui| app.draw_weapon_name(ui, 90.0));
    assert!(text(&output).contains("Finish editing the weapon name"));
    assert!(overflow <= 1.0);
    let library = app.recipe_library.clone().unwrap();
    let (output, _) = render(1320.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_recipe_library_primary(ui, &library, &mut false);
    });
    let tree = output.platform_output.accesskit_update.as_ref().unwrap();
    let save = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Save Changes"))
        .expect("Save button is exposed");
    assert!(
        save.1.is_disabled(),
        "invalid names must visibly disable Save"
    );
    app.save_library_recipe();
    app.save_recipe_copy();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!app.duplicate_recipe());
    assert!(app.batch_request().unwrap_err().contains("Finish editing"));
}

#[test]
fn guarded_save_accepts_formatting_changes_but_refuses_missing_sources() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let baseline = WeaponRecipe::load_json(&path).unwrap();
    std::fs::write(&path, serde_json::to_vec(&baseline).unwrap()).unwrap();
    let mut edited = baseline.clone();
    edited.flavor = "My safe edit".into();
    library
        .save_existing_if_unchanged(&path, &baseline, &edited)
        .unwrap();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), edited);
    std::fs::remove_file(&path).unwrap();
    assert!(
        library
            .save_existing_if_unchanged(&path, &edited, &baseline)
            .is_err()
    );
    assert!(
        !path.exists(),
        "a missing source must not be silently recreated"
    );
}
