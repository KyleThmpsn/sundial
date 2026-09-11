mod custom_runtime;
mod emission;
mod lore;
pub(crate) mod perk_bank;
mod placements;
mod resolve;
mod sources;
#[cfg(test)]
mod tests;
use custom_runtime::*;
pub(crate) use custom_runtime::{preflight_runtime_edits, runtime_hud_key};
mod localization;
use localization::*;
mod socket_columns;
use socket_columns::*;
mod item_fields;
use item_fields::*;
pub(crate) use item_fields::{weapon_equipment_slot, weapon_inventory_slot};
mod table_rows;
use table_rows::*;
mod raw_payload;
use raw_payload::*;

mod validation;
pub(crate) use validation::validate_catalog_with_progress;
pub(crate) use validation::validate_socket_plug_variant_shapes;
#[cfg(test)]
pub(crate) use validation::validate_weapon_clone_specs_against_catalog;
use validation::*;

mod donors;
use donors::*;

mod build;
#[cfg(test)]
pub(crate) use build::build_weapon_project_after_catalog_validation;
#[cfg(test)]
use build::canonical_project_weapons;
pub(crate) use build::{Phase as CompilePhase, compile_with_progress};

#[cfg(test)]
pub use build::build_weapon_project;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    mem::size_of,
    path::{Path, PathBuf},
};

#[cfg(test)]
use sundial::package_authoring::icon_schema::ICON_WATERMARK_LAYER_OFFSET;
#[cfg(test)]
use sundial::package_authoring::sandbox_perk::sandbox_perk_runtime_assignment;
use sundial::package_authoring::{
    FNV1_EMPTY_HASH, SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY, fnv1_name_hash,
    investment_schema::{
        ARC_DAMAGE_PLUG_ITEM_HASH, ARC_DAMAGE_PLUG_ITEM_INDEX, ELEMENTAL_DAMAGE_SOCKET_TYPE,
        GLOBALS_COLLECTIBLE_DISPLAY_TABLE_SLOT, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT,
        GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT, GLOBALS_ITEM_ICON_TABLE_SLOT,
        GLOBALS_ITEM_METADATA_TABLE_SLOT, GLOBALS_ITEM_STRING_TABLE_SLOT,
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT, GLOBALS_OBJECTIVE_STRING_TABLE_SLOT,
        GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT, GLOBALS_RECORD_STRING_TABLE_SLOT,
        GLOBALS_SANDBOX_PATTERN_TABLE_SLOT, GLOBALS_UNLOCK_FLAG_DISPLAY_TABLE_SLOT,
        INVESTMENT_ROOT_CLASS, ITEM_DEFINITION_HASH_OFFSET, ITEM_DEFINITION_INDEX_ROW_CLASS,
        ITEM_EQUIPMENT_BLOCK_CLASS, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET,
        ITEM_EQUIPMENT_SLOT_OFFSET, ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET, ITEM_HASH_INDEX_ROW_CLASS,
        ITEM_HASH_INDEX_ROW_SIZE, ITEM_HASH_INDEX_TABLE_CLASS, ITEM_ICON_CONTAINER_OFFSET,
        ITEM_ICON_ROW_CLASS, ITEM_ICON_ROW_SIZE, ITEM_INDEX_ROW_SIZE, ITEM_INVENTORY_SLOT_OFFSET,
        ITEM_INVESTMENT_STAT_POINTER_OFFSET, ITEM_INVESTMENT_STAT_RESOURCE_CLASS,
        ITEM_INVESTMENT_STAT_ROW_CLASS, ITEM_INVESTMENT_STAT_ROW_SIZE,
        ITEM_LINKED_PLUG_BLOCK_CLASS, ITEM_LINKED_PLUG_INDEX_OFFSET, ITEM_MAX_STACK_SIZE_OFFSET,
        ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET, ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
        ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS, ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE,
        ITEM_ORDINARY_SOCKET_PLUG_MEMBER_WEIGHT_OFFSET, ITEM_ORDINARY_SOCKET_POINTER_OFFSET,
        ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
        ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET,
        ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET, ITEM_ORDINARY_SOCKET_ROW_CLASS,
        ITEM_ORDINARY_SOCKET_ROW_SIZE, ITEM_PLUG_BLOCK_CATEGORY_OFFSET, ITEM_PLUG_BLOCK_CLASS,
        ITEM_PLUG_BLOCK_ROLL_SET_OFFSET, ITEM_PLUG_BLOCK_SEARCH_END, ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_CATEGORY_FALLBACK_OFFSET, ITEM_RARITY_OFFSET,
        ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET, ITEM_SANDBOX_PERK_ROW_CLASS,
        ITEM_SANDBOX_PERK_ROW_SIZE, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET,
        ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET, ITEM_STRING_AMMO_CLASS, ITEM_STRING_AMMO_CLASS_OFFSET,
        ITEM_STRING_AMMO_TYPE_OFFSET,
        ITEM_STRING_DESCRIPTION_REFERENCE_OFFSET as ITEM_DESCRIPTION_REFERENCE_OFFSET,
        ITEM_STRING_ICON_INDEX_OFFSET, ITEM_STRING_INDEX_ROW_CLASS,
        ITEM_STRING_NAME_REFERENCE_OFFSET as ITEM_NAME_REFERENCE_OFFSET,
        ITEM_STRING_SOURCE_REFERENCE_OFFSET as ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET,
        ITEM_STRING_STAT_GROUP_INDEX_OFFSET, ITEM_STRING_STAT_GROUP_POINTER_OFFSET,
        ITEM_STRING_STAT_GROUP_RESOURCE_CLASS,
        ITEM_STRING_TYPE_REFERENCE_OFFSET as ITEM_TYPE_REFERENCE_OFFSET, ITEM_TRAIT_ROW_CLASS,
        ITEM_TRAIT_ROW_SIZE, ITEM_TRAITS_DESCRIPTOR_OFFSET,
        ITEM_TRANSLATION_ART_DESCRIPTOR_OFFSET as TRANSLATION_ART_DESCRIPTOR_OFFSET,
        ITEM_TRANSLATION_ART_ROW_CLASS as TRANSLATION_ART_ROW_CLASS,
        ITEM_TRANSLATION_ART_ROW_SIZE as TRANSLATION_ART_ROW_SIZE,
        ITEM_TRANSLATION_ART_VARIANT_OFFSET as TRANSLATION_ART_VARIANT_OFFSET,
        ITEM_TRANSLATION_BLOCK_CLASS, ITEM_TRANSLATION_BLOCK_POINTER_OFFSET,
        ITEM_TRANSLATION_BLOCK_SIZE,
        ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS as TRANSLATION_DYE_DESCRIPTOR_OFFSETS,
        ITEM_TRANSLATION_DYE_ROW_CLASS as TRANSLATION_DYE_ROW_CLASS,
        ITEM_TRANSLATION_DYE_ROW_SIZE as TRANSLATION_DYE_ROW_SIZE,
        ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET as TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
        ITEM_VERSION_ROW_SIZE, ItemVersionArray,
        LOCALIZED_STRING_INDEX_ROW_CLASS as LOCALIZED_INDEX_ROW_CLASS,
        LOCALIZED_STRING_INDEX_ROW_SIZE as LOCALIZED_INDEX_ROW_SIZE,
        ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT, ROOT_ITEM_DEFINITION_TABLE_SLOT,
        ROOT_ITEM_HASH_INDEX_TABLE_SLOT, ROOT_ITEM_METADATA_INDEX_TABLE_SLOT,
        ROOT_MATERIAL_REQUIREMENT_TABLE_SLOT, ROOT_OBJECTIVE_DEFINITION_TABLE_SLOT,
        ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT, ROOT_RECORD_DEFINITION_TABLE_SLOT,
        ROOT_SANDBOX_PATTERN_INDEX_TABLE_SLOT, ROOT_SANDBOX_PERK_INDEX_TABLE_SLOT,
        ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT, ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT,
        ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT, SOLAR_DAMAGE_PLUG_ITEM_HASH,
        SOLAR_DAMAGE_PLUG_ITEM_INDEX, UNLOCK_FLAG_DEFINITION_ROW_CLASS,
        UNLOCK_FLAG_DISPLAY_ROW_CLASS, UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS,
        UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE, VOID_DAMAGE_PLUG_ITEM_HASH, VOID_DAMAGE_PLUG_ITEM_INDEX,
        investment_globals_table_tag, investment_root_table_tag, item_version_array,
    },
    is_valid_package_tag,
    sandbox_perk::{
        FinishedSandboxPerkName, FinishedSandboxPerkPresentation, SANDBOX_PERK_INDEX_CATALOG_CLASS,
        SANDBOX_PERK_RUNTIME_MAP_CLASS, SANDBOX_PERK_RUNTIME_MAP_TAG, SandboxPerkRuntimeAction,
        clone_and_append_presented_finished_sandbox_perk, clone_and_append_sandbox_perk_index,
        finished_sandbox_perk_at, finished_sandbox_perk_count,
        insert_sandbox_perk_runtime_assignment, load_sandbox_perk_runtime_action,
        sandbox_perk_action_boxed_value_offset, sandbox_perk_index_count,
        sandbox_perk_index_hash_at, sandbox_perk_runtime_assignment_at,
        sandbox_perk_runtime_assignment_count, validate_finished_sandbox_perk_catalog,
        validate_sandbox_perk_index_catalog, validate_sandbox_perk_runtime_map,
    },
    weapon_entity::{
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        SANDBOX_PATTERN_GLOBAL_ID_OFFSET, SANDBOX_PATTERN_INDEX_ROW_CLASS,
        SANDBOX_PATTERN_INDEX_ROW_SIZE, SANDBOX_PATTERN_NESTED_CLASS,
        SANDBOX_PATTERN_NESTED_OFFSET, SANDBOX_PATTERN_ROW_CLASS, SANDBOX_PATTERN_ROW_SIZE,
        SandboxPatternIdentity, WEAPON_ENTITY_CLASS, append_weapon_entity_assignment,
        graft_weapon_component_bindings, retarget_weapon_component_owner,
        retarget_weapon_component_owner_payload, sandbox_pattern_identity,
        sandbox_pattern_identity_at, validate_weapon_entity, weapon_component_bindings,
        weapon_entity_assignment,
    },
    weapon_runtime::{
        WeaponRuntimeValueOverride, encode_weapon_runtime_value, resolve_weapon_runtime_field,
    },
};

use tiger_pkg::{PackageManager, TagHash};

use sundial::{
    investment::{
        InvestmentCatalog, MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES, WeaponDamageCarrierFamily,
        WeaponDamageType, authored_socket_choice_limit,
    },
    package_authoring::{
        open_shadowkeep_package_manager, resolve_item_name, resolve_live_named_tag,
    },
};

use crate::appended_tags::AppendedTagAllocator;
use crate::badge::SunriseBadgeGraphInput;
use crate::error::{invalid, validation};
use crate::extend::extended_overlay_append_start;
use crate::package_profile::{
    ACCOUNT_UNLOCK_BANK, COLLECTION_PACKAGE_ID, HOST_EXPECTED_ENTRY_COUNT, HOST_PACKAGE_ID,
    LOCALIZATION_DONOR_TABLE_INDEX, PARHELION_ASSET_PACKAGE_ID,
    PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT, PRIVATE_PERK_RUNTIME_PACKAGE_ID,
};
use crate::progression::{
    AuthoredCollectibleSpec, BLANK_LOCALIZED_REFERENCE_HASH, BLANK_LOCALIZED_REFERENCE_TABLE_INDEX,
    COLLECTIBLE_CURATED_ACQUISITION_FLAG, COLLECTIBLE_CURATED_ACQUISITION_FLAG_OFFSET,
    COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET, COLLECTIBLE_DEFINITION_ROW_CLASS,
    COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
    COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
    COLLECTIBLE_DISPLAY_ROW_CLASS, COLLECTIBLE_DISPLAY_ROW_SIZE,
    COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET, COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET,
    COLLECTIBLE_HASH_OFFSET, COLLECTIBLE_ITEM_INDEX_OFFSET, COLLECTIBLE_MATERIAL_SET_OFFSET,
    COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET, COLLECTIBLE_REACQUISITION_ENABLED,
    COLLECTIBLE_REACQUISITION_STATE_OFFSET, COLLECTIBLE_ROW_SIZE, CollectibleUnlockClone,
    NESTED_ARRAY_TRAILER, NUMERIC_INSTRUCTION_ROW_SIZE, NUMERIC_PROGRAM_ROW_CLASS,
    PRESENTATION_NODE_INDEX_ROW_CLASS, ProjectAuthoredRow, SunriseAcquiredPoolSelection,
    UNLOCK_DISPLAY_ROW_SIZE, UNLOCK_ROW_SIZE, append_collectible, append_collectible_display,
    append_unlock, append_unlock_display, append_unlock_flag_bank_row,
    classify_sunrise_count_pools, collection_unlock_index, donor_weapon_collection_page,
    first_free_unlock_slot, numeric_program_layout, numeric_program_stack_depth,
    patch_project_acquired_count_programs, patch_project_collection_objectives,
    template_presentation_parents, unlock_flag_bank_descriptor, unlock_sorted_index_position,
    validate_authored_collectible_display_row, validate_authored_collectible_nested_isolation,
    validate_authored_unlock_display_row, validate_authored_unlock_flag_bank_row,
    validate_weapon_material_sets,
};
use crate::recipe::validate_parhelion_namespace;
use crate::shared_tag_memory::{
    build_shared_tag_companion_payload, dependency_set, validate_shared_tag_companion_payload,
};
use crate::tag_payload::{
    array_at, contains_u32_at_offset, contains_u32_row_key, find_u32_row_key, read_i32, read_i64,
    read_u8, read_u16, read_u32, read_u64, relative_target, set_array_count, write_bytes,
    write_i32, write_i64, write_localized_reference, write_relative_pointer, write_u16, write_u32,
    write_u64,
};
use crate::{
    AuthoringError, AuthoringResult, ExtendedOverlayArtifact, NewTagSpec, ReplacementSpec,
    SUNRISE_BADGE_DESCRIPTION, SUNRISE_BADGE_DESCRIPTION_HASH, SUNRISE_BADGE_NAME,
    SUNRISE_BADGE_NAME_HASH, SUNRISE_BADGE_NODE_HASHES, SunriseBadgePlacement,
    SunriseProjectMetadata, SupportedPlugSet, WeaponIconRequest, append_badge_icon_row,
    author_sunrise_badge_graph, build_badge_icon_plan, item_icon_row_with_container,
    sunrise_badge_collectible_parents, validate_socket_column_overrides_with_socket_types,
    validate_stat_overrides,
};

const ITEM_ROW_SIZE: usize = ITEM_INDEX_ROW_SIZE;
const ITEM_HASH_INDEX_DESCRIPTORS: [usize; 2] = [0x08, 0x18];
const ITEM_HASH_INDEX_ITEM_INDEX_OFFSET: usize = 0x04;
const ITEM_METADATA_ROW_SIZE: usize = 0x20;
const ITEM_METADATA_ROW_CLASS: u32 = 0x8080_5DFB;
const ITEM_METADATA_NESTED_OFFSET: usize = 0x10;
const ITEM_METADATA_NESTED_CLASS: u32 = 0x8080_5DFE;
const ITEM_METADATA_SECONDARY_CLASS: u32 = 0x8080_5DFF;
const ITEM_METADATA_INDEX_ROW_SIZE: usize = 4;
const ITEM_METADATA_INDEX_ROW_CLASS: u32 = 0x8080_7484;
pub(crate) const STOCK_ITEM_ICON_COUNT: usize = 15_725;
const ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET: usize = 0xB8;
const ITEM_STRING_CLIENT_CLASSIFICATION_SIZE: usize = 12;
const ITEM_DENSE_ICON_TAG_DESCRIPTOR: usize = 0x08;
const ITEM_DENSE_AUXILIARY_DESCRIPTOR: usize = 0x18;
const ITEM_DENSE_ICON_SELECTOR_DESCRIPTOR: usize = 0x28;
const ITEM_DENSE_PRESENTATION_DESCRIPTOR: usize = 0x38;
const ITEM_DENSE_PRESENTATION_NEXT_DESCRIPTOR: usize = 0x48;
const ITEM_DENSE_ICON_TAG_ROW_SIZE: usize = 0x04;
const ITEM_DENSE_AUXILIARY_ROW_SIZE: usize = 0x04;
const ITEM_DENSE_ICON_SELECTOR_ROW_SIZE: usize = 0x08;
const ITEM_DENSE_PRESENTATION_ROW_SIZE: usize = 0x10;
const ITEM_DENSE_TRAILING_ROW_SIZE: usize = 0x08;
const ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET: usize = 0x08;
const ITEM_DENSE_PRESENTATION_TYPE_OFFSET: usize = 0x04;
#[cfg(test)]
const ITEM_DENSE_PRESENTATION_CLASSIFICATION_OFFSET: usize = 0x0C;
const ITEM_DENSE_ICON_TAG_ROW_CLASS: u32 = 0x8080_0014;
const ITEM_DENSE_AUXILIARY_ROW_CLASS: u32 = 0x8080_0007;
const ITEM_DENSE_ICON_SELECTOR_ROW_CLASS: u32 = 0x8080_56D6;
const ITEM_DENSE_PRESENTATION_ROW_CLASS: u32 = 0x8080_56D1;
const ITEM_DENSE_TRAILING_ROW_CLASS: u32 = 0x8080_56D5;
const ITEM_DENSE_PRESENTATION_TRAILER_CLASS: u32 = 0x8080_9FBD;
// Native 0x808077B5 rows are { i8 class, u8 pad, u16 arrangement }.
// These are the starts of the three 16-byte descriptors in the serialized translation block.
// Their self-relative pointer fields therefore live at +0x30/+0x40/+0x50.
// Native sandbox-pattern row consumed by the runtime weapon-entity path.
const RANDOM_PERKS_NOT_REACQUIRABLE_SOURCE_HASH: u32 = 0x9897_25F1;
const ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET: usize = 0x60;
const ITEM_STRING_SANDBOX_PERK_RESOURCE_CLASS: u32 = 0x8080_5D08;
const ITEM_STRING_SANDBOX_PERK_DESCRIPTOR_OFFSET: usize = 0x08;
const ITEM_STRING_SANDBOX_PERK_ROW_CLASS: u32 = 0x8080_5D0A;
const ITEM_STRING_SANDBOX_PERK_ROW_SIZE: usize = 0x20;
const ITEM_STRING_SANDBOX_PERK_EMPTY_EXPRESSION_TAG: u32 = 0x811C_9DC5;
const PRIVATE_PERK_RESIDENCY_DONOR_ACTION_TAG: TagHash = TagHash(0x80B7_76B8);
// This existing type-16 investment root is actually requested by the game. Its precomputed
// dependency index, unlike the name map alone, schedules new action and graph data for loading.
const RUNTIME_DEPENDENCY_ROOT: TagHash = TagHash(0x80EC_3F62);
const RUNTIME_DEPENDENCY_COMPANION: TagHash = TagHash(0x80EE_8CBD);
const PRIVATE_PERK_RESIDENCY_B9_TEMPLATE: TagHash = TagHash(0x80B7_76B9);
const PRIVATE_PERK_RESIDENCY_BA_TEMPLATE: TagHash = TagHash(0x80B7_76BA);
const PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE: TagHash = TagHash(0x80BC_4225);
const PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE: TagHash = TagHash(0x8151_E74C);
const PRIVATE_PERK_RESIDENCY_B9_CLASS: u32 = 0x8080_9C36;
const PRIVATE_PERK_RESIDENCY_BA_CLASS: u32 = 0x8080_9C0F;
const PRIVATE_PERK_RESIDENCY_ROOT_CLASS: u32 = 0x8080_9BB6;
const PRIVATE_PERK_RESIDENCY_ROOT_BA_OFFSETS: [usize; 1] = [0x08];
const PRIVATE_PERK_RESIDENCY_BA_ROOT_OFFSETS: [usize; 1] = [0x88];
const PRIVATE_PERK_RESIDENCY_BA_B9_OFFSETS: [usize; 3] = [0xCC, 0x498, 0x858];
const PRIVATE_PERK_RESIDENCY_B9_SELF_OFFSETS: [usize; 2] = [0x80, 0xC8];
const PRIVATE_PERK_RESIDENCY_B9_ACTION_OFFSETS: [usize; 1] = [0x118];
const PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS: [usize; 1] = [0x08];
const PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS: [usize; 1] = [0x0C];
const PRIVATE_PERK_RESIDENCY_B9_SIZE: usize = 0x160;
const PRIVATE_PERK_RESIDENCY_BA_SIZE: usize = 0xA14;
const PRIVATE_PERK_RESIDENCY_ROOT_SIZE: usize = 0xC0;
const PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE_SIZE: usize = 0x122;
const PRIVATE_PERK_RESIDENCY_COMPANION_SIZE: usize = 0xE6;
const LOCALIZATION_STOCK_TABLE_COUNT: usize = 3108;
// Bank 2927 is an already-rooted native two-string bank. Its existing hashes and strings are
// preserved while authored weapon and private-plug text is appended in a patch overlay. A new independent
// bank is addressable after package registration but is never queued by the stock root graph.
const LOCALIZATION_HEADER_HASH_CLASS: u32 = 0x8080_0070;
const LOCALIZATION_PART_CLASS: u32 = 0x8080_9A90;
const LOCALIZATION_AUX_CLASS: u32 = 0x8080_0006;
const LOCALIZATION_BYTE_CLASS: u32 = 0x8080_0005;
const LOCALIZATION_COMBO_CLASS: u32 = 0x8080_9A8E;
const LOCALIZATION_DATA_TAG_START: usize = 0x18;
const LOCALIZATION_DATA_TAG_END: usize = 0x4C;
const LOCALIZATION_LOCALE_COUNT: usize =
    (LOCALIZATION_DATA_TAG_END - LOCALIZATION_DATA_TAG_START) / 4;
const LOCALIZATION_DONOR_TABLE_KEY: u32 = 0xBAC0_E456;
const LOCALIZATION_DONOR_STRING_HASHES: [u32; 2] = [0x2DF7_1B81, 0x304D_56FE];
const LOCALIZATION_PART_ROW_SIZE: usize = 0x20;
const LOCALIZATION_COMBO_ROW_SIZE: usize = 0x10;
const LOCALIZATION_AUX_DESCRIPTOR_OFFSET: usize = 0x28;
const LOCALIZATION_BYTE_DESCRIPTOR_OFFSET: usize = 0x38;
const LOCALIZATION_COMBO_DESCRIPTOR_OFFSET: usize = 0x48;
const LOCALIZATION_ARRAY_SENTINEL: u32 = 0x8080_9FBD;
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

    fn validate_for_donor(self, donor_item_hash: u32) -> AuthoringResult<()> {
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
    const fn package_value(self) -> u16 {
        self as u16
    }

    fn from_package_value(value: u16) -> AuthoringResult<Option<Self>> {
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

    const fn package_value(self) -> u8 {
        self as u8
    }

    fn from_package_value(value: u8) -> AuthoringResult<Self> {
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
    const fn shared(self) -> WeaponDamageType {
        match self {
            Self::Kinetic => WeaponDamageType::Kinetic,
            Self::Arc => WeaponDamageType::Arc,
            Self::Solar => WeaponDamageType::Solar,
            Self::Void => WeaponDamageType::Void,
        }
    }

    const fn descriptor(self) -> WeaponDamageDescriptor {
        match self {
            Self::Kinetic => WeaponDamageDescriptor::Empty,
            Self::Arc | Self::Solar | Self::Void => WeaponDamageDescriptor::Elemental(self),
        }
    }

    #[cfg(test)]
    const fn modern_sandbox_perk_index(self) -> Option<u16> {
        WeaponDamageCarrierFamily::ModernFixed.base_sandbox_perk_index(self.shared())
    }
}

impl WeaponInventorySlot {
    const fn bucket_hash(self) -> u32 {
        match self {
            Self::Kinetic => 0x5957_0ADA,
            Self::Energy => 0x92F1_6AD9,
            Self::Power => 0x38DC_DD35,
        }
    }

    const fn from_bucket_hash(value: u32) -> Option<Self> {
        match value {
            0x5957_0ADA => Some(Self::Kinetic),
            0x92F1_6AD9 => Some(Self::Energy),
            0x38DC_DD35 => Some(Self::Power),
            _ => None,
        }
    }

    const fn root_value(self) -> u8 {
        match self {
            Self::Kinetic => 0,
            Self::Energy => 1,
            Self::Power => 2,
        }
    }

    const fn equipment_value(self) -> u16 {
        match self {
            Self::Kinetic => 7,
            Self::Energy => 8,
            Self::Power => 9,
        }
    }

    const fn from_root_value(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Kinetic),
            1 => Some(Self::Energy),
            2 => Some(Self::Power),
            _ => None,
        }
    }

    const fn from_equipment_value(value: u16) -> Option<Self> {
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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeaponCloneOverrides {
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
    pub power_cap_group: Option<u16>,
    /// Complete ordered native version-group values. Mutually exclusive with
    /// [`Self::power_cap_group`] and required to match the donor row count.
    pub power_cap_groups: Option<Vec<u16>>,
    pub rarity: Option<AuthoredWeaponRarity>,
    /// Stock gear-art/runtime row used as the runtime entity source. Compilation combines its
    /// runtime identity with the selected appearance donor's gear-art row. `None` follows the
    /// gameplay donor.
    pub weapon_pattern_index: Option<u16>,
    pub stat_group_index: Option<u16>,
    /// Complete ordered translation-art rows. `None` preserves the selected appearance tuple.
    pub art_arrangements: Option<Vec<WeaponArtArrangementOverride>>,
    /// Complete ordered custom, default, and locked dye-reference rows.
    pub render_dye_rows: Option<[Vec<WeaponDyeReferenceOverride>; 3]>,
    /// Positional donor overrides followed by any added sockets, up to the native lane limit.
    /// Inherited rows preserve their content, with relative pointers rebased if the array grows.
    /// Each added socket requires an explicit type and at least one plug choice.
    pub socket_columns: Vec<Option<WeaponSocketColumnOverride>>,
    /// Private socket-plug variants whose finished perk runtime graphs carry typed edits.
    pub socket_plug_variants: Vec<WeaponSocketPlugVariantOverride>,
    /// Typed values in the effective package-backed runtime graph.
    pub runtime_values: Vec<WeaponRuntimeValueOverride>,
    /// Same-size byte replacements inside the concrete runtime resource selected by a component
    /// binding. Offsets are relative to the selected resource, not its owner tag.
    pub runtime_resource_patches: Vec<WeaponRuntimeResourcePatch>,
    /// Final byte patches applied to this weapon's newly cloned records after structured fields.
    pub raw_payload_patches: Vec<WeaponRawPayloadPatch>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum WeaponRawPayloadTarget {
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

const ITEM_ROOT_RAW_TARGETS: [(WeaponRawPayloadTarget, usize); 19] = [
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

const ITEM_DEFINITION_RAW_SUBTARGETS: [WeaponRawPayloadTarget; 22] = [
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
/// Only this finished-perk row and its runtime action/entity chain become private. Other perks
/// carried by the source plug continue to reference their stock rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponSandboxPerkRuntimeOverride {
    pub program: Option<sundial::package_authoring::sandbox_perk::program::Program>,
    pub projectiles: Vec<sundial::package_authoring::sandbox_perk::projectile::Selection>,
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
    pub description: Option<String>,
    pub additional_sandbox_perks: Vec<u16>,
    pub sandbox_perks: Vec<WeaponSandboxPerkRuntimeOverride>,
}

impl WeaponSocketPlugVariantOverride {
    fn same_definition(&self, other: &Self) -> bool {
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponCloneSpec {
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
    pub(super) fn error_context(&self) -> String {
        let donor = self
            .expected_donor_name
            .as_deref()
            .unwrap_or("Unnamed Item");
        format!(
            "Recipe: {:?} ({})\nItem: 0x{:08X}\nGameplay Donor: {donor:?} (0x{:08X})",
            self.text.name, self.namespace, self.identity.item_hash, self.donor_item_hash
        )
    }

    pub(super) fn icon_error_context(&self) -> String {
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
        validate_weapon_donor_references(self)?;
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
        validate_runtime_resource_patch_shapes(&self.overrides.runtime_resource_patches)?;
        validate_raw_payload_patch_shapes(&self.overrides.raw_payload_patches)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NewWeaponPlan {
    pub item_hash: u32,
    pub definition_tag: TagHash,
    pub string_tag: TagHash,
    pub icon_definition_tag: TagHash,
    pub item_index: u16,
    pub collectible_hash: u32,
    pub collectible_index: u16,
    pub unlock_hash: u32,
    pub unlock_definition_index: u16,
    pub unlock_bank: u8,
    pub unlock_slot: u16,
    pub template_item_hash: u32,
    pub template_definition_tag: TagHash,
    pub template_string_tag: TagHash,
}

/// A coherent project compiled into five investment overlays, one standalone asset package,
/// and any recipe-selected runtime overlays. The build manifest owns the exact output set.
#[derive(Clone, Debug, Eq, PartialEq)]
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
struct AuthoredLocaleData {
    donor_tag: TagHash,
    payload: Vec<u8>,
}

#[derive(Clone, Debug)]
struct AuthoredLocalization {
    index: Vec<u8>,
    merged_header: Vec<u8>,
    locale_data: Vec<AuthoredLocaleData>,
    donor_header_tag: TagHash,
}

impl NewWeaponProjectBundle {
    #[cfg(test)]
    pub fn write_new(&self, directory: &Path) -> AuthoringResult<Vec<PathBuf>> {
        self.write_new_with_progress(directory, &mut |_, _, _| {})
    }

    pub(crate) fn write_new_with_progress(
        &self,
        directory: &Path,
        report: &mut dyn FnMut(&str, usize, usize),
    ) -> AuthoringResult<Vec<PathBuf>> {
        fs::create_dir_all(directory).map_err(|error| {
            AuthoringError::io("create weapon-project staging directory", directory, error)
        })?;
        for artifact in &self.artifacts {
            let target = directory.join(&artifact.plan.output_file_name);
            if target.exists() {
                return Err(AuthoringError::InvalidInput(format!(
                    "Refusing to replace existing package {}",
                    target.display()
                )));
            }
        }
        self.artifacts
            .iter()
            .enumerate()
            .map(|(index, artifact)| {
                report(&artifact.plan.output_file_name, index, self.artifacts.len());
                let path = artifact.write_new(directory)?;
                report(
                    &artifact.plan.output_file_name,
                    index + 1,
                    self.artifacts.len(),
                );
                Ok(path)
            })
            .collect::<AuthoringResult<Vec<_>>>()
    }
}

#[derive(Clone)]
struct ResolvedPresentationDonor {
    item_index: usize,
    definition: Vec<u8>,
    strings: Vec<u8>,
    icon_index: u16,
    icon_container: TagHash,
    inventory_slot: WeaponInventorySlot,
}

#[derive(Clone, Copy)]
struct ResolvedIconDonor {
    item_index: usize,
    icon_index: u16,
    icon_container: TagHash,
}

#[derive(Clone)]
struct ResolvedRenderGearDonor {
    definition: Vec<u8>,
}

#[derive(Clone, Copy)]
struct ResolvedRuntimeComponentDonor {
    binding_hash: u32,
    pattern_item_hash: u32,
}

#[derive(Clone)]
struct ResolvedPrivateSandboxPerk {
    program: Option<sundial::package_authoring::sandbox_perk::program::Program>,
    projectiles: Vec<sundial::package_authoring::sandbox_perk::projectile::Selection>,
    source_index: usize,
    activation: Option<sundial::package_authoring::sandbox_perk::activation::PerkActivation>,
    hidden: bool,
    runtime_action: SandboxPerkRuntimeAction,
    authored_perk_hash: u32,
    authored_runtime_key: u32,
    runtime_values: Vec<WeaponRuntimeValueOverride>,
    action_float_values: Vec<WeaponSandboxPerkActionFloatOverride>,
}

#[derive(Clone)]
struct ResolvedCustomPlug {
    replace_effects: bool,
    investment_stats: Vec<(u16, i32)>,
    uses: Vec<CustomPlugUse>,
    source_item_hash: u32,
    source_item_index: usize,
    source_definition_tag: TagHash,
    source_string_tag: TagHash,
    source_definition: Vec<u8>,
    source_strings: Vec<u8>,
    source_icon_container: TagHash,
    authored_item_hash: u32,
    authored_item_index: u16,
    authored_definition_tag: TagHash,
    authored_string_tag: TagHash,
    authored_name_hash: Option<u32>,
    authored_name: Option<String>,
    authored_description_hash: Option<u32>,
    authored_description: Option<String>,
    additional_sandbox_perks: Vec<u16>,
    classification: Option<crate::plug_classification::PlugClassification>,
    classification_item_index: Option<usize>,
    classification_perk_index: Option<usize>,
    sandbox_perks: Vec<ResolvedPrivateSandboxPerk>,
}

#[derive(Clone, Copy)]
struct CustomPlugUse {
    weapon_ordinal: usize,
    socket_index: usize,
    choice_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResolvedSandboxPatternSource {
    item_hash: u32,
    row_index: u16,
    pattern_global_id_hash: u32,
    weapon_content_group_hash: u32,
    weapon_translation_group_hash: u32,
}

#[derive(Clone)]
struct ResolvedDamageCarrierSource {
    family: WeaponDamageCarrierFamily,
    topology_definition: Option<Vec<u8>>,
}

impl TryFrom<SandboxPatternIdentity> for ResolvedSandboxPatternSource {
    type Error = AuthoringError;

    fn try_from(identity: SandboxPatternIdentity) -> Result<Self, Self::Error> {
        Ok(Self {
            item_hash: identity.item_hash,
            row_index: u16::try_from(identity.row_index)
                .map_err(|_| invalid("Weapon pattern row index does not fit 16 bits"))?,
            pattern_global_id_hash: identity.pattern_global_id_hash,
            weapon_content_group_hash: identity.weapon_content_group_hash,
            weapon_translation_group_hash: identity.weapon_translation_group_hash,
        })
    }
}

#[derive(Clone)]
struct ResolvedRuntimeResourcePatch {
    label: String,
    binding_hash: u32,
    resource_index: usize,
    start: usize,
    end: usize,
    bytes: Vec<u8>,
}

fn open_manager(package_directory: &Path) -> AuthoringResult<PackageManager> {
    open_shadowkeep_package_manager(package_directory)
        .map_err(|error| invalid(format!("Could not open Shadowkeep packages: {error}")))
}

fn root_child_tag(data: &[u8], slot: usize) -> AuthoringResult<TagHash> {
    investment_root_table_tag(data, slot)
        .map(TagHash)
        .map_err(invalid)
}

fn globals_child_tag(data: &[u8], slot: usize) -> AuthoringResult<TagHash> {
    investment_globals_table_tag(data, slot)
        .map(TagHash)
        .map_err(invalid)
}

/// Sunrise's cached stat rows per definition, not the native array capacity.
pub(crate) const SUNRISE_STAT_CONTRIBUTION_CAPACITY: usize = 16;

/// An independent perk's stat list is complete, so removing a row in the
/// workbench must also remove the presentation template's contribution.
fn replace_custom_plug_stats(data: &mut Vec<u8>, values: &[(u16, i32)]) -> AuthoringResult<()> {
    if values.len() > SUNRISE_STAT_CONTRIBUTION_CAPACITY {
        return Err(invalid("A custom perk supports up to 16 stat bonuses"));
    }
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    let mut removed = if read_u64(data, resource)? == 0 && read_i64(data, resource + 8)? == 0 {
        Vec::new()
    } else {
        let (count, _, rows, class) = array_at(data, resource)?;
        if class != ITEM_INVESTMENT_STAT_ROW_CLASS || count > 256 {
            return Err(invalid("The perk template has an invalid stat array"));
        }
        (0..count)
            .map(|index| read_u8(data, rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE).map(u16::from))
            .collect::<AuthoringResult<Vec<_>>>()?
    };
    removed.retain(|index| !values.iter().any(|(selected, _)| selected == index));
    set_weapon_stats(data, values, &removed)
}

/// Apply sparse stat edits without changing the source or accepting cache-truncated rows.
fn apply_custom_plug_stats(data: &mut Vec<u8>, overrides: &[(u16, i32)]) -> AuthoringResult<()> {
    if overrides.is_empty() {
        return Ok(());
    }
    let mut authored = data.clone();
    set_weapon_stats(&mut authored, overrides, &[])?;
    let resource = relative_target(&authored, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    let (count, _, _, _) = array_at(&authored, resource)?;
    // Sunrise's definition cache retains only its first 16 declared contributions.
    // This is a runtime compatibility guard, not the native array's format limit.
    if count > SUNRISE_STAT_CONTRIBUTION_CAPACITY {
        return Err(invalid(
            "Custom perk stat edits exceed Sunrise's 16-contribution definition cache; reduce the number of added stats",
        ));
    }
    *data = authored;
    Ok(())
}

fn set_weapon_stats(
    data: &mut Vec<u8>,
    overrides: &[(u16, i32)],
    removed_definitions: &[u16],
) -> AuthoringResult<()> {
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Weapon has no recognized investment-stat resource"));
    }
    // Plugs with no declared stats can use a null array descriptor. Resolving its
    // zero pointer as a header would read the adjacent sandbox-perk descriptor.
    let empty_descriptor = read_u64(data, resource)? == 0 && read_i64(data, resource + 8)? == 0;
    let (count, _, rows, class) = if empty_descriptor {
        (0, data.len(), data.len(), ITEM_INVESTMENT_STAT_ROW_CLASS)
    } else {
        array_at(data, resource)?
    };
    if class != ITEM_INVESTMENT_STAT_ROW_CLASS || count > 256 {
        return Err(invalid(format!(
            "Investment-stat array has class 0x{class:08X} and {count} rows; expected 0x{ITEM_INVESTMENT_STAT_ROW_CLASS:08X} with at most 256 rows"
        )));
    }
    let rows_end = rows
        .checked_add(
            count
                .checked_mul(ITEM_INVESTMENT_STAT_ROW_SIZE)
                .ok_or_else(|| invalid("Weapon investment-stat rows overflowed"))?,
        )
        .ok_or_else(|| invalid("Weapon investment-stat rows overflowed"))?;
    let source_rows = data
        .get(rows..rows_end)
        .ok_or_else(|| invalid("Weapon investment-stat rows are truncated"))?;
    let mut authored_rows = Vec::with_capacity(count);
    let mut donor_definitions = BTreeSet::new();
    for index in 0..count {
        let row = index * ITEM_INVESTMENT_STAT_ROW_SIZE;
        let definition_index = u16::from(read_u8(source_rows, row)?);
        if read_u8(source_rows, row + 1)? != 0 {
            return Err(invalid(format!(
                "Weapon investment stat row {index} has a nonzero reserved byte"
            )));
        }
        if !donor_definitions.insert(definition_index) {
            return Err(invalid(format!(
                "Weapon contains duplicate investment stat definition {definition_index}"
            )));
        }
        authored_rows.push(source_rows[row..row + ITEM_INVESTMENT_STAT_ROW_SIZE].to_vec());
    }

    let mut removed = BTreeSet::new();
    for &definition_index in removed_definitions {
        if u8::try_from(definition_index).is_err() {
            return Err(invalid(format!(
                "Removed investment stat definition {definition_index} does not fit the native 8-bit field"
            )));
        }
        if !removed.insert(definition_index) {
            return Err(invalid(format!(
                "Investment stat definition {definition_index} is removed more than once"
            )));
        }
        if !donor_definitions.contains(&definition_index) {
            return Err(invalid(format!(
                "Weapon does not contain investment stat definition {definition_index} to remove"
            )));
        }
    }
    authored_rows.retain(|row| !removed.contains(&u16::from(row[0])));
    let existing = authored_rows
        .iter()
        .enumerate()
        .map(|(index, row)| (u16::from(row[0]), index))
        .collect::<BTreeMap<_, _>>();

    let mut requested = BTreeSet::new();
    let mut additions = Vec::new();
    for &(definition_index, value) in overrides {
        if u8::try_from(definition_index).is_err() {
            return Err(invalid(format!(
                "Investment stat definition {definition_index} does not fit the native 8-bit field"
            )));
        }
        if !requested.insert(definition_index) {
            return Err(invalid(format!(
                "Investment stat definition {definition_index} is overridden more than once"
            )));
        }
        if removed.contains(&definition_index) {
            return Err(invalid(format!(
                "Investment stat definition {definition_index} cannot be both overridden and removed"
            )));
        }
        if let Some(&row_index) = existing.get(&definition_index) {
            write_i32(&mut authored_rows[row_index], 4, value)?;
        } else {
            additions.push((definition_index, value));
        }
    }
    additions.sort_unstable_by_key(|(definition_index, _)| *definition_index);
    let authored_count = authored_rows
        .len()
        .checked_add(additions.len())
        .ok_or_else(|| invalid("Weapon investment-stat count overflowed"))?;
    if authored_count > 256 {
        return Err(invalid(
            "Weapon investment-stat count exceeds the supported maximum of 256",
        ));
    }
    for (definition_index, value) in additions {
        let mut row = vec![0_u8; ITEM_INVESTMENT_STAT_ROW_SIZE];
        write_bytes(
            &mut row,
            0,
            &[u8::try_from(definition_index).map_err(|_| {
                invalid(format!(
                    "Investment stat definition {definition_index} does not fit the native 8-bit field"
                ))
            })?],
        )?;
        write_i32(&mut row, 4, value)?;
        authored_rows.push(row);
    }
    let authored_bytes = authored_rows.concat();

    if authored_count == count && !empty_descriptor {
        data.get_mut(rows..rows_end)
            .ok_or_else(|| invalid("Weapon investment-stat rows are truncated"))?
            .copy_from_slice(&authored_bytes);
    } else {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let header = data.len();
        data.extend_from_slice(&[0_u8; 16]);
        write_u32(data, header + 8, ITEM_INVESTMENT_STAT_ROW_CLASS)?;
        data.extend_from_slice(&authored_bytes);
        set_array_count(data, resource, header, authored_count)?;
        write_relative_pointer(data, resource + 8, header)?;
    }

    let (materialized_count, _, materialized_rows, materialized_class) = array_at(data, resource)?;
    if materialized_class != ITEM_INVESTMENT_STAT_ROW_CLASS
        || materialized_count != authored_count
        || overrides.iter().any(|(definition_index, value)| {
            !(0..materialized_count).any(|index| {
                let row = materialized_rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
                read_u8(data, row).ok().map(u16::from) == Some(*definition_index)
                    && read_u8(data, row + 1).ok() == Some(0)
                    && read_i32(data, row + 4).ok() == Some(*value)
            })
        })
        || removed.iter().any(|definition_index| {
            (0..materialized_count).any(|index| {
                let row = materialized_rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
                read_u8(data, row).ok().map(u16::from) == Some(*definition_index)
            })
        })
    {
        return Err(validation(
            "Authored weapon investment-stat array did not retain every requested value",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ResolvedSocketColumn {
    choices: Vec<u16>,
    socket_type: Option<u16>,
    choice_weight_bits: Vec<u32>,
    choice_conditions: Vec<Vec<WeaponNumericInstruction>>,
    reusable_plug_set_index: Option<u16>,
    randomized_plug_set_index: Option<u16>,
    randomized_selection_program: Vec<WeaponNumericInstruction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WeaponDamageDescriptor {
    Empty,
    Elemental(ModernDamageType),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WeaponPresentationTuple {
    weapon_pattern_index: Option<u16>,
    art_rows: Vec<u8>,
    dye_arrays: [Vec<u8>; TRANSLATION_DYE_DESCRIPTOR_OFFSETS.len()],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WeaponTranslationTopology {
    root: usize,
    art_rows: usize,
    dye_rows: [Option<usize>; TRANSLATION_DYE_DESCRIPTOR_OFFSETS.len()],
    descriptor_counts: [usize; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WeaponDamageCarrier {
    Empty,
    Fixed {
        family: WeaponDamageCarrierFamily,
        damage_type: ModernDamageType,
    },
    PlugDriven {
        damage_type: ModernDamageType,
        lane: usize,
    },
}

impl WeaponDamageCarrier {
    const fn descriptor(self) -> WeaponDamageDescriptor {
        match self {
            Self::Empty => WeaponDamageDescriptor::Empty,
            Self::Fixed { damage_type, .. } | Self::PlugDriven { damage_type, .. } => {
                WeaponDamageDescriptor::Elemental(damage_type)
            }
        }
    }

    const fn family(self) -> Option<WeaponDamageCarrierFamily> {
        match self {
            Self::Empty => None,
            Self::Fixed { family, .. } => Some(family),
            Self::PlugDriven { .. } => Some(WeaponDamageCarrierFamily::PlugDriven),
        }
    }
}

#[derive(Clone, Copy)]
struct KeyedAuxiliaryLayout {
    row_size: usize,
    row_class: u32,
    nested_offset: usize,
    nested_class: u32,
    secondary_class: Option<u32>,
    index_row_size: usize,
    index_row_class: u32,
    description: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyedAuxiliaryDonorPresence {
    Absent,
    Present(usize),
}

#[derive(Clone, Copy)]
struct KeyedAuxiliaryArrays {
    count: usize,
    header: usize,
    rows: usize,
}

#[derive(Clone, Copy)]
struct DenseItemPresentationArraySpec {
    descriptor: usize,
    row_size: usize,
    row_class: u32,
}

#[derive(Clone, Copy)]
struct DenseItemPresentationArray {
    spec: DenseItemPresentationArraySpec,
    count: usize,
    header: usize,
    rows: usize,
    rows_end: usize,
}

const DENSE_ITEM_PRESENTATION_ARRAYS: [DenseItemPresentationArraySpec; 5] = [
    DenseItemPresentationArraySpec {
        descriptor: ITEM_DENSE_ICON_TAG_DESCRIPTOR,
        row_size: ITEM_DENSE_ICON_TAG_ROW_SIZE,
        row_class: ITEM_DENSE_ICON_TAG_ROW_CLASS,
    },
    DenseItemPresentationArraySpec {
        descriptor: ITEM_DENSE_AUXILIARY_DESCRIPTOR,
        row_size: ITEM_DENSE_AUXILIARY_ROW_SIZE,
        row_class: ITEM_DENSE_AUXILIARY_ROW_CLASS,
    },
    DenseItemPresentationArraySpec {
        descriptor: ITEM_DENSE_ICON_SELECTOR_DESCRIPTOR,
        row_size: ITEM_DENSE_ICON_SELECTOR_ROW_SIZE,
        row_class: ITEM_DENSE_ICON_SELECTOR_ROW_CLASS,
    },
    DenseItemPresentationArraySpec {
        descriptor: ITEM_DENSE_PRESENTATION_DESCRIPTOR,
        row_size: ITEM_DENSE_PRESENTATION_ROW_SIZE,
        row_class: ITEM_DENSE_PRESENTATION_ROW_CLASS,
    },
    DenseItemPresentationArraySpec {
        descriptor: ITEM_DENSE_PRESENTATION_NEXT_DESCRIPTOR,
        row_size: ITEM_DENSE_TRAILING_ROW_SIZE,
        row_class: ITEM_DENSE_TRAILING_ROW_CLASS,
    },
];

#[derive(Clone, Copy)]
struct ItemHashIndexArray {
    descriptor: usize,
    count: usize,
    header: usize,
    rows: usize,
    rows_end: usize,
}

fn matching_u32_offsets(data: &[u8], value: u32) -> Vec<usize> {
    let bytes = value.to_le_bytes();
    data.windows(bytes.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == bytes).then_some(offset))
        .collect()
}

fn synchronize_payload_size(mut data: Vec<u8>) -> AuthoringResult<Vec<u8>> {
    let size =
        u64::try_from(data.len()).map_err(|_| invalid("Payload size does not fit 64 bits"))?;
    write_u64(&mut data, 0, size)?;
    if read_u64(&data, 0)? != size {
        return Err(validation("Serialized payload extent was not updated"));
    }
    Ok(data)
}

fn read_tag(manager: &PackageManager, tag: TagHash, description: &str) -> AuthoringResult<Vec<u8>> {
    manager
        .read_tag(tag)
        .map_err(|error| invalid(format!("Could not read {description} tag {tag}: {error}")))
}

fn allocate_identity_hash(
    namespace: &str,
    role: &str,
    occupied: &mut BTreeSet<u32>,
    minimum_exclusive: Option<u32>,
) -> AuthoringResult<u32> {
    for nonce in 0..=u16::MAX {
        let key = if nonce == 0 {
            format!("parhelion/{namespace}/{role}")
        } else {
            format!("parhelion/{namespace}/{role}/{nonce}")
        };
        let hash = fnv1_name_hash(&key);
        if hash != 0
            && hash != FNV1_EMPTY_HASH
            && minimum_exclusive.is_none_or(|minimum| hash > minimum)
            && occupied.insert(hash)
        {
            return Ok(hash);
        }
    }
    Err(invalid(format!(
        "Could not allocate a unique {role} hash for namespace {namespace:?}"
    )))
}
