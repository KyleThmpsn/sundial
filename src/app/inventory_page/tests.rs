//! Focused policy and regression tests for the inventory page feature.
mod profile;
mod recovery;

#[test]
fn blocked_transfer_rows_are_readable_but_never_clickable_or_focusable() {
    use super::{
        interactions::draw_character_transfer_destination, model::CharacterTransferDestination,
    };
    use eframe::egui;

    for enabled in [false, true] {
        let context = egui::Context::default();
        let destination = CharacterTransferDestination {
            character_index: 1,
            label: "Titan".to_owned(),
            detail: "Kinetic Weapons is full".to_owned(),
            tooltip: "Kinetic Weapons is full".to_owned(),
            enabled,
        };
        let mut position = egui::Pos2::ZERO;
        let mut clicked = false;
        for phase in 0..3 {
            let events = if phase == 0 {
                vec![]
            } else {
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: phase == 1,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            let _ = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let response = draw_character_transfer_destination(ui, &destination);
                        position = response.rect.center();
                        assert_eq!(response.sense.is_focusable(), enabled);
                        clicked |= response.clicked();
                    });
                },
            );
        }
        assert_eq!(clicked, enabled);
    }
}

use crate::app::inventory::{InventoryItemAction, InventoryItemLocation};
use serde_json::json;

use super::interactions::apply_inventory_actions_atomic;

#[test]
fn multi_field_inventory_edits_are_atomic() {
    let mut document = super::super::account_workspace::WorkspaceDocument::json_only(json!({
    "version": 6,
    "state": {
        "account": {},
        "characters": [{
            "soid": 1,
            "equipment": {},
            "inventory": [{
                "instance_soid": "0x4000000000000001",
                "definition_hash": "0x0000002A",
                "level": 106,
                "quantity": 1,
                "plugs": null
            }]
        }]
    }
    }));
    let before = document.clone();
    let error = apply_inventory_actions_atomic(
        &mut document,
        InventoryItemLocation {
            character_index: 0,
            item_index: 0,
        },
        vec![
            InventoryItemAction::SetQuantity(2),
            InventoryItemAction::SetFlags(Some(8)),
        ],
    )
    .unwrap_err();

    assert!(error.contains("flags"));
    assert_eq!(document, before);
}
