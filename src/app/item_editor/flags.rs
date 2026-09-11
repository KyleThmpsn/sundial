//! Source-gated item-state control shared by both loadout layouts and stored items.

use crate::account_contract::{INVENTORY_FLAG_LOCKED, INVENTORY_FLAG_MASTERWORK};
use eframe::egui;

/// Outer option is an edit request; inner None removes a now-zero flags field.
pub(crate) fn draw_state_flags(
    ui: &mut egui::Ui,
    flags: Option<u8>,
    masterwork_available: bool,
) -> Option<Option<u8>> {
    for (mask, label, help, available) in [
        (
            INVENTORY_FLAG_LOCKED,
            "Locked",
            "Protects the item from being dismantled.",
            true,
        ),
        (
            INVENTORY_FLAG_MASTERWORK,
            "Masterworked",
            "Marks the item as masterworked. Socket plugs and catalyst objectives are separate.",
            masterwork_available,
        ),
    ] {
        if !available {
            continue;
        }
        let mut enabled = flags.unwrap_or_default() & mask != 0;
        if draw_state_checkbox(ui, &mut enabled, label, help) {
            let flags = if enabled {
                flags.unwrap_or_default() | mask
            } else {
                flags.unwrap_or_default() & !mask
            };
            return Some((flags != 0).then_some(flags));
        }
    }
    None
}

pub(crate) fn draw_seen_flag(ui: &mut egui::Ui, seen: Option<bool>) -> Option<bool> {
    let mut seen = seen?;
    draw_state_checkbox(
        ui,
        &mut seen,
        "Seen",
        "Marks the item as inspected in game, clearing its new-item indicator.",
    )
    .then_some(seen)
}

fn draw_state_checkbox(ui: &mut egui::Ui, value: &mut bool, label: &str, help: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = ui.checkbox(value, label).changed();
        crate::ui_help::info(ui, help);
    });
    if changed {
        ui.close_menu();
    }
    changed
}
