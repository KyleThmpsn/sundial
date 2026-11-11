//! How a perk attaches to an item and comes back, and what makes two perks the same.
//!
//! Attaching gives a perk one form: no catalog sources, stats in index order and a blank
//! description empty. A saved item gives its perks back in that form, and reading a saved
//! variant resolves the effects and stats it inherits from its stock plug. Perks compare in
//! three ways. Content identity is everything but the document id, which the library keeps as an
//! import's id and its import history records. Attached content is the content of the attached
//! form, which the picker compares by equality. Layout-free identity leaves out the layout a
//! source weapon lent. Stored ids and history keys are these digests, so the bytes each hashes
//! stay as they are.
use sha2::{Digest, Sha256};
use sundial::investment::InvestmentCatalog;

use super::PerkRecipe;
use crate::{WeaponSocketPlugVariantRecipe, WeaponStatOverride};

impl PerkRecipe {
    /// This perk in the form attaching gives it.
    #[must_use]
    pub(crate) fn attached(&self) -> Self {
        let mut perk = self.clone();
        perk.sources.clear();
        // Saved recipes keep stats in index order, so an applied perk equals its saved form.
        perk.stats
            .sort_unstable_by_key(|stat| stat.definition_index);
        // A custom perk owns its description, including an intentionally blank one.
        if perk.description.trim().is_empty() {
            perk.description.clear();
        }
        perk
    }

    /// Attachment takes a copy. Subsequent library edits do not mutate weapons.
    #[must_use]
    pub fn at_socket(&self, socket_index: u16, choice_index: u16) -> WeaponSocketPlugVariantRecipe {
        let perk = self.attached();
        WeaponSocketPlugVariantRecipe {
            replace_effects: true,
            socket_index,
            choice_index,
            source_plug_hash: perk.template_plug,
            name: Some(perk.name),
            // None would inherit unrelated gameplay text from the icon donor.
            description: Some(perk.description),
            icon: perk.icon,
            classification_donor_hash: perk.classification,
            offer_everywhere: perk.offer_everywhere,
            projectile: perk.projectile,
            investment_stats: perk.stats,
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: perk.effects,
        }
    }

    /// The perk `variant` attaches, with the effects and stats of its stock plug when it keeps
    /// them, and the stock plug's name and description where it has none of its own.
    #[must_use]
    pub(crate) fn from_variant(
        variant: &WeaponSocketPlugVariantRecipe,
        catalog: &InvestmentCatalog,
    ) -> Self {
        let mut recipe = Self::new();
        let hash = variant.source_plug_hash.parse_u32().unwrap_or_default();
        recipe.template_plug = variant.source_plug_hash.clone();
        recipe.icon = variant.icon.clone();
        recipe.name = variant
            .name
            .clone()
            .unwrap_or_else(|| catalog.plug_label(hash, false));
        recipe.description = variant.description.clone().unwrap_or_else(|| {
            catalog
                .perk_description(hash)
                .unwrap_or_default()
                .to_owned()
        });
        recipe.classification = variant.classification_donor_hash.clone();
        recipe.offer_everywhere = variant.offer_everywhere;
        recipe.projectile = variant.projectile;
        if !variant.replace_effects {
            recipe.effects = catalog
                .item_sandbox_perk_indices(hash)
                .into_iter()
                .map(Self::effect)
                .collect();
            recipe.stats = catalog
                .item_stat_contributions(hash)
                .into_iter()
                .map(|stat| WeaponStatOverride {
                    definition_index: stat.definition_index,
                    value: stat.value,
                })
                .collect();
        }
        for &index in &variant.additional_sandbox_perks {
            if !recipe
                .effects
                .iter()
                .any(|effect| effect.source_perk_index == index)
            {
                recipe.effects.push(Self::effect(index));
            }
        }
        for effect in &variant.sandbox_perks {
            if let Some(existing) = recipe
                .effects
                .iter_mut()
                .find(|existing| existing.source_perk_index == effect.source_perk_index)
            {
                *existing = effect.clone();
            } else {
                recipe.effects.push(effect.clone());
            }
        }
        for stat in &variant.investment_stats {
            if let Some(existing) = recipe
                .stats
                .iter_mut()
                .find(|existing| existing.definition_index == stat.definition_index)
            {
                *existing = stat.clone();
            } else {
                recipe.stats.push(stat.clone());
            }
        }
        recipe
    }

    /// The identity of everything this perk holds but its document id.
    pub(crate) fn content_identity(&self) -> Result<String, String> {
        let mut content = self.clone();
        content.id.clear();
        let bytes = serde_json::to_vec(&content).map_err(|error| error.to_string())?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    /// The identity of what a perk does and shows, without the layout its source lent it. The
    /// template plug lends an item layout. An effect that is a complete program, with nothing
    /// patched into the stock effect beneath it, borrows only that effect's row. Importers take
    /// both from the base of the weapon a perk came from, so one source perk on weapons with
    /// different bases would otherwise be a different perk for each base.
    pub(crate) fn layout_free_identity(&self) -> Result<String, String> {
        let mut content = self.clone();
        content.id.clear();
        content.template_plug = 0.into();
        for effect in &mut content.effects {
            if effect.program.is_some()
                && effect.projectiles.is_empty()
                && effect.activation.is_none()
                && effect.runtime_values.is_empty()
                && effect.action_float_values.is_empty()
            {
                effect.source_perk_index = 0;
            }
        }
        let bytes = serde_json::to_vec(&content).map_err(|error| error.to_string())?;
        // Kept apart from content identities, which share the import history.
        Ok(format!(
            "layout-free-{}",
            hex::encode(Sha256::digest(bytes))
        ))
    }
}
