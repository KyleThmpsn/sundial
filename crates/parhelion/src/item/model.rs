//! Compiler models and checked input contracts shared by every item kind.

use super::*;

/// Collision-sensitive identities for one independently-authored weapon clone.
///
/// These are investment-row identities, not package tag hashes. Parhelion can derive a stable
/// set from a namespace with [`Self::from_namespace`], or callers may provide audited values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponCloneIdentity {
    pub item_hash: u32,
    pub collectible_hash: u32,
    pub unlock_hash: u32,
    pub pattern_global_id_hash: u32,
    pub name_hash: u32,
    pub type_hash: u32,
    pub flavor_hash: u32,
    pub source_hash: u32,
    pub collection_name_hash: u32,
    pub collection_description_hash: u32,
    pub inventory_hint_hash: u32,
    pub collection_requirement_hash: u32,
}

impl WeaponCloneIdentity {
    pub fn from_namespace(namespace: &str) -> AuthoringResult<Self> {
        validate_parhelion_namespace(namespace).map_err(invalid)?;
        let mut occupied = BTreeSet::from(LOCALIZATION_DONOR_STRING_HASHES);
        let item_hash = allocate_identity_hash(namespace, "item", &mut occupied, None)?;
        let collectible_hash =
            allocate_identity_hash(namespace, "collectible", &mut occupied, None)?;
        let unlock_hash = allocate_identity_hash(namespace, "unlock", &mut occupied, None)?;
        let pattern_global_id_hash =
            allocate_identity_hash(namespace, "pattern_global", &mut occupied, None)?;
        let donor_terminal_hash = Some(LOCALIZATION_DONOR_STRING_HASHES[1]);
        let name_hash =
            allocate_identity_hash(namespace, "name", &mut occupied, donor_terminal_hash)?;
        let type_hash =
            allocate_identity_hash(namespace, "type", &mut occupied, donor_terminal_hash)?;
        let flavor_hash =
            allocate_identity_hash(namespace, "flavor", &mut occupied, donor_terminal_hash)?;
        let source_hash =
            allocate_identity_hash(namespace, "source", &mut occupied, donor_terminal_hash)?;
        let collection_name_hash = allocate_identity_hash(
            namespace,
            "collection_name",
            &mut occupied,
            donor_terminal_hash,
        )?;
        let collection_description_hash = allocate_identity_hash(
            namespace,
            "collection_description",
            &mut occupied,
            donor_terminal_hash,
        )?;
        let inventory_hint_hash = allocate_identity_hash(
            namespace,
            "inventory_hint",
            &mut occupied,
            donor_terminal_hash,
        )?;
        let collection_requirement_hash = allocate_identity_hash(
            namespace,
            "collection_requirement",
            &mut occupied,
            donor_terminal_hash,
        )?;
        Ok(Self {
            item_hash,
            collectible_hash,
            unlock_hash,
            pattern_global_id_hash,
            name_hash,
            type_hash,
            flavor_hash,
            source_hash,
            collection_name_hash,
            collection_description_hash,
            inventory_hint_hash,
            collection_requirement_hash,
        })
    }

    pub(super) fn validate_for_donor(self, donor_item_hash: u32) -> AuthoringResult<()> {
        let values = [
            self.item_hash,
            self.collectible_hash,
            self.unlock_hash,
            self.pattern_global_id_hash,
            self.name_hash,
            self.type_hash,
            self.flavor_hash,
            self.source_hash,
            self.collection_name_hash,
            self.collection_description_hash,
            self.inventory_hint_hash,
            self.collection_requirement_hash,
        ];
        if values.contains(&0)
            || values.contains(&FNV1_EMPTY_HASH)
            || values.into_iter().collect::<BTreeSet<_>>().len() != values.len()
        {
            return Err(invalid(
                "Weapon identity hashes must be nonzero, cannot use the reserved empty-name hash, and must be pairwise distinct",
            ));
        }
        if self.item_hash == donor_item_hash {
            return Err(invalid("The authored item hash collides with its donor"));
        }
        if [
            self.flavor_hash,
            self.name_hash,
            self.source_hash,
            self.type_hash,
            self.collection_name_hash,
            self.collection_description_hash,
            self.inventory_hint_hash,
            self.collection_requirement_hash,
        ]
        .into_iter()
        .any(|hash| hash <= LOCALIZATION_DONOR_STRING_HASHES[1])
        {
            return Err(invalid(
                "Localized hashes must sort after the donor bank's terminal hash and cannot use the reserved empty-name hash",
            ));
        }
        Ok(())
    }
}

/// Localized text authored for a weapon clone.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeaponCloneText {
    pub name: String,
    /// Optional authored item-type label. `None` preserves the donor reference.
    pub type_name: Option<String>,
    pub flavor: String,
    pub source: String,
    /// Optional name used only by the Collections collectible display.
    pub collection_name: Option<String>,
    /// Optional description used only by the Collections collectible display.
    pub collection_description: Option<String>,
    /// Optional item-string display-source line shown on the inventory tooltip.
    pub inventory_hint: Option<String>,
    /// Optional requirement/warning line used only by the Collections collectible display.
    pub collection_requirement: Option<String>,
    /// Sparse locale-payload replacements. Locale indices are the native ordered payload slots;
    /// fields left `None` use the recipe's primary text.
    pub locale_overrides: Vec<WeaponLocaleTextOverride>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeaponLocaleTextOverride {
    pub locale_index: u8,
    pub name: Option<String>,
    pub type_name: Option<String>,
    pub flavor: Option<String>,
    pub source: Option<String>,
    pub collection_name: Option<String>,
    pub collection_description: Option<String>,
    pub inventory_hint: Option<String>,
    pub collection_requirement: Option<String>,
}

/// An explicit fixed damage override, resolved against the donor's native carrier family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModernDamageType {
    Kinetic,
    Arc,
    Solar,
    Void,
}

/// Element switching by holding Reload, the way Hard Light and Borealis work.
///
/// The hold itself is client-side and exists only on those two weapons' gear-art rows, so the
/// appearance donor has to be one of them. The switchable effects are the three stock rows on
/// Hard Light's Fundamentals plug, one Set Host Mode node per element, each gated on the selector
/// value the hold steps through: 0 is Void, 1 is Arc, 2 is Solar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponVariableDamage {
    /// Elements the cycle can settle on. A selector step without a chosen element keeps the
    /// element the weapon already has, so a two-element set repeats one element for one hold.
    pub elements: Vec<ModernDamageType>,
}

/// The inventory column occupied by an authored weapon.
///
/// This is intentionally separate from weapon/ammo category. The compact inventory bucket and
/// native equipment-slot block occupy different index spaces; conversions author both only after
/// validating their independent donor values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponInventorySlot {
    Kinetic,
    Energy,
    Power,
}

/// Primary/Special/Heavy classification, authored in both item strings and native weapon-content
/// properties. This does not set magazine capacity, reserves or inventory slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WeaponAmmoType {
    Primary = 1,
    Special = 2,
    Heavy = 3,
}

impl WeaponAmmoType {
    pub(super) const fn package_value(self) -> u16 {
        self as u16
    }

    pub(super) fn from_package_value(value: u16) -> AuthoringResult<Option<Self>> {
        match value {
            0 => Ok(None),
            1 => Ok(Some(Self::Primary)),
            2 => Ok(Some(Self::Special)),
            3 => Ok(Some(Self::Heavy)),
            _ => Err(invalid(format!(
                "Weapon uses unsupported ammunition classification {value}; expected 0 through 3"
            ))),
        }
    }
}

/// The five authored inventory-quality tiers encoded by the verified item rarity byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AuthoredWeaponRarity {
    Common = 1,
    Uncommon = 2,
    Rare = 3,
    Legendary = 4,
    Exotic = 5,
}

impl AuthoredWeaponRarity {
    /// Native Shadowkeep inventory backgrounds, verified against stock weapons in each tier.
    /// Keep this separate from primary artwork: an Exotic appearance does not imply Exotic rarity.
    pub(crate) const fn icon_background_layer(self) -> TagHash {
        TagHash(match self {
            Self::Common => 0x8132_0719,    // Khvostov 7G-02
            Self::Uncommon => 0x8132_1AFD,  // Cydonia-AR1
            Self::Rare => 0x8131_8517,      // Cuboid ARu
            Self::Legendary => 0x8131_819C, // Age-Old Bond
            Self::Exotic => 0x8132_3525,    // Cerberus+1
        })
    }

    pub(super) const fn package_value(self) -> u8 {
        self as u8
    }

    pub(super) fn from_package_value(value: u8) -> AuthoringResult<Self> {
        match value {
            1 => Ok(Self::Common),
            2 => Ok(Self::Uncommon),
            3 => Ok(Self::Rare),
            4 => Ok(Self::Legendary),
            5 => Ok(Self::Exotic),
            _ => Err(invalid(format!(
                "Weapon uses unsupported rarity byte {value}; expected a value from 1 through 5"
            ))),
        }
    }
}

impl ModernDamageType {
    pub(crate) const fn shared(self) -> WeaponDamageType {
        match self {
            Self::Kinetic => WeaponDamageType::Kinetic,
            Self::Arc => WeaponDamageType::Arc,
            Self::Solar => WeaponDamageType::Solar,
            Self::Void => WeaponDamageType::Void,
        }
    }

    pub(super) const fn descriptor(self) -> WeaponDamageDescriptor {
        match self {
            Self::Kinetic => WeaponDamageDescriptor::Empty,
            Self::Arc | Self::Solar | Self::Void => WeaponDamageDescriptor::Elemental(self),
        }
    }

    #[cfg(test)]
    pub(super) const fn modern_sandbox_perk_index(self) -> Option<u16> {
        WeaponDamageCarrierFamily::ModernFixed.base_sandbox_perk_index(self.shared())
    }
}

impl WeaponInventorySlot {
    pub(super) const fn bucket_hash(self) -> u32 {
        match self {
            Self::Kinetic => 0x5957_0ADA,
            Self::Energy => 0x92F1_6AD9,
            Self::Power => 0x38DC_DD35,
        }
    }

    pub(super) const fn from_bucket_hash(value: u32) -> Option<Self> {
        match value {
            0x5957_0ADA => Some(Self::Kinetic),
            0x92F1_6AD9 => Some(Self::Energy),
            0x38DC_DD35 => Some(Self::Power),
            _ => None,
        }
    }

    pub(super) const fn root_value(self) -> u8 {
        match self {
            Self::Kinetic => 0,
            Self::Energy => 1,
            Self::Power => 2,
        }
    }

    pub(super) const fn equipment_value(self) -> u16 {
        match self {
            Self::Kinetic => 7,
            Self::Energy => 8,
            Self::Power => 9,
        }
    }

    pub(super) const fn from_root_value(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Kinetic),
            1 => Some(Self::Energy),
            2 => Some(Self::Power),
            _ => None,
        }
    }

    pub(super) const fn from_equipment_value(value: u16) -> Option<Self> {
        match value {
            7 => Some(Self::Kinetic),
            8 => Some(Self::Energy),
            9 => Some(Self::Power),
            _ => None,
        }
    }
}

/// Optional changes applied after cloning the donor definition.
///
/// Empty/`None` fields inherit the donor bytes. Socket columns are sparse, ordered overrides:
/// `None` preserves donor socket content, while the first choice in an authored
/// column is both the definition's collection/default-roll plug and its first embedded member.
/// Existing inventory instances retain their saved selections. Because authored collection items
/// are curated, an inherited randomized donor lane is automatically fixed to its native default
/// and embedded choices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponCloneOverrides {
    pub sparrow: Option<crate::vehicle::Sparrow>,
    pub remove_lore: bool,
    #[cfg(feature = "d2-model-importer")]
    pub imported_graph: Option<parhelion_import::GraphReference>,
    /// Optional Collections override. `None` derives the ammo and weapon-type destination.
    pub collection_destination: Option<crate::collection::Destination>,
    pub exclude_from_sunrise_badge: bool,
    pub badge: Option<crate::presentation::Badge>,
    pub corner_icon: Option<crate::presentation::Artwork>,
    pub lore: Option<String>,
    pub icon_edit: crate::WeaponIconEdit,
    pub hud_icon: Option<crate::hud_icon::HudImage>,
    pub investment_stats: Vec<(u16, i32)>,
    /// Investment-stat definition rows removed from the gameplay donor.
    pub removed_investment_stats: Vec<u16>,
    /// Complete ordered base-item sandbox-perk indices. `None` preserves the donor array.
    /// Socket-plug perks are authored separately through [`Self::socket_columns`].
    pub base_sandbox_perks: Option<Vec<u16>>,
    /// Complete ordered native item-trait indices. `None` preserves the donor array.
    pub trait_indices: Option<Vec<u16>>,
    /// Native inline inventory quantity limit. Stock instanced weapons normally use one.
    pub max_stack_size: Option<u32>,
    /// Native socket-entry-list row stored in the talent-grid holder.
    pub socket_entry_list_index: Option<u16>,
    /// Native plug-category hash stored in the item's plug metadata.
    pub plug_category_hash: Option<u32>,
    /// Native randomized-roll-set row stored in the item's plug metadata.
    pub roll_set_index: Option<u16>,
    /// Native linked-item row stored in the item's linked-plug metadata.
    pub linked_plug_index: Option<u16>,
    pub inventory_slot: Option<WeaponInventorySlot>,
    pub ammo_type: Option<WeaponAmmoType>,
    pub modern_damage_type: Option<ModernDamageType>,
    /// Reload-hold element switching. Compilation pins The Fundamentals into the first trait
    /// socket, and [`Self::modern_damage_type`] is then the element the weapon rests on.
    pub variable_damage: Option<WeaponVariableDamage>,
    /// Catalogue identifiers of exotic behavior records grafted onto this weapon's variant block.
    pub additional_behaviors: Vec<String>,
    /// Leave each grafted behavior's own intrinsic and trait plugs out of the graft.
    pub skip_behavior_perks: bool,
    /// Whose firing pattern the weapon uses when a borrowed plug changes its burst.
    pub behavior_firing: crate::weapon::behavior::BehaviorFiring,
    /// Raises a grafted projectile's launch speed on a weapon that fires none of its own, and
    /// caps how far it is raised.
    pub behavior_projectile_speed: Option<f32>,
    /// A privately edited projectile the weapon fires as its own firing graph.
    pub fired_graph: Option<crate::weapon::behavior::FiredGraph>,
    /// Values of the projectile the weapon fires, set on a private copy of it.
    pub projectile: Option<crate::weapon::projectile::Edits>,
    pub barrel: Option<crate::weapon::barrel::Edits>,
    /// Another weapon whose first-person animations the weapon plays. Its row has to share the
    /// first-person attachment owner the model's rig uses.
    pub animation_donor: Option<u32>,
    /// Single first-person actions played from another weapon's animations on the same rig.
    pub animation_actions: Vec<(crate::recipe::AnimationAction, u32)>,
    /// Another weapon whose type markers the weapon's runtime block carries in place of the base
    /// weapon's own. Its block has to sit in the same content owner.
    pub type_marker_donor: Option<u32>,
    /// Gear-art markers moved by name, each by an offset in metres along the model's forward,
    /// side and up axes.
    pub marker_offsets: Vec<(u32, [f32; 3])>,
    /// Gameplay components copied from other weapons onto the weapon's own objects, as the
    /// component binding and the donor item.
    pub component_splices: Vec<(u32, u32)>,
    /// How far the whole model and its markers move from the handle the hand holds, in metres
    /// along the model's forward, side and up axes.
    pub held_offset: Option<[f32; 3]>,
    pub power_cap_group: Option<u16>,
    /// Complete ordered native version-group values. Mutually exclusive with
    /// [`Self::power_cap_group`] and required to match the donor row count.
    pub power_cap_groups: Option<Vec<u16>>,
    pub rarity: Option<AuthoredWeaponRarity>,
    /// Armor equip eligibility. None keeps the donor's class.
    pub armor_class: Option<crate::ArmorClass>,
    /// Stock gear-art/runtime row used as the runtime entity source. Compilation combines its
    /// runtime identity with the selected appearance donor's gear-art row. `None` follows the
    /// gameplay donor.
    pub weapon_pattern_index: Option<u16>,
    pub stat_group_index: Option<u16>,
    /// A stat display group of the recipe's own. The build appends it to the stat group table
    /// and sets [`Self::stat_group_index`] to its row.
    pub custom_stat_group: Option<crate::stat_group::CustomStatGroup>,
    /// Complete ordered translation-art rows. `None` preserves the selected appearance tuple.
    pub art_arrangements: Option<Vec<WeaponArtArrangementOverride>>,
    /// Complete ordered custom, default, and locked dye-reference rows.
    pub render_dye_rows: Option<[Vec<WeaponDyeReferenceOverride>; 3]>,
    /// A subclass's abilities and attunements taken from other stock subclasses.
    pub subclass_abilities: Option<crate::subclass::SubclassAbilities>,
    /// Removes the donor-class equip requirement and defaults to Guardian Subclass text.
    pub subclass_every_class: bool,
    /// Points the donor-class equip requirement at another class, which receives the subclass
    /// and names its default type label. None keeps the donor's class.
    pub subclass_class: Option<crate::ArmorClass>,
    /// The damage type a subclass's strings give it, whose icon sits beside its name. None
    /// keeps the donor's.
    pub subclass_damage_type: Option<crate::recipe::RecipeDamageType>,
    /// A shader's custom surface values, by gear type, channel and surface.
    pub dye_edits: Vec<crate::dye::DyeEdit>,
    pub shader_glow: bool,
    /// Keeps the base weapon's type name and Collections page under another weapon's
    /// appearance, which otherwise supplies both.
    pub base_type: bool,
    /// A shader's custom detail textures and tiling, by gear type and channel.
    pub dye_texture_edits: Vec<crate::dye::DyeTextureEdit>,
    /// An emblem's nameplate images, each its base's, another emblem's or a picture.
    pub nameplate: Option<crate::emblem::Nameplate>,
    /// A subclass's screen pictures, each its base's, another subclass's or a picture.
    pub screen_art: Option<crate::subclass::ScreenArt>,
    /// A subclass icon drawn as a diamond in the HUD color, in place of the icon's image.
    pub subclass_icon: Option<crate::subclass::GeneratedIcon>,
    pub stat_trackers: Option<crate::emblem::StatTrackers>,
    /// Positional donor overrides followed by any added sockets, up to the native lane limit.
    /// Inherited rows preserve their content, with relative pointers rebased if the array grows.
    /// Each added socket requires an explicit type and at least one plug choice.
    pub socket_columns: Vec<Option<WeaponSocketColumnOverride>>,
    /// Private socket-plug variants whose finished perk runtime graphs carry typed edits.
    pub socket_plug_variants: Vec<WeaponSocketPlugVariantOverride>,
    /// Typed values in the effective package-backed runtime graph.
    pub runtime_values: Vec<WeaponRuntimeValueOverride>,
    /// Adds private keyed sword attack profiles to the authored weapon runtime.
    /// The matching private perk must add the same key while its effect is active.
    pub sword_profile: Option<SwordProfileOverride>,
    /// Same-size byte replacements inside the concrete runtime resource selected by a component
    /// binding. Offsets are relative to the selected resource, not its owner tag.
    pub runtime_resource_patches: Vec<WeaponRuntimeResourcePatch>,
    /// Final byte patches applied to this weapon's newly cloned records after structured fields.
    pub raw_payload_patches: Vec<WeaponRawPayloadPatch>,
}

/// A private sword profile copied from each ordinary profile, with angular endpoints scaled
/// while `key` is held by the authored private perk's native host-reference effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwordProfileOverride {
    pub key: u32,
    pub near_scale_bits: u32,
    pub far_scale_bits: u32,
}

/// The table or block a raw payload patch rewrites. The recipe's own enum, since the two
/// listed the same forty-one targets.
pub use crate::recipe::RecipeRawPayloadTarget as WeaponRawPayloadTarget;

pub(super) const ITEM_ROOT_RAW_TARGETS: [(WeaponRawPayloadTarget, usize); 19] = [
    (WeaponRawPayloadTarget::ItemActionBlock, 0x08),
    (WeaponRawPayloadTarget::ItemEquippingBlock, 0x10),
    (WeaponRawPayloadTarget::ItemFinisherBlock, 0x18),
    (WeaponRawPayloadTarget::ItemGearsetBlock, 0x20),
    (WeaponRawPayloadTarget::ItemLoreBlock, 0x28),
    (WeaponRawPayloadTarget::ItemObjectiveBlock, 0x30),
    (WeaponRawPayloadTarget::ItemMetricBlock, 0x38),
    (WeaponRawPayloadTarget::ItemPlugBlock, 0x40),
    (WeaponRawPayloadTarget::ItemQualityBlock, 0x48),
    (WeaponRawPayloadTarget::ItemRecordBlock, 0x50),
    (WeaponRawPayloadTarget::ItemSackBlock, 0x58),
    (WeaponRawPayloadTarget::ItemSetBlock, 0x60),
    (WeaponRawPayloadTarget::ItemSocketsBlock, 0x68),
    (WeaponRawPayloadTarget::ItemStatsBlock, 0x70),
    (WeaponRawPayloadTarget::ItemSummaryBlock, 0x78),
    (WeaponRawPayloadTarget::ItemTalentGridBlock, 0x80),
    (WeaponRawPayloadTarget::ItemTranslationBlock, 0x88),
    (WeaponRawPayloadTarget::ItemUnlockBlock, 0x90),
    (WeaponRawPayloadTarget::ItemValueBlock, 0x98),
];

pub(super) const ITEM_DEFINITION_RAW_SUBTARGETS: [WeaponRawPayloadTarget; 22] = [
    WeaponRawPayloadTarget::ItemActionBlock,
    WeaponRawPayloadTarget::ItemEquippingBlock,
    WeaponRawPayloadTarget::ItemFinisherBlock,
    WeaponRawPayloadTarget::ItemGearsetBlock,
    WeaponRawPayloadTarget::ItemLoreBlock,
    WeaponRawPayloadTarget::ItemObjectiveBlock,
    WeaponRawPayloadTarget::ItemMetricBlock,
    WeaponRawPayloadTarget::ItemPlugBlock,
    WeaponRawPayloadTarget::ItemQualityBlock,
    WeaponRawPayloadTarget::ItemRecordBlock,
    WeaponRawPayloadTarget::ItemSackBlock,
    WeaponRawPayloadTarget::ItemSetBlock,
    WeaponRawPayloadTarget::ItemSocketsBlock,
    WeaponRawPayloadTarget::ItemStatsBlock,
    WeaponRawPayloadTarget::ItemSummaryBlock,
    WeaponRawPayloadTarget::ItemTalentGridBlock,
    WeaponRawPayloadTarget::ItemTranslationBlock,
    WeaponRawPayloadTarget::ItemUnlockBlock,
    WeaponRawPayloadTarget::ItemValueBlock,
    WeaponRawPayloadTarget::ItemInventoryBlock,
    WeaponRawPayloadTarget::ItemTraitsDescriptor,
    WeaponRawPayloadTarget::ItemTraitRows,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRawPayloadPatch {
    pub target: WeaponRawPayloadTarget,
    pub offset: u32,
    pub bytes: Vec<u8>,
}

/// One technical byte replacement inside a concrete runtime-component resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeResourcePatch {
    pub binding_hash: u32,
    /// Zero-based resource selected from a multi-resource native binding.
    pub resource_index: u16,
    /// Byte offset relative to the concrete resource start.
    pub offset: u32,
    pub bytes: Vec<u8>,
    /// Edits to a private clone of the graph identified by the four replacement bytes.
    pub graph_values: Vec<WeaponRuntimeValueOverride>,
    /// Component owners that private clone leaves out entirely. Only a behavior graft whose host
    /// cannot carry part of the graph sets these, and recipes never do.
    pub graph_removals: Vec<u32>,
    /// Optional trajectory capacity for an internal private-graph edit. Final weapon compilation
    /// reconciles it with the composed Barrel after all edits. Recipes do not set it directly.
    pub graph_trajectories: Option<u16>,
}

/// Adds a self-contained record to the end of a component owner and points slots at it.
///
/// A patch can only overwrite bytes that already exist, so a weapon whose family has no record of
/// its own needs one appended. Every relative pointer inside the added bytes is self relative, and
/// appending never moves existing data, so the rest of the payload stays valid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeResourceAppend {
    pub binding_hash: u32,
    pub resource_index: u16,
    /// Bytes added at the end of the owner payload.
    pub bytes: Vec<u8>,
    /// Slots to fill in, as the slot's resource-relative offset, the offset inside `bytes` it
    /// should reach, and the count written in the following eight bytes.
    pub slots: Vec<(u32, usize, i64)>,
    /// Native array descriptors to point into the added bytes: the descriptor's resource-relative
    /// offset, the offset inside `bytes` of the array's 16-byte header, and the element count. A
    /// descriptor holds the count and then a pointer relative to its own second word. Bytes with
    /// array headers are placed on a 16-byte boundary.
    pub arrays: Vec<(u32, usize, u64)>,
    /// Relative pointer words in the existing resource and their targets inside `bytes`.
    pub pointers: Vec<(u32, usize)>,
    /// Absolute-offset words inside `bytes` and their targets inside `bytes`. Records carrying
    /// these relocations are placed on a 16-byte boundary before their references are fixed.
    pub references: Vec<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponArtArrangementOverride {
    pub character_class: i8,
    pub arrangement: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponDyeReferenceOverride {
    pub channel_index: i8,
    pub dye_reference_index: u16,
}

/// One curated socket column. Choice order is significant and `choices[0]` is the default.
/// An empty column with socket type `u16::MAX` removes an existing socket without shifting lanes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeaponSocketColumnOverride {
    pub choices: Vec<u32>,
    pub socket_type: Option<u16>,
    /// IEEE-754 bit patterns aligned with `choices`. Empty means 1.0 for every choice.
    pub choice_weight_bits: Vec<u32>,
    /// RPN conditions aligned with `choices`. Empty means every choice is unconditional.
    pub choice_conditions: Vec<Vec<WeaponNumericInstruction>>,
    pub reusable_plug_set_index: Option<u16>,
    pub randomized_plug_set_index: Option<u16>,
    pub randomized_selection_program: Vec<WeaponNumericInstruction>,
}

/// One finished sandbox-perk chain cloned privately for an authored socket plug.
///
/// Only this finished-perk row and its runtime action/entity chain become private. Unmodified
/// fixed-element markers retain their stock identities. Other perks carried by the source plug
/// continue to reference their stock rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponSandboxPerkRuntimeOverride {
    pub program: Option<sundial::package_authoring::sandbox_perk::program::Program>,
    pub projectiles: Vec<sundial::package_authoring::sandbox_perk::entity::Selection>,
    pub source_perk_index: u16,
    pub activation: Option<sundial::package_authoring::sandbox_perk::activation::PerkActivation>,
    pub runtime_values: Vec<WeaponRuntimeValueOverride>,
    /// Scalar values stored directly in the finished-perk action rather than a referenced weapon
    /// entity graph.
    pub action_float_values: Vec<WeaponSandboxPerkActionFloatOverride>,
}

/// One validated float32 replacement in a cloned finished-perk runtime action.
///
/// The locator follows a boxed value from a concrete polymorphic action node, so authored data
/// does not depend on an absolute byte offset in the action payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponSandboxPerkActionFloatOverride {
    pub node_type_handle: u32,
    pub node_occurrence: u16,
    pub value_pointer_offset: u32,
    pub value_type_handle: u32,
    pub expected_bits: u32,
    pub value_bits: u32,
}

/// One socket choice replaced with a private clone of its stock plug item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponSocketPlugVariantOverride {
    pub replace_effects: bool,
    /// Replaces the cloned plug's contribution for each selected native stat row.
    pub investment_stats: Vec<(u16, i32)>,
    pub socket_index: u16,
    pub choice_index: u16,
    pub source_plug_hash: u32,
    /// Optional authored display name for the private plug. `None` preserves the stock name.
    pub name: Option<String>,
    /// Stock plug supplying category, tier, inspection template and item-type text.
    /// Its perks and runtime are not transferred.
    pub classification_donor_hash: Option<u32>,
    pub icon: Option<crate::perk::Icon>,
    pub description: Option<String>,
    /// Also offers the plug wherever a stock shared plug set offers its classification plug.
    pub offer_everywhere: bool,
    pub additional_sandbox_perks: Vec<u16>,
    pub sandbox_perks: Vec<WeaponSandboxPerkRuntimeOverride>,
}

impl WeaponSocketPlugVariantOverride {
    pub(crate) fn same_definition(&self, other: &Self) -> bool {
        let mut other = other.clone();
        other.socket_index = self.socket_index;
        other.choice_index = self.choice_index;
        *self == other
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponNumericInstruction {
    pub opcode: u8,
    pub operand: u16,
}

/// Optional stock donor for geometry and client classification.
///
/// The donor's weapon sandbox-pattern selector is gameplay data and is deliberately not part of
/// this transfer. Render dyes and icons have their own donors, though both follow this donor when
/// no explicit selection is present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponPresentationDonorReference {
    pub item_hash: u32,
    pub expected_name: Option<String>,
}

/// Optional stock donor used only as the source of the authored item icon definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponIconDonorReference {
    pub item_hash: u32,
    pub expected_name: Option<String>,
}

/// Optional stock donor used only for the translation block's three render-dye arrays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRenderGearDonorReference {
    pub item_hash: u32,
    pub expected_name: Option<String>,
}

/// One stock donor whose concrete component binding is grafted into a private clone of the
/// gameplay donor's runtime weapon entity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeComponentDonorReference {
    pub binding_hash: u32,
    pub item_hash: u32,
    pub expected_name: Option<String>,
}

/// A donor-first weapon authoring recipe.
///
/// The gameplay donor supplies inherited definition fields. Compilation also assigns private
/// identities, localized text, presentation assets and Collections placement, and fixes inherited
/// randomized socket lanes to their native defaults. Explicit donors and overrides replace their
/// corresponding fields.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponCloneSpec {
    /// Weapons take the full weapon path. Every other kind builds through the gear path, which
    /// keeps the base item's slot, class and geometry and skips weapon runtime authoring.
    pub kind: ItemKind,
    pub namespace: String,
    pub donor_item_hash: u32,
    pub expected_donor_name: Option<String>,
    pub presentation_donor: Option<WeaponPresentationDonorReference>,
    pub icon_donor: Option<WeaponIconDonorReference>,
    pub render_gear_donor: Option<WeaponRenderGearDonorReference>,
    pub runtime_component_donors: Vec<WeaponRuntimeComponentDonorReference>,
    pub identity: WeaponCloneIdentity,
    pub text: WeaponCloneText,
    pub overrides: WeaponCloneOverrides,
}

impl WeaponCloneSpec {
    pub(crate) fn effective_type_name(&self) -> Option<&str> {
        self.text.type_name.as_deref().or_else(|| {
            (self.kind == ItemKind::Subclass)
                .then(|| {
                    crate::subclass::class_type_name(
                        self.overrides.subclass_every_class,
                        self.overrides.subclass_class,
                    )
                })
                .flatten()
        })
    }

    pub(in super::super) fn error_context(&self) -> String {
        let donor = self
            .expected_donor_name
            .as_deref()
            .unwrap_or("Unnamed Item");
        let role = if self.kind.is_weapon() {
            "Gameplay Donor"
        } else {
            "Base Item"
        };
        format!(
            "Recipe: {:?} ({})\nItem: 0x{:08X}\n{role}: {donor:?} (0x{:08X})",
            self.text.name, self.namespace, self.identity.item_hash, self.donor_item_hash
        )
    }

    /// An error raised while authoring this recipe, labeled with it and naming it for the app.
    pub(in super::super) fn in_recipe(&self, error: AuthoringError) -> AuthoringError {
        self.in_recipe_as(error, self.error_context())
    }

    /// As [`Self::in_recipe`], with a label that adds detail to the recipe's own.
    pub(in super::super) fn in_recipe_as(
        &self,
        error: AuthoringError,
        context: impl Into<String>,
    ) -> AuthoringError {
        error.in_recipe(self.namespace.clone(), context)
    }

    /// The badge includes authored items unless the recipe explicitly excludes them.
    pub(crate) fn joins_sunrise_badge(&self) -> bool {
        !self.overrides.exclude_from_sunrise_badge
    }

    pub(in super::super) fn icon_error_context(&self) -> String {
        let (hash, name) = if let Some(donor) = &self.icon_donor {
            (donor.item_hash, donor.expected_name.as_deref())
        } else if let Some(donor) = &self.presentation_donor {
            (donor.item_hash, donor.expected_name.as_deref())
        } else {
            (self.donor_item_hash, self.expected_donor_name.as_deref())
        };
        format!(
            "{}\nIcon Donor: {:?} (0x{hash:08X})",
            self.error_context(),
            name.unwrap_or("Unnamed Item")
        )
    }

    pub fn validate(&self) -> AuthoringResult<()> {
        validate_parhelion_namespace(&self.namespace).map_err(invalid)?;
        if let Some(sparrow) = &self.overrides.sparrow {
            if self.kind != ItemKind::Sparrow {
                return Err(invalid(
                    "Only Sparrow recipes can set Driving Speed or Summon Vehicle",
                ));
            }
            sparrow.validate().map_err(invalid)?;
            #[cfg(feature = "d2-model-importer")]
            if self.overrides.imported_graph.is_some()
                && sparrow.summon != crate::vehicle::Summon::Sparrow
            {
                return Err(invalid(
                    "Alternate vehicle summoning requires a native Sparrow base without an imported appearance",
                ));
            }
        }
        if self.overrides.shader_glow && self.kind != ItemKind::Weapon {
            return Err(invalid("Shader Glow requires a weapon recipe"));
        }
        #[cfg(feature = "d2-model-importer")]
        if self.overrides.shader_glow && self.overrides.imported_graph.is_some() {
            return Err(invalid(
                "Shader Glow currently requires a native weapon appearance",
            ));
        }
        if self.overrides.subclass_every_class && self.kind != ItemKind::Subclass {
            return Err(invalid("Only subclass recipes can set Every Class"));
        }
        if let Some(class) = self.overrides.subclass_class {
            if self.kind != ItemKind::Subclass {
                return Err(invalid("Only subclass recipes can select a class"));
            }
            if self.overrides.subclass_every_class || class.native_class().is_none() {
                return Err(invalid(
                    "A subclass for every class cannot also select one class",
                ));
            }
        }
        if self.overrides.subclass_damage_type.is_some() && self.kind != ItemKind::Subclass {
            return Err(invalid(
                "Only subclass recipes can select a damage type icon",
            ));
        }
        if self.overrides.armor_class.is_some() && self.kind != ItemKind::Armor {
            return Err(invalid("Only armor recipes can select an armor class"));
        }
        if self.overrides.stat_trackers.is_some() && self.kind != ItemKind::Emblem {
            return Err(invalid("Only emblem recipes can select stat trackers"));
        }
        if !self.kind.is_weapon() {
            gear::validate_spec(self)?;
        } else if self.overrides.subclass_abilities.is_some() {
            return Err(invalid("Weapon recipes cannot set subclass abilities"));
        } else if !self.overrides.dye_edits.is_empty()
            || !self.overrides.dye_texture_edits.is_empty()
        {
            return Err(invalid("Weapon recipes cannot set dyes"));
        }
        validate_weapon_donor_references(self)?;
        variable_damage::validate_spec(self)?;
        validate_weapon_clone_text(&self.text)?;
        self.overrides.icon_edit.validate()?;
        validate_investment_stat_definitions(&self.overrides)?;
        validate_base_perks_and_traits(&self.overrides)?;
        validate_native_scalar_overrides(&self.overrides)?;
        validate_presentation_overrides(&self.overrides)?;
        validate_socket_column_shapes(
            &self.overrides.socket_columns,
            &self.overrides.socket_plug_variants,
        )?;
        validate_socket_plug_variant_shapes(&self.overrides.socket_plug_variants)?;
        validate_runtime_value_override_shapes(&self.overrides.runtime_values)?;
        if let Some(projectile) = &self.overrides.projectile {
            validate_runtime_value_override_shapes(&projectile.values)?;
        }
        if let Some(barrel) = &self.overrides.barrel {
            if !self.kind.is_weapon() {
                return Err(invalid("Only weapons can change Barrel settings"));
            }
            barrel.validate().map_err(invalid)?;
        }
        validate_sword_profile(self)?;
        validate_runtime_resource_patch_shapes(&self.overrides.runtime_resource_patches)?;
        validate_raw_payload_patch_shapes(&self.overrides.raw_payload_patches)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NewWeaponPlan {
    pub kind: ItemKind,
    pub item_hash: u32,
    pub definition_tag: TagHash,
    pub string_tag: TagHash,
    pub icon_definition_tag: TagHash,
    pub custom_plugs: Vec<NewCustomPlugPlan>,
    pub item_index: u16,
    /// The item's Collections entry and the unlock that marks it acquired. A subclass has
    /// neither, as no stock subclass does.
    pub collection: Option<NewCollectionPlan>,
    pub template_item_hash: u32,
    pub template_definition_tag: TagHash,
    pub template_string_tag: TagHash,
    pub(crate) details: Option<WeaponBuildDetails>,
    pub(crate) subclass: Option<SubclassBuildDetails>,
}

/// What the build wrote for an authored subclass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubclassBuildDetails {
    /// The class the base's equip condition names, and the class the written one names, or
    /// `None` once Every Class removed it.
    pub base_class: Option<u8>,
    pub class_condition: Option<u8>,
    /// Its socket-entry list's row in the list table.
    pub list_index: u16,
    /// Its own list's and display record's tags, when it has a list of its own.
    pub own_list: Option<(u32, u32)>,
    /// Attunement paths with names of their own.
    pub path_names: usize,
    /// Each ability and node with edits of its own, in list order.
    pub entries: Vec<SubclassEntryBuild>,
    /// Stock lists with a Super lane, which Sunrise keeps selection state for beside the
    /// authored ones.
    pub stock_super_lane_lists: usize,
}

/// One authored ability or node as the build wrote it: its pool and node record, and what they
/// carry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubclassEntryBuild {
    pub label: String,
    pub pool_tag: u32,
    pub record_tag: u32,
    /// Sandbox perks compiled from its custom perks, and stock perks swapped for private copies.
    pub custom_perks: usize,
    pub retargeted_perks: usize,
    /// Its own icon row, and its ability copy's row, when it has them.
    pub icon_row: Option<u16>,
    pub ability_row: Option<u8>,
    /// Modifiers it applies to the subclass's abilities.
    pub modifiers: usize,
    /// What its ability copy changes: values, palettes, tints, the grade, projectile swaps and
    /// bank values.
    pub values: usize,
    pub palettes: usize,
    pub tints: usize,
    pub grade: bool,
    pub swaps: usize,
    pub bank_values: usize,
}

/// Resolved source rows and the damage carrier in the finished definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WeaponBuildDetails {
    pub runtime_source: Option<u16>,
    pub rig_donor: Option<u32>,
    pub pinned_appearance: Option<u16>,
    pub animation_donor: Option<u16>,
    pub damage_carrier: WeaponDamageCarrier,
}

/// An authored item's collectible and the account unlock flag that marks it acquired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewCollectionPlan {
    pub collectible_hash: u32,
    pub collectible_index: u16,
    pub unlock_hash: u32,
    pub unlock_definition_index: u16,
    pub unlock_bank: u8,
    pub unlock_slot: u16,
}

/// Build-assigned identities for one private socket choice used by a weapon.
#[derive(Clone, Debug)]
pub struct NewCustomPlugPlan {
    pub socket_index: usize,
    pub choice_index: usize,
    pub name: Option<String>,
    pub item_hash: u32,
    pub item_index: u16,
    pub definition_tag: TagHash,
    pub string_tag: TagHash,
    pub icon_definition_tag: Option<TagHash>,
    pub name_hash: Option<u32>,
    pub description_hash: Option<u32>,
    /// The stock shared plug sets the plug joined when offered everywhere.
    pub offered_sets: Vec<u16>,
    pub perks: Vec<NewPrivatePerkPlan>,
}

/// Private finished-perk and runtime identities allocated for one authored effect.
#[derive(Clone, Copy, Debug)]
pub struct NewPrivatePerkPlan {
    pub source_perk_index: usize,
    pub perk_hash: u32,
    pub runtime_key: u32,
}

/// A coherent project compiled into required investment overlays, allocated asset packages,
/// and recipe-selected runtime overlays. The build manifest owns the exact output set.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponProjectSpec {
    pub weapons: Vec<WeaponCloneSpec>,
}

/// Deterministic manifest-facing plan for a multi-weapon project.
#[derive(Clone, Debug)]
pub struct NewWeaponProjectPlan {
    pub weapons: Vec<NewWeaponPlan>,
    pub sunrise: SunriseProjectMetadata,
}

/// One coherent aggregate project with core packages and recipe-selected runtime hosts.
#[derive(Clone, Debug)]
pub struct NewWeaponProjectBundle {
    pub plan: NewWeaponProjectPlan,
    pub artifacts: Vec<ExtendedOverlayArtifact>,
}

#[derive(Clone, Debug)]
pub(super) struct AuthoredLocaleData {
    pub(super) donor_tag: TagHash,
    pub(super) payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(super) struct AuthoredLocalization {
    pub(super) index: Vec<u8>,
    pub(super) merged_header: Vec<u8>,
    pub(super) locale_data: Vec<AuthoredLocaleData>,
    pub(super) donor_header_tag: TagHash,
}
