//! Headless checks for the importer window against a fake catalog.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;

fn weapon(hash: u32, name: &str, kind: &str, installed: bool) -> Weapon {
    Weapon {
        hash,
        name: name.into(),
        weapon_type: kind.into(),
        present_in_native: installed,
        dummy: false,
        icon_index: None,
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
    app.importer.browser.records.insert(
        0x1001,
        status::Record {
            working: true,
            note: "Confirmed in game on 2026-09-01.".into(),
        },
    );
    let mut types = BTreeMap::new();
    for weapon in &app.importer.weapons {
        *types.entry(weapon.weapon_type.clone()).or_insert(0) += 1;
    }
    app.importer.browser.types = types;
    app
}

fn render(app: &mut PackageAuthoringApp, name: &str) -> egui::FullOutput {
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 640.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_importer_contents(ui));
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
fn welcome_state_explains_the_first_step_without_a_folder() {
    let mut app = PackageAuthoringApp::default();
    app.importer.settings.modern_packages = None;
    app.importer.notice.clear();
    let shown = text(&render(&mut app, "d2-importer-welcome"));
    assert!(shown.contains("No folder chosen"));
    assert!(shown.contains("Choose Folder…"));
    assert!(
        !shown.contains("Select Shown"),
        "no list controls before a folder exists"
    );
}

#[test]
fn catalog_view_lists_weapons_with_a_summary_and_says_why_import_is_blocked() {
    let mut app = app_with_catalog();
    let shown = text(&render(&mut app, "d2-importer-catalog"));
    assert!(shown.contains("6 weapons · 1 installed · 1 working"));
    assert!(shown.contains("Ace of Spades"));
    assert!(
        !shown.contains("Quickfang"),
        "installed weapons are hidden by default"
    );
    assert!(shown.contains("5 shown · 0 selected"));
    assert!(shown.contains("Import"));
    assert!(shown.contains("Blocked:"));
    assert!(shown.contains("Recipe library unavailable."));
    assert!(shown.contains("Catalog Order"));
    assert!(shown.contains("Any Status"));
}

#[test]
fn selection_and_outcome_are_summarized_in_the_action_bar() {
    let mut app = app_with_catalog();
    app.importer.selected.extend([0x1001, 0x1002, 0x1003]);
    app.importer.outcome = Some(Outcome {
        added: 2,
        failures: vec!["Recluse: the donor skeleton has no matching rig".into()],
        show_failures: true,
        cancelled: true,
    });
    let shown = text(&render(&mut app, "d2-importer-selection"));
    assert!(shown.contains("5 shown · 3 selected"));
    assert!(shown.contains("Import 3 Weapons"));
    assert!(shown.contains("Added 2 recipes."));
    assert!(shown.contains("1 weapon failed."));
    assert!(shown.contains("Recluse: the donor skeleton has no matching rig"));
    assert!(shown.contains("Stopped."));
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
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_importer_contents(ui));
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
fn filters_that_hide_everything_offer_a_reset() {
    let mut app = app_with_catalog();
    app.importer.browser.query = "no such weapon".into();
    let shown = text(&render(&mut app, "d2-importer-empty-filter"));
    assert!(shown.contains("No matches"));
    assert!(shown.contains("Clear Filters"));
    app.importer.browser.clear_filters();
    let shown = text(&render(&mut app, "d2-importer-cleared"));
    assert!(shown.contains("Ace of Spades"));
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

#[test]
fn import_progress_reports_the_weapon_and_step_in_the_action_bar() {
    let mut app = app_with_catalog();
    let (_sender, receiver) = mpsc::channel();
    app.importer.receiver = Some(receiver);
    app.importer.import_started = Some(Instant::now());
    app.importer.importing = Some(Progress {
        total: 3,
        done: 1,
        slots: vec![
            Some(("Midnight Coup".into(), "Converting materials…".into())),
            Some(("Zephyr".into(), "Opening source packages…".into())),
            None,
        ],
    });
    let shown = text(&render(&mut app, "d2-importer-progress"));
    assert!(shown.contains("1 / 3"));
    assert!(shown.contains("Midnight Coup"));
    assert!(shown.contains("Converting materials…"));
    assert!(shown.contains("Zephyr"));
    assert!(shown.contains("Cancel"));
    app.importer.receiver = None;
}

/// Composites real icons from the configured modern build into the capture directory.
#[test]
#[ignore = "reads the modern Destiny 2 packages named in d2-importer.json"]
fn layered_icons_composite_from_the_configured_build() {
    let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let app = PackageAuthoringApp::default();
    let modern = app
        .importer
        .settings
        .modern_packages
        .clone()
        .expect("d2-importer.json names a modern build");
    let root = data_root().unwrap();
    let cache: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("importer/catalog/weapon-cache.json")).unwrap(),
    )
    .unwrap();
    let mut indices: Vec<usize> = cache["weapons"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|weapon| weapon["icon_index"].as_u64())
        .map(|index| index as usize)
        .collect();
    indices.sort_unstable();
    indices.dedup();
    let step = (indices.len() / 8).max(1);
    let indices: Vec<usize> = indices.into_iter().step_by(step).take(8).collect();
    let mut reader = parhelion_import::d2_mot::reader::Reader::discovery(
        &modern,
        &root.join("importer/icons"),
        true,
    )
    .unwrap();
    let mut report = String::new();
    for index in indices {
        let layers = parhelion_import::d2_mot::icon::read_layers(&mut reader, index).unwrap();
        for (slot, layer) in layers.iter().enumerate() {
            report.push_str(&format!(
                "{index} layer {slot} from 0x{:02X}: texture {:08X} format {} {}x{}",
                layer.slot, layer.texture, layer.format, layer.width, layer.height
            ));
            report.push(char::from(10));
        }
        let (size, rgba) = icons::composite(&layers).unwrap();
        image::save_buffer(
            directory.join(format!("icon-{index}.png")),
            &rgba,
            size[0] as u32,
            size[1] as u32,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    std::fs::write(directory.join("icon-layers.txt"), report).unwrap();
}
