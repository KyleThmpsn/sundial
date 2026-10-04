//! Objective labels and metadata semantics shared by progression inspectors and tables.

use std::collections::HashSet;

use crate::{
    catalog::{
        Catalog, ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ProgressionContextDef,
        ProgressionContextKind, UnlockDefinition,
    },
    hash::format_hash_hex,
};

pub(in crate::app) fn progression_type_label(type_name: &str) -> &str {
    let type_name = type_name.trim();
    if type_name.eq_ignore_ascii_case("General inventory") {
        ""
    } else {
        type_name
    }
}

pub(in crate::app) fn objective_description(objective: &crate::catalog::ObjectiveDef) -> String {
    if objective.description.trim().is_empty() {
        format_hash_hex(objective.hash)
    } else {
        objective.description.clone()
    }
}

pub(in crate::app) fn preferred_objective_owner(
    objective: &ObjectiveDef,
) -> Option<&ObjectiveOwnerDef> {
    objective
        .owners
        .iter()
        .filter(|owner| objective_owner_label(owner).is_some())
        .min_by_key(|owner| objective_owner_priority(owner.kind))
}

pub(in crate::app) fn objective_owner_label(owner: &ObjectiveOwnerDef) -> Option<&str> {
    let name = owner.name.trim();
    if !name.is_empty() {
        return Some(name);
    }
    let type_name = progression_type_label(&owner.type_name);
    (!type_name.is_empty()).then_some(type_name)
}

pub(in crate::app) fn objective_owner_display_label(owner: &ObjectiveOwnerDef) -> Option<String> {
    let label = objective_owner_label(owner)?;
    Some(label.to_owned())
}

const fn objective_owner_priority(kind: ObjectiveOwnerKind) -> u8 {
    match kind {
        ObjectiveOwnerKind::Milestone => 0,
        ObjectiveOwnerKind::Metric => 1,
        ObjectiveOwnerKind::Record => 2,
        ObjectiveOwnerKind::PresentationNode => 3,
        ObjectiveOwnerKind::InventoryItem => 4,
    }
}

pub(in crate::app) fn objective_owner_type(owner: &ObjectiveOwnerDef) -> &str {
    if !owner.type_name.trim().is_empty() {
        owner.type_name.as_str()
    } else {
        match owner.kind {
            ObjectiveOwnerKind::InventoryItem => "Item",
            ObjectiveOwnerKind::Milestone => "Milestone",
            ObjectiveOwnerKind::Metric => "Metric",
            ObjectiveOwnerKind::Record => "Record",
            ObjectiveOwnerKind::PresentationNode => "Presentation Node",
        }
    }
}

pub(in crate::app) fn objective_goal_text(objective: &ObjectiveDef) -> String {
    let description = objective_description(objective);
    let Some(owner) = preferred_objective_owner(objective) else {
        return description;
    };
    let Some(owner_label) = objective_owner_display_label(owner) else {
        return description;
    };
    if owner_label.eq_ignore_ascii_case(description.trim()) {
        return description;
    }
    if objective.description.trim().is_empty() && !owner.name.trim().is_empty() {
        owner_label
    } else {
        format!("{owner_label}: {description}")
    }
}

pub(in crate::app) fn meaningful_definition_contexts(
    definition: &UnlockDefinition,
) -> Vec<&ProgressionContextDef> {
    definition
        .tested_by
        .iter()
        .map(|context| &**context)
        .filter(|context| {
            context.kind != ProgressionContextKind::Objective
                && (!context.name.trim().is_empty()
                    || !progression_type_label(&context.type_name).is_empty()
                    || !context.description.trim().is_empty()
                    || context
                        .paths
                        .iter()
                        .any(|path| path.iter().any(|component| !component.trim().is_empty())))
        })
        .collect()
}

const fn objective_context_priority(kind: ProgressionContextKind) -> u8 {
    match kind {
        ProgressionContextKind::PresentationNode => 0,
        ProgressionContextKind::Record => 1,
        ProgressionContextKind::Collectible => 2,
        ProgressionContextKind::InventoryItem => 3,
        ProgressionContextKind::ActivityAvailability => 4,
        ProgressionContextKind::Activity => 5,
        ProgressionContextKind::LocationRelease => 6,
        ProgressionContextKind::Location => 7,
        ProgressionContextKind::ExpressionMapping => 8,
        ProgressionContextKind::Objective => 9,
        ProgressionContextKind::Progression => 10,
        ProgressionContextKind::Achievement => 11,
        ProgressionContextKind::Requirement => 12,
        ProgressionContextKind::ValueCounter => 13,
        ProgressionContextKind::PackageExpression => 14,
    }
}

pub(in crate::app) fn definition_context_label(context: &ProgressionContextDef) -> Option<&str> {
    let name = context.name.trim();
    if !name.is_empty() {
        return Some(name);
    }
    let type_name = progression_type_label(&context.type_name);
    if !type_name.is_empty() {
        return Some(type_name);
    }
    let description = context.description.trim();
    if !description.is_empty() {
        return Some(description);
    }
    context
        .paths
        .iter()
        .flat_map(|path| path.iter())
        .map(|component| component.trim())
        .find(|component| !component.is_empty())
}

pub(in crate::app) fn resolved_objective_table_text(
    catalog: &Catalog,
    objective: &ObjectiveDef,
    definition: Option<&UnlockDefinition>,
) -> String {
    if !objective.description.trim().is_empty() || preferred_objective_owner(objective).is_some() {
        return objective_goal_text(objective);
    }
    if let Some(name) = catalog.display_name(objective.hash) {
        return name.to_owned();
    }
    let mut labels = objective
        .referenced_objective_indices
        .iter()
        .filter_map(|index| catalog.objective_definition(usize::from(*index)))
        .map(|target| objective_table_text(target, None))
        .filter(|label| !label.starts_with("Objective 0x"))
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    labels.retain(|label| seen.insert(label.clone()));
    if let Some(label) = labels.first() {
        return if labels.len() == 1 {
            format!("{label} · linked objective")
        } else {
            format!("{label} · +{} linked", labels.len() - 1)
        };
    }
    objective_table_text(objective, definition)
}

pub(in crate::app) fn objective_table_text(
    objective: &ObjectiveDef,
    definition: Option<&UnlockDefinition>,
) -> String {
    if !objective.description.trim().is_empty() || preferred_objective_owner(objective).is_some() {
        return objective_goal_text(objective);
    }

    let context = definition.and_then(|definition| {
        meaningful_definition_contexts(definition)
            .into_iter()
            .min_by_key(|context| objective_context_priority(context.kind))
    });
    if let Some(label) = context.and_then(definition_context_label) {
        format!("{label} · objective 0x{:08X}", objective.hash)
    } else if let Some(index) = objective.related_unlock_value_definition_index {
        format!(
            "Objective 0x{:08X} · value definition #{index}",
            objective.hash
        )
    } else {
        format!("Objective 0x{:08X}", objective.hash)
    }
}

pub(in crate::app) fn objective_target_text(objective: &crate::catalog::ObjectiveDef) -> String {
    let target = objective.completion_value;
    if objective.maximum_value().is_some() {
        format!("{target} max")
    } else if objective.minimum_value().is_some() {
        format!("{target} min")
    } else if objective.is_counting_downward {
        format!("≤{target}")
    } else {
        format!("≥{target}")
    }
}
