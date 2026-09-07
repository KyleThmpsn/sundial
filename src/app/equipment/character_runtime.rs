//! Advanced per-character runtime fields, visible only for a v13+ JSON account.

use crate::{
    app::SundialApp,
    app::components::object_form::{self as form, Action, Field, Input},
    persistence::json_account::character_runtime::{self, CURRENT_ACTIVITY},
};
use eframe::egui;

impl SundialApp {
    pub(super) fn draw_character_runtime(&mut self, ui: &mut egui::Ui, index: usize) {
        if !self.document.supports_v13_account() {
            return;
        }
        let Some(character) = self
            .document
            .pointer("/state/characters")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.get(index))
            .cloned()
        else {
            return;
        };
        egui::CollapsingHeader::new("Activity State (Advanced)").id_salt(("character-runtime", index)).show(ui, |ui| {
            ui.label("Travelling-activity definition index from the installed build. Omission uses Sunrise's runtime default.");
            let fields = [Field { key: CURRENT_ACTIVITY, label: "Current activity index", input: Input::Unsigned(u16::MAX as u64), optional: true }];
            if let Some(Action::Apply(row)) = form::draw(ui, ("character-runtime", index), &character, &fields, false, character_runtime::validate_character) {
                match character_runtime::set_current_activity(self.document.json_mut(), index, row.get(CURRENT_ACTIVITY).cloned()) {
                    Ok(true) => { self.dirty = true; self.set_status("Updated activity state; click Save to write it", false); }
                    Ok(false) => {}
                    Err(error) => self.set_status(error, true),
                }
            }
        });
    }
}
