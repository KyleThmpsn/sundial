//! Parhelion's Sundial-hosted UI and offline package-authoring backend.
//!
//! Builds read source packages without modifying them, writes a fresh verified staging run, and
//! installs only the validated authored patch set through an explicit transaction. Installation
//! also synchronizes the authored collection unlocks with the selected account settings.
//!
//! Maintenance boundaries:
//! - `recipe` owns the saved authoring model; `capabilities` validates supported combinations.
//! - `app` owns drafts, selection and background-job lifetimes, not package layout knowledge.
//! - `weapon` resolves donors and builds native definition/runtime plans; `icon_edit`
//!   separates image transforms, previews and controls from private icon-graph emission.
//! - `workflow` coordinates compilation and staging; `install` owns commit and recovery.
//! - Sundial's `investment` and `package_authoring` APIs provide shared catalog/account services.

mod app;
mod appended_tags;
mod artifact;
mod asset_packages;
mod badge;
mod badge_icon;
mod block_codec;
mod capabilities;
mod chain;
mod error;
mod extend;
mod format;
pub mod hud_icon;
mod icon_edit;
mod install;
mod manifest;
mod package_profile;
mod payload_guards;
mod plug_classification;
mod preferences;
mod progression;
mod recipe;
mod recipe_library;
mod runtime;
mod shared_tag_dependency_index;
pub use shared_tag_dependency_index::partition::{LoadingResource, partition_loading_resources};
pub use shared_tag_dependency_index::scoped::{LoadingOwner, clone_scoped_dependencies};
mod shared_tag_memory;
mod tag_payload;
mod watermark;
mod weapon;
mod weapon_ammo;
mod workflow;

pub use app::Parhelion;
pub use artifact::ArtifactMetadata;
pub(crate) use badge::SunriseProjectMetadata;
pub(crate) use badge::{
    SUNRISE_BADGE_DESCRIPTION, SUNRISE_BADGE_DESCRIPTION_HASH, SUNRISE_BADGE_NAME,
    SUNRISE_BADGE_NAME_HASH, SUNRISE_BADGE_NODE_HASHES, SunriseBadgePlacement,
    append_badge_icon_row, author_sunrise_badge_graph, patch_badges_root_objective,
    sunrise_badge_collectible_parents,
};
pub(crate) use badge_icon::build_badge_icon_plan;
pub(crate) use capabilities::{
    CombatProfileAction, SupportedPlugSet, apply_combat_profile_action, authored_inventory_slot,
    presentation_donor_candidate_is_compatible, recipe_combat_profile_action,
    reconcile_presentation_donor, selected_presentation_donor_is_compatible,
    validate_socket_column_overrides_with_socket_types, validate_stat_overrides,
    weapon_authoring_capabilities,
};
pub(crate) use chain::{PackageIdentity, PatchChain, PatchFile};
pub(crate) use error::{AuthoringError, AuthoringResult};
pub(crate) use extend::{
    ExtendedOverlayArtifact, ReplacementSpec, build_extended_overlay,
    build_extended_overlay_with_references, build_standalone_package_with_references,
};
pub(crate) use extend::{NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode};
pub(crate) use format::SUNDIAL_BUILD_SIGNATURE;
pub(crate) use icon_edit::WeaponIconEdit;
pub use install::{
    BackupPruneReport, DEFAULT_PACKAGE_BACKUP_RETENTION, InstallError, InstallReport,
    InstallRequest, MAX_PACKAGE_BACKUP_RETENTION, ReplacementReview, UninstallPlan,
    UninstallReport, install_staged_packages, preview_replacement, preview_uninstall,
    preview_uninstall_with_account_cleanup, prune_package_backups, uninstall_custom_packages,
};
#[cfg(test)]
pub(crate) use recipe::{ARC_LOGIC_DONOR_HASH, RecipeCollectionPlacement};
pub(crate) use recipe::{
    HexHash, RecipeAmmoType, RecipeRarity, RecipeRawPayloadTarget, WeaponArtArrangementRecipe,
    WeaponDonorReference, WeaponDyeReferenceRecipe, WeaponLocaleTextRecipe,
    WeaponNumericInstructionRecipe, WeaponRawPayloadPatchRecipe, WeaponRecipeOverrides,
    WeaponRuntimeResourcePatchRecipe, WeaponSocketColumnRecipe, WeaponStatOverride,
};
pub use recipe::{
    WeaponRecipe, WeaponSandboxPerkActionFloatRecipe, WeaponSandboxPerkRuntimeRecipe,
    WeaponSocketPlugVariantRecipe,
};
pub(crate) use recipe_library::{RecipeLibrary, RecipeLibraryEntry};
pub(crate) use watermark::{WeaponIconRequest, build_watermark_plan, item_icon_row_with_container};
pub(crate) use weapon::{
    AuthoredWeaponRarity, ModernDamageType, NewWeaponPlan, NewWeaponProjectBundle, WeaponAmmoType,
    WeaponArtArrangementOverride, WeaponCloneIdentity, WeaponCloneOverrides, WeaponCloneSpec,
    WeaponCloneText, WeaponDyeReferenceOverride, WeaponIconDonorReference, WeaponInventorySlot,
    WeaponLocaleTextOverride, WeaponNumericInstruction, WeaponPresentationDonorReference,
    WeaponProjectSpec, WeaponRawPayloadPatch, WeaponRawPayloadTarget,
    WeaponRenderGearDonorReference, WeaponRuntimeComponentDonorReference,
    WeaponRuntimeResourcePatch, WeaponSandboxPerkActionFloatOverride,
    WeaponSandboxPerkRuntimeOverride, WeaponSocketColumnOverride, WeaponSocketPlugVariantOverride,
};
pub use workflow::{
    BatchBuildRequest, BatchBuildSnapshot, BuildProgress, BuildReport,
    build_and_stage_snapshot_with_progress,
};
