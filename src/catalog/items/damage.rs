//! Package-backed weapon damage-type decoding.

use serde::{Deserialize, Serialize};

use super::{
    super::package::{array_at, i32_at},
    investment::item_investment_resource,
};

const KINETIC_BUCKET: u64 = 1_498_876_634;
const ENERGY_BUCKET: u64 = 2_465_295_065;
const POWER_BUCKET: u64 = 953_998_645;
const ITEM_DAMAGE_PERK_DESCRIPTOR_OFFSET: usize = 16;
const ITEM_DAMAGE_PERK_CLASS: u32 = 0x8080_77BC;
const ITEM_DAMAGE_PERK_ROW_SIZE: usize = 24;
const ARC_DAMAGE_PERK_INDEX: i32 = 83;
const SOLAR_DAMAGE_PERK_INDEX: i32 = 84;
const VOID_DAMAGE_PERK_INDEX: i32 = 85;

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

    pub(crate) const fn sandbox_perk_definition_index(self) -> Option<u16> {
        match self {
            Self::Kinetic => None,
            Self::Arc => Some(ARC_DAMAGE_PERK_INDEX as u16),
            Self::Solar => Some(SOLAR_DAMAGE_PERK_INDEX as u16),
            Self::Void => Some(VOID_DAMAGE_PERK_INDEX as u16),
        }
    }

    const fn from_package_perk_index(index: i32) -> Option<Self> {
        match index {
            ARC_DAMAGE_PERK_INDEX => Some(Self::Arc),
            SOLAR_DAMAGE_PERK_INDEX => Some(Self::Solar),
            VOID_DAMAGE_PERK_INDEX => Some(Self::Void),
            _ => None,
        }
    }
}

impl super::super::Catalog {
    pub(crate) fn item_damage_type(&self, hash: u64) -> Option<ItemDamageType> {
        self.item_package_metadata(hash)?.damage_type
    }
}

pub(in crate::catalog) fn item_damage_type(
    item: &[u8],
    bucket_hash: u64,
) -> Option<ItemDamageType> {
    if bucket_hash == KINETIC_BUCKET {
        return Some(ItemDamageType::Kinetic);
    }
    if !matches!(bucket_hash, ENERGY_BUCKET | POWER_BUCKET) {
        return None;
    }

    let resource = item_investment_resource(item)?;
    let descriptor = resource.checked_add(ITEM_DAMAGE_PERK_DESCRIPTOR_OFFSET)?;
    let (count, rows, class) = array_at(item, descriptor).ok()?;
    if class != ITEM_DAMAGE_PERK_CLASS || count > 8 {
        return None;
    }

    let mut damage_type = None;
    for index in 0..count {
        let row = rows.checked_add(index.checked_mul(ITEM_DAMAGE_PERK_ROW_SIZE)?)?;
        let Some(candidate) = ItemDamageType::from_package_perk_index(i32_at(item, row).ok()?)
        else {
            continue;
        };
        if damage_type.is_some_and(|selected| selected != candidate) {
            return None;
        }
        damage_type = Some(candidate);
    }
    damage_type
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{catalog::Catalog, test_support::TestDirectory};

    use super::*;

    fn item_with_damage_perks(perks: &[i32]) -> Vec<u8> {
        let mut item = vec![0_u8; 0xD0 + perks.len() * ITEM_DAMAGE_PERK_ROW_SIZE];
        item[0x70..0x78].copy_from_slice(&0x20_i64.to_le_bytes());
        item[0x8C..0x90].copy_from_slice(&0x8080_77B9_u32.to_le_bytes());
        item[0xA0..0xA8].copy_from_slice(&(perks.len() as u64).to_le_bytes());
        item[0xA8..0xB0].copy_from_slice(&0x18_i64.to_le_bytes());
        item[0xC0..0xC8].copy_from_slice(&(perks.len() as u64).to_le_bytes());
        item[0xC8..0xCC].copy_from_slice(&ITEM_DAMAGE_PERK_CLASS.to_le_bytes());
        for (index, perk) in perks.iter().copied().enumerate() {
            let row = 0xD0 + index * ITEM_DAMAGE_PERK_ROW_SIZE;
            item[row..row + 4].copy_from_slice(&perk.to_le_bytes());
        }
        item
    }

    #[test]
    fn damage_type_uses_package_bucket_and_damage_perks() {
        let item = item_with_damage_perks(&[]);
        assert_eq!(
            item_damage_type(&item, KINETIC_BUCKET),
            Some(ItemDamageType::Kinetic)
        );
        assert_eq!(item_damage_type(&item, ENERGY_BUCKET), None);

        for (perk, expected) in [
            (ARC_DAMAGE_PERK_INDEX, ItemDamageType::Arc),
            (SOLAR_DAMAGE_PERK_INDEX, ItemDamageType::Solar),
            (VOID_DAMAGE_PERK_INDEX, ItemDamageType::Void),
        ] {
            assert_eq!(
                item_damage_type(&item_with_damage_perks(&[perk]), ENERGY_BUCKET),
                Some(expected)
            );
        }
    }

    #[test]
    fn variable_element_weapons_are_not_labeled_as_fixed() {
        let item = item_with_damage_perks(&[
            ARC_DAMAGE_PERK_INDEX,
            SOLAR_DAMAGE_PERK_INDEX,
            VOID_DAMAGE_PERK_INDEX,
        ]);
        assert_eq!(item_damage_type(&item, ENERGY_BUCKET), None);
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
    fn supported_shadowkeep_build_reads_weapon_damage_types_from_packages() {
        let install = PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap());
        let temp = TestDirectory::new("damage-types");
        let catalog = Catalog::load_or_scan_with_progress(
            &install,
            temp.0.join("catalog.json"),
            true,
            |_| {},
        )
        .unwrap();

        for (hash, expected) in [
            (347_366_834, ItemDamageType::Kinetic),
            (3_089_417_789, ItemDamageType::Arc),
            (2_907_129_557, ItemDamageType::Solar),
            (3_549_153_978, ItemDamageType::Void),
        ] {
            assert_eq!(catalog.item_damage_type(hash), Some(expected));
        }
    }
}
