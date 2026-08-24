//! Search and picker choices derived from installed inventory definitions.

use std::cmp::Reverse;

use crate::catalog::InventoryDefinition;

use super::{
    super::item_editor::DefinitionChoice,
    buckets::{character_bucket_rank, profile_bucket_rank},
    model::BucketKey,
};

pub(super) fn profile_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    grouped_definition_choices(
        definitions,
        |key| profile_bucket_rank(key.native_id),
        |definitions| {
            definitions.sort_by_cached_key(|definition| {
                (
                    profile_name_priority(*definition),
                    Reverse(definition.metadata.max_stack_size),
                    definition.name.to_lowercase(),
                    definition.hash,
                )
            });
        },
    )
}

pub(super) fn profile_bucket_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    let mut definitions = definitions.into_iter().collect::<Vec<_>>();
    definitions.sort_by_cached_key(|definition| {
        (
            profile_name_priority(*definition),
            Reverse(definition.metadata.max_stack_size),
            definition.name.to_lowercase(),
            definition.hash,
        )
    });
    definitions
        .into_iter()
        .map(definition_choice_without_group)
        .collect()
}

pub(super) fn character_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    grouped_definition_choices(
        definitions,
        |key| character_bucket_rank(key.native_id),
        |definitions| {
            definitions
                .sort_by_cached_key(|definition| (definition.name.to_lowercase(), definition.hash));
        },
    )
}

pub(super) fn without_definition_groups(
    mut choices: Vec<DefinitionChoice>,
) -> Vec<DefinitionChoice> {
    for choice in &mut choices {
        choice.group = None;
    }
    choices
}

pub(super) fn character_bucket_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
) -> Vec<DefinitionChoice> {
    let mut definitions = definitions.into_iter().collect::<Vec<_>>();
    definitions.sort_by_cached_key(|definition| (definition.name.to_lowercase(), definition.hash));
    definitions
        .into_iter()
        .map(definition_choice_without_group)
        .collect()
}

pub(super) fn grouped_definition_choices<'a>(
    definitions: impl IntoIterator<Item = InventoryDefinition<'a>>,
    bucket_rank: impl Fn(BucketKey) -> u16,
    mut sort_definitions: impl FnMut(&mut Vec<InventoryDefinition<'a>>),
) -> Vec<DefinitionChoice> {
    let mut buckets = Vec::<(BucketKey, String, Vec<InventoryDefinition<'a>>)>::new();
    for definition in definitions {
        let key = BucketKey {
            scope: definition.metadata.scope,
            native_id: definition.metadata.native_bucket_id,
        };
        if let Some((_, _, items)) = buckets.iter_mut().find(|(stored, _, _)| *stored == key) {
            items.push(definition);
        } else {
            buckets.push((key, definition.metadata.bucket_label(), vec![definition]));
        }
    }

    for (_, _, definitions) in &mut buckets {
        sort_definitions(definitions);
    }
    buckets.sort_by_cached_key(|(key, label, _)| {
        (bucket_rank(*key), label.to_lowercase(), key.native_id)
    });

    buckets
        .into_iter()
        .flat_map(|(_, group, definitions)| {
            definitions
                .into_iter()
                .map(move |definition| definition_choice(definition, group.clone()))
        })
        .collect()
}

pub(super) fn definition_choice_without_group(
    definition: InventoryDefinition<'_>,
) -> DefinitionChoice {
    DefinitionChoice {
        hash: definition.hash,
        name: definition.name.to_owned(),
        type_name: definition.type_name.to_owned(),
        group: None,
    }
}

pub(super) fn definition_choice(
    definition: InventoryDefinition<'_>,
    group: String,
) -> DefinitionChoice {
    DefinitionChoice {
        hash: definition.hash,
        name: definition.name.to_owned(),
        type_name: definition.type_name.to_owned(),
        group: Some(group),
    }
}

pub(super) fn profile_name_priority(definition: InventoryDefinition<'_>) -> u8 {
    let label = format!("{} {}", definition.name, definition.type_name).to_lowercase();
    if [
        "currency",
        "material",
        "consumable",
        "token",
        "shader",
        "ornament",
        "mod",
    ]
    .iter()
    .any(|term| label.contains(term))
    {
        0
    } else {
        1
    }
}
