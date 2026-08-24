use serde::{Deserialize, Serialize};

mod abilities;
mod damage;
mod inventory;
mod investment;
mod perks;
mod quality;
mod scan;
mod sockets;

pub(in crate::catalog) use abilities::scan_ability_displays;
pub(crate) use abilities::{AbilityChoice, AbilityOptions, AttunementChoice};
pub(crate) use damage::ItemDamageType;
pub(in crate::catalog) use damage::item_damage_type;
pub(in crate::catalog) use inventory::scan_inventory_bucket_descriptors;
pub(crate) use inventory::{
    InventoryDefinition, InventoryMetadata, InventoryScope, ItemStackability,
};
pub(in crate::catalog) use investment::scan_stat_definitions;
pub(crate) use investment::{ItemInvestmentStat, ItemStatDefinition};
pub(crate) use perks::{ItemIntrinsicPerk, SandboxPerkDefinition};
pub(in crate::catalog) use perks::{
    index_intrinsic_perk_references, scan_sandbox_perk_hashes,
    scan_sandbox_perk_runtime_definitions,
};
pub(in crate::catalog) use scan::{ItemScan, ItemScanContext, scan_items};
#[cfg(test)]
pub(in crate::catalog) use scan::{attach_item_objective_owners, item_scan_progress_stride};
pub(crate) use sockets::SocketDef;
pub(in crate::catalog) use sockets::{
    GearKind, build_gear_type_options, format_plug_label, intern_socket_pools, sort_plug_options,
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
}

/// Structural metadata read directly from the installed inventory item definition table.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ItemPackageMetadata {
    pub definition_index: u32,
    pub definition_tag: u32,
    pub plug_category_hash: Option<u64>,
    #[serde(default)]
    pub rarity: ItemRarity,
    #[serde(default)]
    pub power_cap: Option<u16>,
    #[serde(default)]
    pub damage_type: Option<ItemDamageType>,
    #[serde(default)]
    pub investment_stats: Vec<ItemInvestmentStat>,
    #[serde(default)]
    pub intrinsic_perks: Vec<ItemIntrinsicPerk>,
}
