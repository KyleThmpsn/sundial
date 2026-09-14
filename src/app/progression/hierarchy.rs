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
    let defined_indices: HashSet<_> = rows.iter().map(|row| row.definition_index).collect();
    rows.extend(
        authored
            .iter()
            .filter(|(definition_index, _)| !defined_indices.contains(definition_index))
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

#[cfg(test)]
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

#[cfg(test)]
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
