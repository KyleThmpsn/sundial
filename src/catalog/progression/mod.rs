use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use crate::package_payload::{
    array_at, bool_at, i32_at, i64_at, relative_offset, u16_at, u32_at, u64_at,
};

use super::{Catalog, resolve_string};

mod conditions;
mod contexts;
mod definitions;
mod item_contexts;
mod models;
mod objectives;
mod presentation;
mod records;
mod schema;
mod unlocks;

use conditions::*;
#[cfg(test)]
use contexts::*;
#[cfg(test)]
use definitions::*;
#[cfg(test)]
use item_contexts::*;
#[cfg(test)]
use objectives::*;
#[cfg(test)]
use records::*;
use schema::*;
use unlocks::*;

pub(crate) use models::{
    ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef,
    ProgressionContextDef, ProgressionContextKind, ProgressionDefinition,
    ProgressionFactionDefinition, ProgressionRewardDefinition, ProgressionScope,
    ProgressionStepDefinition, UnlockDefinition,
};
pub(super) use models::{PendingProgressionContext, PresentationNodeDef, ProgressionPackageData};

pub(super) use conditions::sort_progression_contexts;
pub(super) use contexts::{
    scan_activity_condition_contexts, scan_location_condition_contexts,
    scan_metric_objective_owners, scan_trait_definitions,
};
pub(super) use definitions::scan_progression_definitions;
pub(super) use item_contexts::{
    ItemProgressionContext, attach_item_condition_contexts, scan_collectible_condition_contexts,
    scan_collectible_item_paths,
};
pub(super) use objectives::{scan_milestone_objective_owners, scan_objectives};
pub(super) use presentation::{
    attach_presentation_node_objective_owners, definition_index_list, presentation_paths,
    scan_presentation_nodes,
};
pub(super) use records::{item_objective_indices, scan_record_objective_owners};
pub(super) use unlocks::{
    add_objective_owner, scan_unlock_flag_definitions, scan_unlock_flag_displays,
    scan_unlock_value_definitions, unlock_state_indices,
};

impl Catalog {
    pub(crate) fn progression_package_error(&self) -> Option<&str> {
        self.progression_package_error.as_deref()
    }

    pub(crate) fn unlock_flag_for_state(
        &self,
        bank: u8,
        slot: usize,
    ) -> Option<(usize, &UnlockDefinition)> {
        let slot = u16::try_from(slot).ok()?;
        let index = *self.unlock_flag_state_indices.get(&(bank, slot))?;
        Some((index, self.unlock_flag_definitions.get(index)?))
    }

    pub(crate) fn unlock_value_for_state(
        &self,
        bank: u8,
        slot: usize,
    ) -> Option<(usize, &UnlockDefinition)> {
        let slot = u16::try_from(slot).ok()?;
        let index = *self.unlock_value_state_indices.get(&(bank, slot))?;
        Some((index, self.unlock_value_definitions.get(index)?))
    }

    pub(crate) fn unlock_flag_definition(&self, index: usize) -> Option<&UnlockDefinition> {
        self.unlock_flag_definitions.get(index)
    }

    pub(crate) fn unlock_flag_definitions(&self) -> &[UnlockDefinition] {
        &self.unlock_flag_definitions
    }

    pub(crate) fn unlock_value_definition(&self, index: usize) -> Option<&UnlockDefinition> {
        self.unlock_value_definitions.get(index)
    }

    pub(crate) fn unlock_value_definitions(&self) -> &[UnlockDefinition] {
        &self.unlock_value_definitions
    }

    pub(crate) fn objective_definition(&self, index: usize) -> Option<&ObjectiveDef> {
        self.objectives.get(index)
    }

    pub(crate) fn objectives(&self) -> &[ObjectiveDef] {
        &self.objectives
    }

    pub(crate) fn progression_definitions(&self) -> &[ProgressionDefinition] {
        &self.progression_definitions
    }

    pub(crate) fn progression_definition(&self, index: usize) -> Option<&ProgressionDefinition> {
        self.progression_definitions.get(index)
    }

    pub(crate) fn objective_for_unlock_value(
        &self,
        definition_index: usize,
    ) -> Option<&ObjectiveDef> {
        self.objectives_by_unlock_value
            .get(&definition_index)
            .and_then(|indices| indices.first())
            .and_then(|index| self.objectives.get(*index))
    }

    pub(crate) fn objective_with_index_for_unlock_value(
        &self,
        definition_index: usize,
    ) -> Option<(usize, &ObjectiveDef)> {
        let index = *self
            .objectives_by_unlock_value
            .get(&definition_index)?
            .first()?;
        self.objectives
            .get(index)
            .map(|objective| (index, objective))
    }

    pub(crate) fn objectives_for_unlock_value(
        &self,
        definition_index: usize,
    ) -> Vec<&ObjectiveDef> {
        self.objectives_by_unlock_value
            .get(&definition_index)
            .into_iter()
            .flatten()
            .filter_map(|index| self.objectives.get(*index))
            .collect()
    }
}

#[cfg(test)]
mod tests;
