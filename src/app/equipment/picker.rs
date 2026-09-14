use crate::app::account_workspace as account;

use super::*;

pub(super) struct EquipmentPicker {
    pub(super) slot: &'static str,
    pub(super) bucket: u64,
    pub(super) class_type: u64,
    pub(super) current_hash: Option<u64>,
    pub(super) is_empty: bool,
    pub(super) show_dummy_items: bool,
    pub(super) allow_cross_class_subclasses: bool,
    pub(super) supports_emote_collection: bool,
}

impl EquipmentPicker {
    pub(super) fn choices(
        &self,
        catalog: &Catalog,
        query: &str,
        existing_inventory: &[ExistingInventoryChoice],
    ) -> DefinitionPickerChoices {
        let candidates = if query.trim().is_empty() {
            catalog.browse(
                self.bucket,
                self.class_type,
                self.show_dummy_items,
                self.allow_cross_class_subclasses,
            )
        } else {
            catalog.search(
                query,
                self.bucket,
                self.class_type,
                self.show_dummy_items,
                self.allow_cross_class_subclasses,
            )
        };
        let inventory_query = CatalogSearchQuery::new(query);
        let weapon_slot = WEAPON_SLOTS.contains(&self.slot);
        DefinitionPickerChoices {
            definitions: equipment_definition_choices(candidates, self.supports_emote_collection),
            existing_inventory: existing_inventory
                .iter()
                .filter(|choice| {
                    inventory_query.matches(
                        catalog,
                        choice.hash,
                        &[&choice.name, &choice.type_name],
                    )
                })
                .cloned()
                .collect(),
            clear: (weapon_slot
                && (query.trim().is_empty() || "empty weapon".contains(&query.to_lowercase())))
            .then(|| ClearDefinitionChoice {
                label: "Empty Weapon".to_owned(),
                tooltip: "Sets this equipment slot to empty.".to_owned(),
                selected: self.is_empty,
            }),
            random_item_builder_hash: self
                .current_hash
                .filter(|_| weapon_slot || ARMOR_SLOTS.contains(&self.slot)),
            empty_message: "No compatible installed items found".to_owned(),
        }
    }
}

pub(super) fn equipment_definition_choices<'a>(
    candidates: impl IntoIterator<Item = &'a ItemDef>,
    supports_emote_collection: bool,
) -> Vec<DefinitionChoice> {
    candidates
        .into_iter()
        .filter(|item| {
            crate::account_contract::definition_available(item.hash, supports_emote_collection)
        })
        .map(|item| DefinitionChoice {
            hash: item.hash,
            name: item.name.clone(),
            type_name: item.type_name.clone(),
            group: None,
        })
        .collect()
}

pub(super) fn equipment_inventory_choices(
    document: &super::super::account_workspace::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    bucket: u64,
    class_type: u64,
    allow_cross_class_subclasses: bool,
) -> Vec<ExistingInventoryChoice> {
    account::character_inventory(document, character_index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|snapshot| {
            if snapshot.quantity != 1 {
                return None;
            }
            let hash = u64::from(snapshot.definition_hash);
            if !crate::account_contract::definition_available(
                hash,
                document.supports_emote_collection(),
            ) {
                return None;
            }
            let definition = catalog.inventory_definition(hash)?;
            if !definition.metadata.is_character_inventory_candidate() {
                return None;
            }
            let item = catalog.get_for_bucket(hash, bucket)?;
            if !item_class_is_compatible(item, class_type, allow_cross_class_subclasses) {
                return None;
            }

            Some(ExistingInventoryChoice {
                item_index: snapshot.location.item_index,
                hash,
                name: item.name.clone(),
                type_name: item.type_name.clone(),
            })
        })
        .collect()
}

pub(in crate::app) fn combo_u64(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u64,
    choices: &[(u64, &str)],
) -> bool {
    let selected = choices
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("Invalid", |(_, name)| *name);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(160.0)
        .show_ui(ui, |ui| {
            let mut requested = false;
            for &(candidate, name) in choices {
                requested |= ui.selectable_value(value, candidate, name).clicked();
            }
            requested
        })
        .inner
        .unwrap_or(false)
}

pub(in crate::app) fn ability_combo(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u64,
    choices: &[AbilityChoice],
    width: f32,
) -> bool {
    let selected = choices
        .iter()
        .find(|choice| choice.entry == *value)
        .map_or_else(
            || format!("Unknown entry {}", *value),
            |choice| choice.name.clone(),
        );
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(width)
        .show_ui(ui, |ui| {
            let mut requested = false;
            for choice in choices {
                requested |= ui
                    .selectable_value(value, choice.entry, &choice.name)
                    .clicked();
            }
            if choices.is_empty() {
                ui.label("No named choices found for this subclass");
            }
            requested
        })
        .inner
        .unwrap_or(false)
}

pub(super) fn character_field_group_layout(available_width: f32) -> (usize, [f32; 3]) {
    const WIDE_COLUMN_WIDTHS: [f32; 3] = [220.0, 310.0, 360.0];
    const COLUMN_GAP: f32 = 18.0;
    const WIDE_LAYOUT_WIDTH: f32 =
        WIDE_COLUMN_WIDTHS[0] + WIDE_COLUMN_WIDTHS[1] + WIDE_COLUMN_WIDTHS[2] + COLUMN_GAP * 2.0;

    if available_width >= WIDE_LAYOUT_WIDTH {
        (3, WIDE_COLUMN_WIDTHS)
    } else {
        (1, [available_width.max(0.0); 3])
    }
}
