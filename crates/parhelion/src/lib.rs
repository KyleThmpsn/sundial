//! Parhelion's Sundial-hosted UI and offline package-authoring backend.
//!
//! Builds read source packages without modifying them, writes a fresh verified staging run, and
//! installs only the validated authored patch set through an explicit transaction. Installation
//! also synchronizes the authored collection unlocks with the selected account settings.
//!
//! Maintenance boundaries:
//! - `recipe` owns the saved authoring model; `capabilities` validates supported combinations.
//! - `app` owns drafts, selection and background-job lifetimes, not package layout knowledge.
//! - `item` is the build every item kind goes through: it resolves donors and builds native
//!   definition and runtime plans. `weapon` and `subclass` hold their kinds' own rules, which it
//!   applies. `icon::edit` separates image transforms, previews and controls from private
//!   icon-graph emission.
//! - `workflow` coordinates compilation and staging; `install` owns commit and recovery.
//! - Sundial's `investment` and `package_authoring` APIs provide shared catalog/account services.

/// Dawn bundle tooling, kept in a folder outside the repository and compiled in by the `dawn`
/// feature. Its command line tool builds from the same sources, so some of it is unused here.
#[cfg(feature = "dawn")]
#[rustfmt::skip]
#[path = "../../parhelion-dawn/src/lib.rs"]
#[allow(dead_code, unreachable_pub)]
mod parhelion_dawn;

mod ability;
mod app;
mod armor;
mod artifact;
pub(crate) mod artwork_browser;
mod asset_packages;
mod badge;
mod branding;
mod bundled_defaults;
mod capabilities;
pub mod collection;
mod dxbc;
pub mod dye;
pub mod emblem;
mod error;
pub mod icon;
mod image_import;
#[cfg(feature = "d2-model-importer")]
mod imported;
mod install;
mod item;
mod item_kind;
mod manifest;
mod mod_family;
pub use mod_family::{ModReport, ModStatus};
pub mod map;
mod package;
pub mod perk;
mod plug_classification;
mod preferences;
pub mod presentation;
mod progression;
mod recipe;
mod recipe_library;
mod runtime;
#[cfg(feature = "d2-model-importer")]
pub mod shader;
mod shared_tag_dependency_index;
pub use shared_tag_dependency_index::partition::{LoadingResource, partition_loading_resources};
pub use shared_tag_dependency_index::scoped::{LoadingOwner, clone_scoped_dependencies};
mod shared_tag_memory;
pub mod stat_group;
pub mod subclass;
mod tag_payload;
#[cfg(test)]
mod test_support;
mod turn_cache;
pub mod vehicle;
pub mod weapon;
mod workflow;

pub use app::Parhelion;
#[cfg(feature = "d2-model-importer")]
pub use app::save_reimported;
pub use armor::Class as ArmorClass;
pub use artifact::ArtifactMetadata;
pub(crate) use badge::SunriseProjectMetadata;
pub(crate) use badge::{
    SunriseBadgePlacement, append_badge_icon_row, author_sunrise_badge_graph,
    sunrise_badge_collectible_parents,
};
pub(crate) use capabilities::{
    CombatProfileAction, SupportedPlugSet, apply_combat_profile_action, authored_inventory_slot,
    presentation_donor_candidate_is_compatible, recipe_combat_profile_action,
    reconcile_presentation_donor, selected_presentation_donor_is_compatible,
    validate_socket_column_overrides_with_variants, validate_stat_overrides,
    weapon_authoring_capabilities,
};
pub(crate) use error::{AuthoringError, AuthoringResult};
#[cfg(test)]
pub(crate) use icon::badge::build_badge_icon_plan;
pub(crate) use icon::edit::WeaponIconEdit;
pub(crate) use icon::watermark::{WeaponIconRequest, item_icon_row_with_container};
pub use install::{
    AccountResyncReport, BackupPruneReport, DEFAULT_PACKAGE_BACKUP_RETENTION, InstallError,
    InstallPhase, InstallProgress, InstallReport, InstallRequest, MAX_PACKAGE_BACKUP_RETENTION,
    ReplacementReview, UninstallPlan, UninstallReport, install_staged_packages,
    install_staged_packages_with_progress, preview_replacement, preview_uninstall,
    preview_uninstall_with_account_cleanup, prune_package_backups, resync_account,
    uninstall_custom_packages,
};
pub(crate) use item::{
    AuthoredWeaponRarity, ModernDamageType, NewCollectionPlan, NewWeaponPlan,
    NewWeaponProjectBundle, RuntimeComponentDonorReference, RuntimeResourcePatch,
    SwordProfileOverride, WeaponAmmoType, WeaponArtArrangementOverride, WeaponCloneIdentity,
    WeaponCloneOverrides, WeaponCloneSpec, WeaponCloneText, WeaponDyeReferenceOverride,
    WeaponIconDonorReference, WeaponInventorySlot, WeaponLocaleTextOverride,
    WeaponNumericInstruction, WeaponPresentationDonorReference, WeaponProjectSpec,
    WeaponRawPayloadPatch, WeaponRenderGearDonorReference, WeaponSandboxPerkActionFloatOverride,
    WeaponSandboxPerkRuntimeOverride, WeaponSocketColumnOverride, WeaponSocketPlugVariantOverride,
    WeaponVariableDamage,
};
pub use item_kind::ItemKind;
pub(crate) use package::chain::{PackageIdentity, PatchChain, PatchFile};
#[cfg(test)]
pub(crate) use package::extend::build_standalone_package_with_references;
pub(crate) use package::extend::{ExtendedOverlayArtifact, ReplacementSpec};
pub(crate) use package::extend::{
    NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
};
pub(crate) use package::format::SUNDIAL_BUILD_SIGNATURE;
pub(crate) use recipe::{
    HexHash, RecipeAmmoType, RecipeRarity, RecipeRawPayloadTarget, RuntimeResourcePatchRecipe,
    WeaponArtArrangementRecipe, WeaponDonorReference, WeaponDyeReferenceRecipe,
    WeaponLocaleTextRecipe, WeaponNumericInstructionRecipe, WeaponRawPayloadPatchRecipe,
    WeaponRecipeOverrides, WeaponSocketColumnRecipe, WeaponStatOverride,
};
pub use recipe::{
    SwordProfileRecipe, WeaponRecipe, WeaponSandboxPerkActionFloatRecipe,
    WeaponSandboxPerkRuntimeRecipe, WeaponSocketPlugVariantRecipe,
};
pub(crate) use recipe_library::{RecipeLibrary, RecipeLibraryEntry};
pub use workflow::{
    BatchBuildRequest, BatchBuildSnapshot, BuildActivity, BuildFailure, BuildProgress, BuildReport,
    MemberProgress, OperationStatus, SkippedRecipe, build_and_stage_snapshot_reporting,
    build_and_stage_snapshot_skipping, build_and_stage_snapshot_with_progress,
};
