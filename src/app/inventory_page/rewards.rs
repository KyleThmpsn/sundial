use crate::{
    app::{SundialApp, account_workspace as account, equipment::class_name, item_editor},
    catalog::{Catalog, InventoryDefinition},
};
use eframe::egui;

#[derive(Clone)]
struct RewardDraft {
    character_slot: usize,
    kind: u8,
    definition_hash: Option<u32>,
    quantity: i32,
    query: String,
}

enum Edit {
    Add(RewardDraft),
    Quantity(i64, i32),
    Remove(i64),
}

impl SundialApp {
    pub(super) fn draw_pending_rewards(&mut self, ui: &mut egui::Ui) {
        let Some(document) = self.document.native_account() else {
            return;
        };
        let rewards = document.pending_rewards().to_vec();
        let characters: Vec<_> = (0..document.characters().characters().len())
            .map(|index| {
                let class = account::character_metadata(&self.document, index)
                    .map_or(3, |metadata| u64::from(metadata.class_type));
                (format!("{} ({})", class_name(class), index + 1), class)
            })
            .collect();
        let id = ui.make_persistent_id("pending-reward-draft");
        let mut draft = ui
            .data(|data| data.get_temp::<RewardDraft>(id))
            .unwrap_or_else(|| RewardDraft {
                character_slot: self.selected_character,
                kind: 0,
                definition_hash: None,
                quantity: 1,
                query: String::new(),
            });
        draft.character_slot = draft.character_slot.min(characters.len().saturating_sub(1));
        let editable = account::can_mutate_character_inventory(&self.document)
            && account::profile_items_editable(&self.document);
        let mut edit = None;
        ui.strong("Pending Rewards");
        ui.label("Sunrise delivers queued rewards in game when inventory space is available.");
        ui.add_space(6.0);
        ui.add_enabled_ui(editable && !characters.is_empty(), |ui| {
            if draw_add_reward(ui, &self.manifest, &characters, &mut draft) {
                edit = Some(Edit::Add(draft.clone()));
            }
        });
        ui.data_mut(|data| data.insert_temp(id, draft));
        ui.add_space(8.0);
        if rewards.is_empty() {
            ui.weak("No pending rewards");
        } else {
            ui.add_enabled_ui(editable, |ui| {
                egui::Grid::new("pending-rewards")
                    .striped(true)
                    .num_columns(4)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Character");
                        ui.strong("Reward");
                        ui.strong("Quantity");
                        ui.label("");
                        ui.end_row();
                        for reward in &rewards {
                            ui.push_id(reward.id, |ui| {
                                ui.label(characters.get(reward.character_slot).map_or_else(
                                    || format!("Character {}", reward.character_slot + 1),
                                    |(label, _)| label.clone(),
                                ));
                            });
                            let hash = u64::from(reward.definition_hash);
                            ui.horizontal(|ui| {
                                if let Some(texture) = self.manifest.icon_texture(ui.ctx(), hash) {
                                    ui.image((texture.id(), egui::vec2(24.0, 24.0)));
                                }
                                ui.label(self.manifest.names.get(&hash).cloned().unwrap_or_else(
                                    || format!("0x{:08X}", reward.definition_hash),
                                ))
                                .on_hover_text(kind_label(reward.kind));
                            });
                            ui.push_id((reward.id, "quantity"), |ui| {
                                if reward.kind == 1 {
                                    let maximum = maximum_quantity(
                                        &self.manifest,
                                        Some(reward.definition_hash),
                                    )
                                    .max(reward.quantity);
                                    let mut quantity = reward.quantity;
                                    if ui
                                        .add(egui::DragValue::new(&mut quantity).range(1..=maximum))
                                        .changed()
                                    {
                                        edit = Some(Edit::Quantity(reward.id, quantity));
                                    }
                                } else {
                                    ui.label(reward.quantity.to_string());
                                }
                            });
                            ui.push_id((reward.id, "remove"), |ui| {
                                if item_editor::draw_trash_button(ui, true, "Remove Reward")
                                    .clicked()
                                {
                                    edit = Some(Edit::Remove(reward.id));
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        }
        if editable
            && ui.is_enabled()
            && let Some(edit) = edit
            && let Some(document) = self.document.native_account_mut()
        {
            let result = match edit {
                Edit::Add(draft) => document.add_pending_reward(
                    draft.character_slot,
                    draft.kind,
                    draft
                        .definition_hash
                        .expect("The Add Reward button requires a definition"),
                    draft.quantity,
                ),
                Edit::Quantity(id, quantity) => document.set_pending_reward_quantity(id, quantity),
                Edit::Remove(id) => document.remove_pending_reward(id),
            };
            self.finish_native_inventory_edit(result);
        }
    }
}

fn draw_add_reward(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    characters: &[(String, u64)],
    draft: &mut RewardDraft,
) -> bool {
    let mut add = false;
    ui.horizontal_wrapped(|ui| {
        let previous_target = (draft.character_slot, draft.kind);
        ui.label("Character");
        egui::ComboBox::from_id_salt("reward-character")
            .selected_text(
                characters
                    .get(draft.character_slot)
                    .map_or("No Character", |(name, _)| name),
            )
            .show_ui(ui, |ui| {
                for (slot, (name, _)) in characters.iter().enumerate() {
                    ui.selectable_value(&mut draft.character_slot, slot, name);
                }
            });
        ui.label("Type");
        egui::ComboBox::from_id_salt("reward-kind")
            .selected_text(kind_label(draft.kind))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut draft.kind, 0, kind_label(0));
                ui.selectable_value(&mut draft.kind, 1, kind_label(1));
            });
        if previous_target != (draft.character_slot, draft.kind) {
            draft.definition_hash = None;
            draft.quantity = 1;
        }
    });
    let class = characters
        .get(draft.character_slot)
        .map_or(3, |(_, class)| *class);
    ui.horizontal_wrapped(|ui| {
        let selected_name = draft
            .definition_hash
            .and_then(|hash| catalog.inventory_definition(u64::from(hash)))
            .map_or("Choose Reward", |definition| definition.name);
        egui::ComboBox::from_id_salt("reward-definition")
            .selected_text(selected_name)
            .width(260.0)
            .truncate()
            .show_ui(ui, |ui| {
                ui.set_min_width(300.0);
                ui.add(egui::TextEdit::singleline(&mut draft.query).hint_text("Search rewards"));
                let choices: Vec<_> = if draft.kind == 1 {
                    catalog.profile_item_candidates(&draft.query).collect()
                } else {
                    catalog
                        .character_inventory_candidates(&draft.query, class, false, false)
                        .filter(|definition| definition.metadata.is_instanced_character_candidate())
                        .collect()
                };
                egui::ScrollArea::vertical().max_height(280.0).show_rows(
                    ui,
                    28.0,
                    choices.len(),
                    |ui, range| {
                        for definition in &choices[range] {
                            ui.push_id(definition.hash, |ui| {
                                ui.horizontal(|ui| {
                                    if let Some(texture) =
                                        catalog.icon_texture(ui.ctx(), definition.hash)
                                    {
                                        ui.image((texture.id(), egui::vec2(24.0, 24.0)));
                                    }
                                    if ui
                                        .selectable_label(
                                            draft.definition_hash.map(u64::from)
                                                == Some(definition.hash),
                                            definition.name,
                                        )
                                        .on_hover_text(definition.type_name)
                                        .clicked()
                                    {
                                        draft.definition_hash = u32::try_from(definition.hash).ok();
                                        ui.close_menu();
                                    }
                                });
                            });
                        }
                    },
                );
                if choices.is_empty() {
                    ui.weak("No matching rewards");
                }
            });
        if draft.kind == 1 {
            let maximum = maximum_quantity(catalog, draft.definition_hash);
            draft.quantity = draft.quantity.clamp(1, maximum);
            ui.label("Quantity");
            ui.add(egui::DragValue::new(&mut draft.quantity).range(1..=maximum));
        } else {
            draft.quantity = 1;
        }
        let valid = draft
            .definition_hash
            .and_then(|hash| catalog.inventory_definition(u64::from(hash)))
            .is_some_and(|definition| valid_reward(definition, draft.kind, class));
        add = ui
            .add_enabled(valid, egui::Button::new("Add Reward"))
            .clicked();
    });
    add
}

fn valid_reward(definition: InventoryDefinition<'_>, kind: u8, class: u64) -> bool {
    if kind == 1 {
        definition.metadata.is_profile_items_candidate()
    } else {
        definition.metadata.is_instanced_character_candidate()
            && definition
                .item
                .is_some_and(|item| item.class_type == 3 || item.class_type == class)
    }
}

fn kind_label(kind: u8) -> &'static str {
    if kind == 1 {
        "Shared Item"
    } else {
        "Character Item"
    }
}

fn maximum_quantity(catalog: &Catalog, hash: Option<u32>) -> i32 {
    hash.and_then(|hash| catalog.inventory_metadata(u64::from(hash)))
        .and_then(|metadata| metadata.max_stack_size)
        .unwrap_or(i32::MAX as u32)
        .clamp(1, i32::MAX as u32) as i32
}
