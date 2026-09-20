//! Runtime-specific views, integrated into the existing inventory and progression pages.
pub(super) mod missions;
mod postmaster;
mod rolls;
pub(super) mod vendors;

use super::{ProgressionSection, SundialApp};
use eframe::egui;

impl SundialApp {
    pub(super) fn draw_dawn_progression_section(
        &mut self,
        ui: &mut egui::Ui,
        read_only: bool,
    ) -> bool {
        if !matches!(
            self.progression_section,
            ProgressionSection::Vendors | ProgressionSection::Missions
        ) {
            return false;
        }
        if self.document.account_is_dawn() {
            self.draw_dawn_progression(ui, read_only);
        } else {
            self.progression_section = ProgressionSection::Collections;
        }
        true
    }

    pub(super) fn draw_dawn_inventory_tabs(&mut self, ui: &mut egui::Ui) -> bool {
        if let Some(reason) = self.document.account_editing_blocked() {
            ui.colored_label(ui.visuals().error_fg_color, reason);
            return true;
        }
        let id = ui.make_persistent_id("dawn-inventory-section");
        let mut tab = ui.data(|d| d.get_temp::<u8>(id)).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut tab, 0, "Items");
            ui.selectable_value(&mut tab, 1, "Postmaster");
            ui.selectable_value(&mut tab, 2, "Saved Rolls");
        });
        ui.data_mut(|d| d.insert_temp(id, tab));
        if tab == 0 {
            return false;
        }
        self.draw_character_tabs(ui);
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt(("dawn-inventory", tab))
            .show(ui, |ui| {
                if tab == 1 {
                    self.draw_postmaster(ui);
                } else {
                    self.draw_saved_rolls(ui);
                }
            });
        true
    }

    pub(super) fn draw_dawn_progression(&mut self, ui: &mut egui::Ui, read_only: bool) {
        if let Some(reason) = self.document.account_editing_blocked() {
            ui.colored_label(ui.visuals().error_fg_color, reason);
            return;
        }
        let Some(doc) = self.document.dawn_account() else {
            return;
        };
        let mut state = doc.activity_state().clone();
        let mut campaigns = doc
            .vendor_campaigns(self.selected_character)
            .unwrap_or_default();
        let before_campaigns = campaigns;
        let owner = doc
            .character_owner(self.selected_character)
            .unwrap_or_default();
        let account = format!("{:016X}", doc.primary_soid().get());
        egui::ScrollArea::vertical()
            .id_salt("dawn-progression-state")
            .show(ui, |ui| {
                ui.set_max_width(800.0);
                ui.add_enabled_ui(!read_only, |ui| {
                    if self.progression_section == ProgressionSection::Vendors {
                        vendors::draw(
                            ui,
                            &self.manifest,
                            &mut state,
                            &account,
                            &owner,
                            &mut campaigns,
                        );
                    } else {
                        missions::draw(ui, &self.manifest, &mut state, &owner);
                    }
                });
            });
        if ui.is_enabled()
            && !read_only
            && (state != *doc.activity_state() || campaigns != before_campaigns)
        {
            let mut candidate = doc.clone();
            let result = candidate.set_activity_state(state).and_then(|()| {
                if campaigns != before_campaigns {
                    candidate.set_vendor_campaigns(self.selected_character, campaigns)
                } else {
                    Ok(())
                }
            });
            match result {
                Ok(()) => {
                    *self.document.dawn_account_mut().unwrap() = candidate;
                    self.record_progression_edit("Dawn Progression Updated");
                    self.set_status("Progression updated. Click Save to write it", false);
                }
                Err(error) => self.set_status(error, true),
            }
        }
    }

    fn finish_dawn_edit(&mut self, result: Result<(), String>, label: &str) {
        match result {
            Ok(()) => {
                self.report_edit(label);
                self.set_status("Changes are ready. Click Save to write them", false);
            }
            Err(error) => self.set_status(error, true),
        }
    }
}

fn item_label(ui: &mut egui::Ui, catalog: &crate::catalog::Catalog, hash: u32) {
    let height = ui.spacing().interact_size.y;
    ui.horizontal(|ui| {
        if let Some(texture) = catalog.icon_texture(ui.ctx(), u64::from(hash)) {
            ui.add(
                egui::Image::new((texture.id(), egui::vec2(height, height)))
                    .bg_fill(super::ui::package_icon_backdrop(ui)),
            );
        } else {
            ui.allocate_exact_size(egui::vec2(height, height), egui::Sense::hover());
        }
        ui.add(
            egui::Label::new(
                catalog
                    .display_name(u64::from(hash))
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Item {hash:08X}")),
            )
            .truncate(),
        );
    });
}
