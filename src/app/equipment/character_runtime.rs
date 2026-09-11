//! Experimental per-character runtime fields for v13+ JSON accounts.

use crate::{
    app::SundialApp,
    app::components::object_form::{self as form, Action, Field, Input},
    persistence::json_account::character_runtime::{self, CURRENT_ACTIVITY},
};
use eframe::egui;

impl SundialApp {
    pub(super) fn draw_character_runtime(&mut self, ui: &mut egui::Ui, index: usize) {
        if !self.document.uses_json_account()
            || !self.preferences.experimental_activity_state
            || !self.document.supports_v13_account()
        {
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
        egui::CollapsingHeader::new("Activity State")
            .id_salt(("character-runtime", index))
            .show(ui, |ui| {
                ui.label("Raw current activity index. Its effect in game is not verified.");
                let fields = [Field {
                    key: CURRENT_ACTIVITY,
                    label: "Current Activity Index",
                    input: Input::Unsigned(u16::MAX as u64),
                    optional: true,
                }];
                if let Some(Action::Apply(row)) = form::draw(
                    ui,
                    ("character-runtime", index),
                    &character,
                    &fields,
                    false,
                    character_runtime::validate_character,
                ) {
                    match character_runtime::set_current_activity(
                        self.document.json_mut(),
                        index,
                        row.get(CURRENT_ACTIVITY).cloned(),
                    ) {
                        Ok(true) => {
                            self.dirty = true;
                            self.set_status(
                                "Updated activity state. Click Save to write it",
                                false,
                            );
                        }
                        Ok(false) => {}
                        Err(error) => self.set_status(error, true),
                    }
                }
            });
    }
}
