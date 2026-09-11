//! Current Sunrise seasonal rules, shared by authoring and inspector previews.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) const ARTIFACT_VENDOR: u64 = 0xAC82_564E;
pub(crate) const POWER_PROGRESSION: usize = 38;
pub(crate) const POINTS_PROGRESSION: usize = 39;
pub(crate) const PASS_PROGRESSION: usize = 40;
pub(crate) const HUD_PROGRESSION: usize = 41;
pub(crate) const POWER_VALUE: usize = 602;
pub(crate) const USED_VALUE: usize = 604;
pub(crate) const EARNED_VALUE: usize = 605;
pub(crate) const PASS_VALUE: usize = 636;
pub(crate) const HUD_VALUE: usize = 637;
pub(crate) const USED_CHARACTER_SLOT: usize = 38;
pub(crate) const XP_PER_RANK: i32 = 100_000;
pub(crate) const PASS_XP_CAP: i32 = 99 * XP_PER_RANK;
const COLUMN_TIERS: [u16; 5] = [0, 1, 4, 7, 10];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ArtifactMod {
    pub sale_index: u16,
    pub category_index: u8,
    pub item_hash: u64,
    pub collectible_hash: u64,
    pub flag_definition: u16,
    pub character_slot: u16,
}

impl ArtifactMod {
    pub fn column(&self) -> usize {
        usize::from(self.category_index)
    }

    pub fn points_required(&self) -> u16 {
        COLUMN_TIERS.get(self.column()).copied().unwrap_or(10)
    }

    pub fn bit(&self) -> u32 {
        1_u32.checked_shl(u32::from(self.sale_index)).unwrap_or(0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum RewardGrant {
    Item,
    ClassPackage(Vec<u64>),
    DestinationResources,
    LegendaryEngram,
    ExoticEngram,
}

impl RewardGrant {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Item => "Item or Currency",
            Self::ClassPackage(_) => "Class Package",
            Self::DestinationResources => "Destination Resources",
            Self::LegendaryEngram => "Legendary Engram",
            Self::ExoticEngram => "Exotic Engram",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Definition {
    pub power_steps: Vec<i32>,
    pub point_steps: Vec<i32>,
    pub mods: Vec<ArtifactMod>,
    pub reward_grants: BTreeMap<u64, RewardGrant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Experience {
    pub total: i32,
    pub rank: i32,
    pub pass_xp: i32,
    pub hud_xp: i32,
    pub power_bonus: u16,
    pub points_earned: u16,
}

impl Definition {
    pub fn experience(&self, total: i32) -> Result<Experience, String> {
        if total < 0 {
            return Err("Seasonal XP must be zero or greater".into());
        }
        Ok(Experience {
            total,
            rank: (1 + total / XP_PER_RANK).min(100),
            pass_xp: total.min(PASS_XP_CAP),
            hud_xp: if total < PASS_XP_CAP {
                total % XP_PER_RANK
            } else {
                total - PASS_XP_CAP
            },
            power_bonus: ladder_ranks(&self.power_steps, total)?,
            points_earned: ladder_ranks(&self.point_steps, total)?,
        })
    }

    pub fn mod_for_flag(&self, index: usize) -> Option<&ArtifactMod> {
        self.mods
            .iter()
            .find(|entry| usize::from(entry.flag_definition) == index)
    }

    pub fn unlock(&self, mask: u32, sale_index: u16, earned: u16) -> Result<u32, String> {
        let entry = self
            .mods
            .iter()
            .find(|entry| entry.sale_index == sale_index)
            .ok_or("This artifact mod is not available in the installed vendor")?;
        let used = mask.count_ones();
        if mask & entry.bit() != 0 {
            return Err("This artifact mod is already unlocked".into());
        }
        if used >= u32::from(earned) {
            return Err("No artifact points are available".into());
        }
        if used < u32::from(entry.points_required()) {
            return Err(format!(
                "Spend {} artifact points before unlocking this column",
                entry.points_required()
            ));
        }
        Ok(mask | entry.bit())
    }
}

impl Experience {
    pub fn lanes(self) -> [(usize, i32); 4] {
        [
            (POWER_PROGRESSION, self.total),
            (POINTS_PROGRESSION, self.total),
            (PASS_PROGRESSION, self.pass_xp),
            (HUD_PROGRESSION, self.hud_xp),
        ]
    }

    pub fn values(self, used: u32) -> [(usize, i32); 3] {
        [
            (POWER_VALUE, i32::from(self.power_bonus)),
            (USED_VALUE, used as i32),
            (EARNED_VALUE, i32::from(self.points_earned)),
        ]
    }
}

fn ladder_ranks(steps: &[i32], total: i32) -> Result<u16, String> {
    let mut remaining = i64::from(total);
    let mut ranks = 0_u16;
    for &cost in steps {
        if cost < 0 {
            return Err("The installed artifact ladder contains a negative XP cost".into());
        }
        if remaining < i64::from(cost) {
            break;
        }
        remaining -= i64::from(cost);
        ranks = ranks
            .checked_add(1)
            .ok_or("The artifact ladder exceeds its rank limit")?;
    }
    Ok(ranks)
}
