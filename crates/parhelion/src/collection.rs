//! Stable recipe destinations and the shared native presentation-node budget.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NODE_CAPACITY: usize = 1024;
pub const BASE_NODE_COUNT: usize = crate::progression::STOCK_PRESENTATION_NODE_COUNT + 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ammo {
    Primary,
    Special,
    Heavy,
}

impl Ammo {
    pub const ALL: [Self; 3] = [Self::Primary, Self::Special, Self::Heavy];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Special => "Special",
            Self::Heavy => "Heavy",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    AutoRifles,
    Bows,
    FusionRifles,
    GrenadeLaunchers,
    HandCannons,
    LinearFusionRifles,
    MachineGuns,
    PulseRifles,
    RocketLaunchers,
    ScoutRifles,
    Shotguns,
    Sidearms,
    SniperRifles,
    SubmachineGuns,
    Swords,
}
impl Family {
    pub const ALL: [Self; 15] = [
        Self::AutoRifles,
        Self::Bows,
        Self::FusionRifles,
        Self::GrenadeLaunchers,
        Self::HandCannons,
        Self::LinearFusionRifles,
        Self::MachineGuns,
        Self::PulseRifles,
        Self::RocketLaunchers,
        Self::ScoutRifles,
        Self::Shotguns,
        Self::Sidearms,
        Self::SniperRifles,
        Self::SubmachineGuns,
        Self::Swords,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::AutoRifles => "Auto Rifles",
            Self::Bows => "Bows",
            Self::FusionRifles => "Fusion Rifles",
            Self::GrenadeLaunchers => "Grenade Launchers",
            Self::HandCannons => "Hand Cannons",
            Self::LinearFusionRifles => "Linear Fusion Rifles",
            Self::MachineGuns => "Machine Guns",
            Self::PulseRifles => "Pulse Rifles",
            Self::RocketLaunchers => "Rocket Launchers",
            Self::ScoutRifles => "Scout Rifles",
            Self::Shotguns => "Shotguns",
            Self::Sidearms => "Sidearms",
            Self::SniperRifles => "Sniper Rifles",
            Self::SubmachineGuns => "Submachine Guns",
            Self::Swords => "Swords",
        }
    }
    pub const fn template(self) -> Destination {
        let ammo = match self {
            Self::FusionRifles | Self::GrenadeLaunchers | Self::Shotguns | Self::SniperRifles => {
                Ammo::Special
            }
            Self::LinearFusionRifles | Self::MachineGuns | Self::RocketLaunchers | Self::Swords => {
                Ammo::Heavy
            }
            _ => Ammo::Primary,
        };
        Destination { ammo, family: self }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub ammo: Ammo,
    pub family: Family,
}

impl Destination {
    pub fn label(self) -> String {
        format!("Weapons / {} / {}", self.ammo.label(), self.family.label())
    }
    /// Audited stock items anchor semantic destinations without persisting table indices.
    pub const fn stock_exemplar(self) -> Option<u32> {
        use Ammo::*;
        use Family::*;
        Some(match (self.ammo, self.family) {
            (Primary, AutoRifles) => 0x61FF_EE96,
            (Primary, Bows) => 0x2AEF_B232,
            (Primary, HandCannons) => 0x3E7B_47F8,
            (Primary, PulseRifles) => 0x1437_3AFC,
            (Primary, ScoutRifles) => 0x74D6_8F77,
            (Primary, Sidearms) => 0x5B67_57E0,
            (Primary, SubmachineGuns) => 0x478C_DF8F,
            (Special, FusionRifles) => 0xCD5D_35CD,
            (Special, GrenadeLaunchers) => 0x7637_40D0,
            (Special, Shotguns) => 0x2B94_6BA9,
            (Special, SniperRifles) => 0xC56A_395D,
            (Heavy, GrenadeLaunchers) => 0x8140_7A43,
            (Heavy, LinearFusionRifles) => 0x9527_F0F7,
            (Heavy, MachineGuns) => 0x23F4_BF01,
            (Heavy, RocketLaunchers) => 0x0837_DFF1,
            (Heavy, Swords) => 0xF8D1_86CA,
            _ => return None,
        })
    }
    pub(crate) fn hash(self, field: &str) -> u32 {
        crate::presentation::text_hash(
            &format!("collection/{:?}/{:?}", self.ammo, self.family),
            field,
        )
    }
}

pub struct NodeBudget {
    pub badges: usize,
    pub pages: usize,
}
impl NodeBudget {
    pub fn new<'a>(
        entries: impl IntoIterator<Item = (Option<&'a str>, Option<Destination>)>,
    ) -> Self {
        let mut badges = BTreeSet::new();
        let mut pages = BTreeSet::new();
        for (badge, destination) in entries {
            badges.extend(badge);
            pages.extend(destination.filter(|page| page.stock_exemplar().is_none()));
        }
        Self {
            badges: badges.len(),
            pages: pages.len(),
        }
    }
    pub const fn used(&self) -> usize {
        BASE_NODE_COUNT + self.badges * 4 + self.pages
    }
    pub(crate) fn validate(&self) -> crate::AuthoringResult<()> {
        if self.used() > NODE_CAPACITY {
            return Err(crate::error::invalid(format!(
                "Collections needs {} of {NODE_CAPACITY} nodes. Remove a custom badge or an added page from this build.",
                self.used()
            )));
        }
        Ok(())
    }
}
