//! Prepared cross-game weapon assets and their verified local references.

/// A conversion limit that depends only on the source weapon. No native donor
/// can change the outcome, so the donor loop reports it once instead of
/// rediscovering it for every candidate.
#[derive(Debug)]
pub struct SourceLimit;

impl std::fmt::Display for SourceLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("this source weapon needs importer support")
    }
}

impl std::error::Error for SourceLimit {}

/// Mark a failure as depending on the source weapon alone.
pub(crate) fn source_limit(error: anyhow::Error) -> anyhow::Error {
    error.context(SourceLimit)
}

/// Does this failure depend on the source weapon rather than the donor?
pub(crate) fn is_source_limit(error: &anyhow::Error) -> bool {
    error.downcast_ref::<SourceLimit>().is_some()
}
pub mod arrays;
pub mod artwork;
mod graph;
pub use graph::GraphReference;

pub mod assets;
pub mod batch;
pub mod bundle;
pub mod catalog;
pub mod convert;
pub mod dye_bundle;
pub mod dyes;
pub mod entity;
pub mod extract;
pub mod geometry;
pub mod icon;
pub mod localization;
pub mod mapping;
pub mod ornaments;
pub mod payload;
pub mod plated;
pub mod profile;
pub mod reader;
pub mod rig;
pub mod rig_convert;
pub mod shadowkeep;
mod skinning;
pub mod texture;

pub(crate) mod compatibility;
pub mod service;

pub mod audit;
pub mod collections;
pub mod inspect;

pub mod hud;
pub mod material;
pub mod native;
pub mod objectives;
pub mod ornament_audit;
pub mod ornament_recipe;
pub mod plates;
pub mod records;
pub mod support;
pub mod tfx;

pub mod gameplay;
pub mod lore;
