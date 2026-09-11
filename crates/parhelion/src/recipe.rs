//! Versioned, human-editable donor-clone weapon recipes for Parhelion.

use std::{
    collections::BTreeSet,
    fmt, fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    AuthoredWeaponRarity, AuthoringError, ModernDamageType, WeaponAmmoType,
    WeaponArtArrangementOverride, WeaponCloneIdentity, WeaponCloneOverrides, WeaponCloneSpec,
    WeaponCloneText, WeaponDyeReferenceOverride, WeaponIconEdit, WeaponInventorySlot,
    WeaponLocaleTextOverride, WeaponNumericInstruction, WeaponRawPayloadPatch,
    WeaponRawPayloadTarget, WeaponRenderGearDonorReference, WeaponSandboxPerkActionFloatOverride,
    WeaponSandboxPerkRuntimeOverride, WeaponSocketColumnOverride, WeaponSocketPlugVariantOverride,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sundial::investment::MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES;
use sundial::package_authoring::{
    weapon_entity::WEAPON_BARREL_COMPONENT_KEY, weapon_runtime::WeaponRuntimeValueOverride,
};

pub const RECIPE_SCHEMA: u32 = 1;
pub const PARHELION_NAMESPACE_PREFIX: &str = "parhelion.";

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
pub const DEFAULT_SOURCE_TEXT: &str = "Source: Guardians Make Their Own Fate";
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

impl From<RecipeAmmoType> for WeaponAmmoType {
    fn from(value: RecipeAmmoType) -> Self {
        match value {
            RecipeAmmoType::Primary => Self::Primary,
            RecipeAmmoType::Special => Self::Special,
            RecipeAmmoType::Heavy => Self::Heavy,
        }
    }
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

impl From<RecipeRarity> for AuthoredWeaponRarity {
    fn from(value: RecipeRarity) -> Self {
        match value {
            RecipeRarity::Common => Self::Common,
            RecipeRarity::Uncommon => Self::Uncommon,
            RecipeRarity::Rare => Self::Rare,
            RecipeRarity::Legendary => Self::Legendary,
            RecipeRarity::Exotic => Self::Exotic,
        }
    }
}

impl From<RecipeInventorySlot> for WeaponInventorySlot {
    fn from(value: RecipeInventorySlot) -> Self {
        match value {
            RecipeInventorySlot::Kinetic => Self::Kinetic,
            RecipeInventorySlot::Energy => Self::Energy,
            RecipeInventorySlot::Power => Self::Power,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeCollectionPlacement {
    SunriseBadge,
}

impl From<RecipeDamageType> for ModernDamageType {
    fn from(value: RecipeDamageType) -> Self {
        match value {
            RecipeDamageType::Kinetic => Self::Kinetic,
            RecipeDamageType::Arc => Self::Arc,
            RecipeDamageType::Solar => Self::Solar,
            RecipeDamageType::Void => Self::Void,
        }
    }
}

impl From<ModernDamageType> for RecipeDamageType {
    fn from(value: ModernDamageType) -> Self {
        match value {
            ModernDamageType::Kinetic => Self::Kinetic,
            ModernDamageType::Arc => Self::Arc,
            ModernDamageType::Solar => Self::Solar,
            ModernDamageType::Void => Self::Void,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDonorReference {
    pub item_hash: HexHash,
    pub expected_name: Option<String>,
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

impl From<WeaponCloneIdentity> for WeaponIdentity {
    fn from(value: WeaponCloneIdentity) -> Self {
        Self {
            item_hash: value.item_hash.into(),
            collectible_hash: value.collectible_hash.into(),
            unlock_hash: value.unlock_hash.into(),
            pattern_global_id_hash: Some(value.pattern_global_id_hash.into()),
            name_hash: value.name_hash.into(),
            type_hash: Some(value.type_hash.into()),
            flavor_hash: value.flavor_hash.into(),
            source_hash: value.source_hash.into(),
            collection_name_hash: Some(value.collection_name_hash.into()),
            collection_description_hash: Some(value.collection_description_hash.into()),
            inventory_hint_hash: Some(value.inventory_hint_hash.into()),
            collection_requirement_hash: Some(value.collection_requirement_hash.into()),
        }
    }
}

impl WeaponIdentity {
    fn to_compiler(&self, namespace: &str) -> Result<WeaponCloneIdentity, RecipeError> {
        let derived = WeaponCloneIdentity::from_namespace(namespace)?;
        Ok(WeaponCloneIdentity {
            item_hash: parse_recipe_hash("identity.item_hash", &self.item_hash)?,
            collectible_hash: parse_recipe_hash(
                "identity.collectible_hash",
                &self.collectible_hash,
            )?,
            unlock_hash: parse_recipe_hash("identity.unlock_hash", &self.unlock_hash)?,
            pattern_global_id_hash: self
                .pattern_global_id_hash
                .as_ref()
                .map_or(Ok(derived.pattern_global_id_hash), |hash| {
                    parse_recipe_hash("identity.pattern_global_id_hash", hash)
                })?,
            name_hash: parse_recipe_hash("identity.name_hash", &self.name_hash)?,
            type_hash: self
                .type_hash
                .as_ref()
                .map_or(Ok(derived.type_hash), |hash| {
                    parse_recipe_hash("identity.type_hash", hash)
                })?,
            flavor_hash: parse_recipe_hash("identity.flavor_hash", &self.flavor_hash)?,
            source_hash: parse_recipe_hash("identity.source_hash", &self.source_hash)?,
            collection_name_hash: self
                .collection_name_hash
                .as_ref()
                .map_or(Ok(derived.collection_name_hash), |hash| {
                    parse_recipe_hash("identity.collection_name_hash", hash)
                })?,
            collection_description_hash: self
                .collection_description_hash
                .as_ref()
                .map_or(Ok(derived.collection_description_hash), |hash| {
                    parse_recipe_hash("identity.collection_description_hash", hash)
                })?,
            inventory_hint_hash: self
                .inventory_hint_hash
                .as_ref()
                .map_or(Ok(derived.inventory_hint_hash), |hash| {
                    parse_recipe_hash("identity.inventory_hint_hash", hash)
                })?,
            collection_requirement_hash: self
                .collection_requirement_hash
                .as_ref()
                .map_or(Ok(derived.collection_requirement_hash), |hash| {
                    parse_recipe_hash("identity.collection_requirement_hash", hash)
                })?,
        })
    }

    pub(crate) fn parsed_hashes(&self, namespace: &str) -> Result<[u32; 12], RecipeError> {
        let derived = WeaponCloneIdentity::from_namespace(namespace)?;
        Ok([
            parse_recipe_hash("identity.item_hash", &self.item_hash)?,
            parse_recipe_hash("identity.collectible_hash", &self.collectible_hash)?,
            parse_recipe_hash("identity.unlock_hash", &self.unlock_hash)?,
            self.pattern_global_id_hash
                .as_ref()
                .map_or(Ok(derived.pattern_global_id_hash), |hash| {
                    parse_recipe_hash("identity.pattern_global_id_hash", hash)
                })?,
            parse_recipe_hash("identity.name_hash", &self.name_hash)?,
            self.type_hash
                .as_ref()
                .map_or(Ok(derived.type_hash), |hash| {
                    parse_recipe_hash("identity.type_hash", hash)
                })?,
            parse_recipe_hash("identity.flavor_hash", &self.flavor_hash)?,
            parse_recipe_hash("identity.source_hash", &self.source_hash)?,
            self.collection_name_hash
                .as_ref()
                .map_or(Ok(derived.collection_name_hash), |hash| {
                    parse_recipe_hash("identity.collection_name_hash", hash)
                })?,
            self.collection_description_hash
                .as_ref()
                .map_or(Ok(derived.collection_description_hash), |hash| {
                    parse_recipe_hash("identity.collection_description_hash", hash)
                })?,
            self.inventory_hint_hash
                .as_ref()
                .map_or(Ok(derived.inventory_hint_hash), |hash| {
                    parse_recipe_hash("identity.inventory_hint_hash", hash)
                })?,
            self.collection_requirement_hash
                .as_ref()
                .map_or(Ok(derived.collection_requirement_hash), |hash| {
                    parse_recipe_hash("identity.collection_requirement_hash", hash)
                })?,
        ])
    }
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
    pub projectiles: Vec<sundial::package_authoring::sandbox_perk::projectile::Selection>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Additional stock effects supplied only while this private plug is equipped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_sandbox_perks: Vec<u16>,
    pub sandbox_perks: Vec<WeaponSandboxPerkRuntimeRecipe>,
}

impl WeaponSocketPlugVariantRecipe {
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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

impl From<RecipeRawPayloadTarget> for WeaponRawPayloadTarget {
    fn from(value: RecipeRawPayloadTarget) -> Self {
        match value {
            RecipeRawPayloadTarget::ItemDefinition => Self::ItemDefinition,
            RecipeRawPayloadTarget::ItemActionBlock => Self::ItemActionBlock,
            RecipeRawPayloadTarget::ItemEquippingBlock => Self::ItemEquippingBlock,
            RecipeRawPayloadTarget::ItemFinisherBlock => Self::ItemFinisherBlock,
            RecipeRawPayloadTarget::ItemGearsetBlock => Self::ItemGearsetBlock,
            RecipeRawPayloadTarget::ItemLoreBlock => Self::ItemLoreBlock,
            RecipeRawPayloadTarget::ItemObjectiveBlock => Self::ItemObjectiveBlock,
            RecipeRawPayloadTarget::ItemMetricBlock => Self::ItemMetricBlock,
            RecipeRawPayloadTarget::ItemPlugBlock => Self::ItemPlugBlock,
            RecipeRawPayloadTarget::ItemQualityBlock => Self::ItemQualityBlock,
            RecipeRawPayloadTarget::ItemRecordBlock => Self::ItemRecordBlock,
            RecipeRawPayloadTarget::ItemSackBlock => Self::ItemSackBlock,
            RecipeRawPayloadTarget::ItemSetBlock => Self::ItemSetBlock,
            RecipeRawPayloadTarget::ItemSocketsBlock => Self::ItemSocketsBlock,
            RecipeRawPayloadTarget::ItemStatsBlock => Self::ItemStatsBlock,
            RecipeRawPayloadTarget::ItemSummaryBlock => Self::ItemSummaryBlock,
            RecipeRawPayloadTarget::ItemTalentGridBlock => Self::ItemTalentGridBlock,
            RecipeRawPayloadTarget::ItemTranslationBlock => Self::ItemTranslationBlock,
            RecipeRawPayloadTarget::ItemUnlockBlock => Self::ItemUnlockBlock,
            RecipeRawPayloadTarget::ItemValueBlock => Self::ItemValueBlock,
            RecipeRawPayloadTarget::ItemInventoryBlock => Self::ItemInventoryBlock,
            RecipeRawPayloadTarget::ItemTraitsDescriptor => Self::ItemTraitsDescriptor,
            RecipeRawPayloadTarget::ItemTraitRows => Self::ItemTraitRows,
            RecipeRawPayloadTarget::ItemStringDefinition => Self::ItemStringDefinition,
            RecipeRawPayloadTarget::ItemDefinitionIndexRow => Self::ItemDefinitionIndexRow,
            RecipeRawPayloadTarget::ItemStringIndexRow => Self::ItemStringIndexRow,
            RecipeRawPayloadTarget::ItemIconRow => Self::ItemIconRow,
            RecipeRawPayloadTarget::DenseIconTagRow => Self::DenseIconTagRow,
            RecipeRawPayloadTarget::DenseIconSelectorRow => Self::DenseIconSelectorRow,
            RecipeRawPayloadTarget::DensePresentationRow => Self::DensePresentationRow,
            RecipeRawPayloadTarget::ItemMetadataRow => Self::ItemMetadataRow,
            RecipeRawPayloadTarget::ItemMetadataIndexRow => Self::ItemMetadataIndexRow,
            RecipeRawPayloadTarget::SandboxPatternRow => Self::SandboxPatternRow,
            RecipeRawPayloadTarget::SandboxPatternIndexRow => Self::SandboxPatternIndexRow,
            RecipeRawPayloadTarget::RuntimeWeaponEntity => Self::RuntimeWeaponEntity,
            RecipeRawPayloadTarget::CollectibleDefinitionRow => Self::CollectibleDefinitionRow,
            RecipeRawPayloadTarget::CollectibleDisplayRow => Self::CollectibleDisplayRow,
            RecipeRawPayloadTarget::UnlockDefinitionRow => Self::UnlockDefinitionRow,
            RecipeRawPayloadTarget::UnlockSortedIndexRow => Self::UnlockSortedIndexRow,
            RecipeRawPayloadTarget::UnlockBankRow => Self::UnlockBankRow,
            RecipeRawPayloadTarget::UnlockDisplayRow => Self::UnlockDisplayRow,
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

impl WeaponLocaleTextRecipe {
    fn to_compiler(&self) -> WeaponLocaleTextOverride {
        WeaponLocaleTextOverride {
            locale_index: self.locale_index,
            name: self.name.clone(),
            type_name: self.type_name.clone(),
            flavor: self.flavor.clone(),
            source: self.source.clone(),
            collection_name: self.collection_name.clone(),
            collection_description: self.collection_description.clone(),
            inventory_hint: self.inventory_hint.clone(),
            collection_requirement: self.collection_requirement.clone(),
        }
    }
}

fn parse_raw_patch_bytes(value: &str, index: usize) -> Result<Vec<u8>, RecipeError> {
    let compact = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '_')
        .collect::<String>();
    let compact = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
        .unwrap_or(&compact);
    if compact.is_empty()
        || compact.len() % 2 != 0
        || !compact.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RecipeError::Validation(format!(
            "Raw payload patch {index} bytes must contain a non-empty even number of hexadecimal digits"
        )));
    }
    compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| {
                    RecipeError::Validation(format!(
                        "Raw payload patch {index} contains malformed hexadecimal bytes"
                    ))
                })
        })
        .collect()
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponRecipeOverrides {
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
    pub power_cap_group: Option<u16>,
    /// Complete native quality/version group sequence. This advanced form preserves the number
    /// and order of the gameplay donor's version rows while allowing every row to differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_cap_groups: Option<Vec<u16>>,
    /// Optional stock rarity tier. `None` preserves the gameplay donor byte.
    pub rarity: Option<RecipeRarity>,
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
    /// Technical same-size edits inside concrete runtime-component resources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_resource_patches: Vec<WeaponRuntimeResourcePatchRecipe>,
    /// Final byte patches for technical fields not yet represented structurally.
    pub raw_payload_patches: Vec<WeaponRawPayloadPatchRecipe>,
}

impl WeaponRecipeOverrides {
    fn to_compiler(&self) -> Result<WeaponCloneOverrides, RecipeError> {
        let socket_columns = self
            .socket_columns
            .iter()
            .enumerate()
            .map(|(socket_index, column)| {
                column
                    .as_ref()
                    .map(|column| {
                        if (column.choices.is_empty() && column.socket_type != Some(u16::MAX))
                            || column.choices.len() > MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES
                        {
                            return Err(RecipeError::Validation(format!(
                                "Socket {socket_index} must contain between 1 and {MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES} ordered choices"
                            )));
                        }
                        let choices = column
                            .choices
                            .iter()
                            .enumerate()
                            .map(|(choice_index, hash)| {
                                parse_recipe_hash(
                                    &format!("socket {socket_index} choice {choice_index}"),
                                    hash,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if choices.contains(&0) {
                            return Err(RecipeError::Validation(format!(
                                "Socket {socket_index} cannot contain plug hash zero"
                            )));
                        }
                        if choices.iter().copied().collect::<BTreeSet<_>>().len() != choices.len() {
                            return Err(RecipeError::Validation(format!(
                                "Socket {socket_index} cannot contain the same plug more than once"
                            )));
                        }
                        Ok(WeaponSocketColumnOverride {
                            choices,
                            socket_type: column.socket_type,
                            choice_weight_bits: column.choice_weight_bits.clone(),
                            choice_conditions: column
                                .choice_conditions
                                .iter()
                                .map(|program| {
                                    program
                                        .iter()
                                        .map(|instruction| WeaponNumericInstruction {
                                            opcode: instruction.opcode,
                                            operand: instruction.operand,
                                        })
                                        .collect()
                                })
                                .collect(),
                            reusable_plug_set_index: column.reusable_plug_set_index,
                            randomized_plug_set_index: column.randomized_plug_set_index,
                            randomized_selection_program: column
                                .randomized_selection_program
                                .iter()
                                .map(|instruction| WeaponNumericInstruction {
                                    opcode: instruction.opcode,
                                    operand: instruction.operand,
                                })
                                .collect(),
                        })
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut investment_stats = self
            .investment_stats
            .iter()
            .map(|stat| (stat.definition_index, stat.value))
            .collect::<Vec<_>>();
        investment_stats.sort_unstable_by_key(|(definition_index, _)| *definition_index);
        let mut removed_investment_stats = self.removed_investment_stats.clone();
        removed_investment_stats.sort_unstable();
        if let Some(hash) = &self.weapon_pattern_donor_hash {
            parse_recipe_hash("weapon-pattern donor", hash)?;
        }
        if let Some(hash) = &self.stat_group_donor_hash {
            parse_recipe_hash("stat-group donor", hash)?;
        }
        Ok(WeaponCloneOverrides {
            icon_edit: self.icon_edit.clone(),
            hud_icon: self.hud_icon.clone(),
            badge: self.badge.clone(),
            exclude_from_sunrise_badge: self.exclude_from_sunrise_badge,
            collection_destination: self.collection_destination,
            corner_icon: self.corner_icon.clone(),
            lore: self.lore.clone(),
            investment_stats,
            removed_investment_stats,
            base_sandbox_perks: self.base_sandbox_perks.clone(),
            trait_indices: self.trait_indices.clone(),
            max_stack_size: self.max_stack_size,
            socket_entry_list_index: self.socket_entry_list_index,
            plug_category_hash: self
                .plug_category_hash
                .as_ref()
                .map(|hash| parse_recipe_hash("plug category", hash))
                .transpose()?,
            roll_set_index: self.roll_set_index,
            linked_plug_index: self.linked_plug_index,
            inventory_slot: self.inventory_slot.map(WeaponInventorySlot::from),
            ammo_type: self.ammo_type.map(WeaponAmmoType::from),
            modern_damage_type: self.modern_damage_type.map(ModernDamageType::from),
            power_cap_group: self.power_cap_group,
            power_cap_groups: self.power_cap_groups.clone(),
            rarity: self.rarity.map(AuthoredWeaponRarity::from),
            weapon_pattern_index: self.weapon_pattern_index,
            stat_group_index: self.stat_group_index,
            art_arrangements: self.art_arrangements.as_ref().map(|rows| {
                rows.iter()
                    .map(|row| WeaponArtArrangementOverride {
                        character_class: row.character_class,
                        arrangement: row.arrangement,
                    })
                    .collect()
            }),
            render_dye_rows: self.render_dye_rows.as_ref().map(|arrays| {
                std::array::from_fn(|array| {
                    arrays[array]
                        .iter()
                        .map(|row| WeaponDyeReferenceOverride {
                            channel_index: row.channel_index,
                            dye_reference_index: row.dye_reference_index,
                        })
                        .collect()
                })
            }),
            socket_columns,
            socket_plug_variants: self
                .socket_plug_variants
                .iter()
                .enumerate()
                .map(|(variant_index, variant)| {
                    Ok(WeaponSocketPlugVariantOverride {
                        replace_effects: variant.replace_effects,
                        investment_stats: variant.investment_stats.iter()
                            .map(|stat| (stat.definition_index, stat.value)).collect(),
                        socket_index: variant.socket_index,
                        choice_index: variant.choice_index,
                        source_plug_hash: parse_recipe_hash(
                            &format!("socket-plug variant {variant_index} source plug"),
                            &variant.source_plug_hash,
                        )?,
                        name: variant.name.clone(),
                        description: variant.description.clone(),
                        additional_sandbox_perks: variant.additional_sandbox_perks.clone(),
                        classification_donor_hash: variant.classification_donor_hash.as_ref()
                            .map(|hash| parse_recipe_hash(
                                &format!("socket-plug variant {variant_index} classification source"), hash))
                            .transpose()?,
                        sandbox_perks: variant
                            .sandbox_perks
                            .iter()
                            .enumerate()
                            .map(|(perk_index, perk)| {
                                Ok(WeaponSandboxPerkRuntimeOverride {
                                    program: perk.program.clone(),
                                    source_perk_index: perk.source_perk_index,
                                    projectiles: perk.projectiles.clone(),
                                    activation: perk.activation,
                                    runtime_values: perk.runtime_values.clone(),
                                    action_float_values: perk
                                        .action_float_values
                                        .iter()
                                        .enumerate()
                                        .map(|(value_index, value)| {
                                            Ok(WeaponSandboxPerkActionFloatOverride {
                                                node_type_handle: parse_recipe_hash(
                                                    &format!(
                                                        "socket-plug variant {variant_index} perk {perk_index} action float {value_index} node type"
                                                    ),
                                                    &value.node_type_handle,
                                                )?,
                                                node_occurrence: value.node_occurrence,
                                                value_pointer_offset: value.value_pointer_offset,
                                                value_type_handle: parse_recipe_hash(
                                                    &format!(
                                                        "socket-plug variant {variant_index} perk {perk_index} action float {value_index} value type"
                                                    ),
                                                    &value.value_type_handle,
                                                )?,
                                                expected_bits: value.expected_bits,
                                                value_bits: value.value_bits,
                                            })
                                        })
                                        .collect::<Result<Vec<_>, RecipeError>>()?,
                                })
                            })
                            .collect::<Result<Vec<_>, RecipeError>>()?,
                    })
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
            runtime_values: self.runtime_values.clone(),
            runtime_resource_patches: self
                .runtime_resource_patches
                .iter()
                .enumerate()
                .map(|(index, patch)| {
                    Ok(crate::WeaponRuntimeResourcePatch {
                        binding_hash: parse_recipe_hash(
                            &format!("runtime resource patch {index} binding"),
                            &patch.binding_hash,
                        )?,
                        resource_index: patch.resource_index,
                        offset: patch.offset,
                        bytes: parse_raw_patch_bytes(&patch.bytes, index)?,
                        graph_values: patch.graph_values.clone(),
                    })
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
            raw_payload_patches: self
                .raw_payload_patches
                .iter()
                .enumerate()
                .map(|(index, patch)| {
                    Ok(WeaponRawPayloadPatch {
                        target: patch.target.into(),
                        offset: patch.offset,
                        bytes: parse_raw_patch_bytes(&patch.bytes, index)?,
                    })
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRecipe {
    pub schema: u32,
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

    pub fn validate(&self) -> Result<(), RecipeError> {
        self.to_spec().map(|_| ())
    }

    pub fn to_spec(&self) -> Result<WeaponCloneSpec, RecipeError> {
        if self.schema != RECIPE_SCHEMA {
            return Err(RecipeError::Validation(format!(
                "Unsupported recipe schema {}; expected {RECIPE_SCHEMA}",
                self.schema
            )));
        }
        let spec = WeaponCloneSpec {
            namespace: self.namespace.clone(),
            donor_item_hash: parse_recipe_hash("donor.item_hash", &self.donor.item_hash)?,
            expected_donor_name: self.donor.expected_name.clone(),
            presentation_donor: self
                .presentation_donor
                .as_ref()
                .map(|donor| {
                    Ok::<_, RecipeError>(crate::WeaponPresentationDonorReference {
                        item_hash: parse_recipe_hash(
                            "presentation_donor.item_hash",
                            &donor.item_hash,
                        )?,
                        expected_name: donor.expected_name.clone(),
                    })
                })
                .transpose()?,
            render_gear_donor: self
                .render_gear_donor
                .as_ref()
                .map(|donor| {
                    Ok::<_, RecipeError>(WeaponRenderGearDonorReference {
                        item_hash: parse_recipe_hash(
                            "render_gear_donor.item_hash",
                            &donor.item_hash,
                        )?,
                        expected_name: donor.expected_name.clone(),
                    })
                })
                .transpose()?,
            icon_donor: self
                .icon_donor
                .as_ref()
                .map(|donor| {
                    Ok::<_, RecipeError>(crate::WeaponIconDonorReference {
                        item_hash: parse_recipe_hash("icon_donor.item_hash", &donor.item_hash)?,
                        expected_name: donor.expected_name.clone(),
                    })
                })
                .transpose()?,
            runtime_component_donors: self
                .runtime_component_donors
                .iter()
                .map(|component| {
                    Ok::<_, RecipeError>(crate::WeaponRuntimeComponentDonorReference {
                        binding_hash: parse_recipe_hash(
                            "runtime_component_donors.binding_hash",
                            &component.binding_hash,
                        )?,
                        item_hash: parse_recipe_hash(
                            "runtime_component_donors.donor.item_hash",
                            &component.donor.item_hash,
                        )?,
                        expected_name: component.donor.expected_name.clone(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            identity: self.identity.to_compiler(&self.namespace)?,
            text: WeaponCloneText {
                name: self.name.clone(),
                type_name: self.type_name.clone(),
                flavor: self.flavor.clone(),
                source: self.source.clone(),
                collection_name: self.collection_name.clone(),
                collection_description: self.collection_description.clone(),
                inventory_hint: self.inventory_hint.clone(),
                collection_requirement: self.collection_requirement.clone(),
                locale_overrides: self
                    .locale_overrides
                    .iter()
                    .map(WeaponLocaleTextRecipe::to_compiler)
                    .collect(),
            },
            overrides: self.overrides.to_compiler()?,
        };
        spec.validate()?;
        Ok(spec)
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

    pub fn from_json_str(encoded: &str) -> Result<Self, RecipeError> {
        let mut recipe: Self = sundial::package_authoring::parse_json(encoded)?;
        if recipe.schema != RECIPE_SCHEMA {
            return Err(RecipeError::Validation(format!(
                "Unsupported recipe schema {}; expected {RECIPE_SCHEMA}",
                recipe.schema
            )));
        }
        recipe.canonicalize_investment_stats();
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn to_json_pretty(&self) -> Result<String, RecipeError> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.canonicalize_investment_stats();
        Ok(serde_json::to_string_pretty(&canonical)?)
    }

    pub fn load_json(path: impl AsRef<Path>) -> Result<Self, RecipeError> {
        let path = path.as_ref();
        let encoded = fs::read_to_string(path).map_err(|source| RecipeError::Io {
            operation: "read recipe",
            path: path.to_owned(),
            source,
        })?;
        Self::from_json_str(&encoded)
    }

    pub fn save_json(&self, path: impl AsRef<Path>) -> Result<(), RecipeError> {
        let path = path.as_ref();
        let mut encoded = self.to_json_pretty()?;
        encoded.push('\n');
        sundial::package_authoring::replace_authoring_file(path, encoded.as_bytes()).map_err(
            |source| RecipeError::Io {
                operation: "write recipe",
                path: path.to_owned(),
                source,
            },
        )
    }

    fn canonicalize_investment_stats(&mut self) {
        for variant in &mut self.overrides.socket_plug_variants {
            variant
                .investment_stats
                .sort_unstable_by_key(|stat| stat.definition_index);
        }
        self.overrides
            .investment_stats
            .sort_unstable_by_key(|stat| stat.definition_index);
        self.overrides.removed_investment_stats.sort_unstable();
        self.locale_overrides
            .sort_unstable_by_key(|locale| locale.locale_index);
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
            "Weapon name produces a namespace longer than 64 characters; shorten it to at most {max_slug_length} ASCII letters or digits"
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

fn parse_recipe_hash(description: &str, hash: &HexHash) -> Result<u32, RecipeError> {
    hash.parse_u32()
        .map_err(|error| RecipeError::Validation(format!("Invalid {description}: {error}")))
}

#[derive(Debug)]
pub enum RecipeError {
    Validation(String),
    Authoring(AuthoringError),
    Json(serde_json::Error),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => formatter.write_str(message),
            Self::Authoring(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "Invalid weapon recipe JSON: {error}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "Could not {operation} {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RecipeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Authoring(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Validation(_) => None,
        }
    }
}

impl From<AuthoringError> for RecipeError {
    fn from(value: AuthoringError) -> Self {
        Self::Authoring(value)
    }
}

impl From<serde_json::Error> for RecipeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(test)]
mod tests;
