//! Stock ornaments, used as appearances in their own right.
//!
//! An ornament is a plug rather than an authoring donor: it carries the translation-art rows that
//! select the equipped model, its own locked dye rows, and an inventory icon. Applying one
//! therefore writes the appearance sources the recipe already has, the art-row and dye-row
//! overrides and the icon donor, instead of adding a build path. Every later control keeps
//! describing what the authored weapon will carry, and changing the appearance donor already
//! restores all three.
//!
//! What an ornament does not carry is a rig. Its translation block disables the gear-art pattern
//! selector, so it names no animation group and no runtime entity, and nothing in it says how the
//! model is held, aimed or reloaded. The weapon whose sockets offer it says all of that. So an
//! ornament from another weapon is offered here paired with that weapon, which becomes the
//! appearance donor, and the ornament's rows then replace its model. The build writes the donor's
//! rows first and the recipe's overrides second, so the pair lands in that order by itself.
use super::*;
use sundial::investment::{WeaponChoiceFilter, WeaponOrnament, WeaponOrnamentAppearance};

/// The ornaments one weapon may wear, rebuilt when the weapon or its appearance changes.
#[derive(Default)]
pub(in crate::app) struct Ornaments {
    key: Option<(u32, u32, WeaponInventorySlot)>,
    appearances: Vec<WeaponOrnamentAppearance>,
}

impl Ornaments {
    /// Every ornament this gameplay donor can wear, each with the weapon that lends it a rig.
    ///
    /// Two sources. Ornaments that change a model come from the whole installation, kept when
    /// their own weapon would be an allowed appearance donor, which is what lets an ornament
    /// from another weapon be worn at all. Ornaments of the current appearance source come from
    /// that weapon directly, including the ones carrying no model rows: those can still lend an
    /// icon, and only this weapon's are worth offering for that.
    ///
    /// The current appearance's own ornaments sort first so the ordinary choice stays at the top.
    fn list(
        &mut self,
        catalog: &InvestmentCatalog,
        all: &[WeaponOrnamentAppearance],
        donors: &[WeaponDonorSummary],
        gameplay_hash: u32,
        appearance: u32,
        target: WeaponInventorySlot,
    ) -> &[WeaponOrnamentAppearance] {
        let key = (gameplay_hash, appearance, target);
        if self.key != Some(key) {
            let hosts: BTreeMap<u32, &WeaponDonorSummary> =
                donors.iter().map(|donor| (donor.hash, donor)).collect();
            let gameplay = hosts.get(&gameplay_hash).copied();
            let mut appearances: Vec<WeaponOrnamentAppearance> = all
                .iter()
                .filter(|candidate| {
                    hosts
                        .get(&candidate.host_item_hash)
                        .zip(gameplay)
                        .is_some_and(|(host, gameplay)| {
                            host.hash == gameplay.hash
                                || presentation_donor_candidate_is_compatible(
                                    host, gameplay, target,
                                )
                        })
                })
                .cloned()
                .collect();
            let host_name = hosts
                .get(&appearance)
                .map_or_else(String::new, |host| host.name.clone());
            for ornament in catalog.weapon_ornaments(appearance) {
                if appearances.iter().any(|candidate| {
                    candidate.ornament.hash == ornament.hash
                        && candidate.host_item_hash == appearance
                }) {
                    continue;
                }
                appearances.push(WeaponOrnamentAppearance {
                    ornament,
                    host_item_hash: appearance,
                    host_name: host_name.clone(),
                });
            }
            appearances.sort_by_cached_key(|candidate| {
                (
                    candidate.host_item_hash != appearance,
                    candidate.host_item_hash != gameplay_hash,
                    candidate.ornament.name.to_lowercase(),
                    candidate.ornament.hash,
                )
            });
            let mut seen = BTreeSet::new();
            appearances.retain(|candidate| seen.insert(candidate.ornament.hash));
            self.appearances = appearances;
            self.key = Some(key);
        }
        &self.appearances
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

/// Returns the offered ornament the recipe currently wears.
///
/// Several ornaments can select the same model, so the one offered by the weapon already lending
/// the appearance wins. Without that the label could name a different ornament with identical
/// rows, and picking it would move the appearance for no reason.
pub(in crate::app) fn applied_appearance<'a>(
    recipe: &WeaponRecipe,
    appearances: &'a [WeaponOrnamentAppearance],
    gameplay_hash: u32,
) -> Option<&'a WeaponOrnamentAppearance> {
    let lending = recipe
        .presentation_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok())
        .unwrap_or(gameplay_hash);
    let worn = |candidate: &WeaponOrnamentAppearance| {
        applied(recipe, std::slice::from_ref(&candidate.ornament)).is_some()
    };
    appearances
        .iter()
        .find(|candidate| worn(candidate) && candidate.host_item_hash == lending)
        .or_else(|| appearances.iter().find(|candidate| worn(candidate)))
}

/// Takes the ornament's model, colours and icon, and the rig of the weapon that offers it.
pub(in crate::app) fn apply_appearance(
    recipe: &mut WeaponRecipe,
    appearance: &WeaponOrnamentAppearance,
    gameplay_hash: u32,
) {
    // Naming the ornament's own weapon as the appearance donor is what carries the skeleton,
    // animations and first-person attachment across; without it the model would be held in the
    // gameplay donor's hands. An ornament of the gameplay donor needs none of that, and naming
    // it would only be rejected as an appearance equal to the base. An ornament with no model
    // rows lends its icon alone, so it leaves the appearance where it is.
    if !appearance.ornament.art_arrangements.is_empty() {
        if appearance.host_item_hash == gameplay_hash {
            recipe.set_presentation_donor(None);
        } else {
            recipe.set_presentation_donor(Some(WeaponDonorReference {
                item_hash: appearance.host_item_hash.into(),
                expected_name: Some(appearance.host_name.clone()),
            }));
        }
    }
    // Setting the appearance donor clears the override fields, so the rows go on afterwards.
    apply(recipe, &appearance.ornament);
}

/// Restores the appearance the recipe had before this ornament, donor included.
pub(in crate::app) fn restore_appearance(
    recipe: &mut WeaponRecipe,
    appearance: &WeaponOrnamentAppearance,
    gameplay_hash: u32,
) {
    restore(recipe, &appearance.ornament);
    // The donor was only named to lend this ornament a rig. Dropping the ornament without it
    // would leave the weapon wearing another weapon's plain model, which nobody chose.
    let lent = recipe
        .presentation_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok())
        == Some(appearance.host_item_hash);
    if lent
        && !appearance.ornament.art_arrangements.is_empty()
        && appearance.host_item_hash != gameplay_hash
    {
        recipe.set_presentation_donor(None);
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
    /// The gameplay donor, the appearance it currently wears, and the slot being authored.
    fn ornament_context(&self) -> Option<(u32, u32, WeaponInventorySlot)> {
        // Read the appearance back from the recipe so a selection made elsewhere is in force.
        let appearance = self.appearance_donor_hash()?;
        let gameplay = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .filter(|hash| *hash != 0)
            .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))?;
        let target = authored_inventory_slot(&self.recipe.overrides, gameplay)?;
        Some((gameplay.hash, appearance, target))
    }

    /// Draws the ornament section, offering every ornament this weapon could wear.
    /// Whether any ornament is offered for the current appearance.
    pub(in crate::app) fn appearance_ornaments_offered(&mut self) -> bool {
        let Some(catalog) = self.catalog.as_ref() else {
            return false;
        };
        let Some((gameplay_hash, appearance, target)) = self.ornament_context() else {
            return false;
        };
        !self
            .appearance_ornaments
            .list(
                catalog,
                &self.ornament_appearances,
                &self.donor_summaries,
                gameplay_hash,
                appearance,
                target,
            )
            .is_empty()
    }

    /// The ornament chooser on the Appearance tab's Model row. Returns whether one is offered.
    pub(in crate::app) fn draw_appearance_ornaments(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(catalog) = self.catalog.as_ref() else {
            return false;
        };
        let Some((gameplay_hash, appearance, target)) = self.ornament_context() else {
            return false;
        };
        let appearances = self.appearance_ornaments.list(
            catalog,
            &self.ornament_appearances,
            &self.donor_summaries,
            gameplay_hash,
            appearance,
            target,
        );
        if appearances.is_empty() {
            return false;
        }
        let base_type = base_weapon_type(&self.donor_summaries, gameplay_hash);
        draw_chooser(
            ui,
            "appearance-tab",
            catalog,
            appearances,
            (gameplay_hash, base_type),
            &mut self.recipe,
            &self.packages,
        );
        true
    }

    /// The same chooser, offered beside the appearance picker so an ornament can be taken
    /// without leaving the main page. Absent when nothing is offered.
    pub(in crate::app) fn draw_appearance_ornament_button(&mut self, ui: &mut egui::Ui) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let Some((gameplay_hash, appearance, target)) = self.ornament_context() else {
            return;
        };
        let appearances = self.appearance_ornaments.list(
            catalog,
            &self.ornament_appearances,
            &self.donor_summaries,
            gameplay_hash,
            appearance,
            target,
        );
        if appearances.is_empty() {
            return;
        }
        let base_type = base_weapon_type(&self.donor_summaries, gameplay_hash);
        draw_chooser(
            ui,
            "weapon-tab",
            catalog,
            appearances,
            (gameplay_hash, base_type),
            &mut self.recipe,
            &self.packages,
        );
    }
}

/// The base weapon's type, which the chooser's filter opens on.
fn base_weapon_type(donors: &[WeaponDonorSummary], gameplay_hash: u32) -> Option<&str> {
    donors
        .iter()
        .find(|donor| donor.hash == gameplay_hash)
        .map(|donor| donor.type_name.trim())
        .filter(|name| !name.is_empty())
}

/// The chooser on its own, so the main page and the Appearance tab offer the same thing.
/// `place` keeps each page's browser state separate. `base` is the base weapon and its type.
fn draw_chooser(
    ui: &mut egui::Ui,
    place: &str,
    catalog: &InvestmentCatalog,
    appearances: &[WeaponOrnamentAppearance],
    (gameplay_hash, base_type): (u32, Option<&str>),
    recipe: &mut WeaponRecipe,
    packages: &Path,
) {
    let current = applied_appearance(recipe, appearances, gameplay_hash);
    let label = current.map_or_else(
        || "Use Ornament".to_owned(),
        |current| current.ornament.name.clone(),
    );
    /// The weapon that lends the rig, shown only when it is not the weapon being authored.
    fn host(candidate: &WeaponOrnamentAppearance, gameplay_hash: u32) -> Option<&str> {
        (candidate.host_item_hash != gameplay_hash && !candidate.host_name.is_empty())
            .then_some(candidate.host_name.as_str())
    }
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        let opened = ui
            .button(&label)
            .on_hover_text("Ornaments from any compatible weapon.")
            .clicked();
        let id = egui::Id::new(("ornament-preview-browser", place));
        if let Some(hash) = sundial::ui::model_preview::chooser::show(
            ui,
            id,
            "Choose Ornament",
            opened,
            |ui, opened| {
                let query_id = id.with("query");
                let mut query = ui
                    .data(|data| data.get_temp::<String>(query_id))
                    .unwrap_or_default();
                let search = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Search Ornaments")
                        .desired_width(ui.available_width()),
                );
                if opened {
                    search.request_focus();
                }
                // The donor pickers' filters, read off the weapon that offers each ornament.
                // Without ornaments of its own, the base weapon's type leads, as the appearance
                // picker opens.
                let filter_id = id.with("filter");
                let mut filter = if opened {
                    let own = appearances
                        .iter()
                        .any(|candidate| candidate.host_item_hash == gameplay_hash);
                    base_type
                        .filter(|_| !own)
                        .map(WeaponChoiceFilter::of_weapon_type)
                        .unwrap_or_default()
                } else {
                    ui.data(|data| data.get_temp::<WeaponChoiceFilter>(filter_id))
                        .unwrap_or_default()
                };
                let hosts: Vec<u32> = appearances
                    .iter()
                    .map(|candidate| candidate.host_item_hash)
                    .collect();
                let filtered = catalog.draw_weapon_choice_filters(
                    ui,
                    filter_id.with("bar"),
                    &hosts,
                    &mut filter,
                );
                let visible: Vec<_> = appearances
                    .iter()
                    .filter(|candidate| {
                        catalog.weapon_choice_passes(candidate.host_item_hash, &filter)
                            && (crate::app::pickers::matches(&query, &candidate.ornament.name)
                                || crate::app::pickers::matches(&query, &candidate.host_name))
                    })
                    .collect();
                ui.data_mut(|data| data.insert_temp(filter_id, filter));
                let keys: Vec<_> = std::iter::once(0)
                    .chain(
                        visible
                            .iter()
                            .map(|candidate| u64::from(candidate.ornament.hash)),
                    )
                    .collect();
                ui.data_mut(|data| data.insert_temp(query_id, query));
                if opened {
                    ui.data_mut(|data| {
                        data.insert_temp(
                            ui.make_persistent_id("inspected-choice"),
                            current.map_or(0, |current| u64::from(current.ornament.hash)),
                        )
                    });
                }
                sundial::ui::catalog::BrowserList {
                    keys: &keys,
                    height: (ui.available_height() - 30.0).max(180.0),
                    reset: search.changed() || filtered,
                    row_height: 48.0,
                    select: None,
                }
                .draw_with_actions_activating(
                    ui,
                    |ui, index, selected| {
                        let chosen = index.checked_sub(1).map(|index| visible[index]);
                        catalog.draw_authoring_choice_row(
                            ui,
                            chosen.map(|candidate| candidate.ornament.hash),
                            chosen.map_or("Default Appearance", |candidate| {
                                candidate.ornament.name.as_str()
                            }),
                            chosen.and_then(|candidate| host(candidate, gameplay_hash)),
                            selected,
                        )
                    },
                    |ui, index, activated| {
                        let picked = index.checked_sub(1).map(|index| visible[index]);
                        let mut candidate = recipe.clone();
                        if let Some(current) = current {
                            restore_appearance(&mut candidate, current, gameplay_hash);
                        }
                        if let Some(picked) = picked {
                            apply_appearance(&mut candidate, picked, gameplay_hash);
                        }
                        if let Some(loadout) = super::preview::loadout(catalog, &candidate) {
                            sundial::ui::model_preview::chooser::preview(
                                ui,
                                packages,
                                catalog.preview_appearance(&loadout),
                                picked.map_or("Default Appearance", |picked| {
                                    picked.ornament.name.as_str()
                                }),
                            );
                        } else {
                            ui.label("No model for this appearance.");
                        }
                        // The use action closes the row at the right. A double-click uses the
                        // ornament, as the button does.
                        let used = ui
                            .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add(
                                    crate::app::style::primary(ui, "Use Ornament")
                                        .min_size(egui::vec2(0.0, 24.0)),
                                )
                                .clicked()
                            })
                            .inner;
                        (used || activated).then_some(keys[index])
                    },
                )
            },
        ) {
            action = Some(
                appearances
                    .iter()
                    .find(|candidate| u64::from(candidate.ornament.hash) == hash),
            );
        }
        if current.is_some() && ui.button("Default Appearance").clicked() {
            action = Some(None);
        }
    });
    match (action, current) {
        (Some(Some(picked)), current) => {
            if let Some(current) = current {
                restore_appearance(recipe, current, gameplay_hash);
            }
            apply_appearance(recipe, picked, gameplay_hash);
        }
        (Some(None), Some(current)) => restore_appearance(recipe, current, gameplay_hash),
        (Some(None), None) | (None, _) => {}
    }
}

#[cfg(test)]
mod tests;
