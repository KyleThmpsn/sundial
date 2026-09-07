use serde::{Deserialize, Serialize};

mod abilities;
mod ammo;
mod damage;
mod inventory;
mod investment;
mod perks;
mod quality;
mod scan;
mod sockets;

#[cfg(test)]
pub(crate) use abilities::AttunementChoice;
pub(in crate::catalog) use abilities::scan_ability_displays;
pub(crate) use abilities::{AbilityChoice, AbilityOptions};
pub(crate) use ammo::ItemWeaponAmmoType;
pub(in crate::catalog) use ammo::item_weapon_ammo_type;
pub(crate) use damage::is_weapon_bucket;
pub(crate) use damage::{ItemDamageProfile, ItemDamageType, ItemWeaponInventorySlot};
pub(in crate::catalog) use damage::{item_damage_profile, resolve_default_plug_damage_profile};
#[cfg(test)]
pub(crate) use inventory::ItemStackability;
pub(in crate::catalog) use inventory::scan_inventory_bucket_descriptors;
pub(crate) use inventory::{InventoryDefinition, InventoryMetadata, InventoryScope};
pub(crate) use investment::{
    InvestmentStatDisplayPoint, ItemInvestmentStat, ItemStatDefinition, ItemStatGroup,
    format_in_game_investment_stat, interpolate_investment_stat_display,
};
pub(in crate::catalog) use investment::{scan_stat_definitions, scan_stat_groups};
pub(crate) use perks::ItemSandboxPerk;
pub(in crate::catalog) use perks::scan_sandbox_perk_catalog;
pub(crate) use quality::PowerCapDefinition;
pub(in crate::catalog) use quality::{item_power_cap, scan_power_cap_definitions};
pub(in crate::catalog) use scan::{ItemScan, ItemScanContext, scan_items};
#[cfg(test)]
pub(in crate::catalog) use scan::{attach_item_objective_owners, item_scan_progress_stride};
pub(crate) use sockets::SocketDef;
pub(in crate::catalog) use sockets::{
    GearKind, build_gear_type_options, build_socket_type_options, format_plug_label,
    intern_socket_pools, sort_plug_options,
};
#[cfg(test)]
pub(in crate::catalog) use sockets::{
    infer_socket_label, infer_socket_plug_types, socket_label_for_plug,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ItemDef {
    pub hash: u64,
    pub name: String,
    pub type_name: String,
    pub bucket_hash: u64,
    pub class_type: u64,
    pub default_plugs: Vec<Option<String>>,
    pub sockets: Vec<SocketDef>,
    #[serde(default)]
    pub abilities: AbilityOptions,
}

const WEAPON_ORNAMENT_TYPE_NAME: &str = "Weapon Ornament";

/// Weapon ornaments occupy weapon buckets but do not contain an authorable weapon definition.
pub(crate) fn is_authorable_weapon_item(item: &ItemDef) -> bool {
    is_weapon_bucket(item.bucket_hash)
        && !item
            .type_name
            .trim()
            .eq_ignore_ascii_case(WEAPON_ORNAMENT_TYPE_NAME)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemRarity {
    #[default]
    Unknown,
    Common,
    Uncommon,
    Rare,
    Legendary,
    Exotic,
}

impl ItemRarity {
    pub(crate) const ALL: [Self; 5] = [
        Self::Exotic,
        Self::Legendary,
        Self::Rare,
        Self::Uncommon,
        Self::Common,
    ];

    pub(crate) const fn from_package_value(value: u8) -> Self {
        match value {
            1 => Self::Common,
            2 => Self::Uncommon,
            3 => Self::Rare,
            4 => Self::Legendary,
            5 => Self::Exotic,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Legendary => "Legendary",
            Self::Exotic => "Exotic",
        }
    }

    pub(crate) const fn package_value(self) -> Option<u8> {
        match self {
            Self::Unknown => None,
            Self::Common => Some(1),
            Self::Uncommon => Some(2),
            Self::Rare => Some(3),
            Self::Legendary => Some(4),
            Self::Exotic => Some(5),
        }
    }
}

/// Structural metadata read directly from the installed inventory item definition table.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ItemPackageMetadata {
    pub definition_index: u32,
    pub definition_tag: u32,
    #[serde(default)]
    pub definition_size: Option<u32>,
    #[serde(default)]
    pub string_definition_tag: Option<u32>,
    #[serde(default)]
    pub stat_group_index: Option<u16>,
    #[serde(default)]
    pub icon_container_tag: Option<u32>,
    #[serde(default)]
    pub plug_category_hash: Option<u64>,
    #[serde(default)]
    pub equipment_slot: Option<u8>,
    #[serde(default)]
    pub socket_entry_list_index: Option<u16>,
    #[serde(default)]
    pub roll_set_index: Option<u16>,
    #[serde(default)]
    pub linked_plug_index: Option<u16>,
    #[serde(default)]
    pub linked_plug_hash: Option<u64>,
    /// Native weapon sandbox-pattern row from the item's translation block.
    #[serde(default, alias = "gear_art_index")]
    pub weapon_pattern_index: Option<u16>,
    /// Native animation group; None means compatibility has not been established.
    #[serde(default)]
    pub weapon_translation_group: Option<u32>,
    #[serde(default)]
    pub art_arrangement_indices: [Option<u16>; 4],
    /// Complete ordered translation-art rows, including class-specific variants.
    #[serde(default)]
    pub art_arrangements: Vec<ItemArtArrangement>,
    #[serde(default)]
    pub render_overrides: Vec<ItemRenderOverride>,
    /// Complete ordered custom, default, and locked dye-reference rows, including disabled rows.
    #[serde(default, alias = "translation_material_rows")]
    pub translation_dye_rows: [Vec<ItemRenderOverride>; 3],
    #[serde(default)]
    pub rarity: ItemRarity,
    #[serde(default)]
    pub power_cap: Option<u32>,
    /// Complete ordered version-group rows from the native item quality block.
    #[serde(default)]
    pub power_cap_groups: Vec<u16>,
    #[serde(default)]
    pub damage_type: Option<ItemDamageType>,
    #[serde(default)]
    pub damage_profile: ItemDamageProfile,
    #[serde(default)]
    pub weapon_inventory_slot: Option<ItemWeaponInventorySlot>,
    /// Primary/Special/Heavy client classification decoded from the item-string definition.
    #[serde(default)]
    pub weapon_ammo_type: Option<ItemWeaponAmmoType>,
    #[serde(default)]
    pub investment_stats: Vec<ItemInvestmentStat>,
    #[serde(default)]
    pub sandbox_perks: Vec<ItemSandboxPerk>,
    /// Ordered native item-trait definition indices from the root `traits` array.
    #[serde(default)]
    pub trait_indices: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemArtArrangement {
    pub character_class: i8,
    pub arrangement: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ItemRenderOverride {
    pub stage: u8,
    pub key: i8,
    pub value: u16,
}

#[cfg(test)]
mod tests {
    use super::{AbilityOptions, ItemDef, is_authorable_weapon_item};

    #[test]
    fn weapon_ornaments_are_not_authoring_donors() {
        let mut item = ItemDef {
            hash: 1,
            name: "Test".to_owned(),
            type_name: "Auto Rifle".to_owned(),
            bucket_hash: 1_498_876_634,
            class_type: 0,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: AbilityOptions::default(),
        };
        assert!(is_authorable_weapon_item(&item));

        item.type_name = " Weapon Ornament ".to_owned();
        assert!(!is_authorable_weapon_item(&item));
    }
}
