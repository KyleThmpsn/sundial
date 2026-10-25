//! Exercise deletion, capacity refresh and unknown-item recovery through the actual UI.
use crate::app::inventory::NewInventoryItem;
use crate::app::tests::driver::*;
use crate::app::*;
use crate::catalog::{InventoryMetadata, InventoryScope, ItemDef, ItemStackability};
use serde_json::json;

const UNKNOWN_HASH: u32 = 0xDEAD_BEEF;

fn inventory_app(directory: &TestDirectory, sqlite: bool, stored: usize) -> SundialApp {
    let mut app = app(directory.0.clone());
    let source = if sqlite {
        let path = directory.0.join("data/investment.sqlite3");
        crate::persistence::sqlite_account::tests::create_fixture(&path, 0);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "DELETE FROM sockets; DELETE FROM items WHERE location != 0 OR position != 0;",
            )
            .unwrap();
        json!({"version": 18})
    } else {
        let mut source: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"
        ))
        .unwrap();
        source["state"]["characters"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        source["state"]["characters"][0]["equipment"] = json!({"kinetic": {
            "instance_soid": 50, "definition_hash": 100, "level": 106,
            "quantity": 1, "plugs": null, "flags": 0,
        }});
        source["state"]["characters"][0]["inventory"] = json!([]);
        source
    };
    app.document = WorkspaceDocument::load(source, &app.settings_path, false);
    app.manifest = Manifest::for_test_with_inventory(
        vec![ItemDef {
            hash: 100,
            name: "Test Weapon".into(),
            type_name: "Weapon".into(),
            bucket_hash: SLOTS
                .iter()
                .find(|(slot, _, _)| *slot == "kinetic")
                .unwrap()
                .2,
            class_type: 3,
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        }],
        HashMap::new(),
        HashMap::from([(
            100,
            InventoryMetadata {
                scope: InventoryScope::Character,
                native_bucket_id: 0,
                stackability: ItemStackability::Instanced,
                max_stack_size: Some(1),
                bucket_capacity: Some(10),
            },
        )]),
    );
    for _ in 0..stored {
        account_workspace::add_inventory_item(
            &mut app.document,
            0,
            NewInventoryItem::single(100, 106),
        )
        .unwrap();
    }
    app.persisted_document = app.document.clone();
    app
}

fn frame(
    app: &mut SundialApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    loadout: bool,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if loadout {
                    app.draw_equipment(ui, 0);
                } else {
                    app.draw_character_inventory_page(ui);
                }
            });
        },
    )
}

fn click(app: &mut SundialApp, ctx: &egui::Context, position: egui::Pos2, loadout: bool) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            crate::test_support::primary_press(position, pressed),
            loadout,
        );
    }
}

#[test]
fn inventory_filters_cannot_hide_invalid_items_and_deleting_them_reopens_space() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("invalid-item-filters");
        let mut app = inventory_app(&directory, sqlite, 8);
        account_workspace::add_inventory_item(
            &mut app.document,
            0,
            NewInventoryItem::single(UNKNOWN_HASH, 106),
        )
        .unwrap();
        app.persisted_document = app.document.clone();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        frame(&mut app, &ctx, vec![], false);
        let output = frame(&mut app, &ctx, vec![], false);
        assert!(!button(&output, "+").1);
        app.character_inventory_query = "unrelated search".into();
        app.character_inventory_source_filter = CharacterInventorySourceFilter::Equipped;
        app.character_inventory_lock_filter = CharacterInventoryLockFilter::Locked;
        frame(&mut app, &ctx, vec![], false);
        let output = frame(&mut app, &ctx, vec![], false);
        assert!(contains_text(&output, "Invalid Item"));
        assert!(contains_text(&output, "0xDEADBEEF"));
        let (remove, enabled) = button(&output, "Delete stored item");
        assert!(enabled);
        click(&mut app, &ctx, remove, false);
        app.character_inventory_query.clear();
        app.character_inventory_source_filter = CharacterInventorySourceFilter::All;
        app.character_inventory_lock_filter = CharacterInventoryLockFilter::All;
        frame(&mut app, &ctx, vec![], false);
        let output = frame(&mut app, &ctx, vec![], false);
        assert!(!contains_text(&output, "Invalid Item"));
        assert!(button(&output, "+").1);
    }
}
