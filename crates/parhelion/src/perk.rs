//! Reusable custom-perk documents, independent of any weapon or socket.
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    HexHash, WeaponSandboxPerkRuntimeRecipe, WeaponSocketPlugVariantRecipe, WeaponStatOverride,
};

mod bundled;
pub mod library;

const SCHEMA: u32 = 1;
/// An ordinary native trait plug supplies the item layout for a new document.
/// Its effects are always replaced by the explicit authored effect list.
pub const DEFAULT_PLUG_LAYOUT: u32 = 0x45A0_BDD7;

/// Package artwork stays a reference. Downloaded artwork travels with the recipe.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Icon {
    Texture {
        tag: HexHash,
    },
    Image {
        name: String,
        image: crate::icon_edit::ImportedIcon,
    },
}

impl Icon {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Texture { tag } => {
                let tag = tiger_pkg::TagHash(tag.parse_u32().map_err(|error| error.to_string())?);
                if !sundial::package_authoring::is_valid_package_tag(tag)
                    || !crate::package_profile::is_stock_item_definition(tag.0)
                    || (crate::package_profile::MIN_AUTHORED_STANDALONE_PACKAGE_ID
                        ..=crate::package_profile::MAX_AUTHORED_STANDALONE_PACKAGE_ID)
                        .contains(&tag.pkg_id())
                {
                    return Err("Perk icon must reference an installed stock texture.".into());
                }
            }
            Self::Image { name, .. } if name.len() > 512 => {
                return Err("Perk icon name is too long.".into());
            }
            Self::Image { .. } => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerkRecipe {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub template_plug: HexHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::perk::Icon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<HexHash>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stats: Vec<WeaponStatOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<WeaponSandboxPerkRuntimeRecipe>,
}

impl Default for PerkRecipe {
    fn default() -> Self {
        Self::new()
    }
}

impl PerkRecipe {
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: SCHEMA,
            id: format!(
                "{:x}-{:x}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            name: "New Perk".into(),
            description: String::new(),
            template_plug: DEFAULT_PLUG_LAYOUT.into(),
            icon: None,
            classification: None,
            stats: Vec::new(),
            effects: Vec::new(),
        }
    }

    #[must_use]
    pub fn effect(index: u16) -> WeaponSandboxPerkRuntimeRecipe {
        WeaponSandboxPerkRuntimeRecipe {
            program: None,
            source_perk_index: index,
            activation: None,
            runtime_values: Vec::new(),
            action_float_values: Vec::new(),
            projectiles: Vec::new(),
        }
    }

    /// Drafts retain incomplete edits. Only the format and document identity
    /// must be valid before restoring them to the workbench.
    pub(crate) fn validate_draft(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("Unsupported custom perk schema {}", self.schema));
        }
        if self.id.is_empty()
            || self.id.len() > 80
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            return Err("The custom perk document has an invalid ID".into());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_draft()?;
        if self.stats.len() > 16 {
            return Err("Choose up to 16 supported stat bonuses".into());
        }
        self.at_socket(0, 0)
            .validate()
            .map_err(|error| error.to_string())
    }

    /// Attachment takes a copy. Subsequent library edits do not mutate weapons.
    #[must_use]
    pub fn at_socket(&self, socket_index: u16, choice_index: u16) -> WeaponSocketPlugVariantRecipe {
        WeaponSocketPlugVariantRecipe {
            replace_effects: true,
            socket_index,
            choice_index,
            source_plug_hash: self.template_plug.clone(),
            name: Some(self.name.clone()),
            // A custom perk owns its description, including an intentionally blank one.
            // None would inherit unrelated gameplay text from the icon donor.
            description: Some(if self.description.trim().is_empty() {
                String::new()
            } else {
                self.description.clone()
            }),
            icon: self.icon.clone(),
            classification_donor_hash: self.classification.clone(),
            investment_stats: self.stats.clone(),
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: self.effects.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
