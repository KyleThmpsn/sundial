//! Package-backed weapon damage-type decoding.

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::investment::schema::{
    LEGACY_SOLAR_DAMAGE_PERK_INDEX, LEGACY_VOID_DAMAGE_PERK_INDEX, MODERN_ARC_DAMAGE_PERK_INDEX,
    MODERN_SOLAR_DAMAGE_PERK_INDEX, MODERN_VOID_DAMAGE_PERK_INDEX,
};

const KINETIC_BUCKET: u64 = 1_498_876_634;
const ENERGY_BUCKET: u64 = 2_465_295_065;
const POWER_BUCKET: u64 = 953_998_645;

pub(crate) const fn is_weapon_bucket(bucket_hash: u64) -> bool {
    matches!(bucket_hash, KINETIC_BUCKET | ENERGY_BUCKET | POWER_BUCKET)
}

#[cfg(test)]
const ITEM_DAMAGE_PERK_CLASS: u32 = 0x8080_77BC;
#[cfg(test)]
const ITEM_DAMAGE_PERK_ROW_SIZE: usize = 24;
// The Fundamentals plug carries one row per element. Hard Light's plug uses 462 for the Arc
// leg and Borealis's uses 461; both compile to the same action, so either satisfies it.
#[cfg(test)]
const VARIABLE_ELEMENT_PERK_INDICES: [u16; 3] = [462, 463, 464];
#[cfg(test)]
const BOREALIS_VARIABLE_ELEMENT_PERK_INDICES: [u16; 3] = [461, 463, 464];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemDamageType {
    Kinetic,
    Arc,
    Solar,
    Void,
}

impl ItemDamageType {
    pub(crate) const ALL: [Self; 4] = [Self::Kinetic, Self::Arc, Self::Solar, Self::Void];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Kinetic => "Kinetic",
            Self::Arc => "Arc",
            Self::Solar => "Solar",
            Self::Void => "Void",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ItemDamageProfile {
    KineticEmpty,
    ModernFixed {
        damage_type: ItemDamageType,
    },
    LegacyFixed {
        damage_type: ItemDamageType,
    },
    PlugOrEmptyAmbiguous {
        damage_type: Option<ItemDamageType>,
    },
    Variable,
    #[default]
    Unknown,
}

impl ItemDamageProfile {
    pub(crate) const fn damage_type(self) -> Option<ItemDamageType> {
        match self {
            Self::KineticEmpty => Some(ItemDamageType::Kinetic),
            Self::ModernFixed { damage_type }
            | Self::LegacyFixed { damage_type }
            | Self::PlugOrEmptyAmbiguous {
                damage_type: Some(damage_type),
            } => Some(damage_type),
            Self::PlugOrEmptyAmbiguous { damage_type: None } | Self::Variable | Self::Unknown => {
                None
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemWeaponInventorySlot {
    Kinetic,
    Energy,
    Power,
}

impl ItemWeaponInventorySlot {
    pub(crate) const fn from_bucket_hash(bucket_hash: u64) -> Option<Self> {
        match bucket_hash {
            KINETIC_BUCKET => Some(Self::Kinetic),
            ENERGY_BUCKET => Some(Self::Energy),
            POWER_BUCKET => Some(Self::Power),
            _ => None,
        }
    }

    pub(crate) const fn from_equipment_slot(equipment_slot: u8) -> Option<Self> {
        match equipment_slot {
            7 => Some(Self::Kinetic),
            8 => Some(Self::Energy),
            9 => Some(Self::Power),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescriptorProfile {
    Empty,
    ModernFixed(ItemDamageType),
    LegacyFixed(ItemDamageType),
    Variable,
    Unknown,
}

impl DescriptorProfile {
    const fn into_item_profile(self) -> ItemDamageProfile {
        match self {
            Self::ModernFixed(damage_type) => ItemDamageProfile::ModernFixed { damage_type },
            Self::LegacyFixed(damage_type) => ItemDamageProfile::LegacyFixed { damage_type },
            Self::Variable => ItemDamageProfile::Variable,
            Self::Empty | Self::Unknown => ItemDamageProfile::Unknown,
        }
    }
}

fn descriptor_profile(item: &[u8]) -> DescriptorProfile {
    use crate::investment::weapon::{
        BaseDamage, DamageFamily, Element, base_sandbox_perks, classify_base_damage,
    };
    let Ok(perks) = base_sandbox_perks(item) else {
        return DescriptorProfile::Unknown;
    };
    if perks.is_empty() {
        return DescriptorProfile::Empty;
    }
    match classify_base_damage(&perks) {
        BaseDamage::Fixed(family, element) => {
            let element = match element {
                Element::Arc => ItemDamageType::Arc,
                Element::Solar => ItemDamageType::Solar,
                Element::Void => ItemDamageType::Void,
            };
            match family {
                DamageFamily::Legacy => DescriptorProfile::LegacyFixed(element),
                DamageFamily::Modern => DescriptorProfile::ModernFixed(element),
            }
        }
        BaseDamage::Variable => DescriptorProfile::Variable,
        BaseDamage::Duplicate | BaseDamage::NoMarker => DescriptorProfile::Unknown,
    }
}

pub(in crate::catalog) fn item_damage_profile(item: &[u8], bucket_hash: u64) -> ItemDamageProfile {
    let descriptor = descriptor_profile(item);
    match bucket_hash {
        KINETIC_BUCKET | ENERGY_BUCKET | POWER_BUCKET => match descriptor {
            DescriptorProfile::Empty => {
                ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None }
            }
            profile => profile.into_item_profile(),
        },
        _ => descriptor.into_item_profile(),
    }
}

pub(in crate::catalog) fn resolve_default_plug_damage_profile(
    base: ItemDamageProfile,
    default_plugs: impl IntoIterator<Item = ItemDamageProfile>,
) -> ItemDamageProfile {
    let mut plug_damage = None;
    for profile in default_plugs {
        if profile == ItemDamageProfile::Variable {
            return ItemDamageProfile::Variable;
        }
        let candidate = match profile {
            ItemDamageProfile::ModernFixed { damage_type }
            | ItemDamageProfile::LegacyFixed { damage_type } => Some(damage_type),
            _ => None,
        };
        if let Some(candidate) = candidate {
            if plug_damage.is_some_and(|selected| selected != candidate) {
                return ItemDamageProfile::Variable;
            }
            plug_damage = Some(candidate);
        }
    }
    match base {
        ItemDamageProfile::PlugOrEmptyAmbiguous { .. } => ItemDamageProfile::PlugOrEmptyAmbiguous {
            damage_type: plug_damage,
        },
        ItemDamageProfile::ModernFixed { damage_type }
        | ItemDamageProfile::LegacyFixed { damage_type }
            if plug_damage.is_some_and(|plug| plug != damage_type) =>
        {
            ItemDamageProfile::Variable
        }
        profile => profile,
    }
}

impl super::super::Catalog {
    pub(crate) fn item_damage_type(&self, hash: u64) -> Option<ItemDamageType> {
        self.item_package_metadata(hash)?.damage_type
    }
}

#[cfg(test)]
mod tests;
