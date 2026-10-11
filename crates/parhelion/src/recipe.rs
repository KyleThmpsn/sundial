//! Versioned saved recipes for every supported authored item kind.

mod compile;
mod document;
mod error;
pub use error::RecipeError;
mod copy;
mod variant;

use std::{collections::BTreeMap, fmt, str::FromStr};

use crate::{
    AuthoredWeaponRarity, ItemKind, ModernDamageType, SwordProfileOverride, WeaponAmmoType,
    WeaponArtArrangementOverride, WeaponCloneIdentity, WeaponCloneOverrides, WeaponCloneSpec,
    WeaponCloneText, WeaponDyeReferenceOverride, WeaponIconEdit, WeaponInventorySlot,
    WeaponLocaleTextOverride, WeaponNumericInstruction, WeaponRawPayloadPatch,
    WeaponRenderGearDonorReference, WeaponSandboxPerkActionFloatOverride,
    WeaponSandboxPerkRuntimeOverride, WeaponSocketColumnOverride, WeaponSocketPlugVariantOverride,
    WeaponVariableDamage,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sundial::investment::MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES;
use sundial::package_authoring::{
    entity::WEAPON_BARREL_COMPONENT_KEY, runtime::WeaponRuntimeValueOverride,
};

pub const RECIPE_SCHEMA: u32 = 1;
pub const PARHELION_NAMESPACE_PREFIX: &str = "parhelion.";

/// How many containers deeper than other authored documents a recipe may nest. A subclass's
/// attunement node holds a custom perk eight containers down, the deepest a recipe holds one,
/// so a perk that loads on its own loads in any recipe, with four to spare.
pub(crate) const RECIPE_NESTING: usize = 8 + 4;

pub(crate) fn validate_parhelion_namespace(namespace: &str) -> Result<(), String> {
    let suffix = namespace
        .strip_prefix(PARHELION_NAMESPACE_PREFIX)
        .filter(|suffix| !suffix.is_empty())
        .ok_or_else(|| {
            format!(
                "Weapon namespace {namespace:?} must begin with {PARHELION_NAMESPACE_PREFIX:?} and include a name"
            )
        })?;
    if namespace.len() > 64
        || !suffix.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(format!(
            "Weapon namespace {namespace:?} must be at most 64 characters and use lowercase ASCII letters, digits, '.', '-', or '_'"
        ));
    }
    Ok(())
}
#[cfg(test)]
pub(crate) const ARC_LOGIC_DONOR_HASH: u32 = 0xA25B_8F8F;
#[cfg(test)]
pub(crate) const ARC_LOGIC_DONOR_NAME: &str = "Arc Logic";
pub const DEFAULT_SOURCE_TEXT: &str = "Source: Guardians Made Their Own Fate";
#[cfg(test)]
const MOUNTAINTOP_DONOR_HASH: u32 = 0xEE06_B019;
#[cfg(test)]
const MOUNTAINTOP_DONOR_NAME: &str = "The Mountaintop";

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HexHash(String);

impl HexHash {
    #[must_use]
    pub fn new(value: u32) -> Self {
        Self(format!("0x{value:08X}"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn set_text(&mut self, value: impl Into<String>) {
        self.0 = value.into();
    }

    pub fn parse_u32(&self) -> Result<u32, ParseHexHashError> {
        parse_canonical_hash(&self.0)
    }
}

impl fmt::Debug for HexHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("HexHash").field(&self.0).finish()
    }
}

impl fmt::Display for HexHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<u32> for HexHash {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl FromStr for HexHash {
    type Err = ParseHexHashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_canonical_hash(value).map(Self::new)
    }
}

fn parse_canonical_hash(value: &str) -> Result<u32, ParseHexHashError> {
    let digits = value
        .strip_prefix("0x")
        .filter(|digits| digits.len() == 8)
        .ok_or_else(|| ParseHexHashError(value.to_owned()))?;
    if !digits
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
    {
        return Err(ParseHexHashError(value.to_owned()));
    }
    u32::from_str_radix(digits, 16).map_err(|_| ParseHexHashError(value.to_owned()))
}

impl Serialize for HexHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HexHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseHexHashError(String);

impl fmt::Display for ParseHexHashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Expected a canonical hash such as 0x89ABCDEF; got {:?}",
            self.0
        )
    }
}

impl std::error::Error for ParseHexHashError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipeDamageType {
    Kinetic,
    Arc,
    Solar,
    Void,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipeInventorySlot {
    Kinetic,
    Energy,
    Power,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipeAmmoType {
    Primary,
    Special,
    Heavy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipeRarity {
    Common,
    Uncommon,
    Rare,
    Legendary,
    Exotic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeCollectionPlacement {
    SunriseBadge,
}

/// One gear-art marker moved from where the appearance puts it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerOffsetRecipe {
    /// The FNV-1 hash of the marker's name, as the gear art stores it.
    pub marker: HexHash,
    /// How far it moves, in micrometres along the model's forward, side and up axes.
    pub offset_um: [i32; 3],
}

/// A first-person action a weapon can play from another weapon's animations on the same rig.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationAction {
    Fire,
    AimFire,
    Holster,
    Sprint,
    Slide,
    Alert,
    Reload,
}

/// The farthest a marker may move on any axis, in micrometres. A weapon is under a metre long.
pub const MARKER_OFFSET_LIMIT_UM: i32 = 500_000;

/// The farthest the weapon may move in the hand on any axis, in micrometres.
pub const HELD_OFFSET_LIMIT_UM: i32 = 200_000;

fn is_zero_offset(offset: &[i32; 3]) -> bool {
    *offset == [0; 3]
}

/// One exotic behavior record grafted from another weapon of the same family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdditionalBehaviorRecipe {
    /// Catalogue identifier from [`crate::weapon::behavior::CATALOG`].
    pub behavior: String,
}

/// Whose firing pattern a weapon uses when a borrowed perk changes its burst.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeBehaviorFiring {
    /// Fire the burst the behavior's source weapon fires.
    Behavior,
    /// Keep this weapon's own burst and shot timing.
    Weapon,
}

/// Element switching by holding Reload, the way Hard Light and Borealis work.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDamageRecipe {
    /// Elements the hold can settle on: any two or all three of Arc, Solar and Void.
    pub elements: Vec<RecipeDamageType>,
}

impl VariableDamageRecipe {
    /// Every element, in the order the hold steps through them.
    #[must_use]
    pub fn all() -> Self {
        Self {
            elements: vec![
                RecipeDamageType::Void,
                RecipeDamageType::Arc,
                RecipeDamageType::Solar,
            ],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDonorReference {
    pub item_hash: HexHash,
    pub expected_name: Option<String>,
}

/// One gameplay component, such as the barrel, whose values come from another weapon while the
/// weapon keeps its own runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentSpliceRecipe {
    pub binding_hash: HexHash,
    pub donor: WeaponDonorReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRuntimeComponentRecipe {
    pub binding_hash: HexHash,
    pub donor: WeaponDonorReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponIdentity {
    pub item_hash: HexHash,
    pub collectible_hash: HexHash,
    pub unlock_hash: HexHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_global_id_hash: Option<HexHash>,
    pub name_hash: HexHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_hash: Option<HexHash>,
    pub flavor_hash: HexHash,
    pub source_hash: HexHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_name_hash: Option<HexHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_description_hash: Option<HexHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory_hint_hash: Option<HexHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_requirement_hash: Option<HexHash>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponStatOverride {
    pub definition_index: u16,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponArtArrangementRecipe {
    pub character_class: i8,
    pub arrangement: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDyeReferenceRecipe {
    #[serde(alias = "key")]
    pub channel_index: i8,
    #[serde(alias = "value")]
    pub dye_reference_index: u16,
}

/// One authored socket column in selection order.
///
/// The first choice is the authored definition's collection/default-roll plug. Additional choices
/// are exposed by the game in the same socket column and are never treated as additional
/// simultaneously-selected sockets. Existing inventory instances retain their saved selections.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponSocketColumnRecipe {
    pub choices: Vec<HexHash>,
    pub socket_type: Option<u16>,
    /// IEEE-754 bit patterns aligned with `choices`. Empty means 1.0 per choice.
    pub choice_weight_bits: Vec<u32>,
    /// RPN conditions aligned with `choices`. Empty means unconditional choices.
    pub choice_conditions: Vec<Vec<WeaponNumericInstructionRecipe>>,
    pub reusable_plug_set_index: Option<u16>,
    pub randomized_plug_set_index: Option<u16>,
    pub randomized_selection_program: Vec<WeaponNumericInstructionRecipe>,
}

/// Runtime edits applied through a private clone of one finished sandbox perk on a socket plug.
///
/// `source_perk_index` addresses the selected stock plug's finished-perk row. The corresponding
/// runtime action/entity chain is cloned when it carries edits; an unchanged private perk reuses
/// the stock action while retaining its own item, display, and finished-perk identities. Other
/// finished perks on that plug remain stock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponSandboxPerkRuntimeRecipe {
    /// A complete authored action program. No source action behavior is inherited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<sundial::package_authoring::sandbox_perk::program::Program>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projectiles: Vec<sundial::package_authoring::sandbox_perk::entity::Selection>,
    pub source_perk_index: u16,
    /// Experimental kill-filter override. Omitted means the original activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<sundial::package_authoring::sandbox_perk::activation::PerkActivation>,
    pub runtime_values: Vec<WeaponRuntimeValueOverride>,
    #[serde(default)]
    pub action_float_values: Vec<WeaponSandboxPerkActionFloatRecipe>,
}

/// One float32 value reached through a concrete node in the cloned perk action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponSandboxPerkActionFloatRecipe {
    pub node_type_handle: HexHash,
    pub node_occurrence: u16,
    pub value_pointer_offset: u32,
    pub value_type_handle: HexHash,
    pub expected_bits: u32,
    pub value_bits: u32,
}

/// One socket choice whose stock plug is replaced with a private authored variant.
///
/// The source plug remains unmodified. An edited finished perk gets its own runtime clone so a
/// recipe can alter its effect without changing the gameplay donor's weapon runtime graph.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponSocketPlugVariantRecipe {
    /// Uses complete authored effect and stat lists, replacing template contributions.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_effects: bool,
    /// Native equipped-plug stat contributions, not conditional action multipliers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub investment_stats: Vec<WeaponStatOverride>,
    pub socket_index: u16,
    pub choice_index: u16,
    pub source_plug_hash: HexHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Copies native plug category, tier, inspection template, and localized item type
    /// from a stock plug without replacing the selected runtime effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification_donor_hash: Option<HexHash>,
    /// Installed texture used by this private perk's independent icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::perk::Icon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Also offers the private plug in every stock socket whose shared plug set offers the plug
    /// its classification comes from, as shaders are offered everywhere.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub offer_everywhere: bool,
    /// Additional stock effects supplied only while this private plug is equipped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_sandbox_perks: Vec<u16>,
    pub sandbox_perks: Vec<WeaponSandboxPerkRuntimeRecipe>,
}

impl WeaponSocketPlugVariantRecipe {
    pub(crate) fn same_definition(&self, other: &Self) -> bool {
        let mut other = other.clone();
        other.socket_index = self.socket_index;
        other.choice_index = self.choice_index;
        *self == other
    }

    #[must_use]
    pub fn effect_indices(&self, inherited: &[u16]) -> Vec<u16> {
        let mut indices = if self.replace_effects {
            self.sandbox_perks
                .iter()
                .map(|effect| effect.source_perk_index)
                .collect::<Vec<_>>()
        } else {
            inherited.to_vec()
        };
        for &index in &self.additional_sandbox_perks {
            if !indices.contains(&index) {
                indices.push(index);
            }
        }
        indices
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponNumericInstructionRecipe {
    pub opcode: u8,
    pub operand: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeRawPayloadTarget {
    #[default]
    ItemDefinition,
    ItemActionBlock,
    ItemEquippingBlock,
    ItemFinisherBlock,
    ItemGearsetBlock,
    ItemLoreBlock,
    ItemObjectiveBlock,
    ItemMetricBlock,
    ItemPlugBlock,
    ItemQualityBlock,
    ItemRecordBlock,
    ItemSackBlock,
    ItemSetBlock,
    ItemSocketsBlock,
    ItemStatsBlock,
    ItemSummaryBlock,
    ItemTalentGridBlock,
    ItemTranslationBlock,
    ItemUnlockBlock,
    ItemValueBlock,
    ItemInventoryBlock,
    ItemTraitsDescriptor,
    ItemTraitRows,
    ItemStringDefinition,
    ItemDefinitionIndexRow,
    ItemStringIndexRow,
    ItemIconRow,
    DenseIconTagRow,
    DenseIconSelectorRow,
    DensePresentationRow,
    ItemMetadataRow,
    ItemMetadataIndexRow,
    SandboxPatternRow,
    SandboxPatternIndexRow,
    RuntimeWeaponEntity,
    CollectibleDefinitionRow,
    CollectibleDisplayRow,
    UnlockDefinitionRow,
    UnlockSortedIndexRow,
    UnlockBankRow,
    UnlockDisplayRow,
}

impl RecipeRawPayloadTarget {
    pub const ALL: [Self; 41] = [
        Self::ItemDefinition,
        Self::ItemActionBlock,
        Self::ItemEquippingBlock,
        Self::ItemFinisherBlock,
        Self::ItemGearsetBlock,
        Self::ItemLoreBlock,
        Self::ItemObjectiveBlock,
        Self::ItemMetricBlock,
        Self::ItemPlugBlock,
        Self::ItemQualityBlock,
        Self::ItemRecordBlock,
        Self::ItemSackBlock,
        Self::ItemSetBlock,
        Self::ItemSocketsBlock,
        Self::ItemStatsBlock,
        Self::ItemSummaryBlock,
        Self::ItemTalentGridBlock,
        Self::ItemTranslationBlock,
        Self::ItemUnlockBlock,
        Self::ItemValueBlock,
        Self::ItemInventoryBlock,
        Self::ItemTraitsDescriptor,
        Self::ItemTraitRows,
        Self::ItemStringDefinition,
        Self::ItemDefinitionIndexRow,
        Self::ItemStringIndexRow,
        Self::ItemIconRow,
        Self::DenseIconTagRow,
        Self::DenseIconSelectorRow,
        Self::DensePresentationRow,
        Self::ItemMetadataRow,
        Self::ItemMetadataIndexRow,
        Self::SandboxPatternRow,
        Self::SandboxPatternIndexRow,
        Self::RuntimeWeaponEntity,
        Self::CollectibleDefinitionRow,
        Self::CollectibleDisplayRow,
        Self::UnlockDefinitionRow,
        Self::UnlockSortedIndexRow,
        Self::UnlockBankRow,
        Self::UnlockDisplayRow,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ItemDefinition => "Gameplay definition",
            Self::ItemActionBlock => "Action item block",
            Self::ItemEquippingBlock => "Equipping item block",
            Self::ItemFinisherBlock => "Finisher item block",
            Self::ItemGearsetBlock => "Gearset item block",
            Self::ItemLoreBlock => "Lore item block",
            Self::ItemObjectiveBlock => "Objective item block",
            Self::ItemMetricBlock => "Metric item block",
            Self::ItemPlugBlock => "Plug item block",
            Self::ItemQualityBlock => "Quality item block",
            Self::ItemRecordBlock => "Record item block",
            Self::ItemSackBlock => "Sack item block",
            Self::ItemSetBlock => "Set item block",
            Self::ItemSocketsBlock => "Sockets item block",
            Self::ItemStatsBlock => "Stats item block",
            Self::ItemSummaryBlock => "Summary item block",
            Self::ItemTalentGridBlock => "Talent-grid item block",
            Self::ItemTranslationBlock => "Translation item block",
            Self::ItemUnlockBlock => "Unlock item block",
            Self::ItemValueBlock => "Value item block",
            Self::ItemInventoryBlock => "Inline inventory block",
            Self::ItemTraitsDescriptor => "Traits array descriptor",
            Self::ItemTraitRows => "Trait rows",
            Self::ItemStringDefinition => "Client item strings",
            Self::ItemDefinitionIndexRow => "Gameplay index row",
            Self::ItemStringIndexRow => "Client-string index row",
            Self::ItemIconRow => "Item icon row",
            Self::DenseIconTagRow => "Dense icon-tag row",
            Self::DenseIconSelectorRow => "Dense icon selector",
            Self::DensePresentationRow => "Dense presentation row",
            Self::ItemMetadataRow => "Item metadata row",
            Self::ItemMetadataIndexRow => "Metadata companion index",
            Self::SandboxPatternRow => "Sandbox-pattern row",
            Self::SandboxPatternIndexRow => "Sandbox-pattern companion index",
            Self::RuntimeWeaponEntity => "Runtime weapon entity",
            Self::CollectibleDefinitionRow => "Collectible definition",
            Self::CollectibleDisplayRow => "Collectible display",
            Self::UnlockDefinitionRow => "Unlock definition",
            Self::UnlockSortedIndexRow => "Unlock sorted-index row",
            Self::UnlockBankRow => "Unlock bank row",
            Self::UnlockDisplayRow => "Unlock display",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponRawPayloadPatchRecipe {
    pub target: RecipeRawPayloadTarget,
    /// Byte offset in the finished cloned record, written as a JSON integer.
    pub offset: u32,
    /// Hexadecimal bytes; ASCII whitespace and underscores are ignored.
    pub bytes: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponRuntimeResourcePatchRecipe {
    pub binding_hash: HexHash,
    /// Zero-based resource within a native binding that selects more than one resource.
    pub resource_index: u16,
    /// Byte offset relative to the selected concrete resource record.
    pub offset: u32,
    /// Hexadecimal bytes; ASCII whitespace and underscores are ignored.
    pub bytes: String,
    /// When present, bytes identify a stock graph to clone and edit before linking it here.
    /// This is weapon-scoped, not conditional on a socket perk being equipped.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub graph_values: Vec<WeaponRuntimeValueOverride>,
}

/// A private keyed sword profile with source-derived angular scale bits.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwordProfileRecipe {
    pub key: HexHash,
    pub near_scale_bits: u32,
    pub far_scale_bits: u32,
}

impl Default for WeaponRuntimeResourcePatchRecipe {
    fn default() -> Self {
        Self {
            binding_hash: HexHash::new(WEAPON_BARREL_COMPONENT_KEY),
            resource_index: 0,
            offset: 0,
            bytes: String::new(),
            graph_values: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponLocaleTextRecipe {
    pub locale_index: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flavor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_requirement: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponRecipeOverrides {
    /// Private driving motion and the complete vehicle graph an authored Sparrow summons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sparrow: Option<crate::vehicle::Sparrow>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove_lore: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg(feature = "d2-model-importer")]
    pub imported_graph: Option<parhelion_import::GraphReference>,
    /// Optional Collections override. `None` combines the authored ammo type with the gameplay
    /// donor's weapon type and creates the shared page when that combination is not stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_destination: Option<crate::collection::Destination>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_sunrise_badge: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<crate::presentation::Badge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_icon: Option<crate::presentation::Artwork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lore: Option<String>,
    /// Optional color/opacity treatment of the selected icon donor's private primary image.
    #[serde(default, skip_serializing_if = "WeaponIconEdit::is_identity")]
    pub icon_edit: WeaponIconEdit,
    /// Independent transparent artwork for the ammunition HUD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_icon: Option<crate::hud_icon::HudImage>,
    pub investment_stats: Vec<WeaponStatOverride>,
    /// Gameplay-donor investment-stat rows removed from the authored definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_investment_stats: Vec<u16>,
    /// Complete ordered base-item sandbox-perk indices. Socket plug perks are configured in
    /// `socket_columns`; `None` preserves the gameplay donor array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sandbox_perks: Option<Vec<u16>>,
    /// Complete ordered native item-trait indices. `None` preserves the gameplay donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trait_indices: Option<Vec<u16>>,
    /// Native inventory quantity bound. Stock instanced weapons normally use one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_stack_size: Option<u32>,
    /// Native socket-entry-list row in the item's talent-grid holder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_entry_list_index: Option<u16>,
    /// Native item plug-category hash. `None` preserves the gameplay donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plug_category_hash: Option<HexHash>,
    /// Native randomized-roll-set row. `None` preserves the gameplay donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll_set_index: Option<u16>,
    /// Native linked-item table row. `None` preserves the gameplay donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_plug_index: Option<u16>,
    pub inventory_slot: Option<RecipeInventorySlot>,
    /// Primary/Special/Heavy classification and native ammo override, applied to the
    /// default and perk-selected runtime variants. Does not rebalance magazine or reserve stats.
    pub ammo_type: Option<RecipeAmmoType>,
    pub modern_damage_type: Option<RecipeDamageType>,
    /// Reload-hold element switching. Compilation pins Hard Light's Fundamentals plug into the
    /// first trait socket, so the appearance donor must be Hard Light or Borealis.
    /// [`Self::modern_damage_type`] is then the element the weapon rests on and must be in the set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_damage: Option<VariableDamageRecipe>,
    /// Exotic behaviors grafted from weapons that share this weapon's content component owner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_behaviors: Vec<AdditionalBehaviorRecipe>,
    /// Leave the source weapon's own intrinsic and trait plugs out of the graft. Several exotics
    /// keep half of their behavior in a perk, so the plugs travel with it by default.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip_behavior_perks: bool,
    /// Whose firing pattern the weapon uses when a borrowed perk changes its burst. Absent means
    /// the behavior's, so the weapon fires what the source weapon fires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_firing: Option<RecipeBehaviorFiring>,
    /// IEEE-754 bit pattern raising the launch speed of a grafted projectile on a weapon that
    /// fires none of its own, and the most it is raised to. Absent means the default boost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_projectile_speed_bits: Option<u32>,
    /// A stock projectile graph, privately cloned with checked owner patches and definition
    /// appends, that the weapon fires as its own instead of through a perk's pattern override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fired_graph: Option<crate::weapon::behavior::FiredGraph>,
    /// Values of the projectile the weapon fires, set on a private copy of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projectile: Option<crate::weapon::projectile::Edits>,
    /// Permanent pellet count and spread geometry on the final private Barrel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barrel: Option<crate::weapon::barrel::Edits>,
    /// Another weapon whose first-person animations the weapon plays, such as a 140 RPM hand
    /// cannon's on a model whose own row plays a 180's. Absent follows the model's own row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_donor: Option<WeaponDonorReference>,
    /// Single actions, such as Fire or Holster, played from another weapon's animations on the
    /// same rig while every other action follows `animation_donor` or the model's own row.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub animation_actions: BTreeMap<AnimationAction, WeaponDonorReference>,
    /// Another weapon whose type markers (its type name, frame key and type label) the runtime
    /// carries, such as `pulse_rifle` on a scout rifle. Absent keeps the base weapon's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_marker_donor: Option<WeaponDonorReference>,
    /// Gear-art markers moved from where the appearance puts them: the sight, the muzzle and the
    /// other named points. Every row of one name moves together. Changing the appearance clears
    /// them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marker_offsets: Vec<MarkerOffsetRecipe>,
    /// How far the whole model and its markers move from the handle the hand holds, in
    /// micrometres along the model's forward, side and up axes. First person shows it most, since
    /// the hands stay where the animations put them. Changing the appearance clears it.
    #[serde(default, skip_serializing_if = "is_zero_offset")]
    pub held_offset_um: [i32; 3],
    /// Gameplay components whose values come from other weapons, one donor each, while the
    /// weapon keeps its own runtime and wiring.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_splices: Vec<ComponentSpliceRecipe>,
    pub power_cap_group: Option<u16>,
    /// Complete native quality/version group sequence. This advanced form preserves the number
    /// and order of the gameplay donor's version rows while allowing every row to differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_cap_groups: Option<Vec<u16>>,
    /// Optional stock rarity tier. `None` preserves the gameplay donor byte.
    pub rarity: Option<RecipeRarity>,
    /// Armor equip eligibility. Omitted follows the donor, Any removes its class requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub armor_class: Option<crate::ArmorClass>,
    /// Optional stock gear-art/runtime row used as the runtime entity source. Compilation combines
    /// its runtime global identity with the appearance donor's gear-art data. `None` follows the
    /// gameplay donor.
    #[serde(
        default,
        alias = "gear_art_index",
        skip_serializing_if = "Option::is_none"
    )]
    pub weapon_pattern_index: Option<u16>,
    /// Stock weapon selected in the UI for [`Self::weapon_pattern_index`]. This preserves the exact
    /// donor label when multiple weapons share one row; compilation uses the row itself.
    #[serde(
        default,
        alias = "gear_art_donor_hash",
        skip_serializing_if = "Option::is_none"
    )]
    pub weapon_pattern_donor_hash: Option<HexHash>,
    /// Optional installed stat-display group used for bounds and in-game interpolation.
    pub stat_group_index: Option<u16>,
    /// Stock weapon selected in the UI for [`Self::stat_group_index`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat_group_donor_hash: Option<HexHash>,
    /// A stat display group of the recipe's own, in place of [`Self::stat_group_index`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_stat_group: Option<crate::stat_group::CustomStatGroup>,
    /// Complete translation-art rows. `None` preserves the selected geometry donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art_arrangements: Option<Vec<WeaponArtArrangementRecipe>>,
    /// Complete custom, default, and locked dye-reference rows.
    #[serde(
        default,
        alias = "material_rows",
        skip_serializing_if = "Option::is_none"
    )]
    pub render_dye_rows: Option<[Vec<WeaponDyeReferenceRecipe>; 3]>,
    /// A subclass's abilities and attunements taken from other stock subclasses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subclass_abilities: Option<crate::subclass::SubclassAbilities>,
    /// A subclass every character receives and every class may equip. The item loses its
    /// donor-class equip requirement and defaults to the Guardian Subclass type label.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subclass_every_class: bool,
    /// A subclass for another class than its base's: Titan, Hunter or Warlock. Its class
    /// requirement names that class, the class's characters receive it, and its type label
    /// defaults to that class's. None keeps the base's class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subclass_class: Option<crate::ArmorClass>,
    /// The damage type a subclass shows, whose icon sits beside its name. None keeps the base's.
    /// It changes no ability's damage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subclass_damage_type: Option<RecipeDamageType>,
    /// A shader's custom surface values, by gear type, channel and surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dye_edits: Vec<crate::dye::DyeEdit>,
    /// Allow the weapon's private materials to consume shader glow on existing glow masks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shader_glow: bool,
    /// Keeps the base weapon's type name and Collections page when the appearance is another
    /// weapon type. Otherwise the weapon takes its appearance's type.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub base_type: bool,
    /// A shader's custom detail textures and tiling, by gear type and channel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dye_texture_edits: Vec<crate::dye::DyeTextureEdit>,
    /// A shader whose icon the page draws from its dyes, into the icon's imported image. The build
    /// compiles that image like any other.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub icon_from_dyes: bool,
    /// An emblem's nameplate images. `None` keeps the base emblem's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nameplate: Option<crate::emblem::Nameplate>,
    /// A subclass's screen pictures. `None` keeps the base subclass's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_art: Option<crate::subclass::ScreenArt>,
    /// A subclass icon drawn as a diamond in the HUD color, with artwork of its own at the middle,
    /// in place of the icon's image. `None` keeps the icon's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subclass_icon: Option<crate::subclass::GeneratedIcon>,
    /// Allowed native tracker categories. Absence follows the base emblem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stat_trackers: Option<crate::emblem::StatTrackers>,
    /// Complete positional socket columns. An empty vector inherits every donor socket unchanged.
    /// A non-empty vector contains every donor socket and may append explicitly typed columns up
    /// to the native socket limit. `None` preserves donor socket content. Added columns must be
    /// `Some` and contain a socket type and ordered authored choices.
    pub socket_columns: Vec<Option<WeaponSocketColumnRecipe>>,
    /// Private plug clones for individual socket choices, including typed runtime edits to their
    /// selected finished sandbox perks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub socket_plug_variants: Vec<WeaponSocketPlugVariantRecipe>,
    /// Typed package-backed runtime values. Every locator is re-resolved against the effective
    /// donor graph before compilation, so stale offsets cannot be written silently.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_values: Vec<WeaponRuntimeValueOverride>,
    /// Private sword profile selected by a matching effect in an authored private perk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sword_profile: Option<SwordProfileRecipe>,
    /// Technical same-size edits inside concrete runtime-component resources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_resource_patches: Vec<WeaponRuntimeResourcePatchRecipe>,
    /// Final byte patches for technical fields not yet represented structurally.
    pub raw_payload_patches: Vec<WeaponRawPayloadPatchRecipe>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRecipe {
    pub schema: u32,
    /// What the recipe builds. Weapon recipes predate kinds and omit it, so their files are
    /// unchanged and an older Parhelion rejects a gear recipe instead of building it as a weapon.
    #[serde(default, skip_serializing_if = "ItemKind::is_weapon")]
    pub kind: ItemKind,
    pub namespace: String,
    pub collection_placement: RecipeCollectionPlacement,
    pub donor: WeaponDonorReference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_donor: Option<WeaponDonorReference>,
    /// Optional stock donor used only for the translation block's custom, default, and locked
    /// dye references. When omitted, render dyes follow the geometry donor, then the gameplay
    /// donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_gear_donor: Option<WeaponDonorReference>,
    /// Optional stock donor used only for the authored icon definition. When omitted, the icon
    /// follows the geometry donor, then the gameplay donor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_donor: Option<WeaponDonorReference>,
    /// Sparse concrete runtime-component donors. Each row replaces one abstract binding in a
    /// private clone of the selected weapon pattern's runtime entity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_component_donors: Vec<WeaponRuntimeComponentRecipe>,
    pub identity: WeaponIdentity,
    pub name: String,
    /// Optional authored item-type label. `None` preserves the gameplay donor's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    pub flavor: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_requirement: Option<String>,
    /// Sparse per-locale replacements keyed by the native locale payload index (0 through 12).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locale_overrides: Vec<WeaponLocaleTextRecipe>,
    #[serde(default)]
    pub overrides: WeaponRecipeOverrides,
}

impl WeaponRecipe {
    /// The weapon whose type this one shows and files under in Collections: its appearance,
    /// unless it keeps the base weapon's type.
    pub(crate) fn type_donor_hash(&self) -> u32 {
        self.presentation_donor
            .as_ref()
            .filter(|_| !self.overrides.base_type)
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .unwrap_or_else(|| self.donor.item_hash.parse_u32().unwrap_or_default())
    }

    #[cfg(test)]
    #[must_use]
    pub fn every_end() -> Self {
        Self::from_json_str(include_str!("../recipes/every-end.parhelion.json"))
            .expect("the bundled Every End recipe must remain valid")
    }

    #[cfg(test)]
    pub fn second_sun() -> Result<Self, RecipeError> {
        Self::from_json_str(include_str!("../recipes/second-sun.parhelion.json"))
    }

    /// This recipe rebuilt on the stock weapon behind `base`, a recipe from the library whose
    /// weapon this one names as its donor.
    ///
    /// A weapon Parhelion built is not in the game's own tables, so the build cannot read it as
    /// a donor. Its recipe is the whole of it, though: a stock donor plus overrides. Building on
    /// it means taking that stock donor and those overrides underneath this recipe's own, so
    /// what this recipe sets wins and everything it leaves alone comes from the base.
    ///
    /// The merge works on the saved form of both recipes. A field this recipe saves as unset,
    /// which is absent, null, an empty list or false, keeps the base's value; a field it sets
    /// replaces the base's. Identity, namespace and the name and text fields are always this
    /// recipe's own, so the result is a new weapon rather than a second copy of the base.
    pub fn rebased_onto(&self, base: &Self) -> Result<Self, RecipeError> {
        use serde_json::Value;
        let mut merged = serde_json::to_value(base).map_err(|error| {
            RecipeError::Validation(format!("Could not read the base recipe: {error}"))
        })?;
        let own = serde_json::to_value(self).map_err(|error| {
            RecipeError::Validation(format!("Could not read the recipe: {error}"))
        })?;
        let (Value::Object(merged_fields), Value::Object(own_fields)) = (&mut merged, own) else {
            return Err(RecipeError::Validation(
                "A recipe must save as an object".into(),
            ));
        };
        let unset = |value: &Value| match value {
            Value::Null => true,
            Value::Bool(flag) => !flag,
            Value::Array(items) => items.is_empty(),
            _ => false,
        };
        for (key, value) in own_fields {
            match key.as_str() {
                "donor" => {}
                "overrides" => {
                    let Value::Object(own_overrides) = value else {
                        continue;
                    };
                    let base_overrides = merged_fields
                        .entry("overrides")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if let Value::Object(base_overrides) = base_overrides {
                        for (key, value) in own_overrides {
                            if !unset(&value) {
                                base_overrides.insert(key, value);
                            }
                        }
                    }
                }
                _ if unset(&value) => {}
                _ => {
                    merged_fields.insert(key, value);
                }
            }
        }
        let encoded = serde_json::to_string(&merged).map_err(|error| {
            RecipeError::Validation(format!("Could not encode the rebased recipe: {error}"))
        })?;
        Self::from_json_str(&encoded)
    }

    #[cfg(test)]
    pub fn new_weapon(namespace: impl Into<String>) -> Result<Self, RecipeError> {
        Self::new_weapon_for_donor(namespace, ARC_LOGIC_DONOR_HASH, ARC_LOGIC_DONOR_NAME)
    }

    #[cfg(test)]
    pub fn new_weapon_for_donor(
        namespace: impl Into<String>,
        donor_item_hash: u32,
        donor_name: impl Into<String>,
    ) -> Result<Self, RecipeError> {
        let recipe = Self::draft_for_donor(namespace.into(), donor_item_hash, donor_name.into())?;
        recipe.validate()?;
        Ok(recipe)
    }

    fn draft_for_donor(
        namespace: String,
        donor_item_hash: u32,
        donor_name: String,
    ) -> Result<Self, RecipeError> {
        let identity = WeaponCloneIdentity::from_namespace(&namespace)?;
        Ok(Self {
            schema: RECIPE_SCHEMA,
            kind: ItemKind::Weapon,
            namespace,
            collection_placement: RecipeCollectionPlacement::SunriseBadge,
            donor: WeaponDonorReference {
                item_hash: donor_item_hash.into(),
                expected_name: (!donor_name.trim().is_empty()).then_some(donor_name),
            },
            presentation_donor: None,
            render_gear_donor: None,
            icon_donor: None,
            runtime_component_donors: Vec::new(),
            identity: identity.into(),
            name: "New Weapon".to_owned(),
            type_name: None,
            flavor: "A weapon authored with Parhelion.".to_owned(),
            source: DEFAULT_SOURCE_TEXT.to_owned(),
            collection_name: None,
            collection_description: None,
            inventory_hint: None,
            collection_requirement: None,
            locale_overrides: Vec::new(),
            overrides: WeaponRecipeOverrides::default(),
        })
    }

    /// Test fixture constructor deriving the namespace and all identities from a display name.
    #[cfg(test)]
    pub fn new_named_weapon_for_donor(
        name: impl Into<String>,
        donor_item_hash: u32,
        donor_name: impl Into<String>,
    ) -> Result<Self, RecipeError> {
        let name = name.into();
        let namespace = namespace_for_weapon_name(&name)?;
        let mut recipe = Self::draft_for_donor(namespace, donor_item_hash, donor_name.into())?;
        recipe.name = name;
        recipe.validate()?;
        Ok(recipe)
    }

    /// Creates an unsaved UI draft that cannot be saved or built until a donor is selected.
    pub(crate) fn new_unbound(name: impl Into<String>) -> Result<Self, RecipeError> {
        let name = name.into();
        let namespace = namespace_for_weapon_name(&name)?;
        let mut recipe = Self::draft_for_donor(namespace, 0, String::new())?;
        recipe.name = name;
        Ok(recipe)
    }

    /// A new recipe of `kind` with no base item yet. A new weapon starts with shader glow on, and
    /// a new subclass for every class. Saved recipes without those fields keep them off, so they
    /// build as they did.
    pub(crate) fn new_unbound_kind(kind: ItemKind) -> Result<Self, RecipeError> {
        if kind.is_weapon() {
            let mut recipe = Self::new_unbound("New Recipe")?;
            recipe.overrides.shader_glow = kind == ItemKind::Weapon;
            return Ok(recipe);
        }
        let mut recipe = Self::new_unbound(format!("New {}", kind.label()))?;
        recipe.kind = kind;
        recipe.flavor = kind.default_flavor().to_owned();
        recipe.overrides.icon_from_dyes = kind == ItemKind::Shader;
        recipe.overrides.subclass_every_class = kind == ItemKind::Subclass;
        Ok(recipe)
    }

    pub fn set_donor(&mut self, item_hash: u32, expected_name: impl Into<String>) {
        let expected_name = expected_name.into();
        self.donor = WeaponDonorReference {
            item_hash: item_hash.into(),
            expected_name: (!expected_name.trim().is_empty()).then_some(expected_name),
        };
        self.presentation_donor = None;
        self.render_gear_donor = None;
        self.icon_donor = None;
        self.runtime_component_donors.clear();
        // A new base invalidates donor-owned rows and socket positions. The authored
        // collection, story and independent artwork do not depend on those rows.
        let previous = std::mem::take(&mut self.overrides);
        self.overrides = WeaponRecipeOverrides {
            collection_destination: previous.collection_destination,
            exclude_from_sunrise_badge: previous.exclude_from_sunrise_badge,
            badge: previous.badge,
            corner_icon: previous.corner_icon,
            lore: previous.lore,
            icon_edit: previous.icon_edit,
            hud_icon: previous.hud_icon,
            icon_from_dyes: previous.icon_from_dyes,
            nameplate: previous.nameplate,
            screen_art: previous.screen_art,
            subclass_icon: previous.subclass_icon,
            stat_trackers: previous.stat_trackers,
            shader_glow: previous.shader_glow,
            sparrow: previous.sparrow,
            // Which classes a subclass is for, and the damage type it shows, hold whatever its
            // base.
            subclass_every_class: previous.subclass_every_class,
            subclass_class: previous.subclass_class,
            subclass_damage_type: previous.subclass_damage_type,
            ..Default::default()
        };
    }

    /// Changes the geometry baseline and restores every presentation sub-source to follow it.
    ///
    /// Art rows and dye rows are indices into donor-owned presentation data. Carrying explicit
    /// values across a geometry change can therefore produce a valid-looking recipe whose model
    /// has missing geometry or materials. Icon color treatment is retained because it is applied
    /// to the newly selected icon rather than indexing the old donor's data.
    pub(crate) fn set_presentation_donor(&mut self, donor: Option<WeaponDonorReference>) {
        self.presentation_donor = donor;
        self.render_gear_donor = None;
        self.icon_donor = None;
        self.overrides.art_arrangements = None;
        self.overrides.render_dye_rows = None;
        // Another model can use another rig, which the borrowed animations may not fit, and
        // carries its own markers.
        self.overrides.animation_donor = None;
        self.overrides.animation_actions.clear();
        self.overrides.marker_offsets.clear();
        self.overrides.held_offset_um = [0; 3];
    }

    /// The weapon a gameplay component's values come from, when another one is chosen.
    pub(crate) fn component_splice(&self, binding_hash: u32) -> Option<&WeaponDonorReference> {
        self.overrides
            .component_splices
            .iter()
            .find(|splice| splice.binding_hash.parse_u32() == Ok(binding_hash))
            .map(|splice| &splice.donor)
    }

    pub(crate) fn set_component_splice(
        &mut self,
        binding_hash: u32,
        donor: Option<WeaponDonorReference>,
    ) {
        self.overrides
            .component_splices
            .retain(|splice| splice.binding_hash.parse_u32() != Ok(binding_hash));
        if let Some(donor) = donor {
            self.overrides
                .component_splices
                .push(ComponentSpliceRecipe {
                    binding_hash: binding_hash.into(),
                    donor,
                });
            self.overrides
                .component_splices
                .sort_by_key(|splice| splice.binding_hash.parse_u32().unwrap_or(u32::MAX));
        }
    }

    pub(crate) fn runtime_component_donor(
        &self,
        binding_hash: u32,
    ) -> Option<&WeaponDonorReference> {
        self.runtime_component_donors
            .iter()
            .find(|component| component.binding_hash.parse_u32() == Ok(binding_hash))
            .map(|component| &component.donor)
    }

    pub(crate) fn set_runtime_component_donor(
        &mut self,
        binding_hash: u32,
        donor: Option<WeaponDonorReference>,
    ) {
        self.runtime_component_donors
            .retain(|component| component.binding_hash.parse_u32() != Ok(binding_hash));
        if let Some(donor) = donor {
            self.runtime_component_donors
                .push(WeaponRuntimeComponentRecipe {
                    binding_hash: binding_hash.into(),
                    donor,
                });
            self.runtime_component_donors
                .sort_by_key(|component| component.binding_hash.parse_u32().unwrap_or(u32::MAX));
        }
    }

    /// Renames an authored item while updating its namespace and all identities as one
    /// transaction. A validation failure leaves the recipe unchanged.
    pub fn rename_authored_item(&mut self, name: impl Into<String>) -> Result<(), RecipeError> {
        let name = name.into();
        let namespace = namespace_for_weapon_name(&name)?;
        let identity = WeaponCloneIdentity::from_namespace(&namespace)?.into();
        self.name = name;
        self.namespace = namespace;
        self.identity = identity;
        Ok(())
    }

    /// Reports whether the current hashes match the name-derived namespace.
    #[must_use]
    pub fn identity_is_name_derived(&self) -> bool {
        let Ok(namespace) = namespace_for_weapon_name(&self.name) else {
            return false;
        };
        let Ok(identity) = WeaponCloneIdentity::from_namespace(&namespace) else {
            return false;
        };
        self.namespace == namespace
            && self
                .identity
                .to_compiler(&namespace)
                .is_ok_and(|current| current == identity)
    }

    #[must_use]
    pub fn slug(&self) -> String {
        let slug = slug_from_text(&self.name);
        if slug.is_empty() {
            self.identity.item_hash.parse_u32().map_or_else(
                |_| "weapon".to_owned(),
                |item_hash| format!("weapon-{item_hash:08x}"),
            )
        } else {
            slug
        }
    }
}

/// Derives the canonical authoring namespace shown by Parhelion's recipe editor.
pub fn namespace_for_weapon_name(name: &str) -> Result<String, RecipeError> {
    let slug = slug_from_text(name);
    if slug.is_empty() {
        return Err(RecipeError::Validation(
            "Weapon name must contain at least one ASCII letter or digit".to_owned(),
        ));
    }
    let max_slug_length = 64 - PARHELION_NAMESPACE_PREFIX.len();
    if slug.len() > max_slug_length {
        return Err(RecipeError::Validation(format!(
            "Weapon name produces a namespace longer than 64 characters. Shorten it to at most {max_slug_length} ASCII letters or digits"
        )));
    }
    Ok(format!("{PARHELION_NAMESPACE_PREFIX}{slug}"))
}

fn slug_from_text(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut separator_pending = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    const MAX_SLUG_BYTES: usize = 80;
    slug.truncate(slug.len().min(MAX_SLUG_BYTES));
    while slug.ends_with('-') {
        slug.pop();
    }
    if is_windows_reserved_stem(&slug) {
        slug.insert_str(0, "weapon-");
    }
    slug
}

fn is_windows_reserved_stem(stem: &str) -> bool {
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

#[cfg(test)]
mod tests;
