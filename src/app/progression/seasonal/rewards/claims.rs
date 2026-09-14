//! Prepare claim flags and queued deliveries together, then publish one reviewed edit.
use super::*;
use std::hash::BuildHasher;

mod pools;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Job {
    source: Value,
    candidate: Value,
    indices: Vec<usize>,
    cursor: usize,
    pub claimed: usize,
    pub issues: Vec<(String, String)>,
    pub rank_change: Option<(i32, i32)>,
}

impl Job {
    pub fn new(
        document: &Value,
        catalog: &Catalog,
        indices: Vec<usize>,
        rank: Option<i32>,
    ) -> Result<Self, String> {
        let mut candidate = document.clone();
        let mut rank_change = None;
        if let Some(rank) = rank {
            if !(1..=100).contains(&rank) {
                return Err("The reward rank is outside the Season Pass".into());
            }
            let current = collection_state_snapshot(document)
                .ok_or("Progression state unavailable")?
                .seasonal_xp();
            let total = ((rank - 1) * crate::investment::seasonal::XP_PER_RANK).max(current);
            if total != current {
                rank_change = Some((
                    (1 + current / crate::investment::seasonal::XP_PER_RANK).clamp(1, 100),
                    rank,
                ));
            }
            super::super::editing::apply(
                &mut candidate,
                catalog,
                super::super::editing::Edit::Experience(total),
            )?;
        }
        Ok(Self {
            source: document.clone(),
            candidate,
            indices,
            cursor: 0,
            claimed: 0,
            issues: Vec::new(),
            rank_change,
        })
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.cursor, self.indices.len())
    }

    pub fn queued(&self) -> usize {
        let count = |document: &Value| {
            document
                .get("_progression_rewards")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        };
        count(&self.candidate).saturating_sub(count(&self.source))
    }

    pub fn changed(&self) -> bool {
        self.source != self.candidate
    }
    pub fn direct_count(&self) -> usize {
        super::super::super::rewards::direct_count(&self.source, &self.candidate)
    }
    pub fn draw_consumables(&self, ui: &mut egui::Ui, catalog: &Catalog) {
        super::super::super::rewards::draw_review(ui, &self.source, &self.candidate, catalog);
    }

    pub fn queues_armor(&self, catalog: &Catalog) -> bool {
        let previous = self
            .source
            .get("_progression_rewards")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        self.candidate
            .get("_progression_rewards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .skip(previous)
            .filter_map(|row| catalog.inventory_definition(row["hash"].as_u64()?))
            .any(|item| {
                item.metadata.is_instanced_character_candidate()
                    && matches!(item.metadata.native_bucket_id, 3..=6)
            })
    }

    pub fn step(&mut self, catalog: &Catalog, pass: &ProgressionDefinition) -> bool {
        let start = std::time::Instant::now();
        while self.cursor < self.indices.len() {
            let index = self.indices[self.cursor];
            if let Some(reward) = pass.reward_items.get(index) {
                match claim(&mut self.candidate, catalog, reward) {
                    Ok(changed) => self.claimed += usize::from(changed),
                    Err(reason) => self.issues.push((
                        catalog
                            .package_item_name(reward.item_hash)
                            .map_or_else(|| format!("Reward #{index}"), str::to_owned),
                        reason,
                    )),
                }
            } else {
                self.issues.push((
                    format!("Reward #{index}"),
                    "The reward definition is unavailable".into(),
                ));
            }
            self.cursor += 1;
            if start.elapsed() >= std::time::Duration::from_millis(8) {
                break;
            }
        }
        self.cursor == self.indices.len()
    }

    pub fn finish(self, document: &mut Value) -> Result<bool, String> {
        if document != &self.source {
            return Err("The account changed during this edit. No changes were applied.".into());
        }
        let changed = self.changed();
        *document = self.candidate;
        Ok(changed)
    }
}

fn claim(
    document: &mut Value,
    catalog: &Catalog,
    reward: &ProgressionRewardDefinition,
) -> Result<bool, String> {
    let snapshot = collection_state_snapshot(document).ok_or("Progression state unavailable")?;
    let definition = catalog
        .seasonal()
        .ok_or("Seasonal definitions are unavailable")?;
    if !snapshot.is_native() {
        return Err("Season Pass claims require a current Sunrise SQLite account".into());
    }
    let flag = reward
        .claim_flag
        .and_then(|index| catalog.unlock_flag_definition(usize::from(index)))
        .ok_or("The reward has no saved claim flag")?;
    if flag.bank() != 1 {
        return Err("The reward claim flag is not in the account bank".into());
    }
    let slot = flag
        .compact_slot
        .ok_or("The reward has no saved claim slot")?;
    if snapshot.native_flag(flag) == Some(true) {
        return Ok(false);
    }
    if snapshot.seasonal_experience(definition)?.rank < reward.rewarded_at_progression_level {
        return Err(format!(
            "Reach Rank {} before claiming this reward",
            reward.rewarded_at_progression_level
        ));
    }
    let items = deliveries(document, catalog, reward)?;
    let mut candidate = document.clone();
    super::super::super::rewards::queue(&mut candidate, catalog, items)?;
    if !super::super::super::mutations::set_unlock_flag(
        &mut candidate,
        "account_flag_runs",
        usize::from(slot),
        true,
    ) {
        return Err("The reward claim flag could not be saved".into());
    }
    super::super::super::validate(&candidate)?;
    *document = candidate;
    Ok(true)
}

fn deliveries(
    document: &Value,
    catalog: &Catalog,
    reward: &ProgressionRewardDefinition,
) -> Result<Vec<(u64, i32)>, String> {
    let class = document["_reward_context"]["class"]
        .as_u64()
        .filter(|class| *class < 3)
        .ok_or("Select a character to receive this reward")?;
    let grant = catalog
        .seasonal()
        .and_then(|definition| definition.reward_grants.get(&reward.item_hash))
        .ok_or("The reward delivery type is unavailable")?;
    if !matches!(grant, RewardGrant::Item) && reward.quantity != 1 {
        return Err("This reward package has an unsupported quantity".into());
    }
    match grant {
        RewardGrant::Item => Ok(vec![(reward.item_hash, reward.quantity)]),
        RewardGrant::ClassPackage(hashes) => {
            let mut items = Vec::new();
            for &hash in hashes {
                let definition = catalog
                    .inventory_definition(hash)
                    .ok_or("A package item is unavailable")?;
                if definition
                    .item
                    .is_none_or(|item| item.class_type == 3 || item.class_type == class)
                {
                    items.push((hash, 1));
                }
            }
            if items.is_empty() {
                return Err("This package has no items for the selected character".into());
            }
            Ok(items)
        }
        RewardGrant::DestinationResources => Ok(pools::DESTINATION_RESOURCE_HASHES
            .iter()
            .map(|&hash| (hash, 50))
            .collect()),
        RewardGrant::LegendaryEngram | RewardGrant::ExoticEngram => {
            let (weapons, armor) = if matches!(grant, RewardGrant::LegendaryEngram) {
                (
                    pools::LEGENDARY_ENGRAM_WEAPONS,
                    [
                        pools::LEGENDARY_TITAN_ARMOUR,
                        pools::LEGENDARY_HUNTER_ARMOUR,
                        pools::LEGENDARY_WARLOCK_ARMOUR,
                    ][class as usize],
                )
            } else {
                (
                    pools::EXOTIC_ENGRAM_WEAPONS,
                    [
                        pools::EXOTIC_TITAN_ARMOUR,
                        pools::EXOTIC_HUNTER_ARMOUR,
                        pools::EXOTIC_WARLOCK_ARMOUR,
                    ][class as usize],
                )
            };
            let installed = weapons
                .iter()
                .chain(armor)
                .copied()
                .filter(|hash| catalog.inventory_definition(*hash).is_some())
                .collect::<Vec<_>>();
            if installed.is_empty() {
                return Err("No items from this engram's reward pool are installed".into());
            }
            let index = std::collections::hash_map::RandomState::new().hash_one(reward.item_hash)
                as usize
                % installed.len();
            Ok(vec![(installed[index], 1)])
        }
    }
}
