//! The related catalog records table under an inspected definition.
use super::*;

pub(super) struct RelatedCatalogRecord {
    pub(super) hash: u64,
    pub(super) kind: &'static str,
    pub(super) label: String,
    pub(super) state: Option<RelatedRecordState>,
}

pub(super) struct RelatedRecordState {
    pub(super) text: String,
    pub(super) tooltip: String,
}

struct RelatedCatalogRecordAccumulator<'a, 'catalog> {
    content: &'a HashInspectorContent<'catalog>,
    records: Vec<RelatedCatalogRecord>,
    positions: std::collections::HashMap<u64, usize>,
}

impl RelatedCatalogRecordAccumulator<'_, '_> {
    fn add(
        &mut self,
        hash: u64,
        kind: &'static str,
        fallback: String,
        state: Option<RelatedRecordState>,
    ) {
        if hash == 0 || hash == self.content.hash {
            return;
        }
        if let Some(&position) = self.positions.get(&hash) {
            let record = &mut self.records[position];
            if record.state.is_none() {
                record.state = state;
            }
            return;
        }
        let label = self
            .content
            .catalog
            .display_name(hash)
            .or_else(|| self.content.catalog.package_item_name(hash))
            .map_or(fallback, str::to_owned);
        self.positions.insert(hash, self.records.len());
        self.records.push(RelatedCatalogRecord {
            hash,
            kind,
            label,
            state,
        });
    }
}

fn add_progression_related_records(
    records: &mut RelatedCatalogRecordAccumulator<'_, '_>,
    content: &HashInspectorContent<'_>,
) {
    for (_, definition, _) in &content.matches.progression_reward_matches {
        records.add(
            definition.hash,
            "Progression",
            progression_display_name(definition)
                .unwrap_or_else(|| format_hash_hex(definition.hash)),
            None,
        );
    }
    for (_, definition, _, _) in &content.matches.progression_faction_matches {
        records.add(
            definition.hash,
            "Progression",
            progression_display_name(definition)
                .unwrap_or_else(|| format_hash_hex(definition.hash)),
            None,
        );
    }
    for (_, objective, _) in &content.matches.owner_matches {
        records.add(
            objective.hash,
            "Objective",
            objective_name_or_hash(objective),
            None,
        );
    }
    for (_, objective, _, _) in &content.matches.trait_matches {
        records.add(
            objective.hash,
            "Objective",
            objective_name_or_hash(objective),
            None,
        );
    }
}

fn objective_name_or_hash(objective: &crate::catalog::ObjectiveDef) -> String {
    if objective.name.trim().is_empty() {
        format_hash_hex(objective.hash)
    } else {
        objective.name.clone()
    }
}

fn add_unlock_related_records(
    records: &mut RelatedCatalogRecordAccumulator<'_, '_>,
    content: &HashInspectorContent<'_>,
) {
    for (kind, definition_index, _) in &content.matches.context_matches {
        let definition = match *kind {
            "Flag" => content.catalog.unlock_flag_definition(*definition_index),
            "Value" => content.catalog.unlock_value_definition(*definition_index),
            _ => None,
        };
        let Some(definition) = definition else {
            continue;
        };
        let state = content.collection_state().map(|snapshot| {
            let text = match *kind {
                "Flag" => snapshot.flag_text(*definition_index, definition),
                "Value" => snapshot.value_text(*definition_index, definition),
                _ => unreachable!("context match kinds are filtered above"),
            };
            RelatedRecordState {
                text,
                tooltip: format!(
                    "Current loaded progression state · bank {}{}",
                    definition.bank(),
                    definition
                        .compact_slot
                        .map_or_else(String::new, |slot| format!(" · compact slot {slot}"))
                ),
            }
        });
        records.add(
            definition.hash,
            "Unlock",
            definition_name(definition)
                .map_or_else(|| format_hash_hex(definition.hash), str::to_owned),
            state,
        );
    }
}

fn add_collection_related_records(
    records: &mut RelatedCatalogRecordAccumulator<'_, '_>,
    content: &HashInspectorContent<'_>,
) {
    for collectible in &content.matches.collectible_matches {
        let state = content.collection_state().map(|snapshot| {
            let (text, tooltip) = crate::app::collections_page::collectible_state(
                collectible,
                snapshot,
                content.catalog,
            );
            RelatedRecordState { text, tooltip }
        });
        records.add(
            collectible.hash,
            "Collectible",
            "Collectible Record".into(),
            state,
        );
        records.add(collectible.item_hash, "Item", "Inventory Item".into(), None);
        records.add(
            collectible.material_requirement_set_hash,
            "Material Requirement Set",
            "Material Requirement Set".into(),
            None,
        );
    }
    for set in &content.matches.material_requirement_set_matches {
        records.add(
            set.hash,
            "Material Requirement Set",
            "Material Requirement Set".into(),
            None,
        );
        for requirement in &set.requirements {
            records.add(
                requirement.item_hash,
                "Required Item",
                "Required Inventory Item".into(),
                None,
            );
        }
    }
}

pub(super) fn related_catalog_records(
    content: &HashInspectorContent<'_>,
) -> Vec<RelatedCatalogRecord> {
    let mut records = RelatedCatalogRecordAccumulator {
        content,
        records: Vec::new(),
        positions: std::collections::HashMap::new(),
    };
    add_progression_related_records(&mut records, content);
    add_unlock_related_records(&mut records, content);
    add_collection_related_records(&mut records, content);
    records.records
}

const RELATED_KIND_WIDTH: f32 = 150.0;
const RELATED_HASH_WIDTH: f32 = 96.0;
const RELATED_STATE_WIDTH: f32 = 180.0;
const RELATED_VISIBLE_ROWS: usize = 16;

pub(super) fn draw_related_records(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    default_open: bool,
) {
    let records = related_catalog_records(content);
    if records.is_empty() {
        return;
    }
    look::section(
        ui,
        ("related_records", content.hash),
        "Related Records",
        Some(records.len()),
        default_open,
        |ui| {
            let query = related_records_filter(ui, content.hash);
            let shown = records
                .iter()
                .filter(|record| related_record_matches(record, &query))
                .collect::<Vec<_>>();
            if shown.is_empty() {
                look::empty_state(ui, "No Matching Records");
                return;
            }
            if !query.is_empty() {
                ui.label(
                    egui::RichText::new(format!("{} of {}", shown.len(), records.len()))
                        .small()
                        .color(look::muted(ui)),
                );
            }
            draw_related_record_rows(ui, content, &shown);
        },
    );
}

/// The filter above Related Records, lower-cased and trimmed.
fn related_records_filter(ui: &mut egui::Ui, hash: u64) -> String {
    let id = egui::Id::new(("related_records_filter", hash));
    let mut query = ui.data(|data| data.get_temp::<String>(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .id(id.with("field"))
                .hint_text("Filter by Name, Kind, or Hash")
                .desired_width(260.0),
        );
        if !query.is_empty() && ui.small_button("Clear").clicked() {
            query.clear();
        }
    });
    let filter = query.trim().to_lowercase();
    ui.data_mut(|data| data.insert_temp(id, query));
    filter
}

fn related_record_matches(record: &RelatedCatalogRecord, query: &str) -> bool {
    query.is_empty()
        || record.label.to_lowercase().contains(query)
        || record.kind.to_lowercase().contains(query)
        || format_hash_hex(record.hash).to_lowercase().contains(query)
}

/// One row per record. Only the rows in view are laid out.
fn draw_related_record_rows(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    records: &[&RelatedCatalogRecord],
) {
    let show_state = records.iter().any(|record| record.state.is_some());
    let state_width = if show_state {
        RELATED_STATE_WIDTH + TABLE_COLUMN_GAP
    } else {
        0.0
    };
    let name_width = (ui.available_width()
        - ui.spacing().scroll.bar_width
        - RELATED_KIND_WIDTH
        - RELATED_HASH_WIDTH
        - state_width
        - TABLE_COLUMN_GAP * 2.0)
        .max(160.0);
    let muted = look::muted(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        let heading = |text: &str| egui::RichText::new(text).small().strong().color(muted);
        table_cell(ui, RELATED_KIND_WIDTH, heading("Kind"));
        table_cell(ui, name_width, heading("Name"));
        table_cell(ui, RELATED_HASH_WIDTH, heading("Hash"));
        if show_state {
            table_cell(ui, RELATED_STATE_WIDTH, heading("Current State"));
        }
    });
    let height =
        (TABLE_CELL_HEIGHT + TABLE_ROW_GAP) * records.len().min(RELATED_VISIBLE_ROWS) as f32;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("related_record_rows", content.hash))
            .min_scrolled_height(height)
            .max_height(height)
            .auto_shrink([false, true])
            .show_rows(ui, TABLE_CELL_HEIGHT, records.len(), |ui, range| {
                for record in &records[range] {
                    draw_related_record_row(ui, content.catalog, record, name_width, show_state);
                }
            });
    });
}

fn draw_related_record_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    record: &RelatedCatalogRecord,
    name_width: f32,
    show_state: bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        let muted = look::muted(ui);
        table_cell(
            ui,
            RELATED_KIND_WIDTH,
            egui::RichText::new(record.kind).color(muted),
        );
        ui.allocate_ui_with_layout(
            egui::vec2(name_width, TABLE_CELL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(name_width, TABLE_CELL_HEIGHT));
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                draw_named_catalog_hash_link(ui, catalog, record.hash, record.label.as_str());
            },
        );
        table_cell(
            ui,
            RELATED_HASH_WIDTH,
            egui::RichText::new(format_hash_hex(record.hash))
                .monospace()
                .color(muted),
        );
        if !show_state {
            return;
        }
        match &record.state {
            Some(state) => {
                table_cell(ui, RELATED_STATE_WIDTH, state.text.as_str())
                    .on_hover_text(state.tooltip.as_str());
            }
            None => {
                table_cell(
                    ui,
                    RELATED_STATE_WIDTH,
                    egui::RichText::new("-").color(muted),
                );
            }
        }
    });
}
