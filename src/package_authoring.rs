//! Host boundary and shared platform helpers for in-process package-authoring tools.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub use crate::package_runtime::reader::PackageManager;
pub use crate::package_runtime::{RuntimeBrand, RuntimeSnapshot};
use eframe::egui;

pub mod ui;
use serde::de::DeserializeOwned;

pub use crate::catalog::class_items::class_from_item_strings;
pub use crate::catalog::stock_subclass_list_classes;
pub use crate::investment::localization::{resolve_item_name, resolve_item_type_name};

/// Shared display-only classification used by the native item pickers.
pub fn is_dummy_item(hash: u32) -> bool {
    crate::catalog::dummy_items::contains(u64::from(hash))
}

/// Validated native weapon bucket capacities, shared with the installed inventory catalog.
pub fn weapon_bucket_capacities(
    manager: &PackageManager,
    root: &[u8],
) -> Result<[usize; 3], String> {
    crate::catalog::weapon_bucket_capacities(manager, root)
}
/// Rows one native bucket holds, checked to belong to a character or, with `profile`, to the
/// profile.
pub fn inventory_bucket_capacity(
    manager: &PackageManager,
    root: &[u8],
    bucket: u8,
    profile: bool,
) -> Result<usize, String> {
    crate::catalog::inventory_bucket_capacity(manager, root, bucket, profile)
}
pub use crate::package_runtime::tft;
pub use crate::package_runtime::{is_valid_package_tag, resolve_live_named_tag};

/// Checked reference fields in a newly authored payload, using a native template's layout.
/// Raw buffers, shader bytecode and audio backing data have no reflected fields.
pub fn declared_payload_references(
    manager: &PackageManager,
    template: u32,
    payload: &[u8],
) -> Result<Vec<(usize, u32)>, String> {
    crate::package_runtime::references::payload_fields(manager, template, payload)
}

pub use crate::dyes::{
    DYE_TEXTURE_EDGE, DyeFinish, DyeMaterial, DyeSource, DyeSurfaceMaterial, DyeTexture,
    DyeTextureSource, IridescenceRow, WeaponDyeColors, decode_source_material, dye_surfaces,
    load_dye_materials, load_dye_textures, load_iridescence_rows, load_weapon_dye_colors,
};

/// Native icon-definition layouts shared by Sundial's reader and package-authoring utilities.
pub mod icon_schema {
    pub use crate::catalog::icons::schema::{
        ICON_BACKGROUND_LAYER_OFFSET, ICON_DEFINITION_CLASS, ICON_DEFINITION_SIZE,
        ICON_FOREGROUND_LAYER_OFFSET, ICON_LAYER_ARRAY_CLASS, ICON_LAYER_CLASS,
        ICON_LAYER_LANE_CLASS, ICON_LAYER_REFERENCE_OFFSETS, ICON_LAYER_TEXTURE_CLASS,
        ICON_PRIMARY_LAYER_OFFSET, ICON_WATERMARK_LAYER_OFFSET, MAX_ICON_LAYER_LANES,
        MAX_ICON_TEXTURES_PER_LANE,
    };
}

/// Native investment table shapes shared by Sundial's reader and package-authoring utilities.
pub mod investment_schema {
    pub use crate::investment::schema::{
        ARC_DAMAGE_PLUG_ITEM_HASH, ARC_DAMAGE_PLUG_ITEM_INDEX, COLLECTIBLE_CONDITION_OFFSETS,
        COLLECTIBLE_DEFINITION_ROW_CLASS, COLLECTIBLE_DEFINITION_ROW_SIZE,
        COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
        COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET,
        COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_ROW_CLASS,
        COLLECTIBLE_DISPLAY_ROW_SIZE, COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET,
        COLLECTIBLE_HASH_OFFSET, COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET,
        COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET, CONDITION_EXPRESSION_ROW_CLASS,
        CONDITION_EXPRESSION_ROW_SIZE, ELEMENTAL_DAMAGE_SOCKET_TYPE, GLOBALS_ART_DYE_TABLE_SLOT,
        GLOBALS_COLLECTIBLE_DISPLAY_TABLE_SLOT, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT,
        GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT, GLOBALS_ITEM_ICON_TABLE_SLOT,
        GLOBALS_ITEM_METADATA_TABLE_SLOT, GLOBALS_ITEM_STRING_TABLE_SLOT,
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT, GLOBALS_OBJECTIVE_STRING_TABLE_SLOT,
        GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT, GLOBALS_RECORD_STRING_TABLE_SLOT,
        GLOBALS_SANDBOX_PATTERN_TABLE_SLOT, GLOBALS_SUBCLASS_DISPLAY_TABLE_SLOT,
        GLOBALS_UNLOCK_FLAG_DISPLAY_TABLE_SLOT, INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET,
        INVESTMENT_ROOT_CLASS, INVESTMENT_ROOT_TABLE_TAGS_OFFSET, INVESTMENT_TABLE_TAG_STRIDE,
        ITEM_DEFINITION_HASH_OFFSET, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_EQUIPMENT_BLOCK_CLASS,
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET, ITEM_EQUIPMENT_SLOT_OFFSET,
        ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET, ITEM_HASH_INDEX_ROW_CLASS, ITEM_HASH_INDEX_ROW_SIZE,
        ITEM_HASH_INDEX_TABLE_CLASS, ITEM_ICON_CONTAINER_OFFSET, ITEM_ICON_ROW_CLASS,
        ITEM_ICON_ROW_SIZE, ITEM_INDEX_ROW_SIZE, ITEM_INSTANCED_OFFSET, ITEM_INVENTORY_SLOT_OFFSET,
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
        ITEM_PLUG_CATEGORY_FALLBACK_OFFSET, ITEM_QUALITY_BLOCK_POINTER_OFFSET,
        ITEM_QUALITY_VERSION_DESCRIPTOR_OFFSET, ITEM_RARITY_OFFSET,
        ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET, ITEM_SANDBOX_PERK_ROW_CLASS,
        ITEM_SANDBOX_PERK_ROW_SIZE, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET,
        ITEM_SOCKET_ENTRY_LIST_BLOCK_SIZE, ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET,
        ITEM_STRING_AMMO_CLASS, ITEM_STRING_AMMO_CLASS_OFFSET, ITEM_STRING_AMMO_TYPE_OFFSET,
        ITEM_STRING_DAMAGE_TYPE_OFFSET, ITEM_STRING_DESCRIPTION_REFERENCE_OFFSET,
        ITEM_STRING_ICON_INDEX_OFFSET, ITEM_STRING_INDEX_ROW_CLASS,
        ITEM_STRING_NAME_REFERENCE_OFFSET, ITEM_STRING_SECONDARY_ICON_INDEX_OFFSET,
        ITEM_STRING_SOURCE_REFERENCE_OFFSET, ITEM_STRING_STAT_GROUP_INDEX_OFFSET,
        ITEM_STRING_STAT_GROUP_POINTER_OFFSET, ITEM_STRING_STAT_GROUP_RESOURCE_CLASS,
        ITEM_STRING_TYPE_REFERENCE_OFFSET, ITEM_STRING_UI_TEMPLATE_HASH_OFFSET,
        ITEM_TRAIT_ROW_CLASS, ITEM_TRAIT_ROW_SIZE, ITEM_TRAITS_DESCRIPTOR_OFFSET,
        ITEM_TRANSLATION_ART_DESCRIPTOR_OFFSET, ITEM_TRANSLATION_ART_ROW_CLASS,
        ITEM_TRANSLATION_ART_ROW_SIZE, ITEM_TRANSLATION_ART_VARIANT_OFFSET,
        ITEM_TRANSLATION_BLOCK_CLASS, ITEM_TRANSLATION_BLOCK_POINTER_OFFSET,
        ITEM_TRANSLATION_BLOCK_SIZE, ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS,
        ITEM_TRANSLATION_DYE_ROW_CLASS, ITEM_TRANSLATION_DYE_ROW_SIZE,
        ITEM_TRANSLATION_DYE_VARIANT_OFFSET, ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
        ITEM_VERSION_MAX_COUNT, ITEM_VERSION_ROW_CLASS, ITEM_VERSION_ROW_SIZE, ItemVersionArray,
        LEGACY_ARC_DAMAGE_PERK_INDEX, LEGACY_SOLAR_DAMAGE_PERK_INDEX,
        LEGACY_VOID_DAMAGE_PERK_INDEX, LOCALIZED_STRING_INDEX_ROW_CLASS,
        LOCALIZED_STRING_INDEX_ROW_SIZE, MATERIAL_REQUIREMENT_ROW_CLASS,
        MATERIAL_REQUIREMENT_ROW_SIZE, MATERIAL_REQUIREMENT_SET_ROW_CLASS,
        MATERIAL_REQUIREMENT_SET_ROW_SIZE, MODERN_ARC_DAMAGE_PERK_INDEX,
        MODERN_SOLAR_DAMAGE_PERK_INDEX, MODERN_VOID_DAMAGE_PERK_INDEX, NESTED_ARRAY_TRAILER,
        OBJECTIVE_COMPLETION_VALUE_OFFSET, OBJECTIVE_DEFINITION_ROW_CLASS,
        OBJECTIVE_DEFINITION_ROW_SIZE, OBJECTIVE_STRING_DESCRIPTION_REFERENCE_OFFSET,
        OBJECTIVE_STRING_NAME_REFERENCE_OFFSET, OBJECTIVE_STRING_PROGRESS_REFERENCE_OFFSET,
        OBJECTIVE_STRING_ROW_CLASS, OBJECTIVE_STRING_ROW_SIZE,
        PRESENTATION_NODE_DEFINITION_ROW_CLASS, PRESENTATION_NODE_DEFINITION_ROW_SIZE,
        PRESENTATION_NODE_HASH_OFFSET, PRESENTATION_NODE_INDEX_ROW_CLASS,
        PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET, PRESENTATION_NODE_PARENTS_OFFSET,
        PRESENTATION_NODE_STRING_ROW_SIZE, RECORD_DEFINITION_ROW_SIZE, RECORD_HASH_OFFSET,
        RECORD_OBJECTIVE_INDEX_ROW_CLASS, RECORD_STRING_ROW_SIZE,
        ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT, ROOT_ITEM_DEFINITION_TABLE_SLOT,
        ROOT_ITEM_HASH_INDEX_TABLE_SLOT, ROOT_ITEM_METADATA_INDEX_TABLE_SLOT,
        ROOT_MATERIAL_REQUIREMENT_TABLE_SLOT, ROOT_OBJECTIVE_DEFINITION_TABLE_SLOT,
        ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT, ROOT_RECORD_DEFINITION_TABLE_SLOT,
        ROOT_REUSABLE_PLUG_SET_TABLE_SLOT, ROOT_SANDBOX_PATTERN_INDEX_TABLE_SLOT,
        ROOT_SANDBOX_PERK_INDEX_TABLE_SLOT, ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT,
        ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT, ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT,
        ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT, SHARED_EXPRESSION_POOL_COUNT,
        SHARED_EXPRESSION_POOL_DIRECT_ROW_CLASS, SHARED_EXPRESSION_POOL_DIRECT_ROW_SIZE,
        SHARED_EXPRESSION_POOL_HASHED_EXPRESSION_OFFSET, SHARED_EXPRESSION_POOL_HASHED_ROW_CLASS,
        SHARED_EXPRESSION_POOL_HASHED_ROW_SIZE, SHARED_EXPRESSION_POOL_PARALLEL_ROW_CLASS,
        SOLAR_DAMAGE_PLUG_ITEM_HASH, SOLAR_DAMAGE_PLUG_ITEM_INDEX,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS, UNLOCK_FLAG_DEFINITION_ROW_SIZE,
        UNLOCK_FLAG_DISPLAY_CONTENT_ROW_CLASS, UNLOCK_FLAG_DISPLAY_CONTENT_ROW_SIZE,
        UNLOCK_FLAG_DISPLAY_ROW_CLASS, UNLOCK_FLAG_DISPLAY_ROW_SIZE,
        UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS, UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE,
        VOID_DAMAGE_PLUG_ITEM_HASH, VOID_DAMAGE_PLUG_ITEM_INDEX, investment_globals_table_tag,
        investment_root_table_tag, item_version_array, set_subclass_equipment_class,
        subclass_equipment_class,
    };
    pub use crate::investment::schema::{
        ITEM_METRIC_BLOCK_CLASS, ITEM_METRIC_BLOCK_POINTER_OFFSET, ITEM_METRIC_CATEGORY_ROW_CLASS,
        emblem_metric_categories,
    };
    pub use crate::investment::schema::{armor_equipment_class, set_armor_equipment_class};
}

/// Ability definition rows, which a Subclass pool record names its ability by.
pub mod ability_definition {
    pub use crate::ability::definition::{
        DEFINITION_TABLE_CLASS, DEFINITION_TABLE_SLOT, Definition, IDENTITY_TABLE_CLASS,
        IDENTITY_TABLE_SLOT, MOST_ROWS, append, definitions, entity, pattern, patterns,
    };
}

/// The glyph an ability's HUD tile shows, which its energy controller names and a bank row
/// can name in its place.
pub mod ability_hud {
    pub use crate::ability::hud::{
        AttachedGlyph, GLYPH_FIELD, GLYPH_TABLE, GlyphSite, VariantGlyph, all_variant_glyphs,
        attached_glyphs, glyph_keys, glyph_site, variant_glyphs,
    };
}

/// The ability keys of On a Specific Ability and Ends on a Specific Ability conditions.
pub mod ability_reference {
    pub use crate::ability::reference::{references, retarget};
}

/// A Subclass ability modifier's bank and the keys of its own property rows.
pub mod ability_modifier {
    pub use crate::ability::modifier::{
        RECHARGE_INPUT, bank_slot, charge_key, entity_bank, is_bank, parameter_key, recharge_key,
        recharge_modifier, settable_parameters, takes_recharge,
    };
}

/// Movement controller values with traced consumers, such as a Blink's distance.
pub mod ability_materials {
    pub use crate::ability::materials::{MaterialRoute, Routes, material_routes};
}

pub mod ability_movement {
    pub use crate::ability::movement::{
        MovementValue, PARAMETER_RESET, RowLane, Unit, discover, parameter_lane, row_lane,
        row_lanes, validate_bank_context,
    };
}

/// The entity graphs an entity spawns or attaches, placed by component resource.
pub mod ability_palette {
    pub use crate::ability::palette::{
        Graphs, MATERIAL_BINDINGS, MATERIAL_CLASS, PALETTE_FORMAT, PALETTE_HEIGHT, PALETTE_WIDTH,
        PARTICLE_SYSTEM_CLASS, Palette, PaletteUse, ParticleSite, SYSTEM_MATERIAL, ability_graphs,
        ability_palettes, binding_tag_offset, bindings, palette_data, palette_pixels, palettes,
        palettes_in_graphs, particle_sites,
    };
}

pub mod ability_tint {
    pub use crate::ability::tint::{
        ColorConstant, ConstantStore, Tint, TintUse, ability_tints, color_constants, is_tint,
        tints_in_graphs,
    };
}

/// Settings of an ability's parts that their native readers act on, such as a region's recovery.
pub mod ability_settings {
    pub use crate::ability::settings::{
        Codec, Creation, Kind, Lane, NativeProperty, Setting, Unit, creations, discover,
        validate_values,
    };
}

pub mod ability_spawns {
    pub use crate::ability::spawns::{
        Links, Spawn, describe, is_table, links, names, reached_graphs, self_spawns,
        spawned_graphs, spawns, table_entries, table_graphs, tables,
    };
}
pub mod ability_damage {
    pub use crate::ability::damage::{
        ARC, KINETIC, Profile, SOLAR, VOID, profile, references, retype,
    };
}

/// Ability bank property rows, and the charge row edit.
pub mod ability_bank {
    pub use crate::ability::bank::{
        AbilityTarget, CHARGE_DEFINITION_CLASS, CHARGE_INSTANCE_CLASS, CHARGE_ROWS,
        CLASS_ABILITY_CHARGE_KEY, CLASS_ABILITY_SLOT, ChargeRow, DEFINITION_ROW_CLASS,
        GRENADE_SLOT, HandlerIndex, INSTANCE_ROW_CLASS, JUMP_SLOT, MELEE_CHARGE_KEY,
        MELEE_DAMAGE_PROFILE, MELEE_SLOT, MOVEMENT_SLOT, Modifier, PARAMETER_NAMES,
        PARAMETER_ROW_CLASS, Parameter, ParameterKind, ParameterName, PropertyRow,
        SCRIPT_DEFINITION_CLASS, SCRIPT_INSTANCE_CLASS, SLOT_BANKS, SLOT_PARAMETERS, SUPER_SLOT,
        StockParameter, UNPLACED_BANKS, bank_name, bank_names, bank_owner, block_count,
        handler_slot, instance_shift, parameter_abilities, parameter_kind, parameter_label,
        parameter_meaning, parameter_name, parameters, property_rows, register_bank_names,
        retarget_references, row_modifiers, slot_banks, slot_name, slot_parameters, tuning_key,
        validate, with_charge_row, with_property_row,
    };
}

/// Finished sandbox-perk catalog and runtime-key map authoring.
pub mod sandbox_perk {
    pub use crate::sandbox_perk::{
        FINISHED_SANDBOX_PERK_CATALOG_CLASS, FINISHED_SANDBOX_PERK_DETAIL_ROW_CLASS,
        FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE, FINISHED_SANDBOX_PERK_ROW_CLASS,
        FINISHED_SANDBOX_PERK_ROW_SIZE, FinishedSandboxPerk, FinishedSandboxPerkName,
        FinishedSandboxPerkPresentation, SANDBOX_PERK_INDEX_CATALOG_CLASS,
        SANDBOX_PERK_INDEX_ROW_CLASS, SANDBOX_PERK_INDEX_ROW_SIZE,
        SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_CLASS, SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE,
        SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_CLASS, SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_SIZE,
        SANDBOX_PERK_RUNTIME_MAP_CLASS, SANDBOX_PERK_RUNTIME_MAP_TAG, SandboxPerkRuntimeAction,
        SandboxPerkRuntimeAssignment, SandboxPerkRuntimeGraphSource,
        clone_and_append_finished_sandbox_perk, clone_and_append_named_finished_sandbox_perk,
        clone_and_append_presented_finished_sandbox_perk, clone_and_append_sandbox_perk_index,
        finished_sandbox_perk_at, finished_sandbox_perk_count,
        insert_sandbox_perk_runtime_assignment, load_sandbox_perk_runtime_action,
        sandbox_perk_action_boxed_value_offset, sandbox_perk_index_count,
        sandbox_perk_index_hash_at, sandbox_perk_runtime_assignment,
        sandbox_perk_runtime_assignment_at, sandbox_perk_runtime_assignment_count,
        sandbox_perk_runtime_graph_sources, validate_finished_sandbox_perk_catalog,
        validate_sandbox_perk_index_catalog, validate_sandbox_perk_runtime_map,
    };
    pub use crate::sandbox_perk::{
        action, activation, dependencies, entity, ingredients, nodes, program,
    };
}

/// Sandbox-pattern and runtime entity graph helpers shared with package authoring tools.
pub mod entity {
    pub use crate::entity::{
        ComponentSplice, ComponentSpliceRecord, ComponentWiring,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        SANDBOX_PATTERN_GLOBAL_ID_OFFSET, SANDBOX_PATTERN_INDEX_ROW_CLASS,
        SANDBOX_PATTERN_INDEX_ROW_SIZE, SANDBOX_PATTERN_NESTED_CLASS,
        SANDBOX_PATTERN_NESTED_OFFSET, SANDBOX_PATTERN_ROW_CLASS, SANDBOX_PATTERN_ROW_SIZE,
        SandboxPatternIdentity, WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY,
        WEAPON_ENTITY_CLASS, WEAPON_ENTITY_COMPONENT_ROW_CLASS, WEAPON_ENTITY_COMPONENT_ROW_SIZE,
        WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS, WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS,
        WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE, WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
        WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE, WEAPON_INPUT_COMPONENT_KEY,
        WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY,
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        WEAPON_TRIGGER_COMPONENT_KEY, WeaponComponentBinding, append_weapon_entity_assignment,
        barrel, barrel_pellets, coupled_weapon_component_bindings, extend_weapon_components,
        extend_weapon_components_detaching, graft_weapon_component_binding,
        graft_weapon_component_bindings, graft_weapon_component_bindings_or_rewire,
        graft_weapon_component_bindings_with, grow_projectile_trajectories, owner,
        plan_component_splice, projectile_trajectory_capacity, remove_weapon_components,
        retarget_weapon_component_owner, retarget_weapon_component_owner_payload,
        sandbox_pattern_identity, sandbox_pattern_identity_at, spread, validate_weapon_entity,
        weapon_component_binding, weapon_component_binding_hashes, weapon_component_bindings,
        weapon_entity_assignment,
    };
}

/// Typed, data-driven runtime entity discovery shared with package authoring tools.
pub mod runtime {
    pub use crate::runtime::{
        BindingHash, GraphTag, NativeMember, NativeStructure, NativeStructureField,
        ResolvedWeaponRuntimeField, SchemaHandle, WeaponRuntimeBinding, WeaponRuntimeEntitySource,
        WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource,
        WeaponRuntimeGraph, WeaponRuntimeOwner, WeaponRuntimePathElement, WeaponRuntimeResource,
        WeaponRuntimeResourceShape, WeaponRuntimeRoot, WeaponRuntimeRootKind, WeaponRuntimeValue,
        WeaponRuntimeValueKind, WeaponRuntimeValueOverride, component_binding_label,
        decode_weapon_runtime_field_value, encode_weapon_runtime_field_value,
        encode_weapon_runtime_value, load_weapon_runtime_entities_at_pattern_indices_with_manager,
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_for_item_with_manager, load_weapon_runtime_entity_with_manager,
        load_weapon_runtime_graph, load_weapon_runtime_graph_for_entity,
        load_weapon_runtime_graph_with_manager, load_weapon_runtime_resource_shape, native_holders,
        native_member_names, native_members, native_type_name, resolve_weapon_runtime_field,
        runtime_fields_share_semantics,
    };
    pub use crate::runtime::{modifiers, presentation};
}

/// Every resource that references a resource, over the whole installation.
pub mod referrers {
    pub use crate::package_runtime::references::referrers::{CANCELLED, Referrers, Usage, read};
}

/// The named points a weapon's gear art carries, and the names recovered for them.
pub mod gear_markers {
    pub use crate::gear_markers::{
        CANCELLED, Marker, MarkerEntry, MarkerIndex, MarkerObject, MarkerSet, Neighbour,
        build_index, cached_index, describe, is_marker_set, marker_name, nearest_named,
        offset_markers, read_appearance, read_appearance_parts, shift_markers,
    };
}

/// Computes the client's case-insensitive 32-bit FNV-1 name hash.
#[must_use]
pub fn fnv1_name_hash(name: &str) -> u32 {
    crate::hash::fnv1_name_hash(name)
}

/// FNV-1's empty-string basis, reserved as the package-backed no-name sentinel.
pub const FNV1_EMPTY_HASH: u32 = crate::hash::FNV1_EMPTY_HASH;

/// Parses authored JSON with Sundial's duplicate-key and nesting checks.
pub fn parse_json<T: DeserializeOwned>(encoded: &str) -> Result<T, serde_json::Error> {
    crate::strict_json::from_str(encoded)
}

/// Parses a document that may nest `envelope` containers deeper than [`parse_json`] allows, such
/// as a bundle of recipes or a recipe holding custom perks, with the same checks.
pub fn parse_json_envelope<T: DeserializeOwned>(
    encoded: &str,
    envelope: usize,
) -> Result<T, serde_json::Error> {
    crate::strict_json::from_str_within(encoded, envelope)
}

/// Reads authored JSON with Sundial's duplicate-key and nesting checks.
pub fn read_json<R: io::Read, T: DeserializeOwned>(reader: R) -> Result<T, serde_json::Error> {
    crate::strict_json::from_reader(reader)
}
pub use crate::system::paths::{path_is_within, paths_equal, resolve_path_for_comparison};

/// Sundial-owned preferences used by an in-process package-authoring utility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackageAuthoringPreferences {
    /// Whether uncertain package-authoring controls should be visible.
    pub show_parhelion_experimental_options: bool,
}

/// Per-frame state returned by a package-authoring window hosted by Sundial.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackageAuthoringUpdate {
    /// Whether the hosted window remains open after this frame.
    pub open: bool,
    /// Whether package authoring has background work that must finish before exit.
    pub busy: bool,
    /// Whether the current recipe contains unsaved edits.
    pub dirty: bool,
    /// Whether a verified package installation completed since the previous update.
    pub packages_changed: bool,
    /// Whether an account operation finished and the active account should be refreshed.
    pub account_changed: bool,
    /// A preference change requested from the hosted Parhelion settings surface.
    pub preferences_changed: Option<PackageAuthoringPreferences>,
    /// Opens the host preferences without closing the workbench.
    pub open_sundial_preferences: bool,
}

/// An optional package-authoring surface composed into Sundial by the desktop executable.
pub trait PackageAuthoringUtility: Send {
    /// Opens or focuses the utility for Sundial's selected Shadowkeep installation.
    fn open(
        &mut self,
        context: &egui::Context,
        install_directory: &Path,
        preferences: PackageAuthoringPreferences,
    ) -> Result<(), String>;

    /// Draws the utility's native child viewport and reports lifecycle events to Sundial.
    fn update(&mut self, context: &egui::Context) -> PackageAuthoringUpdate;

    /// Writes view state that normally saves on focus loss. Sundial can exit with the utility open.
    fn save_on_exit(&mut self) {}
}

/// Creates a directory when needed and opens it in the platform file browser.
pub fn open_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    let mut command = if cfg!(target_os = "windows") {
        Command::new("explorer.exe")
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

fn validate_shadowkeep_install_directory(install: &Path) -> Result<PathBuf, String> {
    let install = fs::canonicalize(install).map_err(|error| {
        format!(
            "Could not resolve Sunrise install {}: {error}",
            install.display()
        )
    })?;
    crate::catalog::validate_install(&install)?;
    Ok(install)
}

/// Resolves a package directory without following it into a different installation.
/// Does not require game files, so interrupted installs remain recoverable.
pub fn resolve_packages_directory(packages: &Path) -> Result<PathBuf, String> {
    let selected = std::path::absolute(packages).map_err(|error| error.to_string())?;
    let selected_root = selected
        .parent()
        .ok_or("The packages directory has no game folder")?;
    let root = fs::canonicalize(selected_root).map_err(|error| {
        format!(
            "Could not resolve the selected game folder {}: {error}",
            selected_root.display()
        )
    })?;
    let resolved = fs::canonicalize(&selected).map_err(|error| {
        format!(
            "Could not resolve Shadowkeep packages directory {}: {error}",
            packages.display()
        )
    })?;
    if !resolved.is_dir() {
        return Err(format!(
            "The packages path is not a directory: {}",
            selected.display()
        ));
    }
    if resolved
        .parent()
        .is_none_or(|parent| !crate::system::paths::paths_equal(parent, &root))
    {
        return Err(format!(
            "The selected packages directory resolves outside its game folder: {} points to {}. Select an installation whose packages belong to that game folder.",
            selected.display(),
            resolved.display()
        ));
    }
    Ok(resolved)
}

/// Validates and canonicalizes the `packages` directory of a Shadowkeep installation.
pub fn validate_shadowkeep_packages_directory(packages: &Path) -> Result<PathBuf, String> {
    let packages = resolve_packages_directory(packages)?;
    let install = packages.parent().ok_or_else(|| {
        format!(
            "Packages directory has no install root: {}",
            packages.display()
        )
    })?;
    let install = validate_shadowkeep_install_directory(install)?;
    let expected = fs::canonicalize(install.join("packages")).map_err(|error| {
        format!(
            "Could not resolve validated packages directory below {}: {error}",
            install.display()
        )
    })?;
    if packages != expected {
        return Err(format!(
            "Selected packages directory {} is not the validated Shadowkeep packages directory {}",
            packages.display(),
            expected.display()
        ));
    }
    Ok(packages)
}

/// Identifies and snapshots the active runtime from its installed DLL.
pub fn installed_runtime(install: &Path) -> Result<RuntimeSnapshot, String> {
    crate::package_runtime::installed_runtime(install)
}

/// Confirms that runtime precedence, location, and DLL bytes still match a prior snapshot.
pub fn verify_installed_runtime(install: &Path, expected: &RuntimeSnapshot) -> Result<(), String> {
    crate::package_runtime::verify_installed_runtime(install, expected)
}

/// Checks that the selected installation advertises the runtime hooks required by authoring.
/// Package headers, generated manifest rows and client cache state are separate install checks.
pub fn validate_package_authoring_runtime(packages: &Path) -> Result<(), String> {
    let packages = validate_shadowkeep_packages_directory(packages)?;
    let install = packages.parent().ok_or_else(|| {
        format!(
            "Packages directory has no install root: {}",
            packages.display()
        )
    })?;
    crate::package_runtime::validate_package_authoring_runtime(install)
}

pub use crate::investment::discovery::open_packages as open_shadowkeep_package_manager;

/// Reports whether the Destiny 2 client is currently running.
pub fn destiny_is_running() -> Result<bool, String> {
    crate::app::platform::destiny_is_running()
}

/// Atomically replaces `destination` with the contents of `source`.
pub fn replace_file_from_path_atomically(source: &Path, destination: &Path) -> Result<(), String> {
    crate::storage::replace_file_from_path(source, destination).map_err(|error| {
        format!(
            "Could not atomically replace {} from {}: {error}",
            destination.display(),
            source.display()
        )
    })
}

/// Replaces `destination` with the contents of `source` only while `unchanged` accepts the
/// destination. It is checked after the copy beside the destination is flushed, immediately
/// before the rename.
pub fn replace_file_from_path_if_unchanged(
    source: &Path,
    destination: &Path,
    unchanged: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    crate::storage::replace_file_from_path_if(source, destination, || {
        unchanged().map_err(io::Error::other)
    })
    .map_err(|error| {
        format!(
            "Could not atomically replace {} from {}: {error}",
            destination.display(),
            source.display()
        )
    })
}

/// Publishes the contents of `source` at `destination` only when nothing is there yet, leaving
/// a file that appeared meanwhile as it is.
pub fn create_file_from_path(source: &Path, destination: &Path) -> Result<(), String> {
    crate::storage::create_file_from_path(source, destination).map_err(|error| {
        format!(
            "Could not create {} from {}: {error}",
            destination.display(),
            source.display()
        )
    })
}

/// Returns Parhelion's writable data directory below Sundial's per-user data directory.
#[must_use]
pub fn parhelion_data_directory() -> Option<PathBuf> {
    crate::system::paths::data_dir().map(|directory| directory.join("parhelion"))
}

/// Returns Parhelion's writable recipe library.
#[must_use]
pub fn parhelion_recipe_library_directory() -> Option<PathBuf> {
    parhelion_data_directory().map(|directory| directory.join("recipes"))
}

/// Atomically replaces a complete package-authoring file.
pub fn replace_authoring_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    crate::storage::replace_file(path, contents)
}

/// Publishes an authoring document only while its observed bytes still match.
/// The caller must hold its document or library write lease as well.
pub fn replace_authoring_file_if_unchanged(
    path: &Path,
    contents: &[u8],
    expected: &[u8],
) -> io::Result<()> {
    crate::storage::replace_file_if_unchanged(path, contents, expected)
}

/// Shared bounds-checked native readers. Mutation and authoring policy remain in Parhelion.
pub mod native_payload {
    pub use crate::package_payload::{bytes_at, native_array_at, relative_offset, write_bytes};
}
pub mod native_weapon {
    pub use crate::investment::weapon::*;
}

/// Checked decoding only. Loading-index writing and enrollment belong to Parhelion.
pub use crate::package_runtime::loading::index as loading_index;

pub mod account;
pub use crate::account::contract::{
    SHADOWKEEP_ACCOUNT_FLAG_BANK, SHADOWKEEP_ACCOUNT_FLAG_EXTENSION_CAPACITY,
    SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY, SHADOWKEEP_ACCOUNT_FLAG_REGION_OFFSET,
    SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS, SHADOWKEEP_ACCOUNT_VALUE_REGION_OFFSET,
};
