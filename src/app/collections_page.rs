use std::{cmp::Ordering, collections::HashMap};

use eframe::egui;
use serde_json::Value;

use crate::catalog::{
    Catalog, CollectibleDef, CollectionConditionDef, CollectionConditionTokenDef, UnlockDefinition,
};

use super::{
    glyphs::{self, Glyph},
    progression::{
        CollectionStateSnapshot, collection_flag_state_text, collection_state_snapshot,
        collection_value_state_text,
    },
};

const TABLE_CELL_HEIGHT: f32 = 20.0;
const TABLE_ROW_GAP: f32 = 4.0;
const TABLE_COLUMN_GAP: f32 = 12.0;
const HIERARCHY_INDENT: f32 = 14.0;
const ACQUISITION_CONDITION_FIELD: u8 = 3;
const FLAG_INSTRUCTION: u32 = 1;
const NOT_INSTRUCTION: u32 = 2;
const OR_INSTRUCTION: u32 = 3;
const EQUAL_INSTRUCTION: u32 = 8;
const VALUE_INSTRUCTION: u32 = 10;
const LITERAL_INSTRUCTION: u32 = 11;
const GREATER_OR_EQUAL_INSTRUCTION: u32 = 14;

#[derive(Debug, Default)]
pub(super) struct UiState {
    query: String,
    sort: TableSort,
    expansion: HashMap<Vec<String>, bool>,
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
    document: &Value,
    catalog: &Catalog,
    state: &mut UiState,
) {
    ui.add(
        egui::TextEdit::singleline(&mut state.query)
            .hint_text("Filter collections…")
            .desired_width(360.0),
    );
    ui.add_space(6.0);

    if let Some(error) = catalog.progression_package_error() {
        ui.colored_label(ui.visuals().warn_fg_color, "Package data incomplete")
            .on_hover_text(error);
        ui.add_space(4.0);
    }

    let Some(snapshot) = collection_state_snapshot(document) else {
        ui.colored_label(ui.visuals().error_fg_color, "Invalid progression settings");
        return;
    };
    let query = state.query.trim().to_lowercase();
    let definitions = catalog
        .collectibles()
        .iter()
        .filter(|definition| collection_matches(&query, definition, catalog))
        .collect::<Vec<_>>();
    ui.label(format!("{} package collectibles", definitions.len()));

    let index_width = 70.0;
    let type_width = (ui.available_width() * 0.2).clamp(100.0, 190.0);
    let state_width = (ui.available_width() * 0.27).clamp(130.0, 280.0);
    let item_width =
        (ui.available_width() - index_width - type_width - state_width - TABLE_COLUMN_GAP * 3.0)
            .max(170.0);
    ui.add_space(4.0);
    draw_header(
        ui,
        &[
            (item_width, "Collection item"),
            (type_width, "Type"),
            (state_width, "Status"),
            (index_width, "Index"),
        ],
        &mut state.sort,
    );
    ui.separator();
    if definitions.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return;
    }

    let mut hierarchy = build_hierarchy(&definitions, &snapshot, catalog);
    sort_hierarchy(&mut hierarchy, state.sort);
    let auto_expand = !query.is_empty();
    let lines = display_lines(&hierarchy, state, auto_expand);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt("collections_table")
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, lines.len(), |ui, range| {
                egui::Grid::new("collections_rows")
                    .num_columns(4)
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
                                    table_cell(ui, state_width, "");
                                    table_cell(ui, index_width, "");
                                }
                                DisplayLine::Leaf { leaf, depth } => {
                                    draw_leaf_cell(
                                        ui,
                                        item_width,
                                        *depth,
                                        collection_name(leaf.definition),
                                    )
                                    .on_hover_text(format!(
                                        "Collectible: 0x{:08X}\nItem: 0x{:08X}",
                                        leaf.definition.hash, leaf.definition.item_hash
                                    ));
                                    table_cell(
                                        ui,
                                        type_width,
                                        if leaf.definition.type_name.trim().is_empty() {
                                            egui::RichText::new("—").weak()
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
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

fn collection_name(definition: &CollectibleDef) -> egui::RichText {
    if definition.name.trim().is_empty() {
        egui::RichText::new(format!("0x{:08X}", definition.hash)).monospace()
    } else {
        egui::RichText::new(&definition.name)
    }
}

fn build_hierarchy<'a>(
    definitions: &[&'a CollectibleDef],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> CollectionHierarchy<'a> {
    let mut hierarchy = CollectionHierarchy::default();
    for &definition in definitions {
        let references = state_lines(definition, snapshot, catalog);
        let mut status = acquisition_status(definition, snapshot, catalog);
        if !references.is_empty() {
            status.tooltip.push_str("\n\nState\n");
            status.tooltip.push_str(&references.join("\n"));
        }
        let leaf = CollectionLeaf { definition, status };
        let paths = collection_paths(&definition.paths);
        if paths.is_empty() {
            hierarchy.leaves.push(leaf);
            continue;
        }
        for path in paths {
            insert_leaf(&mut hierarchy.branches, &path, leaf.clone());
        }
    }
    hierarchy
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
    for condition in definition
        .conditions
        .iter()
        .filter(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
    {
        for token in &condition.tokens {
            let (label, state) = match token.kind {
                FLAG_INSTRUCTION => flag_reference(token, snapshot, catalog),
                VALUE_INSTRUCTION => value_reference(token, snapshot, catalog),
                _ => continue,
            };
            let text = format!("{label}: {state}");
            if !lines.contains(&text) {
                lines.push(text);
            }
        }
    }
    lines
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpressionValue {
    Boolean(bool),
    Number(i32),
}

impl ExpressionValue {
    fn truthy(self) -> bool {
        match self {
            Self::Boolean(value) => value,
            Self::Number(value) => value != 0,
        }
    }

    fn number(self) -> i32 {
        match self {
            Self::Boolean(value) => i32::from(value),
            Self::Number(value) => value,
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
        },
        Some(false) => StateLine {
            text: "Not acquired".into(),
            tooltip: format!("Acquisition condition: false\nProgram: {program}"),
        },
        None => StateLine {
            text: "Unknown".into(),
            tooltip: if program.is_empty() {
                "No acquisition condition".into()
            } else {
                format!("Acquisition expression not resolved\nProgram: {program}")
            },
        },
    }
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
    )
}

fn evaluate_expression_with(
    tokens: &[CollectionConditionTokenDef],
    mut flag: impl FnMut(usize) -> Option<bool>,
    mut value: impl FnMut(usize) -> Option<i32>,
) -> Option<bool> {
    let mut stack = Vec::new();
    for token in tokens {
        match token.kind {
            FLAG_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(ExpressionValue::Boolean(flag(index)?));
            }
            NOT_INSTRUCTION => {
                let value = stack.pop()?;
                stack.push(ExpressionValue::Boolean(!value.truthy()));
            }
            OR_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(ExpressionValue::Boolean(left.truthy() || right.truthy()));
            }
            EQUAL_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(ExpressionValue::Boolean(left.number() == right.number()));
            }
            VALUE_INSTRUCTION => {
                let index = token.operand as usize;
                stack.push(ExpressionValue::Number(value(index)?));
            }
            LITERAL_INSTRUCTION => stack.push(ExpressionValue::Number(token.operand as i32)),
            GREATER_OR_EQUAL_INSTRUCTION => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(ExpressionValue::Boolean(left.number() >= right.number()));
            }
            _ => return None,
        }
    }
    (stack.len() == 1).then(|| stack[0].truthy())
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
            condition.tokens.iter().any(|token| match token.kind {
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
                _ => false,
            })
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

fn header_cell(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    marker: Option<Glyph>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.spacing_mut().item_spacing.x = 3.0;
            if let Some(marker) = marker {
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(10.0, TABLE_CELL_HEIGHT), egui::Sense::hover());
                glyphs::paint(ui, rect, marker);
            }
            ui.add(egui::Label::new(egui::RichText::new(label).strong()).truncate());
        },
    )
    .response
    .interact(egui::Sense::click())
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text("Sort")
}

fn table_cell(ui: &mut egui::Ui, width: f32, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}

fn draw_branch_cell(
    ui: &mut egui::Ui,
    width: f32,
    depth: usize,
    label: &str,
    expanded: bool,
    interactive: bool,
) -> egui::Response {
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(width, TABLE_CELL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
                ui.add_space(depth as f32 * HIERARCHY_INDENT);
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(10.0, TABLE_CELL_HEIGHT), egui::Sense::hover());
                glyphs::paint(
                    ui,
                    rect,
                    if expanded {
                        Glyph::ChevronDown
                    } else {
                        Glyph::ChevronRight
                    },
                );
                ui.add(egui::Label::new(egui::RichText::new(label).strong()).truncate());
            },
        )
        .response;
    response.interact(if interactive {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    })
}

fn draw_leaf_cell(
    ui: &mut egui::Ui,
    width: f32,
    depth: usize,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add_space((depth as f32 + 1.0) * HIERARCHY_INDENT);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
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
    fn acquisition_expression_supports_package_boolean_programs() {
        assert_eq!(
            evaluate_expression_with(
                &[token(1, 4), token(1, 9), token(3, u32::MAX)],
                |index| Some(index == 9),
                |_| None,
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(&[token(1, 4), token(2, 0)], |_| Some(false), |_| None),
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
            ),
            Some(true)
        );
        assert_eq!(
            evaluate_expression_with(
                &[token(10, 842), token(11, 10), token(8, u32::MAX)],
                |_| None,
                |_| Some(9),
            ),
            Some(false)
        );
    }

    #[test]
    fn acquisition_expression_refuses_unknown_or_malformed_programs() {
        assert_eq!(
            evaluate_expression_with(&[token(12, 1)], |_| None, |_| None),
            None
        );
        assert_eq!(
            evaluate_expression_with(&[token(3, u32::MAX)], |_| None, |_| None),
            None
        );
    }
}
