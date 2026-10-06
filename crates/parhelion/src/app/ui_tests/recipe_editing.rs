use super::*;

fn unsaved_frame(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, workbench_style);
            app.draw_discard_confirmation(ctx);
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
fn incomplete_name_edits_are_guarded_and_can_be_discarded_or_completed() {
    let mut app = PackageAuthoringApp::default();
    let original = app.recipe.clone();
    app.edit_weapon_name(String::new());
    assert!(!app.request_recipe_action(PendingRecipeAction::New(ItemKind::Weapon)));
    assert!(app.pending_recipe_action.is_some());
    app.discard_recipe_changes();
    assert!(app.invalid_weapon_name.is_none());
    assert!(!app.recipe_dirty);
    assert!(app.batch_request().is_ok());
    app.edit_weapon_name(String::new());
    app.edit_weapon_name("A Different Tomorrow".into());
    assert!(app.invalid_weapon_name.is_none());
    assert_eq!(app.recipe.name, "A Different Tomorrow");
    assert!(app.recipe.identity_is_name_derived());
    assert_ne!(app.recipe.identity, original.identity);
}

#[test]
fn recipe_changes_invalidate_results_even_while_already_dirty() {
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        ..Default::default()
    };
    app.synchronize_recipe_dirty();
    assert!(
        app.build_status_open,
        "an unchanged frame preserves results"
    );

    app.recipe.flavor.push_str(" first edit");
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    assert!(!app.build_status_open);

    app.build_status_open = true;
    app.build_dialog_step = BuildDialogStep::ReviewInstall;
    app.recipe.flavor.push_str(" second edit");
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    assert!(!app.build_status_open);
    assert_eq!(app.build_dialog_step, BuildDialogStep::Build);

    app.build_status_open = true;
    app.synchronize_recipe_dirty();
    assert!(app.build_status_open, "observing the same edit is a no-op");
}

#[test]
fn restoring_original_content_clears_dirty_but_unsaved_copies_stay_protected() {
    let mut app = PackageAuthoringApp::default();
    app.observed_recipe = app.recipe.clone();
    app.recipe.flavor.push_str(" changed");
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    app.recipe = app.recipe_baseline.clone();
    app.synchronize_recipe_dirty();
    assert!(!app.recipe_dirty);
    assert!(app.duplicate_recipe());
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    app.discard_recipe_changes();
    assert!(
        app.recipe_dirty,
        "an unsaved copy must still require a save or discard on close"
    );
}

#[test]
fn save_conflicts_keep_both_versions_and_remain_visible_outside_settings() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    let path = library.root().join("every-end.parhelion.json");
    assert!(app.open_recipe_path(&path));
    let mut external = app.recipe.clone();
    external.flavor = "Changed outside the workbench".into();
    library.save_existing(&path, &external).unwrap();
    app.recipe.flavor = "My unsaved draft".into();
    app.synchronize_recipe_dirty();
    app.recipe_library = Some(library);
    app.save_library_recipe();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), external);
    assert_eq!(app.recipe.flavor, "My unsaved draft");
    assert!(app.recipe_dirty);
    app.log.push(LogEntry::info("A later background event"));
    let (output, overflow) = render(480.0, |ui| app.draw_action_error(ui));
    assert!(text(&output).contains("changed on disk"));
    assert!(overflow <= 1.0, "save error overflow: {overflow}");
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

#[test]
fn visible_plug_safety_selection_survives_menu_close() {
    fn frame(
        ctx: &egui::Context,
        app: &mut PackageAuthoringApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    ui.horizontal(|ui| app.draw_socket_options(ui, false));
                });
            },
        )
    }
    fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, pos: egui::Pos2) {
        for events in crate::test_support::driver::tap(pos) {
            frame(ctx, app, events);
        }
    }
    let ctx = egui::Context::default();
    let mut app = PackageAuthoringApp {
        plug_selection_mode: PlugSelectionMode::SocketAndGearType,
        ..Default::default()
    };
    let before = app.recipe.clone();
    for target in [PlugSelectionMode::AnyPlug, PlugSelectionMode::Supported] {
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(text(&output).contains("Plug Safety"));
        let current = app.plug_selection_mode.label();
        click(
            &ctx,
            &mut app,
            text_origin(&output, current) + egui::vec2(8.0, 6.0),
        );
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        click(
            &ctx,
            &mut app,
            text_origin(&output, target.label()) + egui::vec2(8.0, 6.0),
        );
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.plug_selection_mode, target);
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn duplicate_preserves_draft_mechanics_and_allocates_a_fresh_identity() {
    let mut app = PackageAuthoringApp {
        recipe: WeaponRecipe::from_json_str(include_str!(
            "../../../recipes/vaultbreaker.parhelion.json"
        ))
        .unwrap(),
        ..Default::default()
    };
    let original = app.recipe.clone();
    app.recipe_path = Some(PathBuf::from("original.parhelion.json"));
    app.recipe_entries.clear();
    assert!(app.duplicate_recipe());
    assert_ne!(app.recipe.namespace, original.namespace);
    assert_ne!(app.recipe.identity, original.identity);
    assert_eq!(app.recipe.donor, original.donor);
    assert_eq!(app.recipe.presentation_donor, original.presentation_donor);
    assert_eq!(app.recipe.overrides, original.overrides);
    assert!(app.recipe_path.is_none());
    let first_copy = app.recipe.clone();
    app.recipe_entries.push(RecipeLibraryEntry {
        kind: first_copy.kind,
        collection_destination: None,
        armor_class: None,
        badge: None,
        corner_icon: None,
        path: PathBuf::from("existing-copy.parhelion.json"),
        name: first_copy.name.clone(),
        namespace: first_copy.namespace.clone(),
        bundled: false,
        donor_hash: first_copy.donor.item_hash.parse_u32().unwrap(),
        type_donor_hash: first_copy.type_donor_hash(),
        identity_hash: first_copy.identity.item_hash.parse_u32().unwrap(),
        type_name: first_copy.type_name.clone(),
        class_type_name: None,
        ammo_type: first_copy.overrides.ammo_type,
        damage_type: first_copy.overrides.modern_damage_type,
        rarity: first_copy.overrides.rarity,
        icon_hash: first_copy.donor.item_hash.parse_u32().unwrap(),
        icon_edit: first_copy.overrides.icon_edit.clone(),
    });
    app.recipe = original;
    assert!(app.duplicate_recipe());
    assert_ne!(app.recipe.namespace, first_copy.namespace);
    assert!(app.recipe.name.ends_with("Copy 2"));
}

#[test]
fn opening_a_recipe_after_the_main_panel_does_not_mark_it_modified() {
    let mut app = PackageAuthoringApp::default();
    app.recipe_library = None;
    app.catalog_load_requested = true;
    // A modal opens the recipe after that frame's normal recipe-change check.
    app.recipe = WeaponRecipe::every_end();
    app.recipe_baseline = app.recipe.clone();
    app.recipe_path = Some(PathBuf::from("saved.parhelion.json"));
    app.recipe_dirty = false;
    let ctx = egui::Context::default();
    let frame = |app: &mut PackageAuthoringApp| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1320.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| app.update_ui(ctx),
        );
    };
    frame(&mut app);
    assert!(!app.recipe_dirty);
    app.recipe.flavor.push_str(" Edited.");
    frame(&mut app);
    assert!(
        app.recipe_dirty,
        "real edits must still trigger save/discard protection"
    );
}
