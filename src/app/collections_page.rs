use std::{cmp::Ordering, collections::HashMap};

use eframe::egui;
use serde_json::Value;

use crate::catalog::{
    Catalog, CollectibleDef, CollectionConditionDef, CollectionConditionTokenDef, UnlockDefinition,
};

use super::{
    glyphs::Glyph,
    progression::{
        CollectionStateSnapshot, HashInspectionState, collection_flag_state_text,
        collection_state_snapshot, collection_value_state_text, draw_catalog_hash_window,
        request_hash_inspection, set_collection_flag, set_collection_value,
        take_hash_inspection_request,
    },
    ui::{
        TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, hierarchy_branch_cell as draw_branch_cell,
        hierarchy_leaf_cell as draw_leaf_cell, inspector_heading,
        sortable_header_cell as header_cell, table_cell, toolbar as collection_toolbar,
    },
};

const TABLE_ROW_GAP: f32 = 2.0;
const ACQUISITION_CONDITION_FIELD: u8 = 3;
const FLAG_INSTRUCTION: u32 = 1;
const NOT_INSTRUCTION: u32 = 2;
const OR_INSTRUCTION: u32 = 3;
const AND_INSTRUCTION: u32 = 4;
const EQUAL_INSTRUCTION: u32 = 8;
const NOT_EQUAL_INSTRUCTION: u32 = 9;
const VALUE_INSTRUCTION: u32 = 10;
const LITERAL_INSTRUCTION: u32 = 11;
const OBJECTIVE_INSTRUCTION: u32 = 12;
const GREATER_THAN_INSTRUCTION: u32 = 13;
const GREATER_OR_EQUAL_INSTRUCTION: u32 = 14;
const LEGACY_LITERAL_ENCODING_INSTRUCTION: u32 = 22;

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

#[derive(Clone)]
struct StateLine {
    text: String,
    tooltip: String,
    state: AcquisitionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcquisitionState {
    Acquired,
    Missing,
    NoRule,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AcquisitionCounts {
    acquired: usize,
    missing: usize,
    no_rule: usize,
    unknown: usize,
}

impl AcquisitionCounts {
    const fn total(self) -> usize {
        self.acquired + self.missing + self.no_rule + self.unknown
    }

    fn add(&mut self, state: AcquisitionState) {
        match state {
            AcquisitionState::Acquired => self.acquired += 1,
            AcquisitionState::Missing => self.missing += 1,
            AcquisitionState::NoRule => self.no_rule += 1,
            AcquisitionState::Unknown => self.unknown += 1,
        }
    }
}

#[derive(Clone)]
struct CollectionLeaf<'a> {
    definition: &'a CollectibleDef,
    status: StateLine,
}

#[derive(Default)]
struct CollectionHierarchy<'a> {
    branches: Vec<CollectionBranch<'a>>,
    leaves: Vec<CollectionLeaf<'a>>,
}

struct CollectionBranch<'a> {
    label: String,
    path: Vec<String>,
    branches: Vec<CollectionBranch<'a>>,
    leaves: Vec<CollectionLeaf<'a>>,
}

enum DisplayLine<'tree, 'data> {
    Branch {
        branch: &'tree CollectionBranch<'data>,
        depth: usize,
        expanded: bool,
    },
    Leaf {
        leaf: &'tree CollectionLeaf<'data>,
        depth: usize,
    },
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

    let changed = draw_collection_metadata_workspace(ui, document, catalog, &snapshot, state);

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
        if ui.button("Expand all").clicked() {
            expansion_action = Some(true);
        }
        if ui.button("Collapse all").clicked() {
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

    let index_width = 70.0;
    let hash_width = 104.0;
    let type_width = (ui.available_width() * 0.2).clamp(100.0, 190.0);
    let state_width = (ui.available_width() * 0.27).clamp(130.0, 280.0);
    let item_width = (ui.available_width()
        - index_width
        - hash_width
        - type_width
        - state_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(170.0);
    ui.add_space(4.0);
    draw_header(
        ui,
        &[
            (item_width, "Collectible"),
            (type_width, "Type"),
            (state_width, "Status"),
            (index_width, "Index"),
            (hash_width, "Hash"),
        ],
        &mut state.sort,
    );
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
    let lines = display_lines(&hierarchy, state, auto_expand);
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
                    .num_columns(5)
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
                                    table_cell(ui, hash_width, "");
                                }
                                DisplayLine::Leaf { leaf, depth } => {
                                    let accessible_name = if leaf.definition.name.trim().is_empty() {
                                        format!("0x{:08X}", leaf.definition.hash)
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
                                    collection_hash_cell(ui, hash_width, leaf.definition.hash);
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    if let Some(hash) = take_hash_inspection_request(ui.ctx()) {
        state.hash_inspection.open(hash);
    }
    draw_catalog_hash_window(
        ui.ctx(),
        catalog,
        Some(document),
        &mut state.hash_inspection,
        "collections",
    );
    changed
}

fn collection_name(definition: &CollectibleDef) -> egui::RichText {
    if definition.name.trim().is_empty() {
        egui::RichText::new(format!("0x{:08X}", definition.hash)).monospace()
    } else {
        egui::RichText::new(&definition.name)
    }
}

fn build_hierarchy<'a>(leaves: &[CollectionLeaf<'a>]) -> CollectionHierarchy<'a> {
    let mut hierarchy = CollectionHierarchy::default();
    for leaf in leaves {
        let paths = collection_paths(&leaf.definition.paths);
        if paths.is_empty() {
            hierarchy.leaves.push(leaf.clone());
            continue;
        }
        for path in paths {
            insert_leaf(&mut hierarchy.branches, &path, leaf.clone());
        }
    }
    hierarchy
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

fn acquisition_counts(leaves: &[CollectionLeaf<'_>]) -> AcquisitionCounts {
    let mut counts = AcquisitionCounts::default();
    for leaf in leaves {
        counts.add(leaf.status.state);
    }
    counts
}

fn branch_counts(branch: &CollectionBranch<'_>) -> AcquisitionCounts {
    let mut counts = acquisition_counts(&branch.leaves);
    for child in &branch.branches {
        let child = branch_counts(child);
        counts.acquired += child.acquired;
        counts.missing += child.missing;
        counts.no_rule += child.no_rule;
        counts.unknown += child.unknown;
    }
    counts
}

fn set_all_expansion(
    hierarchy: &CollectionHierarchy<'_>,
    expansion: &mut HashMap<Vec<String>, bool>,
    expanded: bool,
) {
    fn visit(
        branch: &CollectionBranch<'_>,
        expansion: &mut HashMap<Vec<String>, bool>,
        expanded: bool,
    ) {
        expansion.insert(branch.path.clone(), expanded);
        for child in &branch.branches {
            visit(child, expansion, expanded);
        }
    }
    for branch in &hierarchy.branches {
        visit(branch, expansion, expanded);
    }
}

fn root_first_path(raw_path: &[String]) -> Vec<String> {
    raw_path
        .iter()
        .rev()
        .filter_map(|component| {
            let component = component.trim();
            (!component.is_empty()).then(|| component.to_owned())
        })
        .collect()
}

fn collection_paths(raw_paths: &[Vec<String>]) -> Vec<Vec<String>> {
    raw_paths
        .iter()
        .map(|path| root_first_path(path))
        .filter(|path| !path.is_empty())
        .fold(Vec::<Vec<String>>::new(), |mut paths, path| {
            if !paths.contains(&path) {
                paths.push(path);
            }
            paths
        })
}

fn insert_leaf<'a>(
    branches: &mut Vec<CollectionBranch<'a>>,
    path: &[String],
    leaf: CollectionLeaf<'a>,
) {
    fn insert_at<'a>(
        branches: &mut Vec<CollectionBranch<'a>>,
        path: &[String],
        depth: usize,
        leaf: CollectionLeaf<'a>,
    ) {
        let label = &path[depth];
        let index = branches
            .iter()
            .position(|branch| branch.label == *label)
            .unwrap_or_else(|| {
                branches.push(CollectionBranch {
                    label: label.clone(),
                    path: path[..=depth].to_vec(),
                    branches: Vec::new(),
                    leaves: Vec::new(),
                });
                branches.len() - 1
            });
        if depth + 1 == path.len() {
            branches[index].leaves.push(leaf);
        } else {
            insert_at(&mut branches[index].branches, path, depth + 1, leaf);
        }
    }
    insert_at(branches, path, 0, leaf);
}

fn state_lines(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<String> {
    let mut lines = Vec::new();
    for condition in &definition.conditions {
        for token in &condition.tokens {
            let (label, state) = match token.kind {
                FLAG_INSTRUCTION => {
                    let (scope, state) = flag_reference(token, snapshot, catalog);
                    (
                        format!("{} · {scope}", condition_token_metadata(token, catalog)),
                        state,
                    )
                }
                VALUE_INSTRUCTION => {
                    let (scope, state) = value_reference(token, snapshot, catalog);
                    (
                        format!("{} · {scope}", condition_token_metadata(token, catalog)),
                        state,
                    )
                }
                OBJECTIVE_INSTRUCTION => objective_reference(token, snapshot, catalog),
                _ => continue,
            };
            let text = format!(
                "{} - {label}: {state}",
                condition_field_label(condition.field)
            );
            if !lines.contains(&text) {
                lines.push(text);
            }
        }
    }
    lines
}

fn condition_field_label(field: u8) -> String {
    if field == ACQUISITION_CONDITION_FIELD {
        "Acquisition (field 3)".into()
    } else {
        format!("Field {field}")
    }
}

fn condition_metadata_lines(definition: &CollectibleDef) -> Vec<String> {
    definition
        .conditions
        .iter()
        .map(|condition| {
            format!(
                "{}: {}",
                condition_field_label(condition.field),
                condition_program(condition)
            )
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpressionValue {
    Unknown,
    Boolean(bool),
    Number(i32),
}

impl ExpressionValue {
    fn truthy(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Boolean(value) => Some(value),
            Self::Number(value) => Some(value != 0),
        }
    }

    fn number(self) -> Option<i32> {
        match self {
            Self::Unknown => None,
            Self::Boolean(value) => Some(i32::from(value)),
            Self::Number(value) => Some(value),
        }
    }
}

fn acquisition_status(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> StateLine {
    let conditions = definition
        .conditions
        .iter()
        .filter(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
        .collect::<Vec<_>>();
    let program = conditions
        .first()
        .map(|condition| condition_program(condition))
        .unwrap_or_default();
    let value = (conditions.len() == 1)
        .then(|| evaluate_expression(&conditions[0].tokens, snapshot, catalog))
        .flatten();
    match value {
        Some(true) => StateLine {
            text: "Acquired".into(),
            tooltip: format!("Acquisition condition: true\nProgram: {program}"),
            state: AcquisitionState::Acquired,
        },
        Some(false) => StateLine {
            text: "Missing".into(),
            tooltip: format!("Acquisition condition: false\nProgram: {program}"),
            state: AcquisitionState::Missing,
        },
        None if program.is_empty() => StateLine {
            text: "No condition program".into(),
            tooltip: "No acquisition condition".into(),
            state: AcquisitionState::NoRule,
        },
        None => {
            let mut unsupported = conditions
                .iter()
                .flat_map(|condition| condition.tokens.iter())
                .filter_map(|token| {
                    (!(matches!(token.kind, 1 | 2 | 3 | 4 | 8 | 9 | 10 | 11 | 12 | 13 | 14)
                        || token.kind == LEGACY_LITERAL_ENCODING_INSTRUCTION && token.operand == 0))
                        .then_some(token.kind)
                })
                .collect::<Vec<_>>();
            unsupported.sort_unstable();
            unsupported.dedup();
            let unavailable = conditions.first().map_or_else(Vec::new, |condition| {
                unavailable_acquisition_references(condition, snapshot, catalog)
            });
            let (text, reason) = if !unsupported.is_empty() {
                (
                    "Unsupported package operation".to_owned(),
                    format!(
                        "Unsupported package operation(s): {}",
                        unsupported
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            } else if !unavailable.is_empty() {
                (
                    "State unavailable".to_owned(),
                    format!(
                        "Referenced Sunrise state not present:\n{}",
                        unavailable.join("\n")
                    ),
                )
            } else {
                (
                    "Invalid condition program".to_owned(),
                    "Package condition program does not produce one result".to_owned(),
                )
            };
            StateLine {
                text,
                tooltip: format!("{reason}\nProgram: {program}"),
                state: AcquisitionState::Unknown,
            }
        }
    }
}

fn unavailable_acquisition_references(
    condition: &CollectionConditionDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Vec<String> {
    let mut unavailable = Vec::new();
    for token in &condition.tokens {
        let missing = match token.kind {
            FLAG_INSTRUCTION => catalog
                .unlock_flag_definition(token.operand as usize)
                .is_none_or(|definition| {
                    snapshot
                        .flag_value(token.operand as usize, definition)
                        .is_none()
                }),
            VALUE_INSTRUCTION => catalog
                .unlock_value_definition(token.operand as usize)
                .is_none_or(|definition| {
                    snapshot.value(token.operand as usize, definition).is_none()
                }),
            OBJECTIVE_INSTRUCTION => {
                objective_completion(token.operand as usize, snapshot, catalog).is_none()
            }
            _ => false,
        };
        if missing {
            let label = match token.kind {
                FLAG_INSTRUCTION => format!("Flag #{}", token.operand),
                VALUE_INSTRUCTION => format!("Value #{}", token.operand),
                OBJECTIVE_INSTRUCTION => format!("Objective #{}", token.operand),
                _ => continue,
            };
            if !unavailable.contains(&label) {
                unavailable.push(label);
            }
        }
    }
    unavailable
}

fn evaluate_expression(
    tokens: &[CollectionConditionTokenDef],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    evaluate_expression_with(
        tokens,
        |index| {
            let definition = catalog.unlock_flag_definition(index)?;
            snapshot.flag_value(index, definition)
        },
        |index| {
            let definition = catalog.unlock_value_definition(index)?;
            snapshot.value(index, definition)
        },
        |index| objective_completion(index, snapshot, catalog),
    )
}

fn evaluate_expression_with(
    tokens: &[CollectionConditionTokenDef],
    mut flag: impl FnMut(usize) -> Option<bool>,
    mut value: impl FnMut(usize) -> Option<i32>,
    mut objective: impl FnMut(usize) -> Option<bool>,
) -> Option<bool> {
    let mut stack = Vec::new();
    for token in tokens {
        match token.kind {
            FLAG_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(flag(index).map_or(ExpressionValue::Unknown, ExpressionValue::Boolean));
            }
            NOT_INSTRUCTION => {
                let value = stack.pop()?;
                stack.push(value.truthy().map_or(ExpressionValue::Unknown, |value| {
                    ExpressionValue::Boolean(!value)
                }));
            }
            OR_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left.truthy(), right.truthy()) {
                    (Some(true), _) | (_, Some(true)) => ExpressionValue::Boolean(true),
                    (Some(false), Some(false)) => ExpressionValue::Boolean(false),
                    _ => ExpressionValue::Unknown,
                });
            }
            AND_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left.truthy(), right.truthy()) {
                    (Some(false), _) | (_, Some(false)) => ExpressionValue::Boolean(false),
                    (Some(true), Some(true)) => ExpressionValue::Boolean(true),
                    _ => ExpressionValue::Unknown,
                });
            }
            EQUAL_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left.number(), right.number()) {
                    (Some(left), Some(right)) => ExpressionValue::Boolean(left == right),
                    _ => ExpressionValue::Unknown,
                });
            }
            NOT_EQUAL_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left.number(), right.number()) {
                    (Some(left), Some(right)) => ExpressionValue::Boolean(left != right),
                    _ => ExpressionValue::Unknown,
                });
            }
            VALUE_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(value(index).map_or(ExpressionValue::Unknown, ExpressionValue::Number));
            }
            LITERAL_INSTRUCTION => stack.push(ExpressionValue::Number(token.operand as i32)),
            OBJECTIVE_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(
                    objective(index).map_or(ExpressionValue::Unknown, ExpressionValue::Boolean),
                );
            }
            GREATER_THAN_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left.number(), right.number()) {
                    (Some(left), Some(right)) => ExpressionValue::Boolean(left > right),
                    _ => ExpressionValue::Unknown,
                });
            }
            GREATER_OR_EQUAL_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(match (left.number(), right.number()) {
                    (Some(left), Some(right)) => ExpressionValue::Boolean(left >= right),
                    _ => ExpressionValue::Unknown,
                });
            }
            LEGACY_LITERAL_ENCODING_INSTRUCTION if token.operand == 0 => {
                // Pre-Beyond Light package programs use this immediately after a literal.
                // Encoding mode zero preserves the literal's numeric value.
                let value = stack.pop()?;
                stack.push(value);
            }
            _ => return None,
        }
    }
    (stack.len() == 1).then(|| stack[0].truthy()).flatten()
}

fn objective_completion(
    index: usize,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    let objective = catalog.objective_definition(index)?;
    let definition_index = usize::from(objective.related_unlock_value_definition_index?);
    let definition = catalog.unlock_value_definition(definition_index)?;
    let current = snapshot.value(definition_index, definition)?;
    Some(if objective.is_counting_downward {
        current <= objective.completion_value
    } else {
        current >= objective.completion_value
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollectionStateEdit {
    Flag { definition_index: usize, set: bool },
    Value { definition_index: usize, value: i32 },
}

fn draw_collection_acquisition_action(
    ui: &mut egui::Ui,
    document: &mut Value,
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let current = acquisition_status(definition, snapshot, catalog).state;
    let desired = match current {
        AcquisitionState::Acquired => false,
        AcquisitionState::Missing => true,
        AcquisitionState::NoRule | AcquisitionState::Unknown => return false,
    };
    let edits = collection_state_edits(definition, snapshot, catalog, desired);
    let label = if desired {
        "Set acquired"
    } else {
        "Set missing"
    };
    let response = ui
        .add_enabled(edits.is_some(), egui::Button::new(label))
        .on_hover_text(if edits.is_some() {
            "Update the referenced Sunrise state and verify the acquisition condition"
        } else {
            "No reversible Sunrise state edit can produce this acquisition state"
        });
    let mut changed = false;
    if response.clicked() {
        let result = edits.map_or_else(
            || Err("No reversible Sunrise state edit is available".to_owned()),
            |edits| apply_collection_state_edits(document, definition, catalog, desired, &edits),
        );
        match result {
            Ok(()) => {
                let result = if desired { "Acquired" } else { "Missing" };
                state.mutation_feedback =
                    Some((false, format!("Acquisition state set to {result}")));
                changed = true;
            }
            Err(error) => state.mutation_feedback = Some((true, error)),
        }
    }
    if let Some((error, message)) = &state.mutation_feedback {
        if *error {
            ui.colored_label(ui.visuals().error_fg_color, message);
        } else {
            ui.label(egui::RichText::new(message).weak());
        }
    }
    changed
}

fn collection_state_edits(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> Option<Vec<CollectionStateEdit>> {
    let condition = definition
        .conditions
        .iter()
        .find(|condition| condition.field == ACQUISITION_CONDITION_FIELD)?;
    if definition
        .conditions
        .iter()
        .filter(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
        .count()
        != 1
    {
        return None;
    }

    let mut references = Vec::<(bool, usize)>::new();
    for token in &condition.tokens {
        let reference = match token.kind {
            FLAG_INSTRUCTION => Some((true, token.operand as usize)),
            VALUE_INSTRUCTION => Some((false, token.operand as usize)),
            OBJECTIVE_INSTRUCTION => catalog
                .objective_definition(token.operand as usize)
                .and_then(|objective| objective.related_unlock_value_definition_index)
                .map(|index| (false, usize::from(index))),
            _ => None,
        };
        if let Some(reference) = reference
            && !references.contains(&reference)
        {
            references.push(reference);
        }
    }
    if references.is_empty() || references.len() > 4 {
        return None;
    }

    let mut value_candidates = vec![0, 1];
    for token in &condition.tokens {
        if token.kind == LITERAL_INSTRUCTION {
            let literal = token.operand as i32;
            value_candidates.extend([
                literal,
                literal.saturating_sub(1),
                literal.saturating_add(1),
            ]);
        }
        if token.kind == OBJECTIVE_INSTRUCTION
            && let Some(objective) = catalog.objective_definition(token.operand as usize)
        {
            value_candidates.extend([
                objective.completion_value,
                objective.completion_value.saturating_sub(1),
                objective.completion_value.saturating_add(1),
            ]);
        }
    }
    value_candidates.sort_unstable();
    value_candidates.dedup();

    let mut options = Vec::<Vec<CollectionStateEdit>>::new();
    for (flag, definition_index) in references {
        if flag {
            let definition = catalog.unlock_flag_definition(definition_index)?;
            if definition.compact_slot.is_some() && !matches!(definition.bank(), 1 | 2 | 3 | 6) {
                return None;
            }
            options.push(
                [false, true]
                    .into_iter()
                    .map(|set| CollectionStateEdit::Flag {
                        definition_index,
                        set,
                    })
                    .collect(),
            );
        } else {
            let definition = catalog.unlock_value_definition(definition_index)?;
            if definition.compact_slot.is_some() && !matches!(definition.bank(), 1 | 2) {
                return None;
            }
            let mut candidates = value_candidates.clone();
            if let Some(current) = snapshot.value(definition_index, definition) {
                candidates.push(current);
            }
            candidates.sort_unstable();
            candidates.dedup();
            options.push(
                candidates
                    .into_iter()
                    .map(|value| CollectionStateEdit::Value {
                        definition_index,
                        value,
                    })
                    .collect(),
            );
        }
    }
    if options
        .iter()
        .map(Vec::len)
        .try_fold(1_usize, usize::checked_mul)
        .is_none_or(|count| count > 256)
    {
        return None;
    }

    let mut best = None::<Vec<CollectionStateEdit>>;
    enumerate_collection_edits(&options, 0, &mut Vec::new(), &mut |candidate| {
        let result = evaluate_expression_with(
            &condition.tokens,
            |index| {
                candidate
                    .iter()
                    .find_map(|edit| match edit {
                        CollectionStateEdit::Flag {
                            definition_index,
                            set,
                        } if *definition_index == index => Some(*set),
                        _ => None,
                    })
                    .or_else(|| {
                        let definition = catalog.unlock_flag_definition(index)?;
                        snapshot.flag_value(index, definition)
                    })
            },
            |index| {
                candidate
                    .iter()
                    .find_map(|edit| match edit {
                        CollectionStateEdit::Value {
                            definition_index,
                            value,
                        } if *definition_index == index => Some(*value),
                        _ => None,
                    })
                    .or_else(|| {
                        let definition = catalog.unlock_value_definition(index)?;
                        snapshot.value(index, definition)
                    })
            },
            |index| objective_completion_with_edits(index, candidate, snapshot, catalog),
        );
        if result != Some(desired) {
            return;
        }
        let changed = candidate
            .iter()
            .copied()
            .filter(|edit| collection_edit_changes_state(*edit, snapshot, catalog))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return;
        }
        if best
            .as_ref()
            .is_none_or(|current| changed.len() < current.len())
        {
            best = Some(changed);
        }
    });
    best
}

fn enumerate_collection_edits(
    options: &[Vec<CollectionStateEdit>],
    index: usize,
    current: &mut Vec<CollectionStateEdit>,
    visit: &mut impl FnMut(&[CollectionStateEdit]),
) {
    if index == options.len() {
        visit(current);
        return;
    }
    for edit in &options[index] {
        current.push(*edit);
        enumerate_collection_edits(options, index + 1, current, visit);
        current.pop();
    }
}

fn objective_completion_with_edits(
    index: usize,
    edits: &[CollectionStateEdit],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    let objective = catalog.objective_definition(index)?;
    let definition_index = usize::from(objective.related_unlock_value_definition_index?);
    let definition = catalog.unlock_value_definition(definition_index)?;
    let current = edits
        .iter()
        .find_map(|edit| match edit {
            CollectionStateEdit::Value {
                definition_index: edit_index,
                value,
            } if *edit_index == definition_index => Some(*value),
            _ => None,
        })
        .or_else(|| snapshot.value(definition_index, definition))?;
    Some(if objective.is_counting_downward {
        current <= objective.completion_value
    } else {
        current >= objective.completion_value
    })
}

fn collection_edit_changes_state(
    edit: CollectionStateEdit,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> bool {
    match edit {
        CollectionStateEdit::Flag {
            definition_index,
            set,
        } => {
            catalog
                .unlock_flag_definition(definition_index)
                .and_then(|definition| snapshot.flag_value(definition_index, definition))
                != Some(set)
        }
        CollectionStateEdit::Value {
            definition_index,
            value,
        } => {
            catalog
                .unlock_value_definition(definition_index)
                .and_then(|definition| snapshot.value(definition_index, definition))
                != Some(value)
        }
    }
}

fn apply_collection_state_edits(
    document: &mut Value,
    definition: &CollectibleDef,
    catalog: &Catalog,
    desired: bool,
    edits: &[CollectionStateEdit],
) -> Result<(), String> {
    let mut candidate = document.clone();
    for edit in edits {
        let applied = match *edit {
            CollectionStateEdit::Flag {
                definition_index,
                set,
            } => {
                let definition = catalog
                    .unlock_flag_definition(definition_index)
                    .ok_or_else(|| format!("Flag definition #{definition_index} is unavailable"))?;
                set_collection_flag(&mut candidate, definition_index, definition, set)
            }
            CollectionStateEdit::Value {
                definition_index,
                value,
            } => {
                let definition = catalog
                    .unlock_value_definition(definition_index)
                    .ok_or_else(|| {
                        format!("Value definition #{definition_index} is unavailable")
                    })?;
                set_collection_value(&mut candidate, definition_index, definition, value)
            }
        };
        if !applied {
            return Err("The referenced Sunrise state could not be updated".into());
        }
    }
    let snapshot = collection_state_snapshot(&candidate)
        .ok_or_else(|| "The updated progression settings are invalid".to_owned())?;
    if acquisition_status(definition, &snapshot, catalog).state
        != if desired {
            AcquisitionState::Acquired
        } else {
            AcquisitionState::Missing
        }
    {
        return Err("The acquisition condition did not reach the requested state".into());
    }
    *document = candidate;
    Ok(())
}

fn flag_reference(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let index = token.operand as usize;
    let Some(definition) = catalog.unlock_flag_definition(index) else {
        return (format!("Flag #{index}"), "Unavailable".into());
    };
    (
        definition_state_label("flag", index, definition),
        collection_flag_state_text(snapshot, index, definition),
    )
}

fn value_reference(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let index = token.operand as usize;
    let Some(definition) = catalog.unlock_value_definition(index) else {
        return (format!("Value #{index}"), "Unavailable".into());
    };
    (
        definition_state_label("value", index, definition),
        collection_value_state_text(snapshot, index, definition),
    )
}

fn objective_reference(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> (String, String) {
    let index = token.operand as usize;
    let Some(objective) = catalog.objective_definition(index) else {
        return (format!("Objective #{index}"), "Unavailable".into());
    };
    let label = objective_display_name(objective).map_or_else(
        || format!("Objective #{index} · 0x{:08X}", objective.hash),
        |name| format!("Objective #{index} · {name} · 0x{:08X}", objective.hash),
    );
    let state = objective_completion(index, snapshot, catalog).map_or_else(
        || "State not present in Sunrise settings".into(),
        |completed| {
            if completed {
                "Complete".into()
            } else {
                "Incomplete".into()
            }
        },
    );
    (label, state)
}

fn objective_display_name(objective: &crate::catalog::ObjectiveDef) -> Option<&str> {
    [
        objective.name.as_str(),
        objective.progress_description.as_str(),
        objective.display_description.as_str(),
        objective.description.as_str(),
    ]
    .into_iter()
    .find(|text| !text.trim().is_empty())
}

fn definition_state_label(kind: &str, index: usize, definition: &UnlockDefinition) -> String {
    let Some(slot) = definition.compact_slot else {
        return format!("{kind} #{index}");
    };
    let scope = match (kind, definition.bank()) {
        ("flag", 1) => "Account flag",
        ("flag", 2) => "Profile flag",
        ("flag", 3) => "Selected-character flag",
        ("flag", 6) => "Per-character flag",
        ("value", 1) => "Account value",
        ("value", 2) => "Selected-character value",
        _ => return format!("{kind} #{index}"),
    };
    format!("{scope} {slot}")
}

fn condition_program(condition: &CollectionConditionDef) -> String {
    condition
        .tokens
        .iter()
        .map(|token| format!("{}:{}", token.kind, token.operand))
        .collect::<Vec<_>>()
        .join(" ")
}

fn draw_collection_metadata_workspace(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
    state: &mut UiState,
) -> bool {
    if state.metadata_index.is_none() {
        return false;
    }
    if !state.hash_inspection.is_open()
        && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
    {
        state.metadata_index = None;
        return false;
    }

    let mut changed = false;
    if ui.available_width() >= 980.0 {
        egui::SidePanel::right("collection_inspection_workspace")
            .resizable(true)
            .default_width(540.0)
            .width_range(420.0..=760.0)
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show_inside(ui, |ui| {
                changed |= draw_collection_metadata_panel(ui, document, catalog, snapshot, state);
            });
    } else {
        let maximum_height = (ui.available_height() * 0.7).max(260.0);
        egui::TopBottomPanel::bottom("collection_inspection_workspace_compact")
            .resizable(true)
            .default_height(maximum_height.min(380.0))
            .height_range(240.0..=maximum_height)
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show_inside(ui, |ui| {
                changed |= draw_collection_metadata_panel(ui, document, catalog, snapshot, state);
            });
    }
    changed
}

fn draw_collection_metadata_panel(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
    state: &mut UiState,
) -> bool {
    let Some(index) = state.metadata_index else {
        return false;
    };
    let definition = catalog
        .collectibles()
        .iter()
        .find(|definition| definition.index == index);
    let title = definition
        .filter(|definition| !definition.name.trim().is_empty())
        .map_or_else(
            || format!("Collectible #{index}"),
            |definition| format!("Collectible #{index} · {}", definition.name),
        );
    let close = inspector_heading(ui, title);
    if ui.button("Show selected collectible in table").clicked() {
        state.query.clear();
        state.status_filter = CollectionStatusFilter::All;
        state.reveal_selection = true;
    }
    ui.separator();

    let Some(definition) = definition else {
        ui.label("Collectible definition is no longer available");
        if close {
            state.metadata_index = None;
        }
        return false;
    };
    let mut changed = false;
    egui::ScrollArea::both()
        .id_salt("collection_metadata_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::Grid::new("collection_metadata_fields")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    collection_metadata_field(
                        ui,
                        "Collectible index",
                        definition.index.to_string(),
                    );
                    collection_hash_field(ui, "Collectible hash", definition.hash);
                    collection_metadata_field(
                        ui,
                        "Item definition index",
                        if definition.item_definition_index == u16::MAX {
                            "<unavailable>".into()
                        } else {
                            definition.item_definition_index.to_string()
                        },
                    );
                    collection_hash_field(ui, "Item definition hash", definition.item_hash);
                    collection_metadata_field(
                        ui,
                        "Material requirement set index",
                        definition
                            .material_requirement_set_index
                            .map_or_else(|| "<unavailable>".into(), |index| index.to_string()),
                    );
                    collection_hash_field(
                        ui,
                        "Material requirement set hash",
                        definition.material_requirement_set_hash,
                    );
                    collection_metadata_field(ui, "Name", &definition.name);
                    collection_metadata_field(ui, "Type", &definition.type_name);
                    collection_metadata_field(
                        ui,
                        "Current acquisition state",
                        acquisition_status(definition, snapshot, catalog).text,
                    );
                });
            changed |= draw_collection_acquisition_action(
                ui, document, definition, snapshot, catalog, state,
            );
            if !definition.material_requirements.is_empty() {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(format!(
                    "Material requirements ({})",
                    definition.material_requirements.len()
                ))
                .id_salt(("collection_material_requirements", definition.index))
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(("collection_material_requirement_rows", definition.index))
                        .num_columns(6)
                        .spacing([16.0, 3.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Index");
                            ui.strong("Hash");
                            ui.strong("Quantity");
                            ui.strong("Delete on action");
                            ui.strong("Omit from requirements");
                            ui.strong("Condition");
                            ui.end_row();
                            for requirement in &definition.material_requirements {
                                ui.monospace(requirement.item_definition_index.to_string());
                                collection_hash_cell(ui, 104.0, requirement.item_hash);
                                ui.monospace(requirement.quantity.to_string());
                                ui.label(if requirement.delete_on_action {
                                    "True"
                                } else {
                                    "False"
                                });
                                ui.label(if requirement.omit_from_requirements {
                                    "True"
                                } else {
                                    "False"
                                });
                                ui.monospace(format!("0x{:04X}", requirement.condition));
                                ui.end_row();
                            }
                        });
                });
            }
            for (path_index, path) in definition.paths.iter().enumerate() {
                let path = root_first_path(path);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Package path {} · {}",
                        path_index + 1,
                        path.join(" > ")
                    ))
                    .weak(),
                );
            }
            for condition in &definition.conditions {
                ui.add_space(6.0);
                let result = evaluate_expression(&condition.tokens, snapshot, catalog)
                    .map_or("Unknown", |value| if value { "True" } else { "False" });
                egui::CollapsingHeader::new(format!(
                    "{} · {result}",
                    condition_field_label(condition.field)
                ))
                .id_salt(("collection_condition", condition.field))
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(("collection_condition_tokens", condition.field))
                        .num_columns(5)
                        .spacing([16.0, 3.0])
                        .show(ui, |ui| {
                            ui.strong("#");
                            ui.strong("Operation");
                            ui.strong("Operand");
                            ui.strong("Referenced entry");
                            ui.strong("Current state");
                            ui.end_row();
                            for (token_index, token) in condition.tokens.iter().enumerate() {
                                ui.monospace((token_index + 1).to_string());
                                ui.label(condition_token_label(token.kind));
                                ui.monospace(token.operand.to_string());
                                draw_condition_token_metadata(ui, token, catalog);
                                ui.label(condition_token_state(token, snapshot, catalog));
                                ui.end_row();
                            }
                        });
                    egui::CollapsingHeader::new("Raw package program")
                        .id_salt(("collection_raw_condition", condition.field))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(condition_program(condition)).monospace(),
                                )
                                .wrap(),
                            );
                        });
                });
            }
        });
    if close {
        state.metadata_index = None;
    }
    changed
}

fn collection_hash_text(hash: u64) -> String {
    format!("0x{hash:08X} · {hash}")
}

fn collection_hash_field(ui: &mut egui::Ui, label: &str, hash: u64) {
    ui.label(egui::RichText::new(label).weak());
    if hash == 0 {
        ui.label(egui::RichText::new("<not present>").weak().italics());
        ui.end_row();
        return;
    }
    let canonical = format!("0x{hash:08X}");
    let response = ui
        .add(
            egui::Button::new(egui::RichText::new(collection_hash_text(hash)).monospace())
                .frame(false),
        )
        .on_hover_text(format!("Open details for {canonical}"));
    if response.clicked() {
        request_hash_inspection(ui.ctx(), hash);
    }
    ui.end_row();
}

fn condition_token_state(
    token: &CollectionConditionTokenDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> String {
    match token.kind {
        FLAG_INSTRUCTION => {
            let (scope, state) = flag_reference(token, snapshot, catalog);
            format!("{scope}: {state}")
        }
        VALUE_INSTRUCTION => {
            let (scope, state) = value_reference(token, snapshot, catalog);
            format!("{scope}: {state}")
        }
        OBJECTIVE_INSTRUCTION => objective_reference(token, snapshot, catalog).1,
        _ => "-".into(),
    }
}

fn collection_metadata_field(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(egui::RichText::new(label).weak());
    let value = value.into();
    ui.label(if value.trim().is_empty() {
        egui::RichText::new("<not present>").weak()
    } else {
        egui::RichText::new(value)
    });
    ui.end_row();
}

fn condition_token_label(kind: u32) -> String {
    match kind {
        FLAG_INSTRUCTION => "Flag reference".into(),
        NOT_INSTRUCTION => "Not".into(),
        OR_INSTRUCTION => "Or".into(),
        AND_INSTRUCTION => "And".into(),
        EQUAL_INSTRUCTION => "Equal".into(),
        NOT_EQUAL_INSTRUCTION => "Not equal".into(),
        VALUE_INSTRUCTION => "Value reference".into(),
        LITERAL_INSTRUCTION => "Literal".into(),
        OBJECTIVE_INSTRUCTION => "Objective reference".into(),
        GREATER_THAN_INSTRUCTION => "Greater than".into(),
        GREATER_OR_EQUAL_INSTRUCTION => "Greater than or equal".into(),
        LEGACY_LITERAL_ENCODING_INSTRUCTION => "Literal encoding".into(),
        _ => format!("Opcode {kind}"),
    }
}

fn condition_token_metadata(token: &CollectionConditionTokenDef, catalog: &Catalog) -> String {
    let index = token.operand as usize;
    if token.kind == OBJECTIVE_INSTRUCTION {
        let Some(objective) = catalog.objective_definition(index) else {
            return "Objective unavailable".into();
        };
        return objective_display_name(objective).map_or_else(
            || format!("Objective #{index} · 0x{:08X}", objective.hash),
            |name| format!("{name} · 0x{:08X}", objective.hash),
        );
    }
    let definition = match token.kind {
        FLAG_INSTRUCTION => catalog.unlock_flag_definition(index),
        VALUE_INSTRUCTION => catalog.unlock_value_definition(index),
        _ => return String::new(),
    };
    let Some(definition) = definition else {
        return "Definition unavailable".into();
    };
    definition
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map_or_else(
            || format!("0x{:08X}", definition.hash),
            |name| format!("{name} · 0x{:08X}", definition.hash),
        )
}

fn draw_condition_token_metadata(
    ui: &mut egui::Ui,
    token: &CollectionConditionTokenDef,
    catalog: &Catalog,
) {
    if token.kind == OBJECTIVE_INSTRUCTION {
        let text = condition_token_metadata(token, catalog);
        let Some(objective) = catalog.objective_definition(token.operand as usize) else {
            ui.label(text);
            return;
        };
        let canonical = format!("0x{:08X}", objective.hash);
        let response = ui
            .add(egui::Button::new(egui::RichText::new(text)).frame(false))
            .on_hover_text(format!("Open details for {canonical}"));
        if response.clicked() {
            request_hash_inspection(ui.ctx(), objective.hash);
        }
        return;
    }
    let definition = match token.kind {
        FLAG_INSTRUCTION => catalog.unlock_flag_definition(token.operand as usize),
        VALUE_INSTRUCTION => catalog.unlock_value_definition(token.operand as usize),
        _ => None,
    };
    let text = condition_token_metadata(token, catalog);
    let Some(definition) = definition else {
        ui.label(text);
        return;
    };
    let canonical = format!("0x{:08X}", definition.hash);
    let response = ui
        .add(egui::Button::new(egui::RichText::new(text)).frame(false))
        .on_hover_text(format!("Open details for {canonical}"));
    if response.clicked() {
        request_hash_inspection(ui.ctx(), definition.hash);
    }
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

fn sort_hierarchy(hierarchy: &mut CollectionHierarchy<'_>, sort: TableSort) {
    fn sort_branch(branch: &mut CollectionBranch<'_>, sort: TableSort) {
        branch
            .leaves
            .sort_by(|left, right| compare_leaves(left, right, sort));
        for child in &mut branch.branches {
            sort_branch(child, sort);
        }
    }
    hierarchy
        .leaves
        .sort_by(|left, right| compare_leaves(left, right, sort));
    for branch in &mut hierarchy.branches {
        sort_branch(branch, sort);
    }
}

fn compare_leaves(
    left: &CollectionLeaf<'_>,
    right: &CollectionLeaf<'_>,
    sort: TableSort,
) -> Ordering {
    let ordering = match sort.column {
        0 => left
            .definition
            .name
            .to_lowercase()
            .cmp(&right.definition.name.to_lowercase()),
        1 => left
            .definition
            .type_name
            .to_lowercase()
            .cmp(&right.definition.type_name.to_lowercase()),
        2 => left.status.text.cmp(&right.status.text),
        3 => left.definition.index.cmp(&right.definition.index),
        4 => left.definition.hash.cmp(&right.definition.hash),
        _ => Ordering::Equal,
    };
    if sort.descending {
        ordering.reverse()
    } else {
        ordering
    }
}

fn display_lines<'tree, 'data>(
    hierarchy: &'tree CollectionHierarchy<'data>,
    state: &UiState,
    auto_expand: bool,
) -> Vec<DisplayLine<'tree, 'data>> {
    fn append_leaf<'tree, 'data>(
        output: &mut Vec<DisplayLine<'tree, 'data>>,
        leaf: &'tree CollectionLeaf<'data>,
        depth: usize,
    ) {
        output.push(DisplayLine::Leaf { leaf, depth });
    }
    fn append_branch<'tree, 'data>(
        output: &mut Vec<DisplayLine<'tree, 'data>>,
        branch: &'tree CollectionBranch<'data>,
        state: &UiState,
        auto_expand: bool,
        depth: usize,
    ) {
        let expanded = auto_expand
            || state
                .expansion
                .get(&branch.path)
                .copied()
                .unwrap_or(depth == 0);
        output.push(DisplayLine::Branch {
            branch,
            depth,
            expanded,
        });
        if !expanded {
            return;
        }
        for leaf in &branch.leaves {
            append_leaf(output, leaf, depth + 1);
        }
        for child in &branch.branches {
            append_branch(output, child, state, auto_expand, depth + 1);
        }
    }
    let mut output = Vec::new();
    for branch in &hierarchy.branches {
        append_branch(&mut output, branch, state, auto_expand, 0);
    }
    for leaf in &hierarchy.leaves {
        append_leaf(&mut output, leaf, 0);
    }
    output
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
                    egui::Button::new(egui::RichText::new(format!("0x{hash:08X}")).monospace())
                        .frame(false),
                )
                .on_hover_text(format!("Open details for 0x{hash:08X}"));
            if response.clicked() {
                request_hash_inspection(ui.ctx(), hash);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(kind: u32, operand: u32) -> CollectionConditionTokenDef {
        CollectionConditionTokenDef { kind, operand }
    }

    #[test]
    fn package_paths_render_root_first_without_synthetic_roots() {
        assert_eq!(
            root_first_path(&["Kinetic".into(), "Weapons".into(), "Items".into()]),
            ["Items", "Weapons", "Kinetic"]
        );
    }

    #[test]
    fn collections_preserve_every_package_parent_path() {
        let paths = collection_paths(&[
            vec!["Titan".into(), "Season of the Worthy".into()],
            vec![
                "Season 10".into(),
                "Ships".into(),
                "Equipment".into(),
                "Items".into(),
            ],
            vec!["Season 10".into(), "Ships".into(), "Vehicles".into()],
        ]);
        assert_eq!(
            paths,
            [
                vec![String::from("Season of the Worthy"), String::from("Titan")],
                vec![
                    String::from("Items"),
                    String::from("Equipment"),
                    String::from("Ships"),
                    String::from("Season 10"),
                ],
                vec![
                    String::from("Vehicles"),
                    String::from("Ships"),
                    String::from("Season 10"),
                ],
            ]
        );
    }

    #[test]
    fn collection_progress_counts_and_filters_keep_unknown_state_explicit() {
        let states = [
            AcquisitionState::Acquired,
            AcquisitionState::Acquired,
            AcquisitionState::Missing,
            AcquisitionState::NoRule,
            AcquisitionState::Unknown,
        ];
        let mut counts = AcquisitionCounts::default();
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

    #[test]
    fn collection_metadata_lists_every_package_condition_field_and_raw_opcode() {
        let definition = CollectibleDef {
            index: 1,
            hash: 2,
            item_definition_index: 4,
            item_hash: 3,
            material_requirement_set_index: None,
            material_requirement_set_hash: 0,
            material_requirements: Vec::new(),
            name: "Test".into(),
            type_name: "Record".into(),
            paths: Vec::new(),
            conditions: (0..=3)
                .map(|field| CollectionConditionDef {
                    field,
                    tokens: vec![token(12 + u32::from(field), 40 + u32::from(field))],
                })
                .collect(),
        };

        assert_eq!(
            condition_metadata_lines(&definition),
            [
                "Field 0: 12:40",
                "Field 1: 13:41",
                "Field 2: 14:42",
                "Acquisition (field 3): 15:43",
            ]
        );
    }

    #[test]
    fn acquisition_expression_supports_package_boolean_programs() {
        assert_eq!(
            evaluate_expression_with(
                &[token(1, 4), token(1, 9), token(3, u32::MAX)],
                |index| Some(index == 9),
                |_| None,
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &[token(1, 4), token(2, 0)],
                |_| Some(false),
                |_| None,
                |_| None,
            ),
            Some(true)
        );
    }

    #[test]
    fn acquisition_expression_supports_package_value_comparisons() {
        assert_eq!(
            evaluate_expression_with(
                &[token(10, 465), token(11, 20), token(14, u32::MAX)],
                |_| None,
                |_| Some(20),
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &[token(10, 842), token(11, 10), token(8, u32::MAX)],
                |_| None,
                |_| Some(9),
                |_| None,
            ),
            Some(false)
        );
        assert_eq!(
            evaluate_expression_with(
                &[
                    token(VALUE_INSTRUCTION, 1_613),
                    token(LITERAL_INSTRUCTION, 0),
                    token(GREATER_THAN_INSTRUCTION, u32::MAX),
                ],
                |_| None,
                |_| Some(1),
                |_| None,
            ),
            Some(true)
        );
    }

    #[test]
    fn acquisition_expression_resolves_legacy_quest_literal_encoding() {
        let quest_completed = [
            token(VALUE_INSTRUCTION, 8_029),
            token(LITERAL_INSTRUCTION, 1),
            token(LEGACY_LITERAL_ENCODING_INSTRUCTION, 0),
            token(EQUAL_INSTRUCTION, u32::MAX),
        ];
        assert_eq!(
            evaluate_expression_with(
                &quest_completed,
                |_| None,
                |index| (index == 8_029).then_some(1),
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &quest_completed,
                |_| None,
                |index| (index == 8_029).then_some(0),
                |_| None,
            ),
            Some(false)
        );
    }

    #[test]
    fn acquisition_expression_refuses_unknown_or_malformed_programs() {
        assert_eq!(
            evaluate_expression_with(&[token(99, 1)], |_| None, |_| None, |_| None),
            None
        );
        assert_eq!(
            evaluate_expression_with(&[token(3, u32::MAX)], |_| None, |_| None, |_| None,),
            None
        );
    }

    #[test]
    fn acquisition_expression_resolves_divinity_style_objective_or_flag_programs() {
        let program = [
            token(OBJECTIVE_INSTRUCTION, 6_383),
            token(FLAG_INSTRUCTION, 10_514),
            token(OR_INSTRUCTION, u32::MAX),
        ];
        assert_eq!(
            evaluate_expression_with(
                &program,
                |index| (index == 10_514).then_some(true),
                |_| None,
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(&program, |_| Some(false), |_| None, |_| None),
            None
        );
        assert_eq!(
            evaluate_expression_with(&program, |_| Some(false), |_| None, |_| Some(true)),
            Some(true)
        );
    }
}
