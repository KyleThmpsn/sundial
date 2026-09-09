use crate::catalog::*;
#[derive(Debug, Default)]
pub(super) struct CatalogHashMatchIndex {
    pub(super) progression_definitions: Vec<usize>,
    pub(super) progression_reward_matches: Vec<(usize, usize)>,
    pub(super) progression_faction_matches: Vec<(usize, usize)>,
    pub(super) flag_definitions: Vec<usize>,
    pub(super) value_definitions: Vec<usize>,
    pub(super) objectives: Vec<usize>,
    pub(super) owner_matches: Vec<(usize, usize)>,
    pub(super) trait_matches: Vec<(usize, usize, usize)>,
    pub(super) context_matches: Vec<(&'static str, usize, usize)>,
    pub(super) collectible_matches: Vec<usize>,
    pub(super) material_requirement_set_matches: Vec<usize>,
}

pub(super) struct CatalogHashMatches<'a> {
    pub(super) progression_definitions: Vec<(usize, &'a ProgressionDefinition)>,
    pub(super) progression_reward_matches: Vec<(usize, &'a ProgressionDefinition, usize)>,
    pub(super) progression_faction_matches: Vec<(
        usize,
        &'a ProgressionDefinition,
        usize,
        &'a ProgressionFactionDefinition,
    )>,
    pub(super) flag_definitions: Vec<(usize, &'a UnlockDefinition)>,
    pub(super) value_definitions: Vec<(usize, &'a UnlockDefinition)>,
    pub(super) objectives: Vec<(usize, &'a ObjectiveDef)>,
    pub(super) owner_matches: Vec<(usize, &'a ObjectiveDef, &'a ObjectiveOwnerDef)>,
    pub(super) trait_matches: Vec<(
        usize,
        &'a ObjectiveDef,
        &'a ObjectiveOwnerDef,
        &'a ObjectiveOwnerTraitDef,
    )>,
    pub(super) context_matches: Vec<(&'static str, usize, &'a ProgressionContextDef)>,
    pub(super) collectible_matches: Vec<&'a CollectibleDef>,
    pub(super) material_requirement_set_matches: Vec<&'a MaterialRequirementSetDef>,
    pub(super) bucket_items: Vec<&'a ItemDef>,
    pub(super) item: Option<&'a ItemDef>,
    pub(super) item_package_metadata: Option<&'a ItemPackageMetadata>,
    pub(super) item_stat_definition: Option<&'a ItemStatDefinition>,
    pub(super) investment_stat_references: Vec<(u64, &'a ItemInvestmentStat)>,
    pub(super) inventory_metadata: Option<&'a InventoryMetadata>,
    pub(super) item_material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CatalogMatchGroup {
    pub(super) label: &'static str,
    pub(super) count: usize,
}

impl CatalogHashMatchIndex {
    pub(super) fn collect(catalog: &Catalog, hash: u64) -> Self {
        let progression_definitions = catalog
            .progression_definitions()
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.hash == hash)
            .map(|(index, _)| index)
            .collect();
        let progression_reward_matches = catalog
            .progression_definitions()
            .iter()
            .enumerate()
            .flat_map(|(definition_index, definition)| {
                definition
                    .reward_items
                    .iter()
                    .enumerate()
                    .filter(move |(_, reward)| reward.item_hash == hash)
                    .map(move |(reward_index, _)| (definition_index, reward_index))
            })
            .collect();
        let progression_faction_matches = catalog
            .progression_definitions()
            .iter()
            .enumerate()
            .flat_map(|(definition_index, definition)| {
                definition
                    .factions
                    .iter()
                    .enumerate()
                    .filter(move |(_, faction)| faction.hash == hash)
                    .map(move |(faction_index, _)| (definition_index, faction_index))
            })
            .collect();
        let flag_definitions = catalog
            .unlock_flag_definitions()
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.hash == hash)
            .map(|(index, _)| index)
            .collect();
        let value_definitions = catalog
            .unlock_value_definitions()
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.hash == hash)
            .map(|(index, _)| index)
            .collect();
        let objectives = catalog
            .objectives()
            .iter()
            .enumerate()
            .filter(|(_, objective)| objective.hash == hash)
            .map(|(index, _)| index)
            .collect();
        let owner_matches = catalog
            .objectives()
            .iter()
            .enumerate()
            .flat_map(|(objective_index, objective)| {
                objective
                    .owners
                    .iter()
                    .enumerate()
                    .filter_map(move |(owner_index, owner)| {
                        (owner.hash == hash).then_some((objective_index, owner_index))
                    })
            })
            .collect();
        let trait_matches = catalog
            .objectives()
            .iter()
            .enumerate()
            .flat_map(|(objective_index, objective)| {
                objective
                    .owners
                    .iter()
                    .enumerate()
                    .flat_map(move |(owner_index, owner)| {
                        owner.traits.iter().enumerate().filter_map(
                            move |(trait_index, trait_definition)| {
                                (trait_definition.hash == hash).then_some((
                                    objective_index,
                                    owner_index,
                                    trait_index,
                                ))
                            },
                        )
                    })
            })
            .collect();
        let context_matches = catalog
            .unlock_flag_definitions()
            .iter()
            .enumerate()
            .flat_map(|(index, definition)| {
                definition.tested_by.iter().enumerate().filter_map(
                    move |(context_index, context)| {
                        (context.hash == hash).then_some(("Flag", index, context_index))
                    },
                )
            })
            .chain(
                catalog
                    .unlock_value_definitions()
                    .iter()
                    .enumerate()
                    .flat_map(|(index, definition)| {
                        definition.tested_by.iter().enumerate().filter_map(
                            move |(context_index, context)| {
                                (context.hash == hash).then_some(("Value", index, context_index))
                            },
                        )
                    }),
            )
            .collect();
        let collectible_matches = catalog
            .collectibles()
            .iter()
            .enumerate()
            .filter_map(|(index, definition)| {
                (definition.hash == hash
                    || definition.item_hash == hash
                    || definition.material_requirement_set_hash == hash
                    || definition
                        .material_requirements
                        .iter()
                        .any(|requirement| requirement.item_hash == hash))
                .then_some(index)
            })
            .collect();
        let material_requirement_set_matches = catalog
            .material_requirement_sets()
            .iter()
            .enumerate()
            .filter_map(|(index, set)| {
                (set.hash == hash
                    || set
                        .requirements
                        .iter()
                        .any(|requirement| requirement.item_hash == hash))
                .then_some(index)
            })
            .collect();
        Self {
            progression_definitions,
            progression_reward_matches,
            progression_faction_matches,
            flag_definitions,
            value_definitions,
            objectives,
            owner_matches,
            trait_matches,
            context_matches,
            collectible_matches,
            material_requirement_set_matches,
        }
    }
}

impl<'a> CatalogHashMatches<'a> {
    pub(super) fn from_index(
        catalog: &'a Catalog,
        hash: u64,
        index: &CatalogHashMatchIndex,
    ) -> Self {
        let progression_definitions = index
            .progression_definitions
            .iter()
            .filter_map(|definition_index| {
                catalog
                    .progression_definitions()
                    .get(*definition_index)
                    .map(|definition| (*definition_index, definition))
            })
            .collect();
        let progression_reward_matches = index
            .progression_reward_matches
            .iter()
            .filter_map(|(definition_index, reward_index)| {
                let definition = catalog.progression_definitions().get(*definition_index)?;
                definition.reward_items.get(*reward_index)?;
                Some((*definition_index, definition, *reward_index))
            })
            .collect();
        let progression_faction_matches = index
            .progression_faction_matches
            .iter()
            .filter_map(|(definition_index, faction_index)| {
                let definition = catalog.progression_definitions().get(*definition_index)?;
                let faction = definition.factions.get(*faction_index)?;
                Some((*definition_index, definition, *faction_index, faction))
            })
            .collect();
        let flag_definitions = index
            .flag_definitions
            .iter()
            .filter_map(|definition_index| {
                catalog
                    .unlock_flag_definition(*definition_index)
                    .map(|definition| (*definition_index, definition))
            })
            .collect();
        let value_definitions = index
            .value_definitions
            .iter()
            .filter_map(|definition_index| {
                catalog
                    .unlock_value_definition(*definition_index)
                    .map(|definition| (*definition_index, definition))
            })
            .collect();
        let objectives = index
            .objectives
            .iter()
            .filter_map(|objective_index| {
                catalog
                    .objectives()
                    .get(*objective_index)
                    .map(|objective| (*objective_index, objective))
            })
            .collect();
        let owner_matches = index
            .owner_matches
            .iter()
            .filter_map(|(objective_index, owner_index)| {
                let objective = catalog.objectives().get(*objective_index)?;
                let owner = objective.owners.get(*owner_index)?;
                Some((*objective_index, objective, owner))
            })
            .collect();
        let trait_matches = index
            .trait_matches
            .iter()
            .filter_map(|(objective_index, owner_index, trait_index)| {
                let objective = catalog.objectives().get(*objective_index)?;
                let owner = objective.owners.get(*owner_index)?;
                let trait_definition = owner.traits.get(*trait_index)?;
                Some((*objective_index, objective, owner, trait_definition))
            })
            .collect();
        let context_matches = index
            .context_matches
            .iter()
            .filter_map(|(kind, definition_index, context_index)| {
                let definition = match *kind {
                    "Flag" => catalog.unlock_flag_definition(*definition_index),
                    "Value" => catalog.unlock_value_definition(*definition_index),
                    _ => None,
                }?;
                let context = definition.tested_by.get(*context_index)?;
                Some((*kind, *definition_index, context))
            })
            .collect();
        let collectible_matches = index
            .collectible_matches
            .iter()
            .filter_map(|definition_index| catalog.collectibles().get(*definition_index))
            .collect();
        let material_requirement_set_matches = index
            .material_requirement_set_matches
            .iter()
            .filter_map(|definition_index| {
                catalog.material_requirement_sets().get(*definition_index)
            })
            .collect();
        Self {
            progression_definitions,
            progression_reward_matches,
            progression_faction_matches,
            flag_definitions,
            value_definitions,
            objectives,
            owner_matches,
            trait_matches,
            context_matches,
            collectible_matches,
            material_requirement_set_matches,
            bucket_items: catalog.items_for_bucket(hash).collect(),
            item: catalog.item(hash),
            item_package_metadata: catalog.item_package_metadata(hash),
            item_stat_definition: catalog.item_stat_definition_by_hash(hash),
            investment_stat_references: catalog.item_investment_stat_references(hash),
            inventory_metadata: catalog.inventory_metadata(hash),
            item_material_requirement_set_indices: catalog
                .item_material_requirement_set_indices(hash),
        }
    }

    pub(super) fn match_groups(&self) -> Vec<CatalogMatchGroup> {
        let mut groups = Vec::with_capacity(16);
        push_match_group(
            &mut groups,
            "Item Definition",
            usize::from(self.item_package_metadata.is_some() || self.item.is_some()),
        );
        push_match_group(
            &mut groups,
            "Inventory Metadata",
            usize::from(self.inventory_metadata.is_some()),
        );
        push_match_group(
            &mut groups,
            "Progression Definition",
            self.progression_definitions.len(),
        );
        push_match_group(
            &mut groups,
            "Progression Reward Reference",
            self.progression_reward_matches.len(),
        );
        push_match_group(
            &mut groups,
            "Faction Progression Reference",
            self.progression_faction_matches.len(),
        );
        push_match_group(
            &mut groups,
            "Unlock Flag Definition",
            self.flag_definitions.len(),
        );
        push_match_group(
            &mut groups,
            "Unlock Value Definition",
            self.value_definitions.len(),
        );
        push_match_group(&mut groups, "Objective", self.objectives.len());
        push_match_group(
            &mut groups,
            "Objective Owner Reference",
            self.owner_matches.len(),
        );
        push_match_group(
            &mut groups,
            "Objective Trait Reference",
            self.trait_matches.len(),
        );
        push_match_group(
            &mut groups,
            "Progression Reader Reference",
            self.context_matches.len(),
        );
        push_match_group(&mut groups, "Collectible", self.collectible_matches.len());
        push_match_group(
            &mut groups,
            "Material Requirement Set",
            self.material_requirement_set_matches.len(),
        );
        push_match_group(
            &mut groups,
            "Item Stat Definition",
            usize::from(self.item_stat_definition.is_some()),
        );
        push_match_group(
            &mut groups,
            "Investment Stat Reference",
            self.investment_stat_references.len(),
        );
        push_match_group(
            &mut groups,
            "Inventory Bucket Item",
            self.bucket_items.len(),
        );
        groups
    }

    pub(super) fn count(&self) -> usize {
        self.match_groups().iter().map(|group| group.count).sum()
    }
}

fn push_match_group(groups: &mut Vec<CatalogMatchGroup>, label: &'static str, count: usize) {
    if count > 0 {
        groups.push(CatalogMatchGroup { label, count });
    }
}
