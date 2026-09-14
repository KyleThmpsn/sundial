use super::*;
use crate::catalog::{ProgressionContextKind as Kind, UnlockWriter};

#[derive(Debug)]
pub(super) struct Label {
    pub text: String,
    pub purpose: String,
    pub category: &'static str,
    pub location: String,
    pub reference: bool,
    pub named: bool,
}

fn category(kind: Kind) -> &'static str {
    match kind {
        Kind::Record => "Triumphs",
        Kind::Collectible => "Collections",
        Kind::InventoryItem => "Items",
        Kind::Activity | Kind::ActivityAvailability | Kind::Location | Kind::LocationRelease => {
            "Activities"
        }
        Kind::Progression => "Ranks",
        Kind::Objective => "Objectives",
        _ => "Other",
    }
}

fn role(context: &ProgressionContextDef) -> Option<String> {
    context.direct_references.iter().find_map(|role| {
        let role = role.split(": #").next().unwrap_or(role).trim();
        (!role.is_empty()).then(|| match role {
            "Record completion flag" => "Completion".into(),
            "Record category flag" => "Category".into(),
            "Redeemed interval count" => "Claimed Stages".into(),
            "Triumph progress" => "Progress".into(),
            _ => role.to_owned(),
        })
    })
}

fn context_rank(context: &ProgressionContextDef, catalog: &Catalog) -> (bool, u8, bool) {
    let unnamed = context.name.trim().is_empty() && catalog.display_name(context.hash).is_none();
    let kind = match context.kind {
        Kind::Record => 0,
        Kind::Collectible => 1,
        Kind::InventoryItem => 2,
        Kind::Objective => 3,
        Kind::Activity | Kind::ActivityAvailability => 4,
        Kind::Progression => 5,
        _ => 6,
    };
    (unnamed, kind, context.direct_references.is_empty())
}

pub(super) fn unlock(catalog: &Catalog, index: usize, value: bool) -> Label {
    let mut label = Label {
        text: format!(
            "Unnamed {} #{index}",
            if value { "Counter" } else { "Unlock" }
        ),
        purpose: if value { "Counter" } else { "Unlock" }.into(),
        category: "Other",
        location: String::new(),
        reference: false,
        named: false,
    };
    let Some(definition) = (if value {
        catalog.unlock_value_definition(index)
    } else {
        catalog.unlock_flag_definition(index)
    }) else {
        return label;
    };
    let context = definition
        .tested_by
        .iter()
        .min_by_key(|context| context_rank(context, catalog));
    if value && definition.bank() == 1 && definition.compact_slot == Some(2115) {
        label.text = "Triumph Score".into();
        label.purpose = "Claimed Points".into();
        label.category = "Triumphs";
        label.named = true;
        return label;
    }
    if let Some(context) = context {
        label.category = category(context.kind);
        if let Some(purpose) = role(context) {
            label.purpose = purpose;
        } else if !context.condition_programs.is_empty() {
            label.purpose = "Condition Input".into();
        }
        label.location = context
            .paths
            .first()
            .map(|path| super::hierarchy::normalize_context_path(path).join(" / "))
            .unwrap_or_default();
    }
    if let Some(name) = definition_name(definition)
        .or_else(|| catalog.display_name(definition.hash))
        .filter(|name| !name.trim().is_empty())
    {
        label.text = name.to_owned();
        label.named = true;
        return label;
    }
    if let Some(context) = context {
        if let Some(name) = (!context.name.trim().is_empty())
            .then_some(context.name.as_str())
            .or_else(|| catalog.display_name(context.hash))
        {
            label.text = name.to_owned();
            label.reference = true;
            label.named = true;
            return label;
        }
    }
    if value && let Some(objective) = catalog.objective_for_unlock_value(index) {
        for text in [
            &objective.progress_description,
            &objective.display_description,
            &objective.description,
            &objective.name,
        ] {
            if !text.trim().is_empty() {
                label.text = text.clone();
                label.reference = true;
                label.named = true;
                label.category = "Objectives";
                label.purpose = "Progress".into();
                return label;
            }
        }
    }
    for writer in &definition.runtime_writers {
        let (progression, purpose) = match writer {
            UnlockWriter::ProgressionStep {
                definition_index,
                step_index,
            } => (
                catalog.progression_definition(usize::from(*definition_index)),
                format!("Rank {} Reward", usize::from(*step_index) + 1),
            ),
            UnlockWriter::ProgressionLevel { definition_index } => (
                catalog.progression_definition(usize::from(*definition_index)),
                "Current Rank".into(),
            ),
            UnlockWriter::ValueCounter { .. } => {
                label.purpose = "Computed Counter".into();
                continue;
            }
            UnlockWriter::Context { .. } => {
                label.purpose = "Context Dependent".into();
                continue;
            }
        };
        if let Some(name) = progression.and_then(super::progression_display_name) {
            label.text = name;
            label.reference = true;
            label.named = true;
            label.category = "Ranks";
            label.purpose = purpose;
            return label;
        }
    }
    fallback_context(&mut label, definition, context, index);
    if !label.location.is_empty() {
        label.reference = true;
    }
    label
}

fn fallback_context(
    label: &mut Label,
    definition: &UnlockDefinition,
    context: Option<&ProgressionContextDef>,
    index: usize,
) {
    if let Some(context) = context {
        if let Some(text) = [&context.description, &context.type_name]
            .into_iter()
            .find(|text| !text.trim().is_empty())
        {
            label.text = format!("{text} · #{index}");
            label.reference = true;
            label.named = true;
            return;
        }
        if let Some(path) = context.paths.first().filter(|path| !path.is_empty()) {
            label.text = format!("{} · {} #{index}", path[0], label.purpose);
            label.reference = true;
        }
    } else if definition.runtime_writers.is_empty() {
        label.purpose = "No Known References".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn record_roles_include_definition_suffixes_and_stay_separate_from_names() {
        let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
            vec![UnlockDefinition {
                tested_by: vec![ProgressionContextDef {
                    hash: 100,
                    kind: Kind::Record,
                    name: "A New Beginning".into(),
                    type_name: String::new(),
                    description: String::new(),
                    paths: vec![vec!["Account".into(), "Triumphs".into()]],
                    condition_programs: Vec::new(),
                    direct_references: vec!["Record completion flag: #0".into()],
                }],
                ..Default::default()
            }],
            Vec::new(),
            Vec::new(),
        );
        let label = unlock(&catalog, 0, false);
        assert_eq!(label.text, "A New Beginning");
        assert_eq!(label.purpose, "Completion");
        assert_eq!(label.category, "Triumphs");
        assert_eq!(label.location, "Triumphs / Account");
        assert!(label.named && label.reference);
        assert!(!unlock(&catalog, 123, false).named);
    }
}
