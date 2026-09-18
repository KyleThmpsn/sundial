use super::*;

#[test]
fn bundled_build_checkbox_toggles_all_defaults_without_touching_custom_selection() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut custom = WeaponRecipe::every_end();
    custom
        .rename_authored_item("Checkbox custom weapon")
        .unwrap();
    let custom_path = library.save_new(&custom).unwrap();
    let entries = library.scan().unwrap().entries;
    let mut selected = library.enabled_paths(&entries).unwrap();
    selected.insert(custom_path.clone());
    let before = selected.clone();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        recipe_entries: entries,
        enabled_recipe_paths: selected,
        ..Default::default()
    };
    app.open_build_selection();
    app.build_selection_query = "no matching weapons".into();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 760.0),
        )),
        ..Default::default()
    };
    for expected in [BTreeSet::from([custom_path]), before.clone()] {
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
        }
        let count = app
            .recipe_entries
            .iter()
            .filter(|entry| entry.bundled)
            .count();
        let included = app.build_selection_draft.as_ref().unwrap().len() - 1;
        let label = format!("Include default Parhelion weapons ({included}/{count})");
        let pos = text_origin(&output, &label) + egui::vec2(4.0, 4.0);
        for pressed in [true, false] {
            let mut click = input.clone();
            click.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = ctx.run(click, |ctx| app.draw_library_windows(ctx));
        }
        assert_eq!(app.build_selection_draft.as_ref(), Some(&expected));
        assert_eq!(
            app.enabled_recipe_paths, before,
            "checkbox must remain transactional"
        );
    }
    app.apply_build_selection(BTreeSet::new()).unwrap();
    assert!(
        library
            .enabled_paths(&app.recipe_entries)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn build_pages_replace_each_other_and_selection_precedes_build() {
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        latest_build: Some(Ok(BuildReport {
            weapons: Vec::new(),
            run_directory: PathBuf::from("staged-run"),
            manifest_path: PathBuf::from("staged-run/manifest.json"),
            artifacts: Vec::new(),
            selection_fingerprint: "fingerprint".into(),
            staged_recipe_paths: Vec::new(),
        })),
        ..Default::default()
    };
    let before = app.recipe.clone();
    let (output, _) = render(1320.0, |ui| app.draw_actions(ui));
    assert!(
        text_origin(&output, "0 weapons selected for build…").x
            < text_origin(&output, "Build & Stage").x
    );
    let action_y = text_origin(&output, "Build & Stage").y;
    for label in ["0 weapons selected for build…", "Build & Install Status…"] {
        assert!((text_origin(&output, label).y - action_y).abs() < 1.0);
    }
    for step in [
        BuildDialogStep::Build,
        BuildDialogStep::ReviewInstall,
        BuildDialogStep::Install,
    ] {
        app.build_dialog_step = step;
        let (output, _) = render(1320.0, |ui| app.draw_build_status_window(ui.ctx()));
        let labels = text(&output);
        assert_eq!(labels.matches("Build & Install").count(), 1);
        assert_eq!(
            labels.contains("Build Validated"),
            step == BuildDialogStep::Build
        );
        assert!(
            labels.contains("1. Build")
                && labels.contains("2. Review")
                && labels.contains("3. Install")
        );
        assert_eq!(
            labels.contains("Review Installation"),
            step != BuildDialogStep::Install
        );
        assert_eq!(
            labels.contains("This replaces your installed custom weapon set."),
            step == BuildDialogStep::ReviewInstall
        );
        assert_eq!(
            labels.contains("No installation result"),
            step == BuildDialogStep::Install
        );
        assert_eq!(app.recipe, before);
        assert!(app.install_receiver.is_none());
    }
}

#[test]
fn filtered_build_selection_changes_only_the_draft_and_cancel_discards_it() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let shown = entries[0].path.clone();
    let hidden = entries[1].path.clone();
    let name = entries[0].name.clone();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        enabled_recipe_paths: BTreeSet::from([hidden.clone()]),
        ..Default::default()
    };
    app.open_build_selection();
    app.build_selection_query = name.to_lowercase();
    let before = app.recipe.clone();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(902.0, 760.0),
        )),
        ..Default::default()
    };
    let click_label = |app: &mut PackageAuthoringApp, label: &str| {
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
        }
        let pos = text_origin(&output, label) + egui::vec2(5.0, 5.0);
        assert!(pos.y < 760.0, "{label} must remain reachable");
        for pressed in [true, false] {
            let mut click = input.clone();
            click.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = ctx.run(click, |ctx| app.draw_library_windows(ctx));
        }
    };
    // One checkbox covers the search: ticking it adds every shown recipe, clearing it removes
    // them, and a recipe the search hides keeps its membership either way.
    click_label(&mut app, "Select all shown");
    assert_eq!(
        app.build_selection_draft.as_ref().unwrap(),
        &BTreeSet::from([shown.clone(), hidden.clone()])
    );
    click_label(&mut app, "Select all shown");
    assert_eq!(
        app.build_selection_draft.as_ref().unwrap(),
        &BTreeSet::from([hidden.clone()])
    );
    click_label(&mut app, &name);
    assert!(app.build_selection_draft.as_ref().unwrap().contains(&shown));
    assert_eq!(app.enabled_recipe_paths, BTreeSet::from([hidden]));
    click_label(&mut app, "Cancel");
    assert!(app.build_selection_draft.is_none());
    assert_eq!(app.recipe, before);
    app.open_build_selection();
    assert!(app.build_selection_query.is_empty());
    assert_eq!(
        app.build_selection_draft.as_ref(),
        Some(&app.enabled_recipe_paths)
    );
}

#[test]
fn workbench_tabs_keep_recipe_and_build_selection_unchanged() {
    let mut app = PackageAuthoringApp::default();
    let before = app.recipe.clone();
    let selected = app.enabled_recipe_paths.clone();
    assert_eq!(app.workbench_page, WorkbenchPage::Weapon);
    for page in WorkbenchPage::ALL {
        app.workbench_page = page;
        let (_, overflow) = render(900.0, |ui| {
            app.draw_workbench_tabs(ui);
            app.draw_recipe_editor(ui);
        });
        assert!(overflow < 1.0, "{page:?} overflow: {overflow}");
        assert_eq!(app.recipe, before);
        assert_eq!(app.enabled_recipe_paths, selected);
    }
}

#[test]
fn collection_capacity_separates_excluded_draft_from_selected_build() {
    let mut app = PackageAuthoringApp::default();
    app.recipe_entries.clear();
    app.enabled_recipe_paths.clear();
    app.recipe_path = Some(PathBuf::from("draft.parhelion.json"));
    app.recipe.overrides.rarity = Some(RecipeRarity::Legendary);
    app.recipe.overrides.collection_destination = Some(crate::collection::Destination {
        ammo: crate::collection::Ammo::Special,
        family: crate::collection::Family::Sidearms,
    });
    app.recipe.overrides.badge = Some(crate::presentation::Badge {
        name: "Travelers".into(),
        ..Default::default()
    });
    let before = app.recipe.clone();
    for included in [false, true] {
        if included {
            app.enabled_recipe_paths
                .insert(app.recipe_path.clone().unwrap());
        }
        for width in [320.0, 900.0] {
            let (output, overflow) = render(width, |ui| app.draw_collection_capacity(ui));
            let labels = text(&output);
            assert!(labels.contains(if included {
                "5 / 96 Custom Nodes Used"
            } else {
                "0 / 96 Custom Nodes Used"
            }));
            assert_eq!(labels.contains("This recipe adds 5 nodes."), !included);
            assert!(
                overflow < 1.0,
                "Collections overflow at {width}: {overflow}"
            );
            assert_eq!(app.recipe, before);
        }
    }
}

#[test]
fn reopening_build_selection_preserves_its_draft_and_error() {
    let mut app = PackageAuthoringApp::default();
    app.enabled_recipe_paths.insert("first.json".into());
    app.open_build_selection();
    app.build_selection_draft.as_mut().unwrap().clear();
    app.build_selection_error = Some("Write failed".into());
    app.open_build_selection();
    assert!(app.build_selection_draft.as_ref().unwrap().is_empty());
    assert_eq!(app.build_selection_error.as_deref(), Some("Write failed"));
    assert_eq!(app.enabled_recipe_paths.len(), 1);
    assert!(!app.current_recipe_is_in_build());
    app.recipe_path = Some("first.json".into());
    assert!(app.current_recipe_is_in_build());
}

#[test]
fn build_checks_external_changes_without_replacing_the_open_draft() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    let path = library.root().join("every-end.parhelion.json");
    assert!(app.open_recipe_path(&path));
    app.enabled_recipe_paths.insert(path.clone());
    assert!(app.batch_request().is_ok());
    let mut external = app.recipe.clone();
    external.flavor = "External change".into();
    library.save_existing(&path, &external).unwrap();
    assert!(app.batch_request().unwrap_err().contains("changed on disk"));
    assert_ne!(app.recipe.flavor, external.flavor);
}

#[test]
fn saving_custom_perk_stats_does_not_create_a_false_disk_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    let path = library.root().join("every-end.parhelion.json");
    assert!(app.open_recipe_path(&path));
    app.recipe_library = Some(library.clone());
    app.enabled_recipe_paths.insert(path.clone());
    let mut perk = crate::perk::PerkRecipe::new();
    perk.stats = [29, 3, 21, 13]
        .into_iter()
        .map(|definition_index| crate::WeaponStatOverride {
            definition_index,
            value: 10,
        })
        .collect();
    app.recipe
        .overrides
        .socket_plug_variants
        .push(perk.at_socket(0, 0));
    app.save_library_recipe();
    assert!(!app.recipe_dirty);
    app.enabled_recipe_paths.insert(path.clone());
    assert!(app.batch_request().is_ok());
    app.recipe.flavor = "Another local edit".into();
    app.save_edits_for_build().unwrap();
    assert!(app.batch_request().is_ok());
    let mut external = WeaponRecipe::load_json(&path).unwrap();
    external
        .overrides
        .socket_plug_variants
        .last_mut()
        .unwrap()
        .investment_stats[0]
        .value += 1;
    library.save_existing(&path, &external).unwrap();
    assert!(app.batch_request().unwrap_err().contains("changed on disk"));
    assert!(
        library
            .save_existing_if_unchanged(&path, &app.recipe_baseline, &app.recipe)
            .unwrap_err()
            .contains("changed on disk")
    );
}

#[test]
fn escape_discards_build_selection_without_changing_the_document() {
    let mut app = PackageAuthoringApp::default();
    let selected = BTreeSet::from([PathBuf::from("saved.parhelion.json")]);
    app.enabled_recipe_paths = selected.clone();
    app.build_selection_draft = Some(BTreeSet::new());
    let before = app.recipe.clone();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.draw_library_windows(ctx));
    assert!(app.build_selection_draft.is_none());
    assert_eq!(app.enabled_recipe_paths, selected);
    assert_eq!(app.recipe.clone(), before);
}

#[test]
fn build_selection_is_explicit_and_failed_commits_do_not_change_it() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    app.recipe_entries = library.scan().unwrap().entries;
    app.enabled_recipe_paths = library.enabled_paths(&app.recipe_entries).unwrap();
    app.recipe_library = Some(library);
    let before = app.enabled_recipe_paths.clone();
    let recipe_before = app.recipe.clone();
    app.build_selection_draft = Some(BTreeSet::new());
    assert_eq!(
        app.enabled_recipe_paths, before,
        "draft changes must not commit"
    );
    app.build_selection_draft = None; // Cancel/close.
    assert_eq!(app.enabled_recipe_paths, before);
    assert!(
        app.apply_build_selection([directory.path().join("outside.json")].into())
            .is_err()
    );
    assert_eq!(app.enabled_recipe_paths, before);
    app.apply_build_selection(BTreeSet::new()).unwrap();
    assert!(app.enabled_recipe_paths.is_empty());
    assert!(
        app.recipe_library
            .as_ref()
            .unwrap()
            .enabled_paths(&app.recipe_entries)
            .unwrap()
            .is_empty()
    );
    assert_eq!(app.recipe.clone(), recipe_before);
}

#[test]
fn a_long_build_selection_pins_its_footer_to_the_bottom_of_the_window() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    for index in 0..40 {
        let mut recipe = WeaponRecipe::every_end();
        recipe
            .rename_authored_item(format!("Footer fit weapon {index:02}"))
            .unwrap();
        library.save_new(&recipe).unwrap();
    }
    let entries = library.scan().unwrap().entries;
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        recipe_entries: entries,
        ..Default::default()
    };
    app.open_build_selection();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(902.0, 760.0));
    let input = egui::RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
    }
    // The list claims exactly the height the footer leaves behind: more recipes than fit can
    // neither push the buttons off the bottom of the screen nor strand them under a blank band.
    let window = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Weapons in This Build")))
        .expect("the build selection window must be on screen");
    assert!(
        window.bottom() <= screen.bottom(),
        "window bottom {} ran past the screen at {}",
        window.bottom(),
        screen.bottom()
    );
    let apply = text_origin(&output, "Apply Selection");
    assert!(
        apply.y > window.bottom() - 48.0,
        "the footer sat at {} instead of just above the window bottom {}",
        apply.y,
        window.bottom()
    );
}
