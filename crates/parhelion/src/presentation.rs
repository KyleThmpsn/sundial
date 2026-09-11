//! Optional authored set membership, release watermark artwork and weapon lore.
mod artwork;
pub(crate) mod composition;
mod editor;
pub use artwork::Artwork;
pub(crate) mod ui;

use serde::{Deserialize, Serialize};

/// Sunrise's node reader has 1,024 slots. Stock plus the Sunrise badge use 928.
pub const MAX_CUSTOM_BADGES: usize =
    (crate::collection::NODE_CAPACITY - crate::collection::BASE_NODE_COUNT) / 4;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Badge {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Artwork>,
}

impl Badge {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name != self.name.trim()
            || self.name.len() > 128
            || self.name.chars().any(char::is_control)
        {
            return Err(
                "Badge names must contain 1 to 128 bytes with no surrounding spaces.".into(),
            );
        }
        if self.name.eq_ignore_ascii_case("Project Sunrise") {
            return Err("Choose a custom badge name other than Project Sunrise.".into());
        }
        validate_text(&self.description, 4096, "Badge description")
    }
}

pub(crate) fn validate_text(text: &str, limit: usize, label: &str) -> Result<(), String> {
    if text.len() > limit
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(format!(
            "{label} must fit within {limit} UTF-8 bytes and contain no unsupported control characters."
        ));
    }
    Ok(())
}

/// Use a separate naming domain and the localization bank's audited lower bound.
pub(crate) fn text_hash(namespace: &str, field: &str) -> u32 {
    for salt in 0u32.. {
        let hash = sundial::package_authoring::fnv1_name_hash(&format!(
            "parhelion/presentation/{namespace}/{field}/{salt}"
        ));
        if hash > 0x304D_56FE && hash != sundial::package_authoring::FNV1_EMPTY_HASH {
            return hash;
        }
    }
    unreachable!()
}
