//! Progression page routing with one active-account commit boundary.
use eframe::egui;

use crate::app::{ProgressionSection, SundialApp, collections_page};

impl SundialApp {
    pub(in crate::app) fn draw_progression_page(&mut self, ui: &mut egui::Ui) {
        if self.progression_section != ProgressionSection::Seasonal {
            self.draw_progression_character_tabs(ui);
        }
        let read_only = !self.preferences.experimental_progression
            || self.document.account_editing_blocked().is_some();
        self.progression_ui.read_only = read_only;
        self.collections_ui.read_only = read_only;
        if read_only {
            ui.label("Browsing only. Enable Progression Editing under Preferences > Editing > Experimental to change progression state.");
        }
        ui.heading("Progression");
        ui.add_space(8.0);
        let section_changed = ui
            .horizontal_wrapped(|ui| {
                let mut changed = false;
                for (section, label) in [
                    (ProgressionSection::Collections, "Collections"),
                    (ProgressionSection::Seasonal, "Seasonal"),
                    (ProgressionSection::Unlocks, "Unlocks"),
                    (ProgressionSection::Investment, "Investment"),
                ] {
                    changed |= ui
                        .selectable_value(&mut self.progression_section, section, label)
                        .changed();
                }
                changed
            })
            .inner;
        if section_changed {
            self.progression_ui.reset_navigation();
            self.collections_ui.reset_navigation();
        }
        ui.separator();
        let mut document = if self.progression_section == ProgressionSection::Seasonal
            && self.progression_ui.seasonal.draw_navigation(ui)
            && self.document.native_account().is_some()
        {
            self.draw_artifact_context(ui)
        } else {
            self.document.progression_view(self.selected_character)
        };
        let changed = match self.progression_section {
            ProgressionSection::Seasonal => {
                super::seasonal::draw(ui, &mut document, &self.manifest, &mut self.progression_ui)
            }
            ProgressionSection::Collections => collections_page::draw_content(
                ui,
                &mut document,
                &self.manifest,
                &mut self.collections_ui,
            ),
            ProgressionSection::Unlocks | ProgressionSection::Investment => {
                let view = if self.progression_section == ProgressionSection::Unlocks {
                    super::View::Unlocks
                } else {
                    super::View::Investment
                };
                super::draw_content(
                    ui,
                    &mut document,
                    &self.manifest,
                    self.destiny_symbol_font_error.as_deref(),
                    &mut self.progression_ui,
                    view,
                )
            }
        };
        if changed {
            if let Err(error) = self
                .document
                .apply_progression_view(self.selected_character, document)
            {
                self.progression_ui.invalidate_document();
                self.set_status(error, true);
                return;
            }
            self.dirty = true;
            self.set_status(
                if self.progression_section == ProgressionSection::Seasonal {
                    "Seasonal progression updated. Click Save to write it"
                } else {
                    "Progression state updated. Click Save to write it"
                },
                false,
            );
        }
    }
}
