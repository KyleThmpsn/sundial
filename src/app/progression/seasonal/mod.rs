//! Coordinated seasonal authoring and native runtime previews.
use crate::investment::seasonal::{self as rules, Definition, Experience};

use super::{CollectionStateSnapshot, document::CHARACTER_OBJECT_FLAG_BANK};

mod artifact;
mod context;
mod editing;
pub(in crate::app) mod rewards;
mod view;

pub(in crate::app) use editing::{Edit, apply};
pub(in crate::app) use view::{UiState, draw};

impl CollectionStateSnapshot {
    pub(in crate::app) fn is_native(&self) -> bool {
        self.is_native
    }

    pub(in crate::app) fn native_flag(
        &self,
        definition: &crate::catalog::UnlockDefinition,
    ) -> Option<bool> {
        if !matches!(definition.bank(), 1 | 2 | 3 | 6) {
            return None;
        }
        definition
            .compact_slot
            .map(|slot| self.flags.contains(&(definition.bank(), usize::from(slot))))
    }

    pub(in crate::app) fn seasonal_xp(&self) -> i32 {
        self.account_progressions
            .iter()
            .find(|row| row.definition_index == rules::POWER_PROGRESSION)
            .map_or(0, |row| row.lanes[0])
    }

    pub(in crate::app) fn seasonal_experience(
        &self,
        definition: &Definition,
    ) -> Result<Experience, String> {
        definition.experience(self.seasonal_xp())
    }

    pub(in crate::app) fn artifact_mask(
        &self,
        definition: &Definition,
        after_refresh: bool,
    ) -> u32 {
        definition
            .mods
            .iter()
            .filter(|entry| {
                self.flags.contains(&(
                    CHARACTER_OBJECT_FLAG_BANK,
                    usize::from(entry.character_slot),
                )) || (after_refresh
                    && self.flag_overrides.get(&usize::from(entry.flag_definition)) == Some(&2))
            })
            .fold(0, |mask, entry| mask | entry.bit())
    }

    pub(in crate::app) fn artifact_overrides(&self, definition: &Definition) -> Vec<(usize, u8)> {
        definition
            .mods
            .iter()
            .filter_map(|entry| {
                let index = usize::from(entry.flag_definition);
                self.flag_overrides.get(&index).map(|&value| (index, value))
            })
            .collect()
    }
}

pub(in crate::app) fn is_derived_value(index: usize) -> bool {
    matches!(
        index,
        rules::POWER_VALUE | rules::USED_VALUE | rules::EARNED_VALUE
    )
}

pub(in crate::app) fn progression_help(index: usize) -> Option<&'static str> {
    match index {
        rules::POWER_PROGRESSION => Some(
            "This is Sunrise's authoritative seasonal XP. Use Seasonal to update the pass, HUD, and artifact counters together.",
        ),
        rules::POINTS_PROGRESSION => Some(
            "Sunrise copies seasonal XP here when it refreshes seasonal state. Use Seasonal to change the authoritative XP.",
        ),
        rules::PASS_PROGRESSION => Some(
            "Sunrise caps this copy of seasonal XP at rank 100. Reward eligibility uses the authoritative seasonal XP.",
        ),
        rules::HUD_PROGRESSION => Some(
            "Sunrise rebuilds this HUD bar from seasonal XP. It repeats every 100,000 XP before rank 100, then tracks XP above the pass cap.",
        ),
        _ => None,
    }
}
