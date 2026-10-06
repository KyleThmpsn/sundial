//! Private Sparrow motion and complete native vehicle summon donors for Shadowkeep.
use crate::HexHash;
use serde::{Deserialize, Serialize};

pub(crate) mod authoring;
mod motion;

/// What an authored Sparrow summons. Alternate vehicles need gameplay acceptance.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Summon {
    #[default]
    Sparrow,
    Pike,
    HeavyPike,
    Interceptor,
    SuperInterceptor,
    Tank,
    Other {
        entity: HexHash,
    },
}

impl Summon {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Sparrow => "Sparrow",
            Self::Pike => "Pike",
            Self::HeavyPike => "Heavy Pike",
            Self::Interceptor => "Interceptor",
            Self::SuperInterceptor => "Super Interceptor",
            Self::Tank => "Tank",
            Self::Other { .. } => "Other Vehicle",
        }
    }

    pub(crate) fn entity(&self) -> Result<Option<u32>, String> {
        Ok(match self {
            Self::Sparrow => None,
            Self::Pike => Some(0x80C0_D9FA),
            Self::HeavyPike => Some(0x80C0_C1EC),
            Self::Interceptor => Some(0x80C0_D7A6),
            Self::SuperInterceptor => Some(0x80BC_9A34),
            Self::Tank => Some(0x80BF_AF2D),
            Self::Other { entity } => Some(
                entity
                    .parse_u32()
                    .map_err(|error| format!("Vehicle entity tag: {error}"))?,
            ),
        })
    }
}

/// Percent of the selected vehicle's original forward and reverse motion programs.
/// 100 preserves them. This is independent of the inventory's displayed Speed stat.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sparrow {
    pub summon: Summon,
    pub speed_percent: u16,
}

impl Default for Sparrow {
    fn default() -> Self {
        Self {
            summon: Summon::Sparrow,
            speed_percent: 100,
        }
    }
}

impl Sparrow {
    pub fn has_changes(&self) -> bool {
        self.summon != Summon::Sparrow || self.speed_percent != 100
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=1000).contains(&self.speed_percent) {
            return Err("Driving speed must be between 1% and 1000%.".into());
        }
        if self.summon == Summon::Tank && self.speed_percent != 100 {
            return Err("Tank motion does not support Driving Speed. Use 100%.".into());
        }
        self.summon.entity()?;
        Ok(())
    }
}
