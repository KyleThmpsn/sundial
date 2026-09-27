//! Headless checks for the importer window against a fake catalog.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;

fn weapon(hash: u32, name: &str, kind: &str, installed: bool) -> Weapon {
    Weapon {
        hash,
        name: name.into(),
        weapon_type: kind.into(),
        present_in_native: installed,
        native_item: false,
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
