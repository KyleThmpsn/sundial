use std::collections::HashMap;

use eframe::egui;
use serde_json::Value;

use crate::{
    catalog::{Catalog, CollectibleDef, UnlockDefinition},
    hash::format_hash_hex,
};

use super::{
    glyphs::Glyph,
    inspector::{
        HashInspectionState, draw_catalog_hash_window,
        request_definition as request_hash_inspection,
        take_definition_context as take_hash_inspection_context,
        take_definition_request as take_hash_inspection_request,
    },
    progression::{CollectionStateSnapshot, collection_state_snapshot},
    ui::{
        TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, glyph_button,
        hierarchy_branch_cell as draw_branch_cell, hierarchy_leaf_cell as draw_leaf_cell,
        sortable_header_cell as header_cell, table_cell, toolbar as collection_toolbar,
    },
};

mod acquisition;
mod details;
mod hierarchy;

use acquisition::{
    AcquisitionState, FLAG_INSTRUCTION, OBJECTIVE_INSTRUCTION, VALUE_INSTRUCTION,
    acquisition_status, condition_metadata_lines, condition_program, state_lines,
};
pub(in crate::app) use acquisition::{
    collectible_acquired_state, collectible_acquisition_edit_available, collectible_state,
    set_collectible_acquisition_state,
};
use details::draw_collection_metadata_workspace;
use hierarchy::{
    CollectionLeaf, DisplayLine, acquisition_counts, branch_counts, build_hierarchy, display_lines,
    set_all_expansion, sort_hierarchy,
};

const TABLE_ROW_GAP: f32 = 2.0;

#[derive(Debug, Default)]
pub(super) struct UiState {
    query: String,
    sort: TableSort,
    expansion: HashMap<Vec<String>, bool>,
    metadata_index: Option<u16>,
    hash_inspection: HashInspectionState,
    status_filter: CollectionStatusFilter,
    reveal_selection: bool,
    mutation_feedback: Option<(bool, String)>,
}

impl UiState {
    pub(super) fn reset_navigation(&mut self) {
        self.metadata_index = None;
        self.hash_inspection.close();
        self.reveal_selection = false;
        self.mutation_feedback = None;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CollectionStatusFilter {
    #[default]
    All,
    Acquired,
    Missing,
    NoRule,
    Unknown,
}

impl CollectionStatusFilter {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Acquired,
        Self::Missing,
        Self::NoRule,
        Self::Unknown,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "All states",
            Self::Acquired => "Acquired",
            Self::Missing => "Missing",
            Self::NoRule => "No condition program",
            Self::Unknown => "Unresolved",
        }
    }

    const fn matches(self, state: AcquisitionState) -> bool {
        matches!(self, Self::All)
            || matches!(
                (self, state),
                (Self::Acquired, AcquisitionState::Acquired)
                    | (Self::Missing, AcquisitionState::Missing)
                    | (Self::NoRule, AcquisitionState::NoRule)
                    | (Self::Unknown, AcquisitionState::Unknown)
            )
    }
}

#[derive(Clone, Copy, Debug)]
struct TableSort {
    column: usize,
    descending: bool,
}

impl Default for TableSort {
    fn default() -> Self {
        Self {
            column: 3,
            descending: false,
        }
    }
}

pub(super) fn draw_content(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if let Some(error) = catalog.progression_package_error() {
        ui.colored_label(ui.visuals().warn_fg_color, "Package data incomplete")
            .on_hover_text(error);
        ui.add_space(4.0);
    }

    let Some(snapshot) = collection_state_snapshot(document) else {
        ui.colored_label(ui.visuals().error_fg_color, "Invalid progression settings");
        return false;
    };

    let mut changed = draw_collection_metadata_workspace(ui, document, catalog, &snapshot, state);

    let mut expansion_action = None;
    collection_toolbar(ui, |ui| {
        ui.label(egui::RichText::new("Filter").strong());
        let width = (ui.available_width() * 0.35).clamp(180.0, 420.0);
        ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Name, type, path, index, hash, or condition…")
                .desired_width(width),
        );
        egui::ComboBox::from_id_salt("collection_status_filter")
            .selected_text(state.status_filter.label())
            .width(160.0)
            .show_ui(ui, |ui| {
                for filter in CollectionStatusFilter::ALL {
                    ui.selectable_value(&mut state.status_filter, filter, filter.label());
                }
            });
        if glyph_button(ui, Glyph::ChevronDown, "Expand all collection branches").clicked() {
            expansion_action = Some(true);
        }
        if glyph_button(ui, Glyph::ChevronUp, "Collapse all collection branches").clicked() {
            expansion_action = Some(false);
        }
    });
    ui.add_space(6.0);
    let query = state.query.trim().to_lowercase();
    let leaves = catalog
        .collectibles()
        .iter()
        .filter(|definition| collection_matches(&query, definition, catalog))
        .map(|definition| collection_leaf(definition, &snapshot, catalog))
        .collect::<Vec<_>>();
    let counts = acquisition_counts(&leaves);
    let visible_leaves = leaves
        .iter()
        .filter(|leaf| state.status_filter.matches(leaf.status.state))
        .cloned()
        .collect::<Vec<_>>();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{} / {} acquired", counts.acquired, counts.total()));
        let mut remainder = Vec::new();
        if counts.missing > 0 {
            remainder.push(format!("{} missing", counts.missing));
        }
        if counts.no_rule > 0 {
            remainder.push(format!("{} no condition program", counts.no_rule));
        }
        if counts.unknown > 0 {
            remainder.push(format!("{} unresolved", counts.unknown));
        }
        if !remainder.is_empty() {
            ui.label(egui::RichText::new(format!("· {}", remainder.join(" · "))).weak());
        }
        if visible_leaves.len() != leaves.len() {
            ui.label(
                egui::RichText::new(format!("· {} shown", visible_leaves.len()))
                    .small()
                    .strong(),
            );
        }
    });

    let available_width = ui.available_width();
    let show_hash = available_width >= 680.0;
    let index_width = 70.0;
    let hash_width = if show_hash { 104.0 } else { 0.0 };
    let type_width = (available_width * 0.2).clamp(96.0, 190.0);
    let state_width = (available_width * 0.27).clamp(120.0, 280.0);
    let column_gaps = if show_hash { 4.0 } else { 3.0 };
    let item_width = (ui.available_width()
        - index_width
        - hash_width
        - type_width
        - state_width
        - TABLE_COLUMN_GAP * column_gaps)
        .max(170.0);
    ui.add_space(4.0);
    let mut columns = vec![
        (item_width, "Collectible"),
        (type_width, "Type"),
        (state_width, "Status"),
        (index_width, "Index"),
    ];
    if show_hash {
        columns.push((hash_width, "Hash"));
    }
    draw_header(ui, &columns, &mut state.sort);
    ui.separator();
    if visible_leaves.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return changed;
    }

    let mut hierarchy = build_hierarchy(&visible_leaves);
    if let Some(expanded) = expansion_action {
        set_all_expansion(&hierarchy, &mut state.expansion, expanded);
    }
    if state.reveal_selection {
        set_all_expansion(&hierarchy, &mut state.expansion, true);
    }
    sort_hierarchy(&mut hierarchy, state.sort);
    let auto_expand = !query.is_empty();
    let lines = display_lines(&hierarchy, &state.expansion, auto_expand);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("collections_table")
            .auto_shrink([false, false]);
        if state.reveal_selection {
            if let Some(selected) = state.metadata_index
                && let Some(line_index) = lines.iter().position(|line| {
                    matches!(line, DisplayLine::Leaf { leaf, .. } if leaf.definition.index == selected)
                })
            {
                scroll = scroll.vertical_scroll_offset(line_index as f32 * TABLE_CELL_HEIGHT);
            }
            state.reveal_selection = false;
        }
        scroll.show_rows(ui, TABLE_CELL_HEIGHT, lines.len(), |ui, range| {
                egui::Grid::new("collections_rows")
                    .num_columns(columns.len())
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            match &lines[line_index] {
                                DisplayLine::Branch {
                                    branch,
                                    depth,
                                    expanded,
                                } => {
                                    let response = draw_branch_cell(
                                        ui,
                                        item_width,
                                        *depth,
                                        &branch.label,
                                        *expanded,
                                        !auto_expand,
                                    )
                                    .on_hover_text(branch.path.join(" > "));
                                    if !auto_expand && response.clicked() {
                                        state.expansion.insert(branch.path.clone(), !expanded);
                                    }
                                    table_cell(ui, type_width, "");
                                    let branch_counts = branch_counts(branch);
                                    table_cell(
                                        ui,
                                        state_width,
                                        format!(
                                            "{} / {} acquired{}",
                                            branch_counts.acquired,
                                            branch_counts.total(),
                                            if branch_counts.unknown > 0 {
                                                format!(" · {} unresolved", branch_counts.unknown)
                                            } else {
                                                String::new()
                                            }
                                        ),
                                    );
                                    table_cell(ui, index_width, "");
                                    if show_hash {
                                        table_cell(ui, hash_width, "");
                                    }
                                }
                                DisplayLine::Leaf { leaf, depth } => {
                                    let accessible_name = if leaf.definition.name.trim().is_empty() {
                                        format_hash_hex(leaf.definition.hash)
                                    } else {
                                        leaf.definition.name.clone()
                                    };
                                    let response = draw_leaf_cell(
                                        ui,
                                        item_width,
                                        *depth,
                                        collection_name(leaf.definition),
                                    )
                                    .interact(egui::Sense::click())
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(format!(
                                        "Collectible hash: 0x{:08X}\nItem definition index: {}\nDefinition hash: 0x{:08X}",
                                        leaf.definition.hash,
                                        leaf.definition.item_definition_index,
                                        leaf.definition.item_hash
                                    ));
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            format!("Inspect collectible {accessible_name}"),
                                        )
                                    });
                                    if response.clicked() {
                                        state.metadata_index = Some(leaf.definition.index);
                                        state.mutation_feedback = None;
                                    }
                                    table_cell(
                                        ui,
                                        type_width,
                                        if leaf.definition.type_name.trim().is_empty() {
                        egui::RichText::new("-").weak()
                                        } else {
                                            egui::RichText::new(&leaf.definition.type_name)
                                        },
                                    );
                                    table_cell(ui, state_width, &leaf.status.text)
                                        .on_hover_text(&leaf.status.tooltip);
                                    table_cell(
                                        ui,
                                        index_width,
                                        egui::RichText::new(leaf.definition.index.to_string())
                                            .monospace(),
                                    );
                                    if show_hash {
                                        collection_hash_cell(ui, hash_width, leaf.definition.hash);
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    if let Some(hash) = take_hash_inspection_request(ui.ctx()) {
        let context = take_hash_inspection_context(ui.ctx(), hash);
        state.hash_inspection.open_with_context(hash, context);
    }
    changed |= draw_catalog_hash_window(
        ui.ctx(),
        catalog,
        Some(document),
        true,
        &mut state.hash_inspection,
        "collections",
    );
    changed
}

fn collection_name(definition: &CollectibleDef) -> egui::RichText {
    if definition.name.trim().is_empty() {
        egui::RichText::new(format_hash_hex(definition.hash)).monospace()
    } else {
        egui::RichText::new(&definition.name)
    }
}

fn collection_leaf<'a>(
    definition: &'a CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> CollectionLeaf<'a> {
    let references = state_lines(definition, snapshot, catalog);
    let mut status = acquisition_status(definition, snapshot, catalog);
    if !references.is_empty() {
        status.tooltip.push_str("\n\nReferenced save state\n");
        status.tooltip.push_str(&references.join("\n"));
    }
    let programs = condition_metadata_lines(definition);
    if !programs.is_empty() {
        status.tooltip.push_str("\n\nPackage conditions\n");
        status.tooltip.push_str(&programs.join("\n"));
    }
    CollectionLeaf { definition, status }
}

fn collection_matches(query: &str, definition: &CollectibleDef, catalog: &Catalog) -> bool {
    query.is_empty()
        || definition.name.to_lowercase().contains(query)
        || definition.type_name.to_lowercase().contains(query)
        || definition.index.to_string().contains(query)
        || format!("{:08x}", definition.hash).contains(query)
        || format!("{:08x}", definition.item_hash).contains(query)
        || definition
            .paths
            .iter()
            .flatten()
            .any(|component| component.to_lowercase().contains(query))
        || definition.conditions.iter().any(|condition| {
            condition.field.to_string().contains(query)
                || condition_program(condition).contains(query)
                || condition.tokens.iter().any(|token| match token.kind {
                    FLAG_INSTRUCTION => catalog
                        .unlock_flag_definition(token.operand as usize)
                        .is_some_and(|definition| {
                            unlock_definition_matches(query, token.operand as usize, definition)
                        }),
                    VALUE_INSTRUCTION => catalog
                        .unlock_value_definition(token.operand as usize)
                        .is_some_and(|definition| {
                            unlock_definition_matches(query, token.operand as usize, definition)
                        }),
                    OBJECTIVE_INSTRUCTION => catalog
                        .objective_definition(token.operand as usize)
                        .is_some_and(|objective| {
                            objective_matches(query, token.operand as usize, objective)
                        }),
                    _ => false,
                })
        })
}

fn objective_matches(query: &str, index: usize, objective: &crate::catalog::ObjectiveDef) -> bool {
    index.to_string().contains(query)
        || format!("{:08x}", objective.hash).contains(query)
        || [
            objective.name.as_str(),
            objective.progress_description.as_str(),
            objective.display_description.as_str(),
            objective.description.as_str(),
        ]
        .into_iter()
        .any(|text| text.to_lowercase().contains(query))
}

fn unlock_definition_matches(query: &str, index: usize, definition: &UnlockDefinition) -> bool {
    index.to_string().contains(query)
        || format!("{:08x}", definition.hash).contains(query)
        || definition
            .name
            .as_deref()
            .is_some_and(|name| name.to_lowercase().contains(query))
}

fn draw_header(ui: &mut egui::Ui, columns: &[(f32, &str)], sort: &mut TableSort) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for (column, (width, label)) in columns.iter().enumerate() {
            let marker = if sort.column == column {
                Some(if sort.descending {
                    Glyph::ChevronDown
                } else {
                    Glyph::ChevronUp
                })
            } else {
                None
            };
            if header_cell(ui, *width, label, marker).clicked() {
                if sort.column == column {
                    sort.descending = !sort.descending;
                } else {
                    sort.column = column;
                    sort.descending = false;
                }
            }
        }
    });
}

fn collection_hash_cell(ui: &mut egui::Ui, width: f32, hash: u64) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            if hash == 0 {
                ui.label(egui::RichText::new("-").weak());
                return;
            }
            let response = ui
                .add(
                    egui::Button::new(egui::RichText::new(format_hash_hex(hash)).monospace())
                        .frame(false),
                )
                .on_hover_text(format!("Open details for {}", format_hash_hex(hash)));
            if response.clicked() {
                request_hash_inspection(ui.ctx(), hash);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_progress_counts_and_filters_keep_unknown_state_explicit() {
        let states = [
            AcquisitionState::Acquired,
            AcquisitionState::Acquired,
            AcquisitionState::Missing,
            AcquisitionState::NoRule,
            AcquisitionState::Unknown,
        ];
        let mut counts = acquisition::AcquisitionCounts::default();
        for state in states {
            counts.add(state);
        }

        assert_eq!(counts.total(), 5);
        assert_eq!(counts.acquired, 2);
        assert_eq!(counts.missing, 1);
        assert_eq!(counts.no_rule, 1);
        assert_eq!(counts.unknown, 1);
        assert!(CollectionStatusFilter::Unknown.matches(AcquisitionState::Unknown));
        assert!(!CollectionStatusFilter::Unknown.matches(AcquisitionState::Missing));
        assert!(CollectionStatusFilter::All.matches(AcquisitionState::NoRule));
    }
}
