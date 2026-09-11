//! Reusable custom-perk documents, independent of any weapon or socket.
use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerkRecipe {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub template_plug: HexHash,
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
        if self.name.trim().is_empty()
            || self.name.contains('\0')
            || self.description.contains('\0')
        {
            return Err("Enter a perk name without null characters".into());
        }
        self.template_plug
            .parse_u32()
            .map_err(|error| error.to_string())?;
        if let Some(hash) = &self.classification {
            hash.parse_u32().map_err(|error| error.to_string())?;
        }
        let mut effects = BTreeSet::new();
        for effect in &self.effects {
            if let Some(program) = &effect.program {
                program.validate_structure()?;
                if !effect.runtime_values.is_empty()
                    || !effect.action_float_values.is_empty()
                    || !effect.projectiles.is_empty()
                    || effect.activation.is_some()
                {
                    return Err(
                        "A custom effect program cannot carry stock action overrides.".into(),
                    );
                }
            }
            if !effects.insert(effect.source_perk_index) {
                return Err("The perk contains the same effect more than once".into());
            }
        }
        let mut stats = BTreeSet::new();
        if self.stats.len() > 16 || self.stats.iter().any(|stat| stat.definition_index > 255) {
            return Err("Choose up to 16 supported stat bonuses".into());
        }
        if self
            .stats
            .iter()
            .any(|stat| !stats.insert(stat.definition_index))
        {
            return Err("The perk contains the same stat bonus more than once".into());
        }
        Ok(())
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
            description: Some(self.description.clone()),
            classification_donor_hash: self.classification.clone(),
            investment_stats: self.stats.clone(),
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: self.effects.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
