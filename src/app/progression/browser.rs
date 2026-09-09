//! Explore package definitions even when no saved or authored row exists.
use super::*;

fn matches(query: &str, index: usize, definition: &UnlockDefinition) -> bool {
    query.is_empty()
        || index.to_string().contains(query)
        || format!("{:08x}", definition.hash).contains(query)
        || definition.hash.to_string().contains(query)
        || definition
            .name
            .as_deref()
            .is_some_and(|name| name.to_lowercase().contains(query))
        || definition
            .description
            .as_deref()
            .is_some_and(|text| text.to_lowercase().contains(query))
        || definition
            .compact_slot
            .is_some_and(|slot| slot.to_string().contains(query))
        || definition
            .tested_by
            .iter()
            .any(|context| context.name.to_lowercase().contains(query))
}

pub(super) fn draw(ui: &mut egui::Ui, document: &Value, catalog: &Catalog, state: &mut UiState) {
    egui::CollapsingHeader::new("Browse All Unlock Definitions")
        .id_salt("all_unlock_definitions").show(ui, |ui| {
            ui.label(format!("{} Flag Definitions · {} Value Definitions · {} Progression Definitions", catalog.unlock_flag_definitions().len(), catalog.unlock_value_definitions().len(), catalog.progression_definitions().len()));
            ui.label("Includes definitions without saved state. Inspect a row for objectives, conditions, references, and decoded runtime writers. Evaluated values use available saved state and package data. Unknown requires runtime context.");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.browse_values, false, "Flags");
                ui.selectable_value(&mut state.browse_values, true, "Values");
                ui.add(egui::TextEdit::singleline(&mut state.definition_query).hint_text("Search Names, References, Indexes, Or Hashes").desired_width(ui.available_width()));
            });
            let definitions = if state.browse_values { catalog.unlock_value_definitions() } else { catalog.unlock_flag_definitions() };
            let query = state.definition_query.trim().to_lowercase();
            let rows = definitions.iter().enumerate().filter(|(index, definition)| matches(&query, *index, definition)).collect::<Vec<_>>();
            ui.label(format!("{} / {} Definitions", rows.len(), definitions.len()));
            let snapshot = collection_state_snapshot(document);
            ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
            egui::ScrollArea::both().id_salt("definition_browser_rows").max_height(280.0).auto_shrink([false, false]).show_rows(ui, TABLE_CELL_HEIGHT, rows.len(), |ui, range| {
                egui::Grid::new("definition_browser_grid").striped(true).show(ui, |ui| {
                    for offset in range {
                        let (index, definition) = rows[offset];
                        if ui.small_button(format!("Inspect #{index}")).clicked() {
                            state.metadata_inspector.open(if state.browse_values { MetadataSelection::ValueDefinition(index) } else { MetadataSelection::FlagDefinition(index) });
                        }
                        draw_hash_hex_cell(ui, 110.0, Some(definition.hash));
                        let name = definition_name(definition).or_else(|| catalog.display_name(definition.hash)).map_or_else(|| format!("References: {}", override_meaning(definition)), str::to_owned);
                        table_cell(ui, 220.0, &name).on_hover_text(format!("{name}\n{}", definition_metadata_tooltip(definition)));
                        ui.label(format!("Bank {} · Slot {}", definition.bank(), definition.compact_slot.map_or_else(|| "Unbanked".into(), |slot| slot.to_string())));
                        let saved = snapshot.as_ref().map_or_else(|| "Unavailable".into(), |snapshot| if state.browse_values { snapshot.value_text(index, definition) } else { snapshot.flag_text(index, definition) });
                        ui.label(format!("Saved: {saved}"));
                        let evaluated = snapshot.as_ref().and_then(|snapshot| if state.browse_values {
                            snapshot.evaluated_value(index, catalog).map(|value| value.to_string())
                        } else { snapshot.evaluated_flag(index, catalog).map(|value| if value { "Set" } else { "Clear" }.into()) });
                        ui.label(format!("Evaluated: {}", evaluated.unwrap_or_else(|| "Unknown".into())));
                        ui.label(format!("{} References · {} Writers", definition.tested_by.len(), definition.runtime_writers.len()));
                        ui.end_row();
                    }
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_reads_definitions_without_saved_rows() {
        let definition = UnlockDefinition {
            hash: 0x1234abcd,
            name: Some("Season Reward".into()),
            compact_slot: Some(73),
            ..Default::default()
        };
        assert!(matches("season reward", 8, &definition));
        assert!(matches("1234abcd", 8, &definition));
        assert!(matches("73", 8, &definition));
        assert!(!matches("missing", 8, &definition));
    }
}
