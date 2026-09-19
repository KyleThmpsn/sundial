//! Stock ornaments offered by the selected appearance.
//!
//! An ornament is a plug rather than an authoring donor: it carries the translation-art rows that
//! select the equipped model, its own locked dye rows, and an inventory icon. Applying one
//! therefore writes the appearance sources the recipe already has, the art-row and dye-row
//! overrides and the icon donor, instead of adding a build path. Every later control keeps
//! describing what the authored weapon will carry, and changing the appearance donor already
//! restores all three.
use super::*;
use sundial::investment::WeaponOrnament;

/// Ornaments for one appearance source, reloaded when that source changes.
#[derive(Default)]
pub(in crate::app) struct Ornaments {
    item_hash: Option<u32>,
    ornaments: Vec<WeaponOrnament>,
}

impl Ornaments {
    pub(in crate::app) fn list(
        &mut self,
        catalog: &InvestmentCatalog,
        item_hash: u32,
    ) -> &[WeaponOrnament] {
        if self.item_hash != Some(item_hash) {
            self.ornaments = catalog.weapon_ornaments(item_hash);
            self.item_hash = Some(item_hash);
        }
        &self.ornaments
    }
}

/// Returns the ornament the recipe currently carries.
///
/// The model rows identify it. An ornament that carries no rows of its own is recognized by its
/// icon instead, which is the only appearance it can lend.
pub(in crate::app) fn applied<'a>(
    recipe: &WeaponRecipe,
    ornaments: &'a [WeaponOrnament],
) -> Option<&'a WeaponOrnament> {
    let rows = recipe.overrides.art_arrangements.as_deref();
    let icon = recipe
        .icon_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok());
    ornaments.iter().find(|ornament| match rows {
        Some(rows) => !ornament.art_arrangements.is_empty() && rows == art_rows(ornament),
        None => ornament.art_arrangements.is_empty() && icon == Some(ornament.hash),
    })
}

/// Takes the ornament's model, materials and inventory icon.
pub(in crate::app) fn apply(recipe: &mut WeaponRecipe, ornament: &WeaponOrnament) {
    // The ornament's icon paints its own decorative plate behind the weapon. The authored icon
    // draws the rarity plate and Parhelion's watermark itself, so clear the stock one.
    recipe.overrides.icon_edit.cleared_color = ornament.icon_plate_color();
    if !ornament.art_arrangements.is_empty() {
        recipe.overrides.art_arrangements = Some(art_rows(ornament));
    }
    // A stock ornament keeps its colors in its own locked dye channels. Without them the new
    // geometry would be shaded by the donor's materials instead of the ornament's own.
    if !ornament.render_dye_rows.iter().all(Vec::is_empty) {
        recipe.overrides.render_dye_rows = Some(dye_rows(ornament));
    }
    recipe.icon_donor = Some(WeaponDonorReference {
        item_hash: ornament.hash.into(),
        expected_name: Some(ornament.name.clone()),
    });
}

/// Restores the appearance source's own model, materials and icon.
///
/// Only the values this ornament contributed are cleared, so colors or an icon chosen on the
/// Appearance tab after the ornament survive.
pub(in crate::app) fn restore(recipe: &mut WeaponRecipe, ornament: &WeaponOrnament) {
    if recipe
        .overrides
        .art_arrangements
        .as_deref()
        .is_some_and(|rows| rows == art_rows(ornament))
    {
        recipe.overrides.art_arrangements = None;
    }
    if recipe
        .overrides
        .render_dye_rows
        .as_ref()
        .is_some_and(|rows| *rows == dye_rows(ornament))
    {
        recipe.overrides.render_dye_rows = None;
    }
    if recipe
        .icon_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok())
        == Some(ornament.hash)
    {
        recipe.icon_donor = None;
    }
    if recipe.overrides.icon_edit.cleared_color == ornament.icon_plate_color() {
        recipe.overrides.icon_edit.cleared_color = None;
    }
}

fn art_rows(ornament: &WeaponOrnament) -> Vec<WeaponArtArrangementRecipe> {
    ornament
        .art_arrangements
        .iter()
        .map(|row| WeaponArtArrangementRecipe {
            character_class: row.character_class,
            arrangement: row.arrangement,
        })
        .collect()
}

fn dye_rows(ornament: &WeaponOrnament) -> [Vec<WeaponDyeReferenceRecipe>; 3] {
    std::array::from_fn(|stage| {
        ornament.render_dye_rows[stage]
            .iter()
            .map(|row| WeaponDyeReferenceRecipe {
                channel_index: row.channel_index,
                dye_reference_index: row.dye_reference_index,
            })
            .collect()
    })
}

impl PackageAuthoringApp {
    /// Draws the ornament section for whichever weapon currently lends the appearance.
    pub(in crate::app) fn draw_appearance_ornaments(&mut self, ui: &mut egui::Ui) {
        // Read the appearance back from the recipe so a selection made elsewhere is in force.
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let Some(item_hash) = self.appearance_donor_hash() else {
            return;
        };
        let ornaments = self.appearance_ornaments.list(catalog, item_hash);
        draw(ui, catalog, ornaments, &mut self.recipe);
    }

    /// The same chooser, offered beside the appearance picker so an ornament can be taken
    /// without leaving the main page. Absent when this appearance wears none.
    pub(in crate::app) fn draw_appearance_ornament_button(&mut self, ui: &mut egui::Ui) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let Some(item_hash) = self.appearance_donor_hash() else {
            return;
        };
        let ornaments = self.appearance_ornaments.list(catalog, item_hash);
        if ornaments.is_empty() {
            return;
        }
        draw_chooser(ui, catalog, ornaments, &mut self.recipe);
    }
}

fn draw(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    ornaments: &[WeaponOrnament],
    recipe: &mut WeaponRecipe,
) {
    if ornaments.is_empty() {
        return;
    }
    draw_donor_section_label(
        ui,
        "Ornament",
        Some(
            "Stock ornaments this appearance can wear. Choosing one takes the ornament's model, its own colors and its inventory icon. Everything it sets stays editable on the Appearance tab.",
        ),
    );
    draw_chooser(ui, catalog, ornaments, recipe);
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
}

/// The chooser on its own, so the main page and the Appearance tab offer the same thing.
fn draw_chooser(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    ornaments: &[WeaponOrnament],
    recipe: &mut WeaponRecipe,
) {
    let current = applied(recipe, ornaments);
    let label = current.map_or_else(
        || "Use Ornament".to_owned(),
        |ornament| ornament.name.clone(),
    );
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        catalog
            .draw_item_menu_button(
                ui,
                current.map(|ornament| ornament.hash),
                current.and_then(WeaponOrnament::icon_plate_color),
                &label,
                |ui| {
                ui.set_min_width(340.0);
                for ornament in ornaments {
                    let row = catalog.draw_authoring_choice_row(
                        ui,
                        Some(ornament.hash),
                        &ornament.name,
                        catalog.perk_description(ornament.hash),
                        current.is_some_and(|current| current.hash == ornament.hash),
                    );
                    if row.clicked() {
                        action = Some(Some(ornament));
                        ui.close_menu();
                    }
                }
                },
            )
            .response
            .on_hover_text("Ornaments belong to the selected appearance. Changing the appearance restores its own model, colors and icon.");
        if current.is_some() && ui.button("Default Appearance").clicked() {
            action = Some(None);
        }
    });
    match (action, current) {
        (Some(Some(ornament)), _) => apply(recipe, ornament),
        (Some(None), Some(current)) => restore(recipe, current),
        (Some(None), None) | (None, _) => {}
    }
}

#[cfg(test)]
mod tests;
