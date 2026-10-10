//! The main page for mods. A mod is one custom perk, built on its template plug and offered in
//! every stock socket whose shared plug set offers its type, so the page shows the perk and where
//! it appears, and the workbench edits it.
use super::*;
use crate::app::style;

impl PackageAuthoringApp {
    pub(super) fn draw_mod_editor(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let perk = self.recipe.overrides.socket_plug_variants.first();
        let effects = perk.map_or(0, |perk| perk.sandbox_perks.len());
        let stats = perk.map_or(0, |perk| perk.investment_stats.len());
        // The plug the mod's sockets offer: its type's, else its template's.
        let offered = perk.and_then(|perk| {
            perk.classification_donor_hash
                .as_ref()
                .unwrap_or(&perk.source_plug_hash)
                .parse_u32()
                .ok()
        });
        let (type_name, sets) = match (&self.catalog, offered) {
            (Some(catalog), Some(offered)) => (
                catalog.item_type_name(offered),
                catalog
                    .reusable_set_counts()
                    .get(&offered)
                    .copied()
                    .unwrap_or_default(),
            ),
            _ => (None, 0),
        };
        let mut edit = false;
        style::card(ui, |ui| {
            ui.heading(&self.recipe.name);
            if let Some(type_name) = &type_name {
                ui.label(egui::RichText::new(type_name).weak());
            }
            if !self.recipe.flavor.trim().is_empty() {
                ui.add_space(4.0);
                ui.add(egui::Label::new(&self.recipe.flavor).wrap());
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let count = |count: usize, one: &str, many: &str| {
                    format!("{count} {}", if count == 1 { one } else { many })
                };
                ui.label(count(effects, "Effect", "Effects"));
                ui.separator();
                ui.label(count(stats, "Stat", "Stats"));
                ui.separator();
                ui.label(count(sets, "Plug Set", "Plug Sets"))
                    .on_hover_text("Offered in every socket these sets serve");
            });
            if perk.is_some() && sets == 0 && self.catalog.is_some() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "No socket offers this type. Choose another type.",
                );
            }
            ui.add_space(6.0);
            let button = style::primary(ui, "Edit Mod…");
            edit = ui.add(button).clicked();
        });
        if edit {
            self.perk_request = Some(custom_perks::workbench::Request::Mod);
        }
    }

    /// Gives a new mod recipe its perk: an empty one of the weapon mod type every weapon's mod
    /// socket offers, named as the recipe is.
    pub(super) fn bind_default_mod(&mut self) -> bool {
        let Some(catalog) = self.catalog.as_ref() else {
            return false;
        };
        let mut perk = crate::perk::PerkRecipe::new();
        perk.name.clone_from(&self.recipe.name);
        perk.classification = widest_type_plug(catalog, "Weapon Mod").map(Into::into);
        perk.offer_everywhere = true;
        apply_perk(&mut self.recipe, &perk, catalog).is_ok()
    }
}

/// The stock plug of `type_name` that the most shared plug sets offer, one with effects first,
/// as the workbench's Type picker chooses. For Weapon Mod that is Boss Spec, which every weapon's
/// mod socket offers.
fn widest_type_plug(catalog: &InvestmentCatalog, type_name: &str) -> Option<u32> {
    catalog
        .reusable_set_counts()
        .into_iter()
        .filter(|&(plug, _)| catalog.item_type_name(plug).as_deref() == Some(type_name))
        .max_by_key(|&(plug, sets)| {
            (
                sets,
                !catalog.item_sandbox_perk_indices(plug).is_empty(),
                std::cmp::Reverse(plug),
            )
        })
        .map(|(plug, _)| plug)
}

/// Makes `perk` the mod: its one plug, built on the perk's template plug and offered in every
/// socket of its type, with the perk's name and description as the item's.
pub(in crate::app) fn apply_perk(
    recipe: &mut WeaponRecipe,
    perk: &crate::perk::PerkRecipe,
    catalog: &InvestmentCatalog,
) -> Result<(), String> {
    perk.validate()?;
    if let Some(issue) = crate::perk::preflight::check(perk)
        .into_iter()
        .find(|issue| issue.blocking)
    {
        return Err(issue.message);
    }
    let template = perk
        .template_plug
        .parse_u32()
        .map_err(|error| error.to_string())?;
    let offered = match &perk.classification {
        Some(hash) => hash.parse_u32().map_err(|error| error.to_string())?,
        None => template,
    };
    if catalog
        .reusable_set_counts()
        .get(&offered)
        .is_none_or(|&sets| sets == 0)
    {
        return Err(
            "No socket offers this perk's type. Choose a mod type, such as Weapon Mod.".into(),
        );
    }
    // The mod is named for its perk, and a new name gives it the identity a renamed item takes.
    if recipe.name != perk.name {
        recipe
            .rename_authored_item(perk.name.clone())
            .map_err(|error| error.to_string())?;
    }
    let mut variant = perk.at_socket(0, 0);
    variant.offer_everywhere = true;
    // The template is the perk's own choice, so the base carries no expected name to check.
    recipe.set_donor(template, String::new());
    recipe.overrides.socket_plug_variants = vec![variant];
    recipe.flavor.clone_from(&perk.description);
    Ok(())
}
