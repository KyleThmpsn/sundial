//! Native character controls share the workspace's transactional save and history.
mod abilities;
mod titles;

use super::SundialApp;
use eframe::egui;

#[derive(Default)]
pub(super) struct State {
    titles: titles::Titles,
    ability_unlocks_character: Option<u64>,
}

impl SundialApp {
    pub(super) fn draw_native_character_identity(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        editable: bool,
        label_width: f32,
    ) {
        let Some(document) = self.document.native_account() else {
            return;
        };
        let mut runtime = document.runtime().clone();
        let Some(character) = runtime["characters"].get_mut(index) else {
            return;
        };
        let (Some(mut level), Some(mut title)) = (
            character["level"].as_u64(),
            character["equipped_title"].as_u64(),
        ) else {
            return;
        };
        self.account_details
            .titles
            .refresh(&self.manifest, ui.ctx());
        let original = (level, title);
        let mut selection = None;
        super::ui::field_label(ui, "Level", label_width);
        ui.add_enabled(editable, egui::DragValue::new(&mut level).range(0..=255));
        ui.end_row();
        super::ui::field_label(ui, "Title", label_width);
        ui.horizontal(|ui| {
            if !editable {
                ui.disable();
            }
            selection = self
                .account_details
                .titles
                .draw(ui, &self.manifest, title, index);
            match &self.account_details.titles.result {
                None => {
                    ui.spinner().on_hover_text("Loading installed titles");
                }
                Some(Err(error)) => {
                    ui.label("Titles Unavailable").on_hover_text(error);
                    if ui.small_button("Retry").clicked() {
                        self.account_details.titles.result = None;
                    }
                }
                Some(Ok(_)) => {}
            }
        });
        ui.end_row();
        if let Some(selection) = &selection {
            title = u64::from(selection.index);
        }
        if (original != (level, title) || selection.is_some()) && editable && ui.is_enabled() {
            character["level"] = level.into();
            character["equipped_title"] = title.into();
            let Some(document) = self.document.native_account_mut() else {
                return;
            };
            let unlocked = match selection.and_then(|selection| selection.unlock) {
                Some(unlock) => {
                    match document.set_account_flag(unlock.definition_index, unlock.slot) {
                        Ok(changed) => changed,
                        Err(error) => {
                            self.set_status(error.to_string(), true);
                            return;
                        }
                    }
                }
                None => false,
            };
            if !unlocked && document.runtime() == &runtime {
                return;
            }
            document.set_runtime(runtime);
            self.progression_ui.invalidate_document();
            self.dirty = true;
            self.set_status(
                if unlocked {
                    "Title unlocked and equipped. Click Save to write it"
                } else {
                    "Character updated. Click Save to write it"
                },
                false,
            );
        }
    }

    pub(super) fn validate_title_selections(&self) -> Result<(), String> {
        self.account_details.titles.validate_changes(
            &self.document,
            &self.persisted_document,
            &self.manifest,
        )
    }

    pub(super) fn draw_item_seen(&mut self, ui: &mut egui::Ui, soid: u64) {
        let seen = self
            .document
            .native_account()
            .and_then(|document| document.item_seen(soid));
        if let Some(seen) = super::item_editor::draw_seen_flag(ui, seen) {
            self.set_item_seen(soid, seen);
        }
    }

    pub(super) fn set_item_seen(&mut self, soid: u64, seen: bool) {
        let Some(account) = self.document.native_account_mut() else {
            return;
        };
        let result = account.set_item_seen(soid, seen);
        self.finish_native_inventory_edit(result);
    }

    pub(super) fn draw_profile_item_seen(&mut self, ui: &mut egui::Ui, position: usize) {
        let seen = self
            .document
            .native_account()
            .and_then(|document| document.profile_item_seen(position));
        if let Some(seen) = super::item_editor::draw_seen_flag(ui, seen) {
            let result = self
                .document
                .native_account_mut()
                .unwrap()
                .set_profile_item_seen(position, seen);
            self.finish_native_inventory_edit(result);
        }
    }

    pub(super) fn finish_native_inventory_edit(
        &mut self,
        result: Result<(), crate::persistence::sqlite_account::SqliteAccountError>,
    ) {
        match result {
            Ok(()) => {
                self.dirty = true;
                self.set_status("Inventory updated. Click Save to write it", false);
            }
            Err(error) => self.set_status(error.to_string(), true),
        }
    }
}
