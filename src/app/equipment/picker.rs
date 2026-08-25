use super::*;

pub(super) fn equipment_definition_choices<'a>(
    candidates: impl IntoIterator<Item = &'a ItemDef>,
) -> Vec<DefinitionChoice> {
    candidates
        .into_iter()
        .map(|item| DefinitionChoice {
            hash: item.hash,
            name: item.name.clone(),
            type_name: item.type_name.clone(),
            group: None,
        })
        .collect()
}

pub(super) fn equipment_inventory_choices(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    bucket: u64,
    class_type: u64,
) -> Vec<ExistingInventoryChoice> {
    super::inventory::character_inventory(document, character_index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|snapshot| {
            if snapshot.quantity != 1 {
                return None;
            }
            let hash = u64::from(snapshot.definition_hash);
            let definition = catalog.inventory_definition(hash)?;
            if !definition.metadata.is_character_inventory_candidate() {
                return None;
            }
            let item = catalog.get_for_bucket(hash, bucket)?;
            if item.class_type != 3 && item.class_type != class_type {
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

pub(super) fn existing_inventory_choice_matches(
    catalog: &Catalog,
    choice: &ExistingInventoryChoice,
    query: &str,
) -> bool {
    CatalogSearchQuery::new(query).matches(catalog, choice.hash, &[&choice.name, &choice.type_name])
}

pub(in crate::app) fn combo_u64(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u64,
    choices: &[(u64, &str)],
) {
    let selected = choices
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("Invalid", |(_, name)| *name);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(160.0)
        .show_ui(ui, |ui| {
            for &(candidate, name) in choices {
                ui.selectable_value(value, candidate, name);
            }
        });
}

pub(in crate::app) fn ability_combo(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u64,
    choices: &[AbilityChoice],
    width: f32,
) {
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
            for choice in choices {
                ui.selectable_value(value, choice.entry, &choice.name);
            }
            if choices.is_empty() {
                ui.label("No named choices found for this subclass");
            }
        });
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
