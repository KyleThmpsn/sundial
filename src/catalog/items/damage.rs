//! Package-backed weapon damage-type decoding.

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::investment_schema::{
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
// The Fundamentals plug carries these three rows together.
#[cfg(test)]
const VARIABLE_ELEMENT_PERK_INDICES: [u16; 3] = [462, 463, 464];

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
    use crate::native_weapon::{
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
mod tests {
    use std::path::PathBuf;

    use crate::{catalog::Catalog, test_support::TestDirectory};

    use super::*;

    fn item_with_damage_perks(perks: &[u16]) -> Vec<u8> {
        let mut item = vec![0_u8; 0xD0 + perks.len() * ITEM_DAMAGE_PERK_ROW_SIZE];
        item[0x70..0x78].copy_from_slice(&0x20_i64.to_le_bytes());
        item[0x8C..0x90].copy_from_slice(&0x8080_77B9_u32.to_le_bytes());
        item[0xA0..0xA8].copy_from_slice(&(perks.len() as u64).to_le_bytes());
        item[0xA8..0xB0].copy_from_slice(&0x18_i64.to_le_bytes());
        item[0xC0..0xC8].copy_from_slice(&(perks.len() as u64).to_le_bytes());
        item[0xC8..0xCC].copy_from_slice(&ITEM_DAMAGE_PERK_CLASS.to_le_bytes());
        for (index, perk) in perks.iter().copied().enumerate() {
            let row = 0xD0 + index * ITEM_DAMAGE_PERK_ROW_SIZE;
            item[row..row + 2].copy_from_slice(&perk.to_le_bytes());
            item[row + 2..row + ITEM_DAMAGE_PERK_ROW_SIZE].fill(0xFF);
        }
        item
    }

    #[test]
    fn fixed_damage_decoding_does_not_depend_on_equipment_bucket() {
        for bucket in [KINETIC_BUCKET, ENERGY_BUCKET, POWER_BUCKET] {
            assert_eq!(
                item_damage_profile(
                    &item_with_damage_perks(&[MODERN_ARC_DAMAGE_PERK_INDEX]),
                    bucket
                ),
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Arc
                }
            );
            assert_eq!(
                item_damage_profile(
                    &item_with_damage_perks(&[LEGACY_VOID_DAMAGE_PERK_INDEX]),
                    bucket
                ),
                ItemDamageProfile::LegacyFixed {
                    damage_type: ItemDamageType::Void
                }
            );
        }
    }

    #[test]
    fn package_profiles_distinguish_supported_topologies() {
        let empty = item_with_damage_perks(&[]);
        assert_eq!(
            item_damage_profile(&empty, KINETIC_BUCKET),
            ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None }
        );
        assert_eq!(
            item_damage_profile(&empty, ENERGY_BUCKET),
            ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None }
        );
        for (perk, expected) in [
            (
                LEGACY_VOID_DAMAGE_PERK_INDEX,
                ItemDamageProfile::LegacyFixed {
                    damage_type: ItemDamageType::Void,
                },
            ),
            (
                MODERN_ARC_DAMAGE_PERK_INDEX,
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Arc,
                },
            ),
            (
                MODERN_SOLAR_DAMAGE_PERK_INDEX,
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Solar,
                },
            ),
            (
                MODERN_VOID_DAMAGE_PERK_INDEX,
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Void,
                },
            ),
        ] {
            assert_eq!(
                item_damage_profile(&item_with_damage_perks(&[perk]), ENERGY_BUCKET),
                expected
            );
        }
        assert_eq!(
            item_damage_profile(
                &item_with_damage_perks(&VARIABLE_ELEMENT_PERK_INDICES),
                ENERGY_BUCKET
            ),
            ItemDamageProfile::Variable
        );
    }

    #[test]
    fn fixed_element_can_share_the_array_with_other_sandbox_perks() {
        assert_eq!(
            item_damage_profile(
                &item_with_damage_perks(&[LEGACY_SOLAR_DAMAGE_PERK_INDEX, 1048]),
                ENERGY_BUCKET
            ),
            ItemDamageProfile::LegacyFixed {
                damage_type: ItemDamageType::Solar
            }
        );
        for perks in [vec![1048], vec![MODERN_SOLAR_DAMAGE_PERK_INDEX; 2]] {
            assert_eq!(
                item_damage_profile(&item_with_damage_perks(&perks), ENERGY_BUCKET),
                ItemDamageProfile::Unknown
            );
        }
        assert_eq!(
            item_damage_profile(
                &item_with_damage_perks(&[
                    MODERN_SOLAR_DAMAGE_PERK_INDEX,
                    MODERN_ARC_DAMAGE_PERK_INDEX
                ]),
                ENERGY_BUCKET
            ),
            ItemDamageProfile::Variable
        );
        let mut truncated = item_with_damage_perks(&[MODERN_SOLAR_DAMAGE_PERK_INDEX, 1048]);
        truncated.pop();
        assert_eq!(
            item_damage_profile(&truncated, ENERGY_BUCKET),
            ItemDamageProfile::Unknown
        );
    }

    #[test]
    fn default_plugs_refine_empty_and_variable_profiles() {
        let base = ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None };
        assert_eq!(
            resolve_default_plug_damage_profile(
                base,
                [ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Void,
                }]
            ),
            ItemDamageProfile::PlugOrEmptyAmbiguous {
                damage_type: Some(ItemDamageType::Void)
            }
        );
        assert_eq!(
            resolve_default_plug_damage_profile(
                ItemDamageProfile::LegacyFixed {
                    damage_type: ItemDamageType::Void,
                },
                [ItemDamageProfile::Variable]
            ),
            ItemDamageProfile::Variable
        );
    }

    #[test]
    fn inventory_bucket_and_equipment_slot_decode_independently() {
        assert_eq!(
            ItemWeaponInventorySlot::from_bucket_hash(ENERGY_BUCKET),
            Some(ItemWeaponInventorySlot::Energy)
        );
        assert_eq!(
            ItemWeaponInventorySlot::from_equipment_slot(7),
            Some(ItemWeaponInventorySlot::Kinetic)
        );
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
    fn supported_shadowkeep_build_reads_package_proved_damage_profiles() {
        let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
        let temp = TestDirectory::new("damage-profiles");
        let catalog = Catalog::load_or_scan_with_progress(
            &install,
            temp.0.join("catalog.json"),
            true,
            |_| {},
        )
        .unwrap();

        let cases: [(u32, ItemDamageProfile, ItemWeaponInventorySlot); 8] = [
            (
                0x5E73_BEF2, // Hush: Solar marker plus a non-elemental base perk.
                ItemDamageProfile::LegacyFixed {
                    damage_type: ItemDamageType::Solar,
                },
                ItemWeaponInventorySlot::Energy,
            ),
            (
                0xA25B_8F8F,
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Arc,
                },
                ItemWeaponInventorySlot::Energy,
            ),
            (
                0xEE06_B019,
                ItemDamageProfile::KineticEmpty,
                ItemWeaponInventorySlot::Kinetic,
            ),
            (
                0xC9C7_FC81,
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Void,
                },
                ItemWeaponInventorySlot::Energy,
            ),
            (
                0x23F4_BF01,
                ItemDamageProfile::ModernFixed {
                    damage_type: ItemDamageType::Void,
                },
                ItemWeaponInventorySlot::Power,
            ),
            (
                0xE042_B104,
                ItemDamageProfile::PlugOrEmptyAmbiguous {
                    damage_type: Some(ItemDamageType::Arc),
                },
                ItemWeaponInventorySlot::Energy,
            ),
            (
                0x395D_3E2F,
                ItemDamageProfile::PlugOrEmptyAmbiguous {
                    damage_type: Some(ItemDamageType::Void),
                },
                ItemWeaponInventorySlot::Energy,
            ),
            (
                0xF5DE_4480,
                ItemDamageProfile::Variable,
                ItemWeaponInventorySlot::Energy,
            ),
        ];
        for (hash, profile, slot) in cases {
            let metadata = catalog.item_package_metadata(u64::from(hash)).unwrap();
            assert_eq!(metadata.damage_profile, profile, "item 0x{hash:08X}");
            assert_eq!(
                metadata.weapon_inventory_slot,
                Some(slot),
                "item 0x{hash:08X}"
            );
        }
    }
}
