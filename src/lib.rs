//! Sundial's application and adapters for editing Project Sunrise accounts.
//!
//! Maintenance boundaries:
//! - `sundial-account` owns storage-neutral account rules; `persistence` adapts
//!   them to each supported file format without discarding unknown fields.
//! - `app` owns UI state and workflow orchestration, not native package layouts.
//! - [`investment`] exposes discovered game definitions; its plug-selection
//!   policy is shared by editors and loadout tools.
//! - [`package_authoring`] is the host contract for Parhelion. Native decoding
//!   stays in format-specific modules using checked `package_payload` reads.
//! - `storage` and `backups` own durable replacement and recovery primitives.
//!
//! Keep validation, planning, mutation and commit distinct so a UI action can be
//! tested without installing packages or writing an account.

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("Sundial supports Windows and Linux");

mod account_contract;
pub mod activity_log;
mod app;
mod backups;
mod catalog;
mod class_items;
mod dummy_items;
mod game_settings;
mod hash;
mod http;
mod icon_schema;
pub mod image_processing;
pub mod investment;
mod investment_localization;
mod investment_schema;
mod native_weapon;
pub mod package_authoring;
mod package_payload;
mod package_runtime;
mod paths;
mod persistence;
mod sandbox_perk;
mod storage;
mod strict_json;
mod subclass;
#[cfg(test)]
mod test_support;
mod ui_help;
mod unnamed_plugs;
mod updates;
mod weapon_dyes;
mod weapon_entity;
mod weapon_runtime;

/// Runs Sundial with its in-process package-authoring utility.
pub fn run(
    package_authoring: Box<dyn package_authoring::PackageAuthoringUtility>,
) -> eframe::Result<()> {
    app::run(package_authoring)
}
