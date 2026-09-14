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
                    (ProgressionSection::Triumphs, "Triumphs"),
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
        let artifact_context = if self.progression_section == ProgressionSection::Seasonal {
            let artifact = ui
                .horizontal_wrapped(|ui| {
                    let artifact = self.progression_ui.seasonal.draw_navigation(ui);
                    if self.progression_ui.seasonal.rewards_selected()
                        && self.document.native_account().is_some()
                    {
                        ui.add_space(8.0);
                        self.draw_seasonal_character_picker(ui);
                    }
                    artifact
                })
                .inner;
            ui.separator();
            artifact
        } else {
            false
        };
        let mut document = if artifact_context && self.document.native_account().is_some() {
            self.draw_artifact_context(ui)
        } else {
            self.progression_ui
                .cached_view
                .take()
                .filter(|(character, _)| *character == self.selected_character)
                .map(|(_, document)| document)
                .unwrap_or_else(|| self.document.progression_view(self.selected_character))
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
            ProgressionSection::Unlocks
            | ProgressionSection::Investment
            | ProgressionSection::Triumphs => {
                let view = if self.progression_section == ProgressionSection::Unlocks {
                    super::View::Unlocks
                } else if self.progression_section == ProgressionSection::Triumphs {
                    super::View::Triumphs
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
            self.record_progression_edit("Progression Updated");
            self.set_status(
                if self.progression_section == ProgressionSection::Seasonal {
                    "Seasonal progression updated. Click Save to write it"
                } else {
                    "Progression state updated. Click Save to write it"
                },
                false,
            );
        } else if self.progression_section != ProgressionSection::Seasonal {
            self.progression_ui.cached_view = Some((self.selected_character, document));
        }
    }
}
