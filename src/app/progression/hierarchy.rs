use super::state::*;
use super::*;

pub(super) fn progression_display_rows(
    authored: &[ProgressionValue],
    definitions: &[ProgressionDefinition],
    scope: ProgressionScope,
) -> Vec<ProgressionDisplayRow> {
    let authored = authored
        .iter()
        .map(|row| (row.definition_index, row.lanes))
        .collect::<HashMap<_, _>>();
    let mut rows = definitions
        .iter()
        .filter(|definition| definition.scope == scope)
        .map(|definition| {
            let definition_index = usize::from(definition.definition_index);
            ProgressionDisplayRow {
                definition_index,
                lanes: authored.get(&definition_index).copied(),
            }
        })
        .collect::<Vec<_>>();
    rows.extend(
        authored
            .iter()
            .filter(|(definition_index, _)| {
                !definitions.iter().any(|definition| {
                    usize::from(definition.definition_index) == **definition_index
                        && definition.scope == scope
                })
            })
            .map(|(&definition_index, &lanes)| ProgressionDisplayRow {
                definition_index,
                lanes: Some(lanes),
            }),
    );
    rows.sort_by_key(|row| row.definition_index);
    rows
}

pub(super) fn progression_definition_matches(
    query: &str,
    definition: &ProgressionDefinition,
) -> bool {
    definition.definition_index.to_string().contains(query)
        || format!("{:08x}", definition.hash).contains(query)
        || definition.hash.to_string().contains(query)
        || definition.name.to_lowercase().contains(query)
        || definition.description.to_lowercase().contains(query)
        || definition.source.to_lowercase().contains(query)
        || definition.display_units_name.to_lowercase().contains(query)
        || definition.factions.iter().any(|faction| {
            format!("{:08x}", faction.hash).contains(query)
                || faction.hash.to_string().contains(query)
                || faction.name.to_lowercase().contains(query)
                || faction.description.to_lowercase().contains(query)
        })
        || definition.steps.iter().any(|step| {
            step.name.to_lowercase().contains(query) || step.cost.to_string().contains(query)
        })
        || definition.reward_items.iter().any(|reward| {
            format!("{:08x}", reward.item_hash).contains(query)
                || reward.item_hash.to_string().contains(query)
                || reward
                    .rewarded_at_progression_level
                    .to_string()
                    .contains(query)
                || reward.quantity.to_string().contains(query)
        })
        || definition
            .scope_slot
            .is_some_and(|slot| slot.to_string().contains(query))
}

pub(in crate::app) fn progression_display_name(
    definition: &ProgressionDefinition,
) -> Option<String> {
    let name = definition.name.trim();
    if !name.is_empty() {
        return Some(name.to_owned());
    }
    let mut faction_names = definition
        .factions
        .iter()
        .map(|faction| faction.name.trim())
        .filter(|name| !name.is_empty());
    let faction_name = faction_names.next()?;
    faction_names
        .all(|name| name == faction_name)
        .then(|| format!("Faction: {faction_name}"))
}

#[derive(Clone)]
pub(super) struct ContextDisplayLine<'a> {
    pub(super) name: String,
    pub(super) path: Vec<String>,
    pub(super) contexts: Vec<&'a ProgressionContextDef>,
}

impl ContextDisplayLine<'_> {
    pub(super) fn text(&self) -> String {
        match (self.name.is_empty(), self.path.is_empty()) {
            (false, false) => format!("{}: {}", self.path.join(" > "), self.name),
            (false, true) => self.name.clone(),
            (true, false) => self.path.join(" > "),
            (true, true) => "-".into(),
        }
    }
}

pub(super) struct DefinitionContextDisplayLine<'a> {
    pub(super) row_index: usize,
    pub(super) definition_index: Option<usize>,
    pub(super) definition: Option<&'a UnlockDefinition>,
    pub(super) context: Option<ContextDisplayLine<'a>>,
    pub(super) primary: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ObjectiveHierarchyLeaf<'a> {
    pub(super) row: &'a IndexedValue,
    pub(super) definition_index: Option<usize>,
    pub(super) definition: Option<&'a UnlockDefinition>,
    pub(super) objective_index: Option<usize>,
    pub(super) objective: Option<&'a ObjectiveDef>,
}

#[derive(Default)]
pub(super) struct ObjectiveHierarchy<'a> {
    pub(super) branches: Vec<ObjectiveHierarchyBranch<'a>>,
    pub(super) leaves: Vec<ObjectiveHierarchyLeaf<'a>>,
}

pub(super) struct ObjectiveHierarchyBranch<'a> {
    pub(super) label: String,
    pub(super) path: Vec<String>,
    pub(super) children: Vec<ObjectiveHierarchyBranch<'a>>,
    pub(super) leaves: Vec<ObjectiveHierarchyLeaf<'a>>,
}

impl ObjectiveHierarchyBranch<'_> {
    pub(super) fn new(label: String, path: Vec<String>) -> Self {
        Self {
            label,
            path,
            children: Vec::new(),
            leaves: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ObjectiveMatrixLine<'tree, 'data> {
    Branch {
        branch: &'tree ObjectiveHierarchyBranch<'data>,
        depth: usize,
        expanded: bool,
    },
    Leaf {
        leaf: &'tree ObjectiveHierarchyLeaf<'data>,
        depth: usize,
    },
}

pub(super) fn draw_context_cell(
    ui: &mut egui::Ui,
    width: f32,
    context: Option<&ContextDisplayLine<'_>>,
) {
    let Some(context) = context else {
        table_cell(ui, width, egui::RichText::new("-").weak());
        return;
    };
    let text = context.text();
    let rich_text = if text == "-" {
        egui::RichText::new(&text).weak()
    } else {
        egui::RichText::new(&text)
    };
    table_cell(ui, width, rich_text).on_hover_text(context_relation_tooltip(context));
}

pub(super) fn definition_context_display_lines<'a>(
    row_count: usize,
    mut definition_for_row: impl FnMut(usize) -> Option<(usize, &'a UnlockDefinition)>,
    reverse_contexts: bool,
) -> Vec<DefinitionContextDisplayLine<'a>> {
    let mut display_lines = Vec::new();
    for row_index in 0..row_count {
        let Some((definition_index, definition)) = definition_for_row(row_index) else {
            display_lines.push(DefinitionContextDisplayLine {
                row_index,
                definition_index: None,
                definition: None,
                context: None,
                primary: true,
            });
            continue;
        };
        let mut contexts = definition_context_lines(definition);
        if reverse_contexts {
            contexts.reverse();
        }
        if contexts.is_empty() {
            display_lines.push(DefinitionContextDisplayLine {
                row_index,
                definition_index: Some(definition_index),
                definition: Some(definition),
                context: None,
                primary: true,
            });
            continue;
        }
        display_lines.extend(
            contexts
                .into_iter()
                .enumerate()
                .map(|(context_index, context)| DefinitionContextDisplayLine {
                    row_index,
                    definition_index: Some(definition_index),
                    definition: Some(definition),
                    context: Some(context),
                    primary: context_index == 0,
                }),
        );
    }
    display_lines
}

pub(super) fn definition_context_lines(
    definition: &UnlockDefinition,
) -> Vec<ContextDisplayLine<'_>> {
    let mut lines = Vec::<ContextDisplayLine<'_>>::new();
    for context in &definition.tested_by {
        let paths = if context.paths.is_empty() {
            vec![Vec::new()]
        } else {
            let mut paths = Vec::new();
            for raw_path in &context.paths {
                let path = normalize_context_path(raw_path);
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            paths
        };
        let name = if context.name.trim().is_empty() {
            progression_type_label(&context.type_name).to_owned()
        } else {
            context.name.trim().to_owned()
        };
        if name.is_empty() && paths.iter().all(Vec::is_empty) {
            continue;
        }
        for path in paths {
            if let Some(line) = lines
                .iter_mut()
                .find(|line| line.name == name && line.path == path)
            {
                if !line
                    .contexts
                    .iter()
                    .any(|existing| existing.kind == context.kind && existing.hash == context.hash)
                {
                    line.contexts.push(context);
                }
            } else {
                lines.push(ContextDisplayLine {
                    name: name.clone(),
                    path,
                    contexts: vec![context],
                });
            }
        }
    }
    lines.sort_by_cached_key(|line| line.text().to_lowercase());
    lines
}

pub(super) fn context_relation_tooltip(context: &ContextDisplayLine<'_>) -> String {
    let mut lines = vec!["Package condition reference".to_owned()];
    if !context.name.is_empty() {
        lines.push(format!("Name: {}", context.name));
    }
    if !context.path.is_empty() {
        lines.push(format!("Path: {}", context.path.join(" > ")));
    }
    let mut type_names = Vec::new();
    let mut descriptions = Vec::new();
    for reference in &context.contexts {
        lines.push(format!(
            "{}: 0x{:08X}",
            progression_context_kind_label(reference.kind),
            reference.hash
        ));
        let type_name = reference.type_name.trim();
        if !type_name.is_empty() && !type_names.contains(&type_name) {
            type_names.push(type_name);
        }
        let description = reference.description.trim();
        if !description.is_empty() && !descriptions.contains(&description) {
            descriptions.push(description);
        }
    }
    lines.extend(type_names.into_iter().map(|value| format!("Type: {value}")));
    lines.extend(
        descriptions
            .into_iter()
            .map(|value| format!("Description: {value}")),
    );
    lines.join("\n")
}

pub(super) fn canonical_root(component: &str) -> Option<&'static str> {
    CANONICAL_ROOTS
        .into_iter()
        .find(|root| component.trim().eq_ignore_ascii_case(root))
}

pub(super) fn normalize_hierarchy_path(raw_path: &[String], fallback_root: &str) -> Vec<String> {
    let root = raw_path
        .iter()
        .rev()
        .find_map(|component| canonical_root(component))
        .unwrap_or(fallback_root);
    let mut path = raw_path
        .iter()
        .filter_map(|component| {
            let component = component.trim();
            (!component.is_empty() && !component.eq_ignore_ascii_case(root))
                .then(|| component.to_owned())
        })
        .collect::<Vec<_>>();
    path.reverse();
    path.insert(0, root.to_owned());
    path
}

pub(super) fn normalize_context_path(raw_path: &[String]) -> Vec<String> {
    if let Some(root) = raw_path
        .iter()
        .rev()
        .find_map(|component| canonical_root(component))
    {
        normalize_hierarchy_path(raw_path, root)
    } else {
        raw_path
            .iter()
            .filter_map(|component| {
                let component = component.trim();
                (!component.is_empty()).then(|| component.to_owned())
            })
            .collect()
    }
}

pub(super) fn objective_hierarchy_paths(objective: &ObjectiveDef) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for owner in &objective.owners {
        for raw_path in &owner.paths {
            let path = raw_path
                .iter()
                .rev()
                .filter_map(|component| {
                    let component = component.trim();
                    (!component.is_empty()).then(|| component.to_owned())
                })
                .collect::<Vec<_>>();
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

pub(super) fn definition_hierarchy_paths(definition: &UnlockDefinition) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for context in meaningful_definition_contexts(definition) {
        for raw_path in &context.paths {
            let path = normalize_context_path(raw_path);
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

pub(super) fn build_objective_hierarchy<'a>(
    rows: &[&'a IndexedValue],
    bank: u8,
    catalog: &'a Catalog,
) -> ObjectiveHierarchy<'a> {
    let mut hierarchy = ObjectiveHierarchy::default();
    for &row in rows {
        let definition = catalog.unlock_value_for_state(bank, row.index);
        let objective = definition.and_then(|(definition_index, _)| {
            catalog.objective_with_index_for_unlock_value(definition_index)
        });
        let leaf = ObjectiveHierarchyLeaf {
            row,
            definition_index: definition.map(|(definition_index, _)| definition_index),
            definition: definition.map(|(_, definition)| definition),
            objective_index: objective.map(|(objective_index, _)| objective_index),
            objective: objective.map(|(_, objective)| objective),
        };
        let mut paths = objective
            .map(|(_, objective)| objective_hierarchy_paths(objective))
            .unwrap_or_default();
        if paths.is_empty() {
            paths = definition
                .map(|(_, definition)| definition_hierarchy_paths(definition))
                .unwrap_or_default();
        }
        if paths.is_empty() {
            hierarchy.leaves.push(leaf);
        } else {
            for path in paths {
                insert_objective_leaf(&mut hierarchy.branches, &path, leaf);
            }
        }
    }
    hierarchy
}

pub(super) fn insert_objective_leaf<'a>(
    branches: &mut Vec<ObjectiveHierarchyBranch<'a>>,
    path: &[String],
    leaf: ObjectiveHierarchyLeaf<'a>,
) {
    pub(super) fn insert_at<'a>(
        branches: &mut Vec<ObjectiveHierarchyBranch<'a>>,
        path: &[String],
        depth: usize,
        leaf: ObjectiveHierarchyLeaf<'a>,
    ) {
        let label = &path[depth];
        let position = branches
            .iter()
            .position(|branch| branch.label == *label)
            .unwrap_or_else(|| {
                let branch_path = path[..=depth].to_vec();
                branches.push(ObjectiveHierarchyBranch::new(label.clone(), branch_path));
                branches.len() - 1
            });
        let branch = &mut branches[position];
        if depth + 1 == path.len() {
            branch.leaves.push(leaf);
        } else {
            insert_at(&mut branch.children, path, depth + 1, leaf);
        }
    }

    if !path.is_empty() {
        insert_at(branches, path, 0, leaf);
    }
}

pub(super) fn canonical_root_position(label: &str) -> usize {
    CANONICAL_ROOTS
        .iter()
        .position(|root| label.eq_ignore_ascii_case(root))
        .unwrap_or(CANONICAL_ROOTS.len())
}

pub(super) fn sort_objective_hierarchy(hierarchy: &mut ObjectiveHierarchy<'_>, sort: TableSort) {
    pub(super) fn sort_branch(branch: &mut ObjectiveHierarchyBranch<'_>, sort: TableSort) {
        branch
            .children
            .sort_by_cached_key(|child| child.label.to_lowercase());
        sort_objective_leaves(&mut branch.leaves, sort);
        for child in &mut branch.children {
            sort_branch(child, sort);
        }
    }

    sort_objective_leaves(&mut hierarchy.leaves, sort);
    hierarchy.branches.sort_by_cached_key(|branch| {
        (
            canonical_root_position(&branch.label),
            branch.label.to_lowercase(),
        )
    });
    for branch in &mut hierarchy.branches {
        sort_branch(branch, sort);
    }
}

pub(super) fn objective_matrix_lines<'tree, 'data>(
    hierarchy: &'tree ObjectiveHierarchy<'data>,
    table: &'static str,
    state: &UiState,
    auto_expand: bool,
) -> Vec<ObjectiveMatrixLine<'tree, 'data>> {
    pub(super) fn append<'tree, 'data>(
        output: &mut Vec<ObjectiveMatrixLine<'tree, 'data>>,
        branch: &'tree ObjectiveHierarchyBranch<'data>,
        table: &'static str,
        state: &UiState,
        auto_expand: bool,
        depth: usize,
    ) {
        let key = ObjectiveBranchKey {
            table,
            path: branch.path.clone(),
        };
        let expanded = auto_expand
            || state
                .objective_expansion
                .get(&key)
                .copied()
                .unwrap_or(depth == 0);
        output.push(ObjectiveMatrixLine::Branch {
            branch,
            depth,
            expanded,
        });
        if !expanded {
            return;
        }
        output.extend(branch.leaves.iter().map(|leaf| ObjectiveMatrixLine::Leaf {
            leaf,
            depth: depth + 1,
        }));
        for child in &branch.children {
            append(output, child, table, state, auto_expand, depth + 1);
        }
    }

    let mut output = Vec::new();
    for branch in &hierarchy.branches {
        append(&mut output, branch, table, state, auto_expand, 0);
    }
    output.extend(
        hierarchy
            .leaves
            .iter()
            .map(|leaf| ObjectiveMatrixLine::Leaf { leaf, depth: 0 }),
    );
    output
}

pub(super) fn compare_ordering(ordering: Ordering, descending: bool) -> Ordering {
    if descending {
        ordering.reverse()
    } else {
        ordering
    }
}

pub(super) fn compare_optional<T: Ord>(
    left: Option<T>,
    right: Option<T>,
    descending: bool,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_ordering(left.cmp(&right), descending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub(super) fn sort_by_optional_cached_key<T, K: Ord>(
    rows: &mut [T],
    descending: bool,
    mut key: impl FnMut(&T) -> Option<K>,
) {
    if descending {
        rows.sort_by_cached_key(|row| {
            let key = key(row);
            (key.is_none(), Reverse(key))
        });
    } else {
        rows.sort_by_cached_key(|row| {
            let key = key(row);
            (key.is_none(), key)
        });
    }
}

pub(super) fn sort_objective_leaves(rows: &mut [ObjectiveHierarchyLeaf<'_>], sort: TableSort) {
    match sort.column {
        0 => sort_by_optional_cached_key(rows, sort.descending, |leaf| {
            leaf.objective
                .map(|objective| objective_table_text(objective, leaf.definition).to_lowercase())
        }),
        1 => sort_by_optional_cached_key(rows, sort.descending, |leaf| {
            leaf.objective
                .and_then(objective_traits_text)
                .map(|traits| traits.to_lowercase())
        }),
        _ => rows.sort_by(|left, right| compare_objective_leaves(left, right, sort)),
    }
}

pub(super) fn definition_context_sort_key(definition: &UnlockDefinition) -> Option<String> {
    definition_context_lines(definition)
        .into_iter()
        .map(|line| line.text().to_lowercase())
        .next()
}

pub(super) fn compare_flag_slots(
    left: usize,
    right: usize,
    bank: u8,
    catalog: &Catalog,
    sort: TableSort,
) -> Ordering {
    let left_definition = catalog.unlock_flag_for_state(bank, left);
    let right_definition = catalog.unlock_flag_for_state(bank, right);
    match sort.column {
        0 => compare_optional(
            left_definition.map(|(index, _)| index),
            right_definition.map(|(index, _)| index),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|(_, definition)| definition.hash),
            right_definition.map(|(_, definition)| definition.hash),
            sort.descending,
        ),
        2 => compare_optional(
            left_definition.and_then(|(_, definition)| definition_context_sort_key(definition)),
            right_definition.and_then(|(_, definition)| definition_context_sort_key(definition)),
            sort.descending,
        ),
        3 => compare_ordering(left.cmp(&right), sort.descending),
        _ => Ordering::Equal,
    }
}

pub(super) fn compare_flag_overrides(
    left: &FlagOverride,
    right: &FlagOverride,
    catalog: &Catalog,
    sort: TableSort,
) -> Ordering {
    let left_definition = catalog.unlock_flag_definition(left.definition_index);
    let right_definition = catalog.unlock_flag_definition(right.definition_index);
    match sort.column {
        0 => compare_ordering(
            left.definition_index.cmp(&right.definition_index),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|definition| definition.hash),
            right_definition.map(|definition| definition.hash),
            sort.descending,
        ),
        2 => compare_ordering(left.value.cmp(&right.value), sort.descending),
        3 => compare_optional(
            left_definition.map(|definition| override_meaning(definition).to_lowercase()),
            right_definition.map(|definition| override_meaning(definition).to_lowercase()),
            sort.descending,
        ),
        _ => Ordering::Equal,
    }
}

pub(super) fn compare_value_overrides(
    left: &ValueOverride,
    right: &ValueOverride,
    catalog: &Catalog,
    sort: TableSort,
) -> Ordering {
    let left_definition = catalog.unlock_value_definition(left.definition_index);
    let right_definition = catalog.unlock_value_definition(right.definition_index);
    match sort.column {
        0 => compare_ordering(
            left.definition_index.cmp(&right.definition_index),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|definition| definition.hash),
            right_definition.map(|definition| definition.hash),
            sort.descending,
        ),
        2 => compare_ordering(left.value.cmp(&right.value), sort.descending),
        3 => compare_optional(
            left_definition.map(|definition| override_meaning(definition).to_lowercase()),
            right_definition.map(|definition| override_meaning(definition).to_lowercase()),
            sort.descending,
        ),
        _ => Ordering::Equal,
    }
}

pub(super) fn compare_objective_leaves(
    left: &ObjectiveHierarchyLeaf<'_>,
    right: &ObjectiveHierarchyLeaf<'_>,
    sort: TableSort,
) -> Ordering {
    match sort.column {
        0 => compare_optional(
            left.objective
                .map(|objective| objective_table_text(objective, left.definition).to_lowercase()),
            right
                .objective
                .map(|objective| objective_table_text(objective, right.definition).to_lowercase()),
            sort.descending,
        ),
        1 => compare_optional(left.objective_index, right.objective_index, sort.descending),
        2 => compare_optional(
            left.objective.map(|objective| objective.hash),
            right.objective.map(|objective| objective.hash),
            sort.descending,
        ),
        3 => compare_ordering(left.row.value.cmp(&right.row.value), sort.descending),
        4 => compare_ordering(left.row.index.cmp(&right.row.index), sort.descending),
        _ => Ordering::Equal,
    }
}

pub(super) fn resolved_objective_hierarchy_paths(
    catalog: &Catalog,
    objective: &ObjectiveDef,
) -> Vec<Vec<String>> {
    let direct = objective_hierarchy_paths(objective);
    if !direct.is_empty() {
        return direct;
    }
    let mut paths = objective
        .referenced_objective_indices
        .iter()
        .filter_map(|index| catalog.objective_definition(usize::from(*index)))
        .flat_map(objective_hierarchy_paths)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

pub(super) fn flag_slot_matches(query: &str, slot: usize, bank: u8, catalog: &Catalog) -> bool {
    if query.is_empty() || slot.to_string().contains(query) {
        return true;
    }
    catalog
        .unlock_flag_for_state(bank, slot)
        .is_some_and(|(index, definition)| definition_matches(query, index, definition))
}

pub(super) fn objective_value_matches(
    query: &str,
    row: &IndexedValue,
    bank: u8,
    catalog: &Catalog,
) -> bool {
    if query.is_empty()
        || row.index.to_string().contains(query)
        || row.value.to_string().contains(query)
    {
        return true;
    }
    let Some((definition_index, definition)) = catalog.unlock_value_for_state(bank, row.index)
    else {
        return false;
    };
    definition_matches(query, definition_index, definition)
        || catalog
            .objective_for_unlock_value(definition_index)
            .is_some_and(|objective| resolved_objective_matches(catalog, query, objective))
}

pub(super) fn objective_hierarchy_row_matches(
    query: &str,
    row: &IndexedValue,
    bank: u8,
    catalog: &Catalog,
) -> bool {
    if objective_value_matches(query, row, bank, catalog) {
        return true;
    }
    let definition = catalog.unlock_value_for_state(bank, row.index);
    let objective = definition
        .and_then(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index));
    match objective {
        Some(objective) => {
            let mut paths = resolved_objective_hierarchy_paths(catalog, objective);
            if paths.is_empty() {
                paths = definition
                    .map(|(_, definition)| definition_hierarchy_paths(definition))
                    .unwrap_or_default();
            }
            paths.iter().any(|path| {
                path.iter()
                    .any(|component| component.to_lowercase().contains(query))
            })
        }
        None => false,
    }
}

pub(super) fn family5_flag_matches(query: &str, row: &FlagOverride, catalog: &Catalog) -> bool {
    query.is_empty()
        || row.definition_index.to_string().contains(query)
        || row.value.to_string().contains(query)
        || catalog
            .unlock_flag_definition(row.definition_index)
            .is_some_and(|definition| definition_matches(query, row.definition_index, definition))
}

pub(super) fn family5_value_matches(query: &str, row: &ValueOverride, catalog: &Catalog) -> bool {
    query.is_empty()
        || row.definition_index.to_string().contains(query)
        || row.value.to_string().contains(query)
        || catalog
            .unlock_value_definition(row.definition_index)
            .is_some_and(|definition| definition_matches(query, row.definition_index, definition))
        || catalog
            .objective_for_unlock_value(row.definition_index)
            .is_some_and(|objective| resolved_objective_matches(catalog, query, objective))
}

pub(super) fn resolved_objective_matches(
    catalog: &Catalog,
    query: &str,
    objective: &ObjectiveDef,
) -> bool {
    objective_matches(query, objective)
        || objective
            .referenced_objective_indices
            .iter()
            .filter_map(|index| catalog.objective_definition(usize::from(*index)))
            .any(|target| objective_matches(query, target))
}

pub(super) fn objective_matches(query: &str, objective: &ObjectiveDef) -> bool {
    objective.description.to_lowercase().contains(query)
        || objective.name.to_lowercase().contains(query)
        || objective.display_description.to_lowercase().contains(query)
        || objective
            .progress_description
            .to_lowercase()
            .contains(query)
        || formatted_hash_matches(query, objective.hash)
        || objective.completion_value.to_string().contains(query)
        || (objective.maximum_value().is_some() && "maximum max capped".contains(query))
        || (objective.minimum_value().is_some() && "minimum min capped".contains(query))
        || (objective.allow_overcompletion
            && "overcompletion threshold no maximum no minimum".contains(query))
        || objective
            .intrinsic_perk_flag_definition_indices
            .iter()
            .any(|index| index.to_string().contains(query))
        || objective.condition_programs.iter().flatten().any(|token| {
            token[0].to_string().contains(query) || token[1].to_string().contains(query)
        })
        || objective.owners.iter().any(|owner| {
            owner.name.to_lowercase().contains(query)
                || owner.description.to_lowercase().contains(query)
                || objective_owner_type(owner).to_lowercase().contains(query)
                || formatted_hash_matches(query, owner.hash)
                || owner.traits.iter().any(|trait_definition| {
                    trait_definition.name.to_lowercase().contains(query)
                        || trait_definition.description.to_lowercase().contains(query)
                        || formatted_hash_matches(query, trait_definition.hash)
                })
                || owner
                    .paths
                    .iter()
                    .flatten()
                    .any(|part| part.to_lowercase().contains(query))
        })
}

pub(super) fn definition_matches(query: &str, index: usize, definition: &UnlockDefinition) -> bool {
    index.to_string().contains(query)
        || format!("#{index}").contains(query)
        || formatted_hash_matches(query, definition.hash)
        || definition_name(definition).is_some_and(|name| name.to_lowercase().contains(query))
        || definition
            .description
            .as_deref()
            .is_some_and(|description| description.to_lowercase().contains(query))
        || definition
            .tested_by
            .iter()
            .any(|context| progression_context_matches(query, context))
}

pub(super) fn progression_context_matches(query: &str, context: &ProgressionContextDef) -> bool {
    formatted_hash_matches(query, context.hash)
        || context.name.to_lowercase().contains(query)
        || context.type_name.to_lowercase().contains(query)
        || context.description.to_lowercase().contains(query)
        || progression_context_kind_label(context.kind)
            .to_lowercase()
            .contains(query)
        || context
            .paths
            .iter()
            .flatten()
            .any(|component| component.to_lowercase().contains(query))
        || context.condition_programs.iter().flatten().any(|token| {
            token[0].to_string().contains(query) || token[1].to_string().contains(query)
        })
}

pub(super) fn formatted_hash_matches(query: &str, hash: u64) -> bool {
    format!("{hash:08x}").contains(query) || format!("0x{hash:08x}").contains(query)
}
