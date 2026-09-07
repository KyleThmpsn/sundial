//! Data-only donor, stat and socket contracts exposed to authoring consumers.
use crate::catalog::{
    InvestmentStatDisplayPoint, ItemDamageProfile, ItemDamageType, ItemRarity, ItemWeaponAmmoType,
    ItemWeaponInventorySlot, format_in_game_investment_stat, interpolate_investment_stat_display,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponDamageType {
    Kinetic,
    Arc,
    Solar,
    Void,
}

impl WeaponDamageType {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Kinetic => "Kinetic",
            Self::Arc => "Arc",
            Self::Solar => "Solar",
            Self::Void => "Void",
        }
    }
}

impl From<ItemDamageType> for WeaponDamageType {
    fn from(value: ItemDamageType) -> Self {
        match value {
            ItemDamageType::Kinetic => Self::Kinetic,
            ItemDamageType::Arc => Self::Arc,
            ItemDamageType::Solar => Self::Solar,
            ItemDamageType::Void => Self::Void,
        }
    }
}

/// Native location used by a stock weapon to contribute its fixed elemental sandbox perk.
///
/// These families are not interchangeable. Older definitions store the marker on the item using
/// rows 83 through 85, newer definitions use rows 449 through 451, and some definitions delegate
/// it to the default plug in their dedicated type-68 socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WeaponDamageCarrierFamily {
    LegacyFixed,
    ModernFixed,
    PlugDriven,
}

impl WeaponDamageCarrierFamily {
    /// Returns the stock base-item sandbox-perk index for this carrier family and element.
    #[must_use]
    pub const fn base_sandbox_perk_index(self, damage_type: WeaponDamageType) -> Option<u16> {
        use crate::investment_schema::{
            LEGACY_ARC_DAMAGE_PERK_INDEX, LEGACY_SOLAR_DAMAGE_PERK_INDEX,
            LEGACY_VOID_DAMAGE_PERK_INDEX, MODERN_ARC_DAMAGE_PERK_INDEX,
            MODERN_SOLAR_DAMAGE_PERK_INDEX, MODERN_VOID_DAMAGE_PERK_INDEX,
        };
        match (self, damage_type) {
            (_, WeaponDamageType::Kinetic) | (Self::PlugDriven, _) => None,
            (Self::LegacyFixed, WeaponDamageType::Arc) => Some(LEGACY_ARC_DAMAGE_PERK_INDEX),
            (Self::LegacyFixed, WeaponDamageType::Solar) => Some(LEGACY_SOLAR_DAMAGE_PERK_INDEX),
            (Self::LegacyFixed, WeaponDamageType::Void) => Some(LEGACY_VOID_DAMAGE_PERK_INDEX),
            (Self::ModernFixed, WeaponDamageType::Arc) => Some(MODERN_ARC_DAMAGE_PERK_INDEX),
            (Self::ModernFixed, WeaponDamageType::Solar) => Some(MODERN_SOLAR_DAMAGE_PERK_INDEX),
            (Self::ModernFixed, WeaponDamageType::Void) => Some(MODERN_VOID_DAMAGE_PERK_INDEX),
        }
    }

    /// Returns the stock default-plug item index for a type-68 carrier.
    #[must_use]
    pub const fn default_plug_item_index(self, damage_type: WeaponDamageType) -> Option<u16> {
        use crate::investment_schema::{
            ARC_DAMAGE_PLUG_ITEM_INDEX, SOLAR_DAMAGE_PLUG_ITEM_INDEX, VOID_DAMAGE_PLUG_ITEM_INDEX,
        };
        if !matches!(self, Self::PlugDriven) {
            return None;
        }
        match damage_type {
            WeaponDamageType::Kinetic => None,
            WeaponDamageType::Arc => Some(ARC_DAMAGE_PLUG_ITEM_INDEX),
            WeaponDamageType::Solar => Some(SOLAR_DAMAGE_PLUG_ITEM_INDEX),
            WeaponDamageType::Void => Some(VOID_DAMAGE_PLUG_ITEM_INDEX),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponDamageProfile {
    KineticEmpty,
    ModernFixed(WeaponDamageType),
    LegacyFixed(WeaponDamageType),
    PlugOrEmptyAmbiguous(Option<WeaponDamageType>),
    Variable,
    Unknown,
}

impl From<ItemDamageProfile> for WeaponDamageProfile {
    fn from(value: ItemDamageProfile) -> Self {
        match value {
            ItemDamageProfile::KineticEmpty => Self::KineticEmpty,
            ItemDamageProfile::ModernFixed { damage_type } => Self::ModernFixed(damage_type.into()),
            ItemDamageProfile::LegacyFixed { damage_type } => Self::LegacyFixed(damage_type.into()),
            ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type } => {
                Self::PlugOrEmptyAmbiguous(damage_type.map(WeaponDamageType::from))
            }
            ItemDamageProfile::Variable => Self::Variable,
            ItemDamageProfile::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponInventorySlot {
    Kinetic,
    Energy,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponAmmoType {
    Primary,
    Special,
    Heavy,
}

impl WeaponAmmoType {
    pub const ALL: [Self; 3] = [Self::Primary, Self::Special, Self::Heavy];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Special => "Special",
            Self::Heavy => "Heavy",
        }
    }
}

impl From<ItemWeaponAmmoType> for WeaponAmmoType {
    fn from(value: ItemWeaponAmmoType) -> Self {
        match value {
            ItemWeaponAmmoType::Primary => Self::Primary,
            ItemWeaponAmmoType::Special => Self::Special,
            ItemWeaponAmmoType::Heavy => Self::Heavy,
        }
    }
}

impl WeaponInventorySlot {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Kinetic => "Kinetic",
            Self::Energy => "Energy",
            Self::Power => "Power",
        }
    }
}

impl From<ItemWeaponInventorySlot> for WeaponInventorySlot {
    fn from(value: ItemWeaponInventorySlot) -> Self {
        match value {
            ItemWeaponInventorySlot::Kinetic => Self::Kinetic,
            ItemWeaponInventorySlot::Energy => Self::Energy,
            ItemWeaponInventorySlot::Power => Self::Power,
        }
    }
}

/// The rarity tier decoded from an installed item definition.
///
/// Authoring may explicitly replace the verified tier byte, while donor-backed socket, icon,
/// quality-presentation, and collection data remain independently selected inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponRarity {
    Unknown,
    Common,
    Uncommon,
    Rare,
    Legendary,
    Exotic,
}

impl WeaponRarity {
    #[must_use]
    pub const fn label(self) -> &'static str {
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

impl From<ItemRarity> for WeaponRarity {
    fn from(value: ItemRarity) -> Self {
        match value {
            ItemRarity::Unknown => Self::Unknown,
            ItemRarity::Common => Self::Common,
            ItemRarity::Uncommon => Self::Uncommon,
            ItemRarity::Rare => Self::Rare,
            ItemRarity::Legendary => Self::Legendary,
            ItemRarity::Exotic => Self::Exotic,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponDonorSummary {
    pub hash: u32,
    pub name: String,
    pub type_name: String,
    pub bucket_hash: u64,
    pub collection_backed: bool,
    pub power_cap: Option<u32>,
    pub damage_type: Option<WeaponDamageType>,
    pub inventory_slot: Option<WeaponInventorySlot>,
    pub ammo_type: Option<WeaponAmmoType>,
    /// Native weapon sandbox-pattern row referenced by this item's translation block.
    pub weapon_pattern_index: Option<u16>,
    pub weapon_translation_group: Option<u32>,
    /// Installed stat-group row referenced by this item's string definition.
    pub stat_group_index: Option<u16>,
    pub damage_profile: WeaponDamageProfile,
    pub rarity: WeaponRarity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponStatDisplayPoint {
    pub investment_value: i32,
    pub display_value: i32,
}

impl InvestmentStatDisplayPoint for WeaponStatDisplayPoint {
    fn investment_value(&self) -> i32 {
        self.investment_value
    }

    fn display_value(&self) -> i32 {
        self.display_value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponInvestmentStat {
    pub definition_index: u16,
    pub definition_hash: Option<u32>,
    pub name: String,
    pub value: i32,
    /// Lower raw input bound derived from the donor's stat-group interpolation domain.
    pub minimum_value: Option<i32>,
    /// Upper raw input bound stored as `maximumValue` in the donor's stat group.
    pub maximum_value: Option<i32>,
    pub display_as_numeric: bool,
    pub is_linear: bool,
    pub display_interpolation: Vec<WeaponStatDisplayPoint>,
}

impl WeaponInvestmentStat {
    #[must_use]
    pub fn value_range(&self) -> Option<(i32, i32)> {
        self.minimum_value
            .zip(self.maximum_value)
            .filter(|(minimum, maximum)| minimum <= maximum)
    }

    /// Returns exactly what the historical client display conversion produces. Stats without a
    /// scaled display row remain useful raw values instead of appearing as unknown.
    #[must_use]
    pub fn in_game_display_value(&self, investment_value: i32) -> i32 {
        interpolate_investment_stat_display(
            &self.display_interpolation,
            self.is_linear,
            investment_value,
        )
        .unwrap_or(investment_value)
    }

    /// Formats the in-game value using the unit used by the Shadowkeep weapon UI when known.
    #[must_use]
    pub fn in_game_display_label(&self, investment_value: i32) -> String {
        let display_value = self.in_game_display_value(investment_value);
        format_in_game_investment_stat(self.definition_hash.map(u64::from), display_value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponSocket {
    pub index: usize,
    pub socket_type: u16,
    pub label: String,
    pub native_default: Option<u32>,
    /// The donor's complete embedded member list in native package order.
    ///
    /// This is a curated starting column, not the full compatibility pool used by the plug
    /// picker. Empty means that no complete embedded package list was decoded.
    pub ordered_embedded_choices: Vec<u32>,
    /// Maximum number of ordered choices the package compiler can safely author for this lane.
    /// A value of zero marks a disabled socket.
    pub max_authored_choices: usize,
    /// Number of entries in the broader, normalized compatibility pool used by the plug picker.
    pub compatible_plug_count: usize,
    pub reusable_plug_set_index: Option<u16>,
    pub randomized_plug_set_index: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponDonor {
    pub summary: WeaponDonorSummary,
    /// Complete ordered version-group values from the native quality block.
    pub power_cap_groups: Vec<u16>,
    /// Equipment-slot presentation decoded independently from the inventory bucket.
    pub equipment_slot: Option<WeaponInventorySlot>,
    pub sockets: Vec<WeaponSocket>,
    pub investment_stats: Vec<WeaponInvestmentStat>,
    /// Installed weapon-stat definitions that are structurally authorable but absent from this
    /// donor. Every decoded installed stat-definition row that fits the native item field is
    /// exposed, including rows not referenced by a stock weapon.
    pub addable_investment_stats: Vec<WeaponInvestmentStat>,
    /// Active sandbox-perk rows applied directly by the base item before socket plugs.
    pub base_sandbox_perks: Vec<u16>,
    /// Ordered native item-trait definition indices.
    pub trait_indices: Vec<u16>,
    /// Native inline inventory quantity bound. Stock instanced weapons normally use one.
    pub max_stack_size: Option<u32>,
    /// Native socket-entry-list row selected by the item's talent-grid holder.
    pub socket_entry_list_index: Option<u16>,
    /// Native plug-category hash selected by the item's plug metadata.
    pub plug_category_hash: Option<u32>,
    /// Native randomized-roll-set row selected by the item's plug metadata.
    pub roll_set_index: Option<u16>,
    /// Native linked-item row selected by the item's linked-plug metadata.
    pub linked_plug_index: Option<u16>,
    /// Resolved installed item hash for [`Self::linked_plug_index`], when available.
    pub linked_plug_hash: Option<u32>,
    /// Complete ordered native translation-art rows from the selected weapon definition.
    pub art_arrangements: Vec<WeaponArtArrangement>,
    /// Complete ordered custom, default, and locked dye-reference rows.
    pub render_dye_rows: [Vec<WeaponDyeReference>; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponArtArrangement {
    pub character_class: i8,
    pub arrangement: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponDyeReference {
    pub channel_index: i8,
    pub dye_reference_index: u16,
}

/// One sandbox-perk index declared by an installed item or plug, with a display representative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponSandboxPerkChoice {
    pub perk_index: u16,
    pub representative_hash: u32,
    pub representative_name: String,
    pub representative_type_name: String,
}

/// One installed item-trait definition available to the native item trait-index array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponTraitChoice {
    pub trait_index: u16,
    pub hash: u32,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerCapChoice {
    pub version_group_start: u16,
    pub version_group_end: u16,
    pub authoring_version_group: u16,
    pub power_cap: u32,
    pub definition_hash: u32,
    pub version_group_range_label: String,
    pub picker_label: String,
}

/// Data-only compatible plug set decoded from one installed donor socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponSupportedPlugSet {
    pub socket_index: usize,
    pub plug_hashes: Vec<u32>,
    pub allows_disabled: bool,
}

/// One socket category observed in the installed package catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponSocketTypeChoice {
    pub socket_type: u16,
    pub label: String,
    pub compatible_plug_count: usize,
}

impl WeaponSocketTypeChoice {
    #[must_use]
    pub fn display_label(&self) -> String {
        if self.label.trim().is_empty() {
            format!("Type {}", self.socket_type)
        } else {
            format!("{} · {}", self.label, self.socket_type)
        }
    }
}

#[cfg(test)]
mod stat_display_tests {
    use super::*;

    #[test]
    fn historical_rpm_curve_translates_arc_logic_value() {
        let stat = WeaponInvestmentStat {
            definition_index: 14,
            definition_hash: Some(0xFF66_4809),
            name: "Rounds Per Minute".to_owned(),
            value: 80,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: true,
            is_linear: false,
            display_interpolation: vec![
                WeaponStatDisplayPoint {
                    investment_value: 0,
                    display_value: 360,
                },
                WeaponStatDisplayPoint {
                    investment_value: 20,
                    display_value: 450,
                },
                WeaponStatDisplayPoint {
                    investment_value: 80,
                    display_value: 600,
                },
                WeaponStatDisplayPoint {
                    investment_value: 100,
                    display_value: 720,
                },
            ],
        };

        assert_eq!(stat.in_game_display_value(80), 600);
        assert_eq!(stat.in_game_display_label(80), "600 RPM");
        assert_eq!(stat.in_game_display_value(50), 525);
        assert_eq!(stat.in_game_display_value(100), 720);
        assert_eq!(stat.in_game_display_label(100), "720 RPM");
        assert_eq!(stat.value_range(), Some((0, 100)));
    }

    #[test]
    fn identity_display_values_are_not_repeated() {
        let stat = WeaponInvestmentStat {
            definition_index: 0,
            definition_hash: None,
            name: "Identity".to_owned(),
            value: 50,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: false,
            is_linear: false,
            display_interpolation: vec![
                WeaponStatDisplayPoint {
                    investment_value: 0,
                    display_value: 0,
                },
                WeaponStatDisplayPoint {
                    investment_value: 100,
                    display_value: 100,
                },
            ],
        };

        assert_eq!(stat.in_game_display_value(50), 50);
        assert_eq!(stat.in_game_display_label(50), "50");
    }

    #[test]
    fn native_linear_flag_preserves_unlisted_values_but_not_exact_points() {
        let stat = WeaponInvestmentStat {
            definition_index: 0,
            definition_hash: None,
            name: "Linear".to_owned(),
            value: 50,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: true,
            is_linear: true,
            display_interpolation: vec![WeaponStatDisplayPoint {
                investment_value: 50,
                display_value: 600,
            }],
        };

        assert_eq!(stat.in_game_display_value(49), 49);
        assert_eq!(stat.in_game_display_value(50), 600);
    }

    #[test]
    fn native_curves_clamp_values_outside_the_decoded_domain() {
        let stat = WeaponInvestmentStat {
            definition_index: 0,
            definition_hash: None,
            name: "Bounded".to_owned(),
            value: 50,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: true,
            is_linear: false,
            display_interpolation: vec![
                WeaponStatDisplayPoint {
                    investment_value: 20,
                    display_value: 450,
                },
                WeaponStatDisplayPoint {
                    investment_value: 80,
                    display_value: 600,
                },
            ],
        };

        assert_eq!(stat.in_game_display_value(-1), 450);
        assert_eq!(stat.in_game_display_value(10), 450);
        assert_eq!(stat.in_game_display_value(90), 600);
    }

    #[test]
    fn unscaled_stats_display_their_raw_package_value() {
        let stat = WeaponInvestmentStat {
            definition_index: 32,
            definition_hash: None,
            name: "Zoom".to_owned(),
            value: 16,
            minimum_value: Some(0),
            maximum_value: Some(100),
            display_as_numeric: false,
            is_linear: false,
            display_interpolation: Vec::new(),
        };

        assert_eq!(stat.in_game_display_label(16), "16");
    }
}
