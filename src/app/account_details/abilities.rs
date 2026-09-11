use crate::app::{SundialApp, account_workspace as account};
use eframe::egui;
use std::collections::BTreeMap;

impl SundialApp {
    pub(in crate::app) fn open_ability_unlocks(&mut self, index: usize) {
        self.account_details.ability_unlocks_character =
            account::character_soid(&self.document, index);
    }

    pub(in crate::app) fn draw_ability_unlocks_window(
        &mut self,
        ui: &egui::Ui,
        index: usize,
        editable: bool,
    ) {
        let Some(character_id) = self.account_details.ability_unlocks_character else {
            return;
        };
        if account::character_soid(&self.document, index) != Some(character_id) {
            self.account_details.ability_unlocks_character = None;
            return;
        }
        let Some(original) = self.document.native_account().and_then(|document| {
            document.runtime()["characters"][index]["acquired_subclass_mask"].as_u64()
        }) else {
            self.account_details.ability_unlocks_character = None;
            return;
        };
        let subclass = account::equipped_item_snapshots(&self.document, index)
            .ok()
            .and_then(|items| {
                items
                    .into_iter()
                    .find(|item| item.slot == "subclass")
                    .and_then(|item| item.definition_hash)
            })
            .and_then(|hash| self.manifest.item(hash));
        let mut mask = original;
        let mut open = true;
        let enabled = editable && ui.is_enabled();
        egui::Window::new("Ability Unlocks")
            .id(egui::Id::new(("ability-unlocks", character_id)))
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(360.0);
                ui.add_enabled_ui(enabled, |ui| {
                    draw_unlocks(ui, subclass, &mut mask, index);
                });
            });
        if !open {
            self.account_details.ability_unlocks_character = None;
        }
        if enabled && mask != original {
            let Some(document) = self.document.native_account_mut() else {
                return;
            };
            let mut runtime = document.runtime().clone();
            runtime["characters"][index]["acquired_subclass_mask"] = mask.into();
            document.set_runtime(runtime);
            self.progression_ui.invalidate_document();
            self.dirty = true;
            self.set_status("Ability unlocks updated. Click Save to write them", false);
        }
    }
}

fn draw_unlocks(
    ui: &mut egui::Ui,
    subclass: Option<&crate::catalog::ItemDef>,
    mask: &mut u64,
    index: usize,
) {
    let mut entries = BTreeMap::new();
    if let Some(subclass) = subclass {
        let abilities = &subclass.abilities;
        for choice in abilities
            .movement
            .iter()
            .chain(&abilities.grenade)
            .chain(&abilities.super_ability)
            .chain(&abilities.melee)
            .chain(&abilities.class_ability)
            .chain(abilities.attunements.iter().flat_map(|tree| {
                tree.perks
                    .iter()
                    .chain(&tree.super_abilities)
                    .chain(std::iter::once(&tree.melee))
            }))
        {
            if choice.entry < 64 {
                entries.entry(choice.entry).or_insert(&choice.name);
            }
        }
        ui.strong(&subclass.name);
    }
    if entries.is_empty() {
        ui.label("Equip a subclass with ability definitions to edit its acquired abilities.");
        return;
    }
    let known_mask = entries
        .keys()
        .fold(0_u64, |bits, entry| bits | (1_u64 << entry));
    ui.horizontal(|ui| {
        if ui.small_button("Acquire All").clicked() {
            *mask |= known_mask;
        }
        if ui.small_button("Clear All").clicked() {
            *mask &= !known_mask;
        }
    });
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            egui::Grid::new(("ability-acquisition", index))
                .num_columns(2)
                .max_col_width(174.0)
                .show(ui, |ui| {
                    for (number, (entry, name)) in entries.into_iter().enumerate() {
                        let bit = 1_u64 << entry;
                        let mut acquired = *mask & bit != 0;
                        if ui.checkbox(&mut acquired, name).changed() {
                            if acquired {
                                *mask |= bit;
                            } else {
                                *mask &= !bit;
                            }
                        }
                        if number % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        });
}
