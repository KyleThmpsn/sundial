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
    // Borealis ships the other Arc effect. Both compile to one action, so reading it as
    // a fixed Void weapon was wrong: the legacy Void marker it also carries is only the
    // unwritten default the effects select between.
    assert_eq!(
        item_damage_profile(
            &item_with_damage_perks(&BOREALIS_VARIABLE_ELEMENT_PERK_INDICES),
            ENERGY_BUCKET
        ),
        ItemDamageProfile::Variable
    );
    // One leg alone is not enough. A weapon carrying only the Arc effect keeps whatever
    // fixed marker it has.
    assert_ne!(
        item_damage_profile(&item_with_damage_perks(&[461]), ENERGY_BUCKET),
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
#[ignore = "requires SUNDIAL_INSTALL pointing to the supported Shadowkeep build"]
fn supported_shadowkeep_build_reads_package_proved_damage_profiles() {
    let install = PathBuf::from(std::env::var("SUNDIAL_INSTALL").unwrap());
    let temp = TestDirectory::new("damage-profiles");
    let catalog =
        Catalog::load_or_scan_with_progress(&install, temp.0.join("catalog.json"), true, |_| {})
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
