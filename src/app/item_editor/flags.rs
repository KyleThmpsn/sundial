//! Source-gated item-state control shared by both loadout layouts and stored items.

use crate::account_contract::INVENTORY_FLAG_MASTERWORK;
use eframe::egui;

/// Outer option is an edit request; inner None removes a now-zero flags field.
pub(crate) fn draw_masterwork_flag(
    ui: &mut egui::Ui,
    flags: Option<u8>,
    available: bool,
) -> Option<Option<u8>> {
    if !available {
        return None;
    }
    let mut masterworked = flags.unwrap_or_default() & INVENTORY_FLAG_MASTERWORK != 0;
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = ui.checkbox(&mut masterworked, "Masterworked").changed();
        crate::ui_help::info(
            ui,
            "Marks the item as masterworked. Socket plugs and catalyst objectives are separate.",
        );
    });
    if changed {
        ui.close_menu();
    }
    changed.then(|| {
        let flags = if masterworked {
            flags.unwrap_or_default() | INVENTORY_FLAG_MASTERWORK
        } else {
            flags.unwrap_or_default() & !INVENTORY_FLAG_MASTERWORK
        };
        (flags != 0).then_some(flags)
    })
}
