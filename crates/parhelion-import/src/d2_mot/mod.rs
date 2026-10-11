//! Prepared cross-game weapon assets and their verified local references.

/// A conversion limit that depends only on the source weapon. No native donor
/// can change the outcome, so the donor loop reports it once instead of
/// rediscovering it for every candidate.
#[derive(Debug)]
pub struct SourceLimit;

impl std::fmt::Display for SourceLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("this source item needs importer support")
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
pub(crate) use crate::tiger::audio;
pub use audio::bank::{
    Bank as ConvertedAudioBank, Namespace as AudioNamespace, lower as lower_audio_bank,
    lower_with_settings as lower_audio_bank_with_settings, settings::Settings as AudioSettings,
};
pub use audio::cue;
pub use audio::legacy::fit_variations;
pub use audio::modern::default_layers;
pub use audio::transcode::{mix_pcm, normalize_pcm_wem, pcm_bank_template};
pub use audio::{prepare as prepare_audio, prepare_clip_events, prepare_sounds, sound_assets};
pub mod artwork;
pub mod crosshair;
use crate::graph;
pub use crate::tiger::markers;
pub use graph::{GraphReference, manifest_converter_revision};

pub mod assets;
pub mod batch;
pub mod bundle;
pub mod catalog;
pub mod cloth;
pub mod convert;
pub mod dye_bundle;
pub mod dyes;
pub mod entity;
pub mod extract;
pub use crate::tiger::geometry;
pub mod icon;
pub mod kept_parts;
pub mod localization;
pub mod mapping;
pub mod ornaments;
pub mod particles;
pub use crate::tiger::payload;
pub mod plated;
pub mod profile;
pub use crate::tiger::reader;
pub use crate::tiger::rig;
pub mod rig_convert;
pub use crate::tiger::shadowkeep;
pub(crate) use crate::tiger::skinning;
pub use crate::tiger::texture;

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
pub(crate) mod glaive;
pub mod lore;
