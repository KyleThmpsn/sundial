//! Host boundary and shared platform helpers for in-process package-authoring tools.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use eframe::egui;
use serde::de::DeserializeOwned;
use tiger_pkg::PackageManager;

pub use crate::investment_localization::resolve_item_name;
pub use crate::package_runtime::{is_valid_package_tag, resolve_live_named_tag};
pub use crate::weapon_dyes::{WeaponDyeColors, load_weapon_dye_colors};

/// Native icon-definition layouts shared by Sundial's reader and package-authoring utilities.
pub mod icon_schema {
    pub use crate::icon_schema::{
        ICON_BACKGROUND_LAYER_OFFSET, ICON_DEFINITION_CLASS, ICON_DEFINITION_SIZE,
        ICON_FOREGROUND_LAYER_OFFSET, ICON_LAYER_ARRAY_CLASS, ICON_LAYER_CLASS,
        ICON_LAYER_LANE_CLASS, ICON_LAYER_REFERENCE_OFFSETS, ICON_LAYER_TEXTURE_CLASS,
        ICON_PRIMARY_LAYER_OFFSET, ICON_WATERMARK_LAYER_OFFSET, MAX_ICON_LAYER_LANES,
        MAX_ICON_TEXTURES_PER_LANE,
    };
}

/// Native investment table shapes shared by Sundial's reader and package-authoring utilities.
pub mod investment_schema {
    pub use crate::investment_schema::{
        ARC_DAMAGE_PLUG_ITEM_HASH, ARC_DAMAGE_PLUG_ITEM_INDEX, COLLECTIBLE_CONDITION_OFFSETS,
        COLLECTIBLE_DEFINITION_ROW_CLASS, COLLECTIBLE_DEFINITION_ROW_SIZE,
        COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
        COLLECTIBLE_DISPLAY_NAME_REFERENCE_OFFSET,
        COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET, COLLECTIBLE_DISPLAY_ROW_CLASS,
        COLLECTIBLE_DISPLAY_ROW_SIZE, COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET,
        COLLECTIBLE_HASH_OFFSET, COLLECTIBLE_INVENTORY_ITEM_INDEX_OFFSET,
        COLLECTIBLE_MATERIAL_REQUIREMENT_SET_INDEX_OFFSET,
        COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET, CONDITION_EXPRESSION_ROW_CLASS,
        CONDITION_EXPRESSION_ROW_SIZE, ELEMENTAL_DAMAGE_SOCKET_TYPE,
        GLOBALS_COLLECTIBLE_DISPLAY_TABLE_SLOT, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT,
        GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT, GLOBALS_ITEM_ICON_TABLE_SLOT,
        GLOBALS_ITEM_METADATA_TABLE_SLOT, GLOBALS_ITEM_STRING_TABLE_SLOT,
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT, GLOBALS_OBJECTIVE_STRING_TABLE_SLOT,
        GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT, GLOBALS_RECORD_STRING_TABLE_SLOT,
        GLOBALS_SANDBOX_PATTERN_TABLE_SLOT, GLOBALS_UNLOCK_FLAG_DISPLAY_TABLE_SLOT,
        INVESTMENT_GLOBALS_TABLE_TAGS_OFFSET, INVESTMENT_ROOT_CLASS,
        INVESTMENT_ROOT_TABLE_TAGS_OFFSET, INVESTMENT_TABLE_TAG_STRIDE,
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
        ITEM_STRING_DESCRIPTION_REFERENCE_OFFSET, ITEM_STRING_ICON_INDEX_OFFSET,
        ITEM_STRING_INDEX_ROW_CLASS, ITEM_STRING_NAME_REFERENCE_OFFSET,
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
        ROOT_SANDBOX_PATTERN_INDEX_TABLE_SLOT, ROOT_SANDBOX_PERK_INDEX_TABLE_SLOT,
        ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT, ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT,
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
        investment_root_table_tag, item_version_array,
    };
}

/// Finished sandbox-perk catalog and runtime-key map authoring.
pub mod sandbox_perk {
    pub use crate::sandbox_perk::activation;
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
        sandbox_perk_runtime_graph_sources, sunrise_perk_projection_warning,
        validate_finished_sandbox_perk_catalog, validate_sandbox_perk_index_catalog,
        validate_sandbox_perk_runtime_map,
    };
}

/// Weapon sandbox-pattern and runtime entity graph helpers shared with package authoring tools.
pub mod weapon_entity {
    pub use crate::weapon_entity::{
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
        graft_weapon_component_binding, graft_weapon_component_bindings,
        retarget_weapon_component_owner, retarget_weapon_component_owner_payload,
        sandbox_pattern_identity, sandbox_pattern_identity_at, validate_weapon_entity,
        weapon_component_binding, weapon_component_binding_hashes, weapon_component_bindings,
        weapon_entity_assignment,
    };
}

/// Typed, data-driven weapon runtime discovery shared with package authoring tools.
pub mod weapon_runtime {
    pub use crate::weapon_runtime::{
        ResolvedWeaponRuntimeField, WeaponRuntimeBinding, WeaponRuntimeEntitySource,
        WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource,
        WeaponRuntimeGraph, WeaponRuntimeOwner, WeaponRuntimePathElement, WeaponRuntimeRoot,
        WeaponRuntimeRootKind, WeaponRuntimeValue, WeaponRuntimeValueKind,
        WeaponRuntimeValueOverride, encode_weapon_runtime_value,
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager, load_weapon_runtime_graph,
        load_weapon_runtime_graph_for_entity, load_weapon_runtime_graph_with_manager,
        resolve_weapon_runtime_field,
    };
}

/// Computes the client's case-insensitive 32-bit FNV-1 name hash.
#[must_use]
pub fn fnv1_name_hash(name: &str) -> u32 {
    crate::hash::fnv1_name_hash(name)
}

/// FNV-1's empty-string basis, reserved as the package-backed no-name sentinel.
pub const FNV1_EMPTY_HASH: u32 = crate::hash::FNV1_EMPTY_HASH;

/// One-based unlock-map bank backed by the Shadowkeep account object's primary flag region.
pub const SHADOWKEEP_ACCOUNT_FLAG_BANK: u8 = 1;
/// Start of the Shadowkeep account object's primary unlock-flag byte region.
pub const SHADOWKEEP_ACCOUNT_FLAG_REGION_OFFSET: usize = 29_740;
/// Start of the next account-object region after the primary unlock-flag bytes.
pub const SHADOWKEEP_ACCOUNT_VALUE_REGION_OFFSET: usize = 42_040;
/// Stock rows mapped into the primary account unlock-flag region.
pub const SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS: usize = 11_923;
/// Complete byte capacity available before the following account-object region begins.
///
/// Authored flag-map rows may claim the stock padding after
/// [`SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS`], but must never cross this boundary.
pub const SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY: usize =
    SHADOWKEEP_ACCOUNT_VALUE_REGION_OFFSET - SHADOWKEEP_ACCOUNT_FLAG_REGION_OFFSET;
/// Number of primary account unlock-flag rows available to authored extensions.
pub const SHADOWKEEP_ACCOUNT_FLAG_EXTENSION_CAPACITY: usize =
    SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY - SHADOWKEEP_ACCOUNT_FLAG_STOCK_ROWS;

/// Parses authored JSON with Sundial's duplicate-key and nesting checks.
pub fn parse_json<T: DeserializeOwned>(encoded: &str) -> Result<T, serde_json::Error> {
    crate::strict_json::from_str(encoded)
}

/// Reads authored JSON with Sundial's duplicate-key and nesting checks.
pub fn read_json<R: io::Read, T: DeserializeOwned>(reader: R) -> Result<T, serde_json::Error> {
    crate::strict_json::from_reader(reader)
}
pub use crate::paths::{path_is_within, paths_equal, resolve_path_for_comparison};

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
    /// A preference change requested from the hosted Parhelion settings surface.
    pub preferences_changed: Option<PackageAuthoringPreferences>,
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
            "Could not resolve Shadowkeep install {}: {error}",
            install.display()
        )
    })?;
    crate::catalog::validate_install(&install)?;
    Ok(install)
}

/// Validates and canonicalizes the `packages` directory of a Shadowkeep installation.
pub fn validate_shadowkeep_packages_directory(packages: &Path) -> Result<PathBuf, String> {
    let packages = fs::canonicalize(packages).map_err(|error| {
        format!(
            "Could not resolve Shadowkeep packages directory {}: {error}",
            packages.display()
        )
    })?;
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

/// Opens a Shadowkeep package directory through Sundial's cross-platform runtime.
///
/// The directory may be the installed package set or an isolated authoring view whose parent is
/// laid out like a Shadowkeep installation.
pub fn open_shadowkeep_package_manager(packages: &Path) -> Result<PackageManager, String> {
    let install = packages.parent().ok_or_else(|| {
        format!(
            "Packages directory has no install root: {}",
            packages.display()
        )
    })?;
    crate::package_runtime::open_shadowkeep_packages(install)
}

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

/// Returns Parhelion's writable data directory below Sundial's per-user data directory.
#[must_use]
pub fn parhelion_data_directory() -> Option<PathBuf> {
    crate::paths::data_dir().map(|directory| directory.join("parhelion"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_account_flags_are_bounded_by_the_next_native_region() {
        assert_eq!(SHADOWKEEP_ACCOUNT_FLAG_BANK, 1);
        assert_eq!(SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY, 12_300);
        assert_eq!(SHADOWKEEP_ACCOUNT_FLAG_EXTENSION_CAPACITY, 377);
        assert_eq!(
            SHADOWKEEP_ACCOUNT_FLAG_REGION_OFFSET + SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY,
            SHADOWKEEP_ACCOUNT_VALUE_REGION_OFFSET
        );
    }
}
/// Shared bounds-checked native readers. Mutation and authoring policy remain in Parhelion.
pub mod native_payload {
    pub use crate::package_payload::{bytes_at, native_array_at, relative_offset, write_bytes};
}
pub mod native_weapon {
    pub use crate::native_weapon::*;
}
