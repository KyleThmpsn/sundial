//! Character selection for the artifact workspace.

use eframe::egui;
use serde_json::Value;

use crate::app::{SundialApp, account_workspace as account, equipment::class_name};

impl SundialApp {
    pub(in crate::app::progression) fn draw_artifact_context(
        &mut self,
        ui: &mut egui::Ui,
    ) -> Value {
        let document = ui
            .horizontal_wrapped(|ui| {
                self.draw_artifact_character_picker(ui);
                let document = self.document.progression_view(self.selected_character);
                if let Some(definition) = self.manifest.seasonal()
                    && let Some(snapshot) = super::super::collection_state_snapshot(&document)
                {
                    ui.add_space(12.0);
                    super::view::draw_summary(ui, &snapshot, definition);
                }
                document
            })
            .inner;
        ui.add_space(8.0);
        document
    }

    fn draw_artifact_character_picker(&mut self, ui: &mut egui::Ui) {
        let characters: Vec<_> = (0..self.character_count())
            .map(|index| {
                let class = account::character_metadata(&self.document, index)
                    .map_or(99, |metadata| u64::from(metadata.class_type));
                format!("Character {} - {}", index + 1, class_name(class))
            })
            .collect();
        let before = self.selected_character;
        egui::ComboBox::from_id_salt("artifact_character")
            .selected_text(
                characters
                    .get(before)
                    .map_or("No Character", String::as_str),
            )
            .show_ui(ui, |ui| {
                for (index, label) in characters.iter().enumerate() {
                    ui.selectable_value(&mut self.selected_character, index, label);
                }
            });
        if self.selected_character != before {
            self.progression_ui.invalidate_document();
            self.collections_ui.reset_navigation();
        }
    }
}
