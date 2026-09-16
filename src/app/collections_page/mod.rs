use std::collections::{HashMap, HashSet};

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
        hierarchy_branch_cell as draw_branch_cell, hierarchy_selection_cell,
        sortable_header_cell as header_cell, table_cell, toolbar as collection_toolbar,
    },
};

mod acquisition;
mod bulk;
#[cfg(test)]
pub(in crate::app) use bulk::benchmark as bulk_benchmark;
mod cache;
mod details;
mod hierarchy;

use acquisition::{
    AcquisitionState, FLAG_INSTRUCTION, POOL_INSTRUCTION, VALUE_INSTRUCTION, acquisition_status,
    condition_metadata_lines, condition_program, for_each_expression_token, state_lines,
};
pub(in crate::app) use acquisition::{
    ExpressionValue, collectible_acquired_state, collectible_acquisition_edit_available,
    collectible_state, evaluate_expression_value_with, is_supported_condition_instruction,
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
    pub(super) read_only: bool,
    query: String,
    sort: TableSort,
    expansion: HashMap<Vec<String>, bool>,
    metadata_index: Option<u16>,
    hash_inspection: HashInspectionState,
    status_filter: CollectionStatusFilter,
    reveal_selection: bool,
    mutation_feedback: Option<(bool, String)>,
    bulk_feedback: Option<(bool, String)>,
    bulk_job: Option<bulk::Job>,
    bulk_ready: Option<bulk::Job>,
    selected: HashSet<u16>,
    browse: Option<cache::Cache>,
    cached_status: Option<StatusCache>,
}

#[derive(Debug)]
struct StatusCache {
    source: [Value; 3],
    native: bool,
    snapshot: CollectionStateSnapshot,
    rows: Vec<acquisition::StateLine>,
}
impl StatusCache {
    fn new(document: &Value, catalog: &Catalog) -> Option<Self> {
        let snapshot = collection_state_snapshot(document)?;
        let rows = catalog
            .collectibles()
            .iter()
            .map(|definition| collection_leaf(definition, &snapshot, catalog).status)
            .collect();
        Some(Self {
            source: [
                document["state"].clone(),
                document["_native_progression"]["family"].clone(),
                document["version"].clone(),
            ],
            native: document.get("_native_progression").is_some(),
            snapshot,
            rows,
        })
    }
}

impl UiState {
    pub(super) fn reset_navigation(&mut self) {
        self.cached_status = None;
        self.browse = None;
        self.metadata_index = None;
        self.hash_inspection.close();
        self.reveal_selection = false;
        self.mutation_feedback = None;
        self.bulk_feedback = None;
        self.bulk_job = None;
        self.bulk_ready = None;
        self.selected.clear();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CollectionStatusFilter {
    #[default]
    All,
    Acquired,
    NotAcquired,
    Unknown,
}

impl CollectionStatusFilter {
    const ALL: [Self; 4] = [Self::All, Self::Acquired, Self::NotAcquired, Self::Unknown];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "All States",
            Self::Acquired => "Acquired",
            Self::NotAcquired => "Not Acquired",
            Self::Unknown => "Unresolved",
        }
    }

    const fn matches(self, state: AcquisitionState) -> bool {
        matches!(self, Self::All)
            || matches!(
                (self, state),
                (Self::Acquired, AcquisitionState::Acquired)
                    | (Self::NotAcquired, AcquisitionState::NotAcquired)
                    | (Self::Unknown, AcquisitionState::Unknown)
            )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    let cached = state.cached_status.take().filter(|cached| {
        cached.native == document.get("_native_progression").is_some()
            && cached.source[0] == document["state"]
            && cached.source[1] == document["_native_progression"]["family"]
            && cached.source[2] == document["version"]
            && cached.rows.len() == catalog.collectibles().len()
    });
    if cached.is_none() {
        state.browse = None;
    }
    let Some(cached) = cached.or_else(|| StatusCache::new(document, catalog)) else {
        ui.colored_label(ui.visuals().error_fg_color, "Invalid progression settings");
        return false;
    };
    let mut changed =
        draw_collection_metadata_workspace(ui, document, catalog, &cached.snapshot, state);
    if changed {
        state.browse = None;
        ui.ctx().request_repaint();
        return true;
    }
    state.cached_status = Some(cached);

    if bulk::jobs(ui, document, catalog, state) {
        ui.ctx().request_repaint();
        return true;
    }

    let mut expansion_action = None;
    let cache = collection_toolbar(ui, |ui| {
        ui.strong("Filter");
        let width = (ui.available_width() * 0.25).clamp(160.0, 260.0);
        ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Name, type, path, index, hash, or condition…")
                .desired_width(width),
        );
        egui::ComboBox::from_id_salt("collection_status_filter")
            .selected_text(state.status_filter.label())
            .width(120.0)
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
        let cached = state
            .browse
            .take()
            .filter(|cache| cache.current(state) && expansion_action.is_none());
        let cache = cached.unwrap_or_else(|| cache::Cache::build(catalog, state, expansion_action));
        let counts = cache.counts;
        ui.label(format!("{} / {} acquired", counts.acquired, counts.total()))
            .on_hover_text("Calculated from the saved account and package definitions. Conditions that need live game context remain unresolved.");
        let mut remainder = Vec::new();
        if counts.not_acquired > 0 {
            remainder.push(format!("{} not acquired", counts.not_acquired));
        }
        if counts.unknown > 0 {
            remainder.push(format!("{} unresolved", counts.unknown));
        }
        if !remainder.is_empty() {
            ui.weak(format!("· {}", remainder.join(" · ")));
        }
        if cache.indices.len() != counts.total() {
            ui.label(
                egui::RichText::new(format!("· {} shown", cache.indices.len()))
                    .small()
                    .strong(),
            );
        }
        bulk::selection(ui, document, catalog, &cache.indices, state);
        cache
    });
    let query = state.query.trim().to_lowercase();

    let available_width = ui.available_width();
    let show_hash = available_width >= 680.0;
    let index_width = 70.0;
    let hash_width = if show_hash { 104.0 } else { 0.0 };
    let type_width = (available_width * 0.2).clamp(96.0, 190.0);
    let state_width = (available_width * 0.27).clamp(120.0, 280.0);
    let column_gaps = if show_hash { 4.0 } else { 3.0 };
    let item_width = (available_width
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
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        hierarchy_selection_cell(ui, item_width, 0, |ui, width| {
            bulk::checkbox(ui, &cache.indices, &mut state.selected);
            draw_header(ui, &[(width, "Collectible")], &mut state.sort, 0);
        });
        draw_header(ui, &columns[1..], &mut state.sort, 1);
    });
    ui.separator();
    if cache.indices.is_empty() {
        ui.weak("No matching rows");
        state.browse = Some(cache);
        return changed;
    }

    let auto_expand = !query.is_empty();
    let lines = &cache.lines;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("collections_table")
            .auto_shrink([false, false]);
        if state.reveal_selection {
            if let Some(selected) = state.metadata_index
                && let Some(line_index) = lines.iter().position(|line| {
                    matches!(line, cache::Line::Leaf { position, .. } if catalog.collectibles()[*position].index == selected)
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
                                cache::Line::Branch {label,path,depth,expanded,indices,counts} => {
                                    let response = hierarchy_selection_cell(ui, item_width, *depth, |ui, width| {
                                        bulk::checkbox(ui, indices, &mut state.selected);
                                        draw_branch_cell(
                                        ui,
                                        width,
                                        0,
                                        label,
                                        *expanded,
                                        !auto_expand,
                                    )})
                                    .on_hover_text(path.join(" > "));
                                    if !auto_expand && response.clicked() {
                                        state.expansion.insert(path.clone(), !expanded);
                                    }
                                    table_cell(ui, type_width, "");
                                    let branch_counts = counts;
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
                                cache::Line::Leaf { position, depth } => {
                                    let leaf=CollectionLeaf {definition:&catalog.collectibles()[*position],status:state.cached_status.as_ref().expect("statuses").rows[*position].clone()};
                                    let accessible_name = if leaf.definition.name.trim().is_empty() {
                                        format_hash_hex(leaf.definition.hash)
                                    } else {
                                        leaf.definition.name.clone()
                                    };
                                    let response = hierarchy_selection_cell(ui, item_width, *depth, |ui, width| {
                                        bulk::checkbox(ui, &[leaf.definition.index], &mut state.selected);
                                        table_cell(
                                        ui,
                                        width,
                                        collection_name(ui, leaf.definition),
                                    )})
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
        !state.read_only,
        &mut state.hash_inspection,
        "collections",
    );
    state.browse = Some(cache);
    changed
}

fn collection_name(ui: &egui::Ui, definition: &CollectibleDef) -> egui::RichText {
    if definition.name.trim().is_empty() {
        egui::RichText::new(format_hash_hex(definition.hash)).monospace()
    } else {
        super::ui::destiny_text(ui, &definition.name)
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
                || {
                    let mut matched = false;
                    for_each_expression_token(&condition.tokens, catalog, |token| {
                        matched |= match token.kind {
                            FLAG_INSTRUCTION => catalog
                                .unlock_flag_definition(token.operand as usize)
                                .is_some_and(|definition| {
                                    unlock_definition_matches(
                                        query,
                                        token.operand as usize,
                                        definition,
                                    )
                                }),
                            VALUE_INSTRUCTION => catalog
                                .unlock_value_definition(token.operand as usize)
                                .is_some_and(|definition| {
                                    unlock_definition_matches(
                                        query,
                                        token.operand as usize,
                                        definition,
                                    )
                                }),
                            POOL_INSTRUCTION => token.operand.to_string().contains(query),
                            _ => false,
                        };
                    });
                    matched
                }
        })
}

fn unlock_definition_matches(query: &str, index: usize, definition: &UnlockDefinition) -> bool {
    index.to_string().contains(query)
        || format!("{:08x}", definition.hash).contains(query)
        || definition
            .name
            .as_deref()
            .is_some_and(|name| name.to_lowercase().contains(query))
}

fn draw_header(ui: &mut egui::Ui, columns: &[(f32, &str)], sort: &mut TableSort, first: usize) {
    for (column, (width, label)) in columns.iter().enumerate() {
        let column = first + column;
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
}

fn collection_hash_cell(ui: &mut egui::Ui, width: f32, hash: u64) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            if hash == 0 {
                ui.weak("-");
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
            AcquisitionState::NotAcquired,
            AcquisitionState::Unknown,
        ];
        let mut counts = acquisition::AcquisitionCounts::default();
        for state in states {
            counts.add(state);
        }

        assert_eq!(counts.total(), 4);
        assert_eq!(counts.acquired, 2);
        assert_eq!(counts.not_acquired, 1);
        assert_eq!(counts.unknown, 1);
        assert!(CollectionStatusFilter::Unknown.matches(AcquisitionState::Unknown));
        assert!(!CollectionStatusFilter::Unknown.matches(AcquisitionState::NotAcquired));
        assert!(CollectionStatusFilter::All.matches(AcquisitionState::Acquired));
    }
}
