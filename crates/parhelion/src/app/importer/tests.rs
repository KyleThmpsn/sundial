//! Headless checks for the importer window against a fake catalog.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;

#[test]
fn shader_selection_and_reimport_keep_authored_library_edits() {
    let mut app = app_with_catalog();
    app.importer
        .weapons
        .push(weapon(0x2001, "Source Shader", "Shader", false));
    app.importer.browser.kind = "Shader".into();
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for events in [
        vec![],
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        vec![key(egui::Key::Space, egui::Modifiers::NONE)],
        // The footer is laid out before the list handles its keyboard input.
        vec![],
    ] {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_importer_contents(ui));
            },
        );
    }
    assert_eq!(
        app.importer.selected.iter().copied().collect::<Vec<_>>(),
        [0x2001]
    );
    assert!(text(&output).contains("Import 1 Shader"));
    capture::write(&ctx, &output, "d2-importer-shader-selection");

    let root = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(root.path().join("library")).unwrap();
    let mut shader = WeaponRecipe::new_unbound_kind(ItemKind::Shader).unwrap();
    shader.set_donor(0x3001, "Native Shader");
    shader.name = "Source Shader".into();
    let converted = root.path().join("converted.json");
    shader.save_json(&converted).unwrap();
    let path = save_imported_recipe(&library, &[], &converted).unwrap();
    let mut authored = WeaponRecipe::load_json(&path).unwrap();
    authored.name = "My Shader".into();
    authored.flavor = "My description".into();
    authored.save_json(&path).unwrap();
    let saved = save_imported_recipe(
        &library,
        &[(path.clone(), shader.identity.item_hash.parse_u32().unwrap())],
        &converted,
    )
    .unwrap();
    let saved = WeaponRecipe::load_json(saved).unwrap();
    assert_eq!(saved.kind, ItemKind::Shader);
    assert_eq!(saved.name, "My Shader");
    assert_eq!(saved.flavor, "My description");
    assert!(
        saved.presentation_donor.is_none(),
        "Shaders must build through their shader base"
    );
}

fn weapon(hash: u32, name: &str, kind: &str, installed: bool) -> Weapon {
    Weapon {
        hash,
        name: name.into(),
        weapon_type: kind.into(),
        present_in_native: installed,
        native_item: false,
        dummy: false,
        icon_index: None,
        ..Weapon::default()
    }
}

fn app_with_catalog() -> PackageAuthoringApp {
    let mut app = PackageAuthoringApp::default();
    app.importer.settings.modern_packages =
        Some(PathBuf::from("fixtures").join("modern").join("packages"));
    app.importer.read_requested = true;
    app.importer.notice.clear();
    app.importer.browser = browser::Browser::default();
    app.importer.weapons = vec![
        weapon(0x1001, "Ace of Spades", "Hand Cannon", false),
        weapon(0x1002, "Midnight Coup", "Hand Cannon", false),
        weapon(0x1003, "Zephyr", "Sword", false),
        weapon(0x1004, "Quickfang", "Sword", true),
        weapon(0x1005, "Riskrunner", "Submachine Gun", false),
        weapon(0x1006, "Recluse", "Submachine Gun", false),
    ];
    app
}

/// Browse through real controls, preserve hidden selections and reload the saved view.
/// No package reads, installation writes or personal data paths are used.
#[test]
#[allow(clippy::cognitive_complexity)]
fn source_filters_selection_and_saved_views_work_at_both_window_sizes() {
    for width in [980.0, 680.0] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("importer-view.json");
        std::fs::write(
            &path,
            r#"{"status":"Working","order":"Status","show_installed":false}"#,
        )
        .unwrap();
        let mut app = app_with_catalog();
        app.importer.view_path = Some(path.clone());
        app.importer.browser.apply_view(browser::View::load(&path));
        app.importer.weapons = serde_json::from_value(serde_json::json!([
            {"hash":1,"name":"Source Kinetic Exotic","weapon_type":"Hand Cannon","bucket_hash":1498876634,"rarity":5,"ammo":1,"damage":"kinetic","present_in_native":false},
            {"hash":2,"name":"Source Arc Exotic","weapon_type":"Submachine Gun","bucket_hash":2465295065_u32,"rarity":5,"ammo":1,"damage":"arc","present_in_native":false},
            {"hash":3,"name":"Source Arc Legendary","weapon_type":"Submachine Gun","bucket_hash":2465295065_u32,"rarity":4,"ammo":1,"damage":"arc","present_in_native":false},
            {"hash":4,"name":"Source Heavy Exotic","weapon_type":"Sword","bucket_hash":953998645,"rarity":5,"ammo":3,"damage":"arc","present_in_native":false},
            {"hash":5,"name":"Source Titan Armor","weapon_type":"Chest Armor","bucket_hash":14239492,"class_type":0,"rarity":5,"present_in_native":false},
            {"hash":6,"name":"Source Hunter Armor","weapon_type":"Chest Armor","bucket_hash":14239492,"class_type":1,"rarity":5,"present_in_native":false},
            {"hash":7,"name":"Installed Source","weapon_type":"Submachine Gun","bucket_hash":2465295065_u32,"rarity":5,"ammo":1,"damage":"arc","present_in_native":true},
            {"hash":8,"name":"Display Source","weapon_type":"Submachine Gun","bucket_hash":2465295065_u32,"rarity":5,"ammo":1,"damage":"arc","dummy":true,"present_in_native":false}
        ])).unwrap();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        filter_frame(&mut app, &ctx, width, Vec::new());
        let mut output = filter_frame(&mut app, &ctx, width, Vec::new());
        assert_eq!(visible_hashes(&app), [2, 3, 4, 6, 1, 5]);
        assert!(!text(&output).contains("Not Tested"));
        assert!(!text(&output).contains(" working"));
        capture::write(
            &ctx,
            &output,
            &format!("d2-importer-browse-{}", width as u32),
        );

        choose_filter(&mut app, &ctx, width, &mut output, "Rarity", "Exotic");
        choose_filter(&mut app, &ctx, width, &mut output, "Item Kind", "Weapon");
        choose_filter(&mut app, &ctx, width, &mut output, "Damage Type", "Arc");
        choose_filter(&mut app, &ctx, width, &mut output, "Ammo Type", "Primary");
        choose_filter(
            &mut app,
            &ctx,
            width,
            &mut output,
            "Equipment Slot",
            "Energy",
        );
        choose_filter(
            &mut app,
            &ctx,
            width,
            &mut output,
            "Item Type",
            "Submachine Gun",
        );
        assert_eq!(visible_hashes(&app), [2]);
        assert!(text(&output).contains("Source Arc Exotic"));
        click_control(&mut app, &ctx, width, &mut output, "Select Shown");
        assert_eq!(app.importer.selected, BTreeSet::from([2]));

        choose_filter(&mut app, &ctx, width, &mut output, "Ammo Type", "Heavy");
        assert!(text(&output).contains("No Matches"));
        assert!(visible_hashes(&app).is_empty());
        click_control(&mut app, &ctx, width, &mut output, "Reset Filters");
        assert_eq!(visible_hashes(&app), [2, 3, 4, 6, 1, 5]);
        assert_eq!(app.importer.selected, BTreeSet::from([2]));

        choose_filter(&mut app, &ctx, width, &mut output, "Item Kind", "Armor");
        choose_filter(&mut app, &ctx, width, &mut output, "Armor Class", "Titan");
        assert_eq!(visible_hashes(&app), [5]);
        // Search words can be entered in any order, as in the other pickers.
        click_control(&mut app, &ctx, width, &mut output, "Search Items");
        output = filter_frame(
            &mut app,
            &ctx,
            width,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text("armor titan".into()),
            ],
        );
        click_control(&mut app, &ctx, width, &mut output, "Select Shown");
        assert_eq!(app.importer.selected, BTreeSet::from([2, 5]));
        assert!(text(&output).contains("2 selected"));
        let mut reopened = browser::Browser::default();
        reopened.apply_view(browser::View::load(&path));
        reopened.refresh(&app.importer.weapons);
        assert_eq!(reopened.query, "armor titan");
        assert_eq!(
            reopened
                .visible
                .iter()
                .map(|&i| app.importer.weapons[i].hash)
                .collect::<Vec<_>>(),
            [5]
        );
        capture::write(
            &ctx,
            &output,
            &format!("d2-importer-filtered-{}", width as u32),
        );

        click_control(&mut app, &ctx, width, &mut output, "Reset Filters");
        click_control(&mut app, &ctx, width, &mut output, "Show Installed");
        click_control(&mut app, &ctx, width, &mut output, "Include Dummy Items");
        assert_eq!(visible_hashes(&app).len(), 8);
        click_control(&mut app, &ctx, width, &mut output, "Reset Filters");
        assert_eq!(visible_hashes(&app), [2, 3, 4, 6, 1, 5]);
        assert_eq!(app.importer.selected_weapons().count(), 2);
    }
}

fn visible_hashes(app: &PackageAuthoringApp) -> Vec<u32> {
    app.importer
        .browser
        .visible
        .iter()
        .map(|&i| app.importer.weapons[i].hash)
        .collect()
}

fn filter_frame(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 640.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_importer_contents(ui));
        },
    );
    capture::record(&output);
    let tree = output.platform_output.accesskit_update.as_ref().unwrap();
    for (_, node) in &tree.nodes {
        if node.supports_action(egui::accesskit::Action::Click)
            && let Some(bounds) = node.bounds()
        {
            assert!(
                bounds.x0 >= 0.0 && bounds.x1 <= f64::from(width) + 1.0,
                "Control overflows: {:?} {bounds:?}",
                node.label()
            );
        }
    }
    output
}

fn click_control(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    width: f32,
    output: &mut egui::FullOutput,
    label: &str,
) {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
        .unwrap_or_else(|| panic!("Missing control: {label}\n{}", text(output)));
    let pos = egui::pos2(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    );
    filter_frame(app, ctx, width, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        *output = filter_frame(
            app,
            ctx,
            width,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    *output = filter_frame(app, ctx, width, Vec::new());
}

fn choose_filter(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    width: f32,
    output: &mut egui::FullOutput,
    filter: &str,
    value: &str,
) {
    click_control(app, ctx, width, output, filter);
    click_control(app, ctx, width, output, value);
}

fn render(app: &mut PackageAuthoringApp, name: &str) -> egui::FullOutput {
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 640.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_importer_contents(ui));
            },
        );
    }
    capture::write(&ctx, &output, name);
    output
}

fn text(output: &egui::FullOutput) -> String {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn weapons_without_a_donor_are_flagged_and_left_out_of_the_import() {
    let mut app = app_with_catalog();
    app.importer.browser.no_donor.insert(0x1003);
    app.importer.selected.extend([0x1001, 0x1002, 0x1003]);
    let shown = text(&render(&mut app, "d2-importer-no-donor"));
    assert!(shown.contains("No Donor"));
    assert!(shown.contains("Import 2 Weapons"));
    assert_eq!(app.importer.selected_weapons().count(), 2);
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn arrow_keys_move_a_cursor_and_space_toggles_it() {
    let mut app = app_with_catalog();
    let ctx = egui::Context::default();
    let run = |app: &mut PackageAuthoringApp, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_importer_contents(ui));
            },
        );
    };
    run(&mut app, Vec::new());
    run(
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    run(
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    run(&mut app, vec![key(egui::Key::Space, egui::Modifiers::NONE)]);
    assert_eq!(app.importer.browser.cursor, Some(1));
    assert_eq!(
        app.importer.selected.iter().copied().collect::<Vec<_>>(),
        [0x1002]
    );
    run(&mut app, vec![key(egui::Key::A, egui::Modifiers::COMMAND)]);
    assert_eq!(app.importer.selected.len(), 5);
}

#[test]
fn scan_errors_replace_an_empty_list_but_only_annotate_a_loaded_one() {
    let mut app = app_with_catalog();
    app.importer.weapons.clear();
    app.importer.scan_error = Some("packages folder is missing pkg 0x0F".into());
    let shown = text(&render(&mut app, "d2-importer-scan-error"));
    assert!(shown.contains("Catalog read failed"));
    assert!(shown.contains("packages folder is missing pkg 0x0F"));
    assert!(shown.contains("Try Again"));
    assert!(
        !shown.contains("Select Shown"),
        "no action bar without a catalog"
    );

    let mut app = app_with_catalog();
    app.importer.scan_error = Some("packages folder is missing pkg 0x0F".into());
    let shown = text(&render(&mut app, "d2-importer-refresh-error"));
    assert!(shown.contains("Refresh failed:"));
    assert!(
        shown.contains("Ace of Spades"),
        "the earlier catalog stays usable"
    );
}
