//! Objective labels and metadata semantics shared by progression inspectors and tables.

use crate::{
    catalog::{
        Catalog, ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef,
        ProgressionContextDef, ProgressionContextKind, UnlockDefinition,
    },
    hash::format_hash_hex,
};

use crate::app::inspector::progression_context_kind_label;

use super::definitions::definition_name;

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

pub(in crate::app) fn objective_owner_trait_label(
    trait_definition: &ObjectiveOwnerTraitDef,
) -> String {
    let name = trait_definition.name.trim();
    if name.is_empty() {
        format_hash_hex(trait_definition.hash)
    } else {
        name.to_owned()
    }
}

pub(in crate::app) fn objective_owner_display_label(owner: &ObjectiveOwnerDef) -> Option<String> {
    let label = objective_owner_label(owner)?;
    Some(label.to_owned())
}

pub(in crate::app) fn objective_traits_text(objective: &ObjectiveDef) -> Option<String> {
    let owner = preferred_objective_owner(objective)?;
    (!owner.traits.is_empty()).then(|| {
        owner
            .traits
            .iter()
            .map(objective_owner_trait_label)
            .collect::<Vec<_>>()
            .join(", ")
    })
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
            ObjectiveOwnerKind::PresentationNode => "Presentation node",
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

pub(in crate::app) fn override_meaning_contexts(
    definition: &UnlockDefinition,
) -> Vec<&ProgressionContextDef> {
    definition
        .tested_by
        .iter()
        .filter(|context| {
            !context.name.trim().is_empty()
                || !progression_type_label(&context.type_name).is_empty()
                || !context.description.trim().is_empty()
                || context
                    .paths
                    .iter()
                    .any(|path| path.iter().any(|component| !component.trim().is_empty()))
        })
        .collect()
}

pub(in crate::app) fn override_meaning(definition: &UnlockDefinition) -> String {
    if let Some(name) = definition_name(definition) {
        return name.trim().to_owned();
    }

    let contexts = override_meaning_contexts(definition);
    let mut labels = contexts
        .iter()
        .filter_map(|context| definition_context_label(context))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    labels.sort_by_key(|label| label.to_lowercase());
    labels.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    match labels.len() {
        1 => return labels.remove(0),
        2 => return labels.join(" · "),
        count if count > 2 => {
            return format!("{} · {} · +{} more", labels[0], labels[1], count - 2);
        }
        _ => {}
    }
    if contexts.is_empty() {
        return "Reader not resolved".to_owned();
    }

    let mut kinds = contexts
        .iter()
        .map(|context| context.kind)
        .collect::<Vec<_>>();
    kinds.sort_by_key(|kind| objective_context_priority(*kind));
    kinds.dedup();
    if kinds.len() == 1 {
        format!("{} conditions", progression_context_kind_label(kinds[0]))
    } else {
        let mut labels = kinds
            .iter()
            .take(2)
            .map(|kind| progression_context_kind_label(*kind).to_owned())
            .collect::<Vec<_>>();
        if kinds.len() > 2 {
            labels.push(format!("+{} more", kinds.len() - 2));
        }
        labels.join(" · ")
    }
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
    labels.dedup();
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

pub(in crate::app) fn objective_details_tooltip(objective: &ObjectiveDef) -> String {
    let mut lines = vec![
        format!("Objective: {}", objective_description(objective)),
        format!("Objective hash: 0x{:08X}", objective.hash),
    ];
    for (label, value) in [
        ("Name", objective.name.as_str()),
        (
            "Display description",
            objective.display_description.as_str(),
        ),
        (
            "Progress description",
            objective.progress_description.as_str(),
        ),
    ] {
        let value = value.trim();
        if !value.is_empty() && !value.eq_ignore_ascii_case(objective.description.trim()) {
            lines.push(format!("{label}: {value}"));
        }
    }
    if objective
        .owners
        .iter()
        .any(|owner| objective_owner_label(owner).is_some())
    {
        lines.push("Package owners:".into());
        for owner in objective
            .owners
            .iter()
            .filter(|owner| objective_owner_label(owner).is_some())
        {
            let owner_type = objective_owner_type(owner);
            let owner_label = objective_owner_label(owner).unwrap_or(owner_type);
            if owner_type.eq_ignore_ascii_case(owner_label) {
                lines.push(owner_label.to_owned());
            } else {
                lines.push(format!("{owner_type}: {owner_label}"));
            }
            lines.push(format!("{owner_type} hash: 0x{:08X}", owner.hash));
            let description = owner.description.trim();
            if !description.is_empty() && !description.eq_ignore_ascii_case(owner_label) {
                lines.push(format!("Description: {description}"));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
pub(in crate::app) fn objective_traits_tooltip(objective: &ObjectiveDef) -> String {
    let Some(owner) = preferred_objective_owner(objective) else {
        return "No package traits".into();
    };
    if owner.traits.is_empty() {
        return "No package traits".into();
    }
    let mut lines = Vec::new();
    for trait_definition in &owner.traits {
        let trait_label = objective_owner_trait_label(trait_definition);
        lines.push(format!("{trait_label}: 0x{:08X}", trait_definition.hash));
        let description = trait_definition.description.trim();
        if !description.is_empty() && !description.eq_ignore_ascii_case(&trait_label) {
            lines.push(format!("{trait_label}: {description}"));
        }
    }
    lines.join("\n")
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

#[cfg(test)]
pub(in crate::app) fn objective_target_tooltip(objective: &crate::catalog::ObjectiveDef) -> String {
    let target = objective.completion_value;
    let counts_downward = if objective.is_counting_downward {
        "yes"
    } else {
        "no"
    };
    let overcompletion = if objective.allow_overcompletion {
        "allowed"
    } else {
        "not allowed"
    };
    let negative = if objective.allow_negative_value {
        "allowed"
    } else {
        "not allowed"
    };
    let completed_changes = if objective.allow_value_change_when_completed {
        "allowed"
    } else {
        "not allowed"
    };
    format!(
        "Completion value: {target}\nCounts downward: {counts_downward}\nOver-completion: {overcompletion}\nNegative values: {negative}\nChanges after completion: {completed_changes}"
    )
}
