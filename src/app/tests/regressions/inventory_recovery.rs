//! Exercise deletion, capacity refresh and unknown-item recovery through the actual UI.
use super::*;
use crate::app::inventory::NewInventoryItem;
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
    app.document = WorkspaceDocument::load(source, &app.settings_path);
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
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if loadout {
                    app.draw_equipment(ui, 0);
                } else {
                    app.draw_character_inventory_page(ui);
                }
            });
        },
    )
}

pub(super) fn button(output: &egui::FullOutput, label: &str) -> (egui::Pos2, bool) {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            if node.label() != Some(label) {
                return None;
            }
            let rect = node.bounds()?;
            Some((
                egui::pos2(
                    ((rect.x0 + rect.x1) * 0.5) as f32,
                    ((rect.y0 + rect.y1) * 0.5) as f32,
                ),
                !node.is_disabled(),
            ))
        })
        .filter(|(position, _)| position.y > 0.0 && position.y < 740.0)
        .min_by(|(left, _), (right, _)| left.y.total_cmp(&right.y))
        .unwrap_or_else(|| panic!("missing visible button {label}"))
}

fn click(app: &mut SundialApp, ctx: &egui::Context, position: egui::Pos2, loadout: bool) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            loadout,
        );
    }
}

pub(super) fn contains_text(output: &egui::FullOutput, needle: &str) -> bool {
    fn contains(shape: &egui::Shape, needle: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text.contains(needle),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, needle)),
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| contains(&shape.shape, needle))
}

#[test]
fn deleting_from_a_full_bucket_refreshes_the_count_and_add_button_for_both_sources() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("inventory-delete-capacity");
        let mut app = inventory_app(&directory, sqlite, 9);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        frame(&mut app, &ctx, vec![], false);
        let output = frame(&mut app, &ctx, vec![], false);
        assert!(contains_text(&output, "10 / 10"));
        assert!(!button(&output, "+").1);
        let (remove, enabled) = button(&output, "Delete stored item");
        assert!(enabled);
        click(&mut app, &ctx, remove, false);
        assert!(app.document_repaint_pending);
        assert_eq!(app.undo_history.len(), 1);
        let output = frame(&mut app, &ctx, vec![], false);
        assert!(contains_text(&output, "9 / 10"));
        assert!(button(&output, "+").1);
        assert_eq!(
            account_workspace::character_inventory(&app.document, 0)
                .unwrap()
                .unwrap()
                .len(),
            8
        );
        assert!(app.dirty);
    }
}

#[test]
fn invalid_items_remain_visible_and_removable_in_both_loadout_layouts_and_sources() {
    for sqlite in [false, true] {
        for layout in [
            CharacterInventoryLayout::Cards,
            CharacterInventoryLayout::Panoptes,
        ] {
            let directory = TestDirectory::new("invalid-item-loadout");
            let mut app = inventory_app(&directory, sqlite, 1);
            account_workspace::add_inventory_item(
                &mut app.document,
                0,
                NewInventoryItem::single(UNKNOWN_HASH, 106),
            )
            .unwrap();
            app.persisted_document = app.document.clone();
            app.preferences.character_inventory_layout = layout;
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            frame(&mut app, &ctx, vec![], true);
            let output = frame(&mut app, &ctx, vec![], true);
            assert!(contains_text(&output, "Invalid Item"));
            assert!(contains_text(&output, "0xDEADBEEF"));
            let (remove, enabled) = button(&output, "Delete stored item");
            assert!(enabled);
            click(&mut app, &ctx, remove, true);
            let items = account_workspace::character_inventory(&app.document, 0)
                .unwrap()
                .unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].definition_hash, 100);
        }
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
