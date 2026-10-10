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

mod ability;
pub mod account;
pub mod activity_log;
mod app;
mod backups;
mod catalog;
mod dyes;
mod entity;
mod expression;
mod game_settings;
mod gear_markers;
mod hash;
pub mod image_processing;
pub mod investment;
mod model_preview;
pub mod package_authoring;
mod package_payload;
mod package_runtime;
mod persistence;
mod runtime;
mod sandbox_perk;
pub mod storage;
mod strict_json;
mod system;
#[cfg(test)]
mod test_support;
/// The headless capture helpers the dependent crates' tests share.
#[cfg(all(not(test), feature = "test-support"))]
pub mod test_support {
    pub mod capture;
}
pub mod ui;
mod updates;
pub mod version;

/// Runs Sundial with its in-process package-authoring utility.
pub use app::run;
