use super::*;
use crate::app::inspector::{DefinitionInspectionContext, definition_name};
use crate::app::{
    glyphs::Glyph,
    ui::{glyph_button, toolbar},
};

pub(in crate::app) fn draw_catalog_hash_window(
    ctx: &egui::Context,
    catalog: &Catalog,
    document: Option<&mut Value>,
    progression_editable: bool,
    hash_inspection: &mut HashInspectionState,
    viewport_salt: &'static str,
) -> bool {
    let Some(hash) = hash_inspection.current else {
        return false;
    };
    hash_inspection
        .runtime
        .prepare(catalog.install_path(), hash, catalog.inspection_access());
    hash_inspection.runtime.poll(ctx);
    let match_index = hash_inspection.match_index(catalog, hash);
    let matches = CatalogHashMatches::from_index(catalog, hash, &match_index);
    let match_groups = matches.match_groups();
    let match_count = match_groups.iter().map(|group| group.count).sum();
    let resolved_name = if matches.item_package_metadata.is_some() {
        catalog.package_item_name(hash).map(str::to_owned)
    } else if let Some(definition) = matches.item_stat_definition {
        (!definition.name.trim().is_empty()).then(|| definition.name.clone())
    } else if !matches.progression_definitions.is_empty() {
        matches
            .progression_definitions
            .iter()
            .find_map(|(_, definition)| progression_display_name(definition))
    } else {
        catalog.display_name(hash).map(str::to_owned).or_else(|| {
            matches
                .bucket_items
                .iter()
                .find_map(|item| catalog.inventory_metadata(item.hash))
                .map(|metadata| metadata.bucket_label())
        })
    };
    let title = resolved_name.as_deref().map_or_else(
        || format!("Definition Inspector: 0x{hash:08X}"),
        |name| format!("Definition Inspector: {name}"),
    );
    let default_size = hash_inspector_default_size(&matches);
    let history = hash_inspection
        .history
        .iter()
        .map(|entry| entry.hash)
        .collect::<Vec<_>>();
    let forward = hash_inspection
        .forward
        .iter()
        .map(|entry| entry.hash)
        .collect::<Vec<_>>();
    let mut lookup = hash_inspection.lookup.clone();
    let mut lookup_error = hash_inspection.lookup_error;
    let viewport_id = egui::ViewportId::from_hash_of(("catalog_hash_inspector", viewport_salt));
    let (action, close_requested) = {
        let document_ref = document.as_deref();
        let collection_state = document_ref.and_then(collection_state_snapshot);
        let content = HashInspectorContent {
            catalog,
            document: document_ref,
            collection_state: collection_state.as_ref(),
            progression_editable: progression_editable && document_ref.is_some(),
            mutation_feedback: hash_inspection.mutation_feedback.as_ref(),
            hash,
            resolved_name: &resolved_name,
            history: &history,
            forward: &forward,
            matches: &matches,
            match_groups: &match_groups,
            match_count,
            source_context: hash_inspection.source_context.as_ref(),
        };
        ctx.show_viewport_immediate(
            viewport_id,
            egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size(default_size)
                .with_min_inner_size([720.0, 520.0])
                .with_max_inner_size([1_600.0, 1_100.0])
                .with_resizable(true),
            |child_ctx, class| {
                let mut action = HashInspectorAction::default();
                let mut embedded_open = true;
                if class == egui::ViewportClass::Embedded {
                    egui::Window::new(&title)
                        .id(egui::Id::new((
                            "embedded_catalog_hash_inspector",
                            viewport_salt,
                        )))
                        .open(&mut embedded_open)
                        .resizable(true)
                        .default_size(default_size)
                        .show(child_ctx, |ui| {
                            draw_hash_inspector_contents(
                                ui,
                                &content,
                                &mut action,
                                &mut lookup,
                                &mut lookup_error,
                                &mut hash_inspection.runtime,
                            );
                        });
                } else {
                    egui::CentralPanel::default().show(child_ctx, |ui| {
                        draw_hash_inspector_contents(
                            ui,
                            &content,
                            &mut action,
                            &mut lookup,
                            &mut lookup_error,
                            &mut hash_inspection.runtime,
                        );
                    });
                }
                action.open_hash = action
                    .open_hash
                    .or_else(|| take_hash_inspection_request(child_ctx));
                let close_requested = !embedded_open
                    || child_ctx.input(|input| {
                        input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
                    });
                (action, close_requested)
            },
        )
    };
    hash_inspection.lookup = lookup;
    hash_inspection.lookup_error = lookup_error;

    let changed = progression_editable
        && action.progression_edit.is_some_and(|edit| {
            match document
                .ok_or_else(|| "No editable Sunrise state is loaded".to_owned())
                .and_then(|document| apply_inspector_progression_edit(document, catalog, edit))
            {
                Ok(message) => {
                    hash_inspection.mutation_feedback = Some((false, message));
                    true
                }
                Err(error) => {
                    hash_inspection.mutation_feedback = Some((true, error));
                    false
                }
            }
        });

    if close_requested {
        hash_inspection.close();
    } else if let Some(history_index) = action.history_index {
        hash_inspection.navigate_history(history_index);
    } else if action.navigate_back {
        hash_inspection.back();
    } else if action.navigate_forward {
        hash_inspection.forward();
    } else if let Some(requested_hash) = action.open_hash {
        hash_inspection.open(requested_hash);
    }
    changed
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct HashInspectorAction {
    navigate_back: bool,
    navigate_forward: bool,
    history_index: Option<usize>,
    open_hash: Option<u64>,
    pub(super) progression_edit: Option<InspectorProgressionEdit>,
}

fn apply_inspector_progression_edit(
    document: &mut Value,
    catalog: &Catalog,
    edit: InspectorProgressionEdit,
) -> Result<String, String> {
    match edit {
        InspectorProgressionEdit::Flag {
            definition_index,
            set,
        } => {
            let definition = catalog
                .unlock_flag_definition(definition_index)
                .ok_or_else(|| {
                    format!("Unlock Flag Definition #{definition_index} is unavailable")
                })?;
            set_collection_flag(document, definition_index, definition, set)
                .then(|| {
                    format!(
                        "{} {}",
                        definition
                            .name
                            .as_deref()
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or("Unlock flag"),
                        if set { "set" } else { "unset" }
                    )
                })
                .ok_or_else(|| "The unlock flag could not be updated".to_owned())
        }
        InspectorProgressionEdit::Value {
            definition_index,
            value,
        } => {
            let definition = catalog
                .unlock_value_definition(definition_index)
                .ok_or_else(|| {
                    format!("Unlock Value Definition #{definition_index} is unavailable")
                })?;
            set_collection_value(document, definition_index, definition, value)
                .then(|| {
                    format!(
                        "{} set to {value}",
                        definition
                            .name
                            .as_deref()
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or("Unlock value")
                    )
                })
                .ok_or_else(|| "The unlock value could not be updated".to_owned())
        }
        InspectorProgressionEdit::Collectible {
            collectible_index,
            acquired,
        } => {
            let definition = catalog
                .collectibles()
                .iter()
                .find(|definition| definition.index == collectible_index)
                .ok_or_else(|| format!("Collectible #{collectible_index} is unavailable"))?;
            let snapshot = collection_state_snapshot(document)
                .ok_or_else(|| "The loaded progression settings are invalid".to_owned())?;
            crate::app::collections_page::set_collectible_acquisition_state(
                document, definition, &snapshot, catalog, acquired,
            )?;
            Ok(format!(
                "{} marked {}",
                collectible_item_name(catalog, definition),
                if acquired { "acquired" } else { "missing" }
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashInspectorSection {
    Item,
    Progression,
    Collections,
    Unlocks,
}

struct HashInspectorContent<'a> {
    catalog: &'a Catalog,
    document: Option<&'a Value>,
    collection_state: Option<&'a CollectionStateSnapshot>,
    progression_editable: bool,
    mutation_feedback: Option<&'a (bool, String)>,
    hash: u64,
    resolved_name: &'a Option<String>,
    history: &'a [u64],
    forward: &'a [u64],
    matches: &'a CatalogHashMatches<'a>,
    match_groups: &'a [CatalogMatchGroup],
    match_count: usize,
    source_context: Option<&'a DefinitionInspectionContext>,
}

impl HashInspectorSection {
    const fn label(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Progression => "Progression",
            Self::Collections => "Collections",
            Self::Unlocks => "Unlocks",
        }
    }
}

fn draw_hash_inspector_contents(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
    lookup: &mut String,
    lookup_error: &mut bool,
    runtime: &mut super::runtime::RuntimeInspectionState,
) {
    let HashInspectorContent {
        catalog,
        document,
        collection_state,
        progression_editable,
        mutation_feedback,
        hash,
        resolved_name,
        history,
        forward,
        matches,
        match_groups: _,
        match_count,
        source_context,
    } = content;
    if !history.is_empty()
        && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft))
    {
        action.navigate_back = true;
    }
    if !forward.is_empty()
        && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight))
    {
        action.navigate_forward = true;
    }
    let sections = hash_inspector_sections(matches);
    toolbar(ui, |ui| {
        let previous_label = history.last().copied().map_or_else(
            || "No previous definition".to_owned(),
            |previous| {
                format!(
                    "Back to {} (Alt+Left)",
                    hash_history_label(catalog, previous)
                )
            },
        );
        if ui
            .add_enabled_ui(!history.is_empty(), |ui| {
                glyph_button(ui, Glyph::ChevronLeft, &previous_label)
            })
            .inner
            .clicked()
        {
            action.navigate_back = true;
        }
        let next_label = forward.last().copied().map_or_else(
            || "No next definition".to_owned(),
            |next| {
                format!(
                    "Forward to {} (Alt+Right)",
                    hash_history_label(catalog, next)
                )
            },
        );
        if ui
            .add_enabled_ui(!forward.is_empty(), |ui| {
                glyph_button(ui, Glyph::ChevronRight, &next_label)
            })
            .inner
            .clicked()
        {
            action.navigate_forward = true;
        }
        ui.separator();
        ui.label(egui::RichText::new("Inspect Hash").strong());
        let response = ui.add(
            egui::TextEdit::singleline(lookup)
                .hint_text("0x00000000")
                .desired_width(130.0),
        );
        if response.changed() {
            *lookup_error = false;
        }
        let submitted = ui.button("Open").clicked()
            || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        if submitted {
            if let Some(requested_hash) = parse_hash_hex(lookup).filter(|hash| *hash != 0) {
                action.open_hash = Some(requested_hash);
                *lookup_error = false;
            } else {
                *lookup_error = true;
            }
        }
        ui.menu_button("More", |ui| {
            if ui.button("Copy Hash").clicked() {
                ui.ctx().copy_text(format_hash_hex(*hash));
                ui.close_menu();
            }
            if ui.button("Copy Technical Report").clicked() {
                ui.ctx().copy_text(hash_inspector_report(content));
                ui.close_menu();
            }
            if !history.is_empty() {
                ui.separator();
                ui.strong("Recent");
                for (index, previous_hash) in history.iter().copied().enumerate().rev() {
                    if ui
                        .button(hash_history_label(catalog, previous_hash))
                        .clicked()
                    {
                        action.history_index = Some(index);
                        ui.close_menu();
                    }
                }
            }
        });
    });
    if *lookup_error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Enter a non-zero, 0x-prefixed hexadecimal hash.",
        );
    }
    if let Some((error, message)) = mutation_feedback {
        if *error {
            ui.colored_label(ui.visuals().error_fg_color, message);
        } else {
            ui.label(egui::RichText::new(message).weak());
        }
    }
    ui.separator();

    let is_item = matches.item.is_some() || matches.item_package_metadata.is_some();
    let page_id = ui.id().with(("item_inspector_page", *hash));
    let mut page = ui.data_mut(|data| {
        data.get_temp::<super::item::ItemPage>(page_id)
            .unwrap_or_default()
    });
    if is_item {
        ui.horizontal_wrapped(|ui| {
            for candidate in super::item::ItemPage::ALL {
                ui.selectable_value(&mut page, candidate, candidate.label());
            }
        });
        ui.data_mut(|data| data.insert_temp(page_id, page));
        ui.separator();
    }
    egui::ScrollArea::vertical()
        .id_salt(("catalog_hash_metadata_scroll", *hash, page))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if matches.item_package_metadata.is_none() && matches.item.is_none() {
                draw_hash_answer_layer(ui, content);
                ui.add_space(8.0);
            }

            if sections.contains(&HashInspectorSection::Item) {
                draw_hash_item_matches(
                    ui,
                    super::item::ItemInspection { catalog, hash: *hash, resolved_name, matches, source_context: *source_context },
                    runtime,
                    page,
                );
            }
            let mut draw_related_sections = |ui: &mut egui::Ui| {
                if sections.contains(&HashInspectorSection::Progression) {
                    draw_hash_progression_matches(ui, catalog, *document, *hash, matches);
                }
                if sections.contains(&HashInspectorSection::Collections) {
                    draw_hash_collection_matches(
                        ui,
                        catalog,
                        *hash,
                        matches,
                        *collection_state,
                        *progression_editable,
                        action,
                    );
                }
                if sections.contains(&HashInspectorSection::Unlocks) {
                    draw_hash_unlock_matches(
                        ui,
                        catalog,
                        matches,
                        *collection_state,
                        *progression_editable,
                        action,
                    );
                }
            };
            if !is_item || page == super::item::ItemPage::Related {
                draw_related_sections(ui);
                draw_related_catalog_records(ui, content);
                if is_item && sections.len() == 1 && related_catalog_records(content).is_empty() {
                    ui.weak("No related progression, collection, or unlock records are indexed for this item.");
                }
            }
            if *match_count == 0 {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("No directly indexed package entity uses this hash.")
                        .weak(),
                );
            }
        });
}

fn draw_hash_answer_layer(ui: &mut egui::Ui, content: &HashInspectorContent<'_>) {
    let kind = if content.matches.item.is_some() || content.matches.item_package_metadata.is_some()
    {
        content
            .catalog
            .package_item_type_name(content.hash)
            .unwrap_or("Inventory Item")
    } else if !content.matches.progression_definitions.is_empty() {
        "Progression Definition"
    } else if !content.matches.objectives.is_empty() {
        "Objective"
    } else if !content.matches.collectible_matches.is_empty() {
        "Collectible"
    } else if !content.matches.flag_definitions.is_empty() {
        "Unlock flag"
    } else if !content.matches.value_definitions.is_empty() {
        "Unlock value"
    } else {
        "Catalog Hash"
    };
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            crate::app::item_editor::catalog_item_tooltip(
                ui.label(
                    egui::RichText::new(
                        content
                            .resolved_name
                            .as_deref()
                            .unwrap_or("No display name resolved"),
                    )
                    .strong()
                    .size(18.0),
                ),
                content.catalog,
                content.hash,
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(kind);
                ui.separator();
                crate::app::item_editor::catalog_item_tooltip(
                    ui.label(
                        egui::RichText::new(format_hash_hex(content.hash))
                            .monospace()
                            .weak(),
                    ),
                    content.catalog,
                    content.hash,
                );
            });
            if content.match_count > 1 {
                draw_catalog_match_groups(ui, content);
            }
            if let Some(context) = content.source_context {
                ui.add_space(5.0);
                ui.label(egui::RichText::new(format!("Opened from {}", context.source)).weak());
            } else if content.match_count == 0 {
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new("No indexed catalog entity uses this hash.")
                        .weak()
                        .italics(),
                );
            }
        });
}

fn draw_catalog_match_groups(ui: &mut egui::Ui, content: &HashInspectorContent<'_>) {
    ui.add_space(5.0);
    egui::CollapsingHeader::new(format!("Catalog Locations ({})", content.match_count))
        .id_salt(("catalog_match_groups", content.hash))
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new(("catalog_match_group_rows", content.hash))
                .num_columns(2)
                .spacing([16.0, 3.0])
                .show(ui, |ui| {
                    for group in content.match_groups {
                        ui.label(group.label);
                        ui.label(
                            egui::RichText::new(group.count.to_string())
                                .monospace()
                                .weak(),
                        );
                        ui.end_row();
                    }
                });
        });
}

struct RelatedCatalogRecord {
    hash: u64,
    kind: &'static str,
    label: String,
    state: Option<RelatedRecordState>,
}

struct RelatedRecordState {
    text: String,
    tooltip: String,
}

struct RelatedCatalogRecordAccumulator<'a, 'catalog> {
    content: &'a HashInspectorContent<'catalog>,
    records: Vec<RelatedCatalogRecord>,
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
        if let Some(record) = self.records.iter_mut().find(|record| record.hash == hash) {
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
        let state = content.collection_state.map(|snapshot| {
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
        let state = content.collection_state.map(|snapshot| {
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

fn add_item_related_records(
    records: &mut RelatedCatalogRecordAccumulator<'_, '_>,
    content: &HashInspectorContent<'_>,
) {
    for item in &content.matches.bucket_items {
        records.add(
            item.hash,
            "Inventory Bucket Item",
            format_hash_hex(item.hash),
            None,
        );
    }
    for (item_hash, _) in &content.matches.investment_stat_references {
        records.add(
            *item_hash,
            "Item Using This Stat",
            format_hash_hex(*item_hash),
            None,
        );
    }
}

fn related_catalog_records(content: &HashInspectorContent<'_>) -> Vec<RelatedCatalogRecord> {
    let mut records = RelatedCatalogRecordAccumulator {
        content,
        records: Vec::new(),
    };
    add_progression_related_records(&mut records, content);
    add_unlock_related_records(&mut records, content);
    add_collection_related_records(&mut records, content);
    add_item_related_records(&mut records, content);
    records.records
}

fn draw_related_catalog_records(ui: &mut egui::Ui, content: &HashInspectorContent<'_>) {
    let records = related_catalog_records(content);
    if records.is_empty() {
        return;
    }
    let title = format!("Related Records ({})", records.len());
    let show_state = records.iter().any(|record| record.state.is_some());
    hash_metadata_section(ui, &title, false, |ui| {
        egui::Grid::new(("related_catalog_records", content.hash))
            .num_columns(if show_state { 4 } else { 3 })
            .spacing([14.0, 3.0])
            .striped(true)
            .show(ui, |ui| {
                for record in records {
                    ui.label(egui::RichText::new(record.kind).weak());
                    draw_named_catalog_hash_link(ui, content.catalog, record.hash, record.label);
                    draw_catalog_hash_link(
                        ui,
                        content.catalog,
                        record.hash,
                        format_hash_hex(record.hash),
                    );
                    if show_state {
                        if let Some(state) = record.state {
                            ui.label(format!("Current: {}", state.text))
                                .on_hover_text(state.tooltip);
                        } else {
                            ui.label(egui::RichText::new("-").weak());
                        }
                    }
                    ui.end_row();
                }
            });
    });
}

fn hash_inspector_report(content: &HashInspectorContent<'_>) -> String {
    let mut report = String::new();
    append_report_overview(&mut report, content);
    append_report_source(&mut report, content);
    append_report_related_records(&mut report, content);
    append_report_item_data(&mut report, content);
    append_report_progression_data(&mut report, content);
    append_report_collection_data(&mut report, content);
    append_report_unlock_data(&mut report, content);
    report.trim_end().to_owned()
}

fn append_report_overview(report: &mut String, content: &HashInspectorContent<'_>) {
    report.push_str("# Sundial definition inspector report\n\n");
    report.push_str("## Overview\n\n| Field | Value |\n| --- | --- |\n");
    report_table_row(report, "Hash", &format_hash_hex_and_decimal(content.hash));
    report_table_row(
        report,
        "Name",
        content.resolved_name.as_deref().unwrap_or("Not resolved"),
    );
    report_table_row(
        report,
        "Catalog Locations",
        &content.match_count.to_string(),
    );
    report_table_row(
        report,
        "Sections",
        &hash_inspector_sections(content.matches)
            .into_iter()
            .map(HashInspectorSection::label)
            .collect::<Vec<_>>()
            .join(", "),
    );
    report.push_str("\n### Catalog locations\n\n| Location | Matches |\n| --- | ---: |\n");
    if content.match_groups.is_empty() {
        report.push_str("| None | 0 |\n");
    } else {
        for group in content.match_groups {
            report_table_row(report, group.label, &group.count.to_string());
        }
    }
}

fn append_report_source(report: &mut String, content: &HashInspectorContent<'_>) {
    let Some(context) = content.source_context else {
        return;
    };
    report.push_str("\n## Selected instance\n\n| Field | Value |\n| --- | --- |\n");
    report_table_row(report, "Opened from", &context.source);
    if let Some(instance_id) = &context.instance_id {
        report_table_row(report, "Instance", instance_id);
    }
    if let Some(level) = context.authored_level {
        report_table_row(report, "Authored level", &level.to_string());
        report_table_row(
            report,
            "Displayed Power",
            &crate::app::item_editor::displayed_item_power(level).to_string(),
        );
    }
    if let Some(flags) = context.flags {
        report_table_row(report, "Flags", &format!("0x{flags:02X} · {flags}"));
    }
    if let Some(plug_count) = context.plug_count {
        report_table_row(report, "Authored plugs", &plug_count.to_string());
    }
    append_report_json(
        report,
        "Opening-time source snapshot (not live state)",
        &serde_json::json!(context),
    );
}

fn append_report_related_records(report: &mut String, content: &HashInspectorContent<'_>) {
    let records = related_catalog_records(content);
    if records.is_empty() {
        return;
    }
    report.push_str(
        "\n## Related records\n\n| Kind | Name | Hash | Current state |\n| --- | --- | --- | --- |\n",
    );
    for record in records {
        report.push_str("| ");
        report.push_str(&markdown_cell(record.kind));
        report.push_str(" | ");
        report.push_str(&markdown_cell(&record.label));
        report.push_str(" | ");
        report.push_str(&format_hash_hex(record.hash));
        report.push_str(" | ");
        report.push_str(&markdown_cell(
            record
                .state
                .as_ref()
                .map_or("N/A", |state| state.text.as_str()),
        ));
        report.push_str(" |\n");
    }
}

fn append_report_item_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    let has_item_data = matches.item.is_some()
        || matches.item_package_metadata.is_some()
        || matches.inventory_metadata.is_some()
        || matches.item_stat_definition.is_some()
        || !matches.investment_stat_references.is_empty()
        || !matches.bucket_items.is_empty();
    if !has_item_data {
        return;
    }
    let investment_references = matches
        .investment_stat_references
        .iter()
        .map(|(item_hash, stat)| {
            serde_json::json!({
                "item_hash": item_hash,
                "item_name": content.catalog.package_item_name(*item_hash),
                "stat": stat,
            })
        })
        .collect::<Vec<_>>();
    let bucket_items = matches
        .bucket_items
        .iter()
        .map(|item| serde_json::json!({ "definition": item }))
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "item_definition": matches.item,
        "package_metadata": matches.item_package_metadata,
        "inventory_metadata": matches.inventory_metadata,
        "material_requirement_set_indices": matches.item_material_requirement_set_indices,
        "item_stat_definition": matches.item_stat_definition,
        "resolved_stat_group": content.catalog.item_stat_group(content.hash),
        "resolved_socket_pools": matches.item.map(|item| super::item_details::resolved_socket_pools(content.catalog, item)),
        "resolved_item_traits": matches.item_package_metadata.map(|metadata| metadata.trait_indices.iter().map(|index| serde_json::json!({
            "index": index, "definition": content.catalog.trait_definitions().get(usize::from(*index)),
        })).collect::<Vec<_>>()),
        "investment_stat_references": investment_references,
        "inventory_bucket_items": bucket_items,
    });
    append_report_json(report, "Item package data", &data);
}

fn append_report_progression_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    let has_progression_data = !matches.progression_definitions.is_empty()
        || !matches.progression_reward_matches.is_empty()
        || !matches.progression_faction_matches.is_empty()
        || !matches.objectives.is_empty()
        || !matches.owner_matches.is_empty()
        || !matches.trait_matches.is_empty()
        || !matches.context_matches.is_empty();
    if !has_progression_data {
        return;
    }
    let definitions = matches
        .progression_definitions
        .iter()
        .map(|(index, definition)| serde_json::json!({ "index": index, "definition": definition }))
        .collect::<Vec<_>>();
    let rewards = matches
        .progression_reward_matches
        .iter()
        .map(|(index, definition, reward_index)| {
            serde_json::json!({
                "progression_index": index,
                "progression": definition,
                "reward_index": reward_index,
                "matched_reward": definition.reward_items[*reward_index],
            })
        })
        .collect::<Vec<_>>();
    let factions = matches
        .progression_faction_matches
        .iter()
        .map(|(index, definition, faction_index, faction)| {
            serde_json::json!({
                "progression_index": index,
                "progression": definition,
                "faction_index": faction_index,
                "matched_faction": faction,
            })
        })
        .collect::<Vec<_>>();
    let objectives = matches
        .objectives
        .iter()
        .map(|(index, objective)| serde_json::json!({ "index": index, "objective": objective }))
        .collect::<Vec<_>>();
    let owners = matches
        .owner_matches
        .iter()
        .map(|(objective_index, objective, owner)| {
            serde_json::json!({
                "objective_index": objective_index,
                "objective": objective,
                "matched_owner": owner,
            })
        })
        .collect::<Vec<_>>();
    let traits = matches
        .trait_matches
        .iter()
        .map(|(objective_index, objective, owner, trait_definition)| {
            serde_json::json!({
                "objective_index": objective_index,
                "objective": objective,
                "owner": owner,
                "matched_trait": trait_definition,
            })
        })
        .collect::<Vec<_>>();
    let readers = matches
        .context_matches
        .iter()
        .map(|(kind, definition_index, context)| {
            serde_json::json!({
                "source_kind": kind,
                "source_definition_index": definition_index,
                "matched_reader": context,
            })
        })
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "progression_definitions": definitions,
        "reward_references": rewards,
        "faction_references": factions,
        "objectives": objectives,
        "objective_owner_references": owners,
        "objective_trait_references": traits,
        "progression_readers": readers,
    });
    append_report_json(report, "Progression package data", &data);
}

fn append_report_collection_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    if matches.collectible_matches.is_empty() && matches.material_requirement_set_matches.is_empty()
    {
        return;
    }
    let collectibles = matches
        .collectible_matches
        .iter()
        .map(|collectible| {
            let state = content.collection_state.map(|snapshot| {
                crate::app::collections_page::collectible_state(
                    collectible,
                    snapshot,
                    content.catalog,
                )
                .0
            });
            serde_json::json!({ "definition": collectible, "current_state": state })
        })
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "collectibles": collectibles,
        "material_requirement_sets": matches.material_requirement_set_matches,
    });
    append_report_json(report, "Collections package data", &data);
}

fn append_report_unlock_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    if matches.flag_definitions.is_empty() && matches.value_definitions.is_empty() {
        return;
    }
    let flags = matches
        .flag_definitions
        .iter()
        .map(|(index, definition)| {
            let state = content
                .collection_state
                .map(|snapshot| snapshot.flag_text(*index, definition));
            serde_json::json!({ "index": index, "definition": definition, "current_state": state })
        })
        .collect::<Vec<_>>();
    let values = matches
        .value_definitions
        .iter()
        .map(|(index, definition)| {
            let state = content
                .collection_state
                .map(|snapshot| snapshot.value_text(*index, definition));
            serde_json::json!({ "index": index, "definition": definition, "current_state": state })
        })
        .collect::<Vec<_>>();
    let data = serde_json::json!({ "flag_definitions": flags, "value_definitions": values });
    append_report_json(report, "Unlock package data", &data);
}

fn append_report_json<T: serde::Serialize>(report: &mut String, heading: &str, value: &T) {
    report.push_str("\n## ");
    report.push_str(heading);
    report.push_str("\n\n```json\n");
    match serde_json::to_string_pretty(value) {
        Ok(json) => report.push_str(&json),
        Err(error) => report.push_str(&format!("{{\"serialization_error\":\"{error}\"}}")),
    }
    report.push_str("\n```\n");
}

fn report_table_row(report: &mut String, label: &str, value: &str) {
    report.push_str("| ");
    report.push_str(&markdown_cell(label));
    report.push_str(" | ");
    report.push_str(&markdown_cell(value));
    report.push_str(" |\n");
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn hash_history_label(catalog: &Catalog, hash: u64) -> String {
    catalog.display_name(hash).map_or_else(
        || format_hash_hex(hash),
        |name| format!("{name} · {}", format_hash_hex(hash)),
    )
}

fn hash_inspector_sections(matches: &CatalogHashMatches<'_>) -> Vec<HashInspectorSection> {
    let mut sections = Vec::with_capacity(4);
    if matches.item.is_some()
        || matches.item_package_metadata.is_some()
        || matches.item_stat_definition.is_some()
        || !matches.investment_stat_references.is_empty()
        || matches.inventory_metadata.is_some()
        || !matches.bucket_items.is_empty()
    {
        sections.push(HashInspectorSection::Item);
    }
    if !matches.progression_definitions.is_empty()
        || !matches.progression_reward_matches.is_empty()
        || !matches.progression_faction_matches.is_empty()
        || !matches.objectives.is_empty()
        || !matches.owner_matches.is_empty()
        || !matches.trait_matches.is_empty()
        || !matches.context_matches.is_empty()
    {
        sections.push(HashInspectorSection::Progression);
    }
    if !matches.collectible_matches.is_empty()
        || !matches.material_requirement_set_matches.is_empty()
    {
        sections.push(HashInspectorSection::Collections);
    }
    if !matches.flag_definitions.is_empty() || !matches.value_definitions.is_empty() {
        sections.push(HashInspectorSection::Unlocks);
    }
    sections
}

fn hash_inspector_default_size(matches: &CatalogHashMatches<'_>) -> egui::Vec2 {
    if matches.item.is_some() || matches.item_package_metadata.is_some() {
        return egui::vec2(1_000.0, 720.0);
    }
    if matches.progression_definitions.len() == 1 && matches.count() == 1 {
        let steps = matches.progression_definitions[0].1.steps.len() as f32;
        return egui::vec2(920.0, (560.0 + steps.min(12.0) * 20.0).clamp(640.0, 820.0));
    }
    if matches.count() <= 2 {
        egui::vec2(880.0, 640.0)
    } else {
        egui::vec2(1_000.0, 760.0)
    }
}
