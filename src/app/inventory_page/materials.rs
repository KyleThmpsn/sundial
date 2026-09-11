use crate::app::{SundialApp, account_workspace as account};
use eframe::egui;

enum Edit {
    Add(u32),
    Quantity(usize, i32),
    Remove(usize),
}

impl SundialApp {
    pub(super) fn draw_character_materials(&mut self, ui: &mut egui::Ui) {
        self.draw_character_tabs(ui);
        ui.separator();
        let index = self.selected_character;
        let Some(document) = self.document.native_account() else {
            return;
        };
        let stacks = document.character_stacks(index).to_vec();
        let has_character = index < document.characters().characters().len();
        let mut edit = None;
        ui.horizontal_wrapped(|ui| {
            ui.strong("Character Materials");
            ui.weak(format!("{} / 32", stacks.len()));
            ui.add_enabled_ui(has_character && stacks.len() < 32, |ui| {
                egui::ComboBox::from_id_salt(("add-character-material", index))
                    .selected_text("Add Material")
                    .show_ui(ui, |ui| {
                        let query = self
                            .searches
                            .entry("character-material-picker".into())
                            .or_default();
                        ui.add(egui::TextEdit::singleline(query).hint_text("Search materials"));
                        let inventory = account::character_inventory(&self.document, index)
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        let equipment = account::equipped_item_snapshots(&self.document, index)
                            .unwrap_or_default();
                        let mut count = 0;
                        for definition in self.manifest.character_material_candidates(query) {
                            let Ok(hash) = u32::try_from(definition.hash) else {
                                continue;
                            };
                            if stacks.iter().any(|stack| stack.definition_hash == hash) {
                                continue;
                            }
                            let bucket = definition.metadata.native_bucket_id;
                            let in_bucket = |hash| {
                                self.manifest
                                    .inventory_metadata(hash)
                                    .is_some_and(|metadata| {
                                        metadata.native_bucket_id == bucket
                                            && metadata.scope == definition.metadata.scope
                                    })
                            };
                            let used = stacks
                                .iter()
                                .filter(|stack| in_bucket(u64::from(stack.definition_hash)))
                                .count()
                                + inventory
                                    .iter()
                                    .filter(|item| in_bucket(u64::from(item.definition_hash)))
                                    .count()
                                + equipment
                                    .iter()
                                    .filter(|item| item.definition_hash.is_some_and(in_bucket))
                                    .count();
                            if definition
                                .metadata
                                .authored_row_capacity()
                                .is_none_or(|capacity| used >= usize::from(capacity))
                            {
                                continue;
                            }
                            count += 1;
                            if ui
                                .selectable_label(false, definition.name)
                                .on_hover_text(definition.metadata.bucket_label())
                                .clicked()
                            {
                                edit = Some(Edit::Add(hash));
                                ui.close_menu();
                            }
                        }
                        if count == 0 {
                            ui.weak("No available materials match");
                        }
                    });
            });
        });
        if stacks.is_empty() {
            ui.label("This character has no material stacks.");
        }
        egui::ScrollArea::vertical()
            .id_salt(("character-materials", index))
            .show(ui, |ui| {
                for (position, stack) in stacks.iter().enumerate() {
                    ui.push_id(("material", index, position), |ui| {
                        ui.horizontal(|ui| {
                            if let Some(texture) = self
                                .manifest
                                .icon_texture(ui.ctx(), u64::from(stack.definition_hash))
                            {
                                ui.image((texture.id(), egui::vec2(24.0, 24.0)));
                            }
                            let definition = self
                                .manifest
                                .inventory_definition(u64::from(stack.definition_hash));
                            ui.label(definition.map_or_else(
                                || format!("0x{:08X}", stack.definition_hash),
                                |definition| definition.name.to_owned(),
                            ));
                            let mut quantity = stack.quantity;
                            let maximum = definition
                                .and_then(|definition| definition.metadata.max_stack_size)
                                .unwrap_or(i32::MAX as u32)
                                .min(i32::MAX as u32)
                                as i32;
                            ui.label("Quantity");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut quantity)
                                        .range(1..=maximum.max(stack.quantity)),
                                )
                                .changed()
                            {
                                edit = Some(Edit::Quantity(position, quantity));
                            }
                            if super::super::item_editor::draw_trash_button(
                                ui,
                                true,
                                "Remove Material",
                            )
                            .clicked()
                            {
                                edit = Some(Edit::Remove(position));
                            }
                        });
                    });
                }
            });
        if let Some(edit) = edit
            && ui.is_enabled()
            && let Some(document) = self.document.native_account_mut()
        {
            let result = match edit {
                Edit::Add(hash) => document.add_character_stack(index, hash, 1),
                Edit::Quantity(position, quantity) => {
                    document.set_character_stack_quantity(index, position, quantity)
                }
                Edit::Remove(position) => document.remove_character_stack(index, position),
            };
            self.finish_native_inventory_edit(result);
        }
    }
}
