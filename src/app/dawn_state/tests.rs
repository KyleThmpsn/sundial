//! Real view interaction and history checks. All state is in temporary fixtures.
use crate::app::tests::driver::*;
use crate::app::*;
use crate::catalog::{InventoryMetadata, InventoryScope, ItemStackability};

fn item(hash: u64, name: &str) -> crate::catalog::ItemDef {
    crate::catalog::ItemDef {
        hash,
        name: name.into(),
        type_name: "Test Item".into(),
        bucket_hash: 0,
        class_type: 3,
        default_plugs: vec![],
        sockets: vec![],
        abilities: Default::default(),
    }
}

fn seeded(directory: &TestDirectory) -> SundialApp {
    let path = directory.0.join("player-state.db");
    crate::persistence::dawn_account::tests::create_fixture(&path);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "UPDATE character_items SET postmaster=1 WHERE location=1;
        INSERT INTO vendor_progress VALUES('9EAA300100100100',0,20,4000,1);
        INSERT INTO missions VALUES('9EAA300100100101',123,456,2,7,9,0,100);",
        )
        .unwrap();
    let mut app = app(directory.0.clone());
    app.document =
        WorkspaceDocument::load(serde_json::json!({"version":6}), &app.settings_path, true);
    app.preferences.experimental_progression = true;
    app.manifest = Manifest::for_test_with_inventory(
        vec![
            item(2715114534, "Mail Weapon"),
            item(4070132608, "Equipped Helmet"),
        ],
        HashMap::new(),
        HashMap::from([
            (
                2715114534,
                InventoryMetadata {
                    scope: InventoryScope::Character,
                    native_bucket_id: 0,
                    stackability: ItemStackability::Instanced,
                    max_stack_size: Some(1),
                    bucket_capacity: Some(10),
                },
            ),
            (
                4070132608,
                InventoryMetadata {
                    scope: InventoryScope::Character,
                    native_bucket_id: 3,
                    stackability: ItemStackability::Instanced,
                    max_stack_size: Some(1),
                    bucket_capacity: Some(10),
                },
            ),
        ]),
    );
    app.persisted_document = app.document.clone();
    app
}

fn frame(
    app: &mut SundialApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    progression: bool,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(840.0, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if progression {
                    app.draw_progression_page(ui);
                } else {
                    app.draw_character_inventory_page(ui);
                }
            });
        },
    )
}

fn click(app: &mut SundialApp, ctx: &egui::Context, pos: egui::Pos2, progression: bool) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            crate::test_support::primary_press(pos, pressed),
            progression,
        );
    }
}

fn text_x(output: &egui::FullOutput, value: &str) -> f32 {
    fn find(shape: &egui::Shape, value: &str) -> Option<f32> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == value => Some(text.pos.x),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, value)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|s| find(&s.shape, value))
        .unwrap_or_else(|| panic!("Missing text {value}"))
}

#[test]
fn postmaster_recover_records_one_edit_and_survives_save_undo_redo() {
    let dir = TestDirectory::new("postmaster-ui");
    let mut app = seeded(&dir);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    frame(&mut app, &ctx, vec![], false);
    let output = frame(&mut app, &ctx, vec![], false);
    click(&mut app, &ctx, button(&output, "Postmaster").0, false);
    let output = frame(&mut app, &ctx, vec![], false);
    assert!(contains_text(&output, "Mail Weapon"));
    assert!(text_x(&output, "1") >= text_x(&output, "Quantity"));
    crate::test_support::capture::write(&ctx, &output, "dawn-postmaster");
    click(&mut app, &ctx, button(&output, "Recover").0, false);
    assert!(
        !app.document
            .dawn_account()
            .unwrap()
            .is_postmaster(0x400000000000001A),
        "{}",
        app.status
    );
    assert_eq!(app.undo_history.len(), 1);
    app.document.save_dawn().unwrap();
    app.mark_document_saved();
    app.undo();
    assert!(
        app.document
            .dawn_account()
            .unwrap()
            .is_postmaster(0x400000000000001A)
    );
    app.document.save_dawn().unwrap();
    app.mark_document_saved();
    app.redo();
    assert!(
        !app.document
            .dawn_account()
            .unwrap()
            .is_postmaster(0x400000000000001A)
    );
    app.document.save_dawn().unwrap();
}

#[test]
fn dawn_vendor_and_mission_tabs_render_and_clear_checkpoint_is_undoable() {
    let dir = TestDirectory::new("dawn-activity-ui");
    let mut app = seeded(&dir);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    app.progression_section = ProgressionSection::Vendors;
    frame(&mut app, &ctx, vec![], true);
    let output = frame(&mut app, &ctx, vec![], true);
    assert!(contains_text(&output, "Packages Claimed"));
    assert!(contains_text(&output, "Available"));
    assert!(text_x(&output, "4000") >= text_x(&output, "Reputation"));
    assert!(text_x(&output, "4000") < text_x(&output, "Packages Claimed"));
    crate::test_support::capture::write(&ctx, &output, "dawn-vendors");
    app.progression_section = ProgressionSection::Missions;
    let output = frame(&mut app, &ctx, vec![], true);
    click(
        &mut app,
        &ctx,
        button(&output, "Mission 0x0000007B").0,
        true,
    );
    frame(&mut app, &ctx, vec![], true);
    let output = frame(&mut app, &ctx, vec![], true);
    crate::test_support::capture::write(&ctx, &output, "dawn-missions");
    click(&mut app, &ctx, button(&output, "Clear Checkpoint").0, true);
    assert_eq!(
        app.document
            .dawn_account()
            .unwrap()
            .activity_state()
            .missions[0]
            .checkpoint,
        0
    );
    assert_eq!(app.undo_history.len(), 1);
    app.undo();
    assert_eq!(
        app.document
            .dawn_account()
            .unwrap()
            .activity_state()
            .missions[0]
            .checkpoint,
        456
    );
    assert!(
        app.document
            .dawn_account()
            .unwrap()
            .reward_debts()
            .is_empty()
    );
}
