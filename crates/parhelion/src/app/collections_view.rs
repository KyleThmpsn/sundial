use super::*;
use crate::collection::{
    Ammo, BASE_NODE_COUNT, Destination, Family, GearPage, NODE_CAPACITY, NodeBudget,
    custom_node_hashes, gear_page_node_hashes,
};
use std::collections::BTreeSet;

impl PackageAuthoringApp {
    pub(super) fn draw_collection_capacity(&self, ui: &mut egui::Ui) {
        let entries = self.recipe_entries.iter().filter(|entry| {
            self.enabled_recipe_paths.contains(&entry.path)
                && self.recipe_path.as_ref() != Some(&entry.path)
        });
        let mut gear_entries = entries
            .clone()
            .map(|entry| {
                (
                    entry.kind,
                    entry.donor_hash,
                    entry.rarity,
                    entry.armor_class,
                )
            })
            .collect::<Vec<_>>();
        let mut members = entries
            .map(|entry| {
                let summary = |hash: u32| self.donor_summaries.iter().find(|d| d.hash == hash);
                (
                    entry.badge.as_ref().map(|badge| badge.name.as_str()),
                    entry
                        .collection_destination
                        .or_else(|| {
                            automatic_destination(
                                summary(entry.donor_hash),
                                summary(entry.type_donor_hash),
                                entry.ammo_type,
                            )
                        })
                        .filter(|_| !self.collection_is_exotic(entry.rarity, entry.donor_hash)),
                )
            })
            .collect::<Vec<_>>();
        let exotic = self.collection_is_exotic(
            self.recipe.overrides.rarity,
            self.recipe.donor.item_hash.parse_u32().unwrap_or_default(),
        );
        let current = (
            self.recipe
                .overrides
                .badge
                .as_ref()
                .map(|badge| badge.name.as_str()),
            self.recipe
                .overrides
                .collection_destination
                .or_else(|| self.current_automatic_destination())
                .filter(|_| !exotic),
        );
        let included = self
            .recipe_path
            .as_ref()
            .is_some_and(|path| self.enabled_recipe_paths.contains(path));
        let current_gear = (
            self.recipe.kind,
            self.recipe.donor.item_hash.parse_u32().unwrap_or_default(),
            self.recipe.overrides.rarity,
            self.recipe.overrides.armor_class,
        );
        if included {
            members.push(current);
            gear_entries.push(current_gear);
        }
        let gear_pages = self.gear_collection_pages(&gear_entries);
        let budget = NodeBudget::new(members.iter().copied()).with_gear_pages(gear_pages.len());
        let mut selected_nodes = custom_node_hashes(members.iter().copied());
        selected_nodes.extend(gear_page_node_hashes(gear_pages.iter().copied()));
        let installed_nodes = self.catalog.as_ref().map_or_else(BTreeSet::new, |catalog| {
            installed_custom_nodes(catalog.presentation_node_hashes())
        });
        let accounted_nodes = combined_custom_nodes(&installed_nodes, &selected_nodes);
        let custom_used = accounted_nodes.len();
        let used = BASE_NODE_COUNT + custom_used;
        let custom_capacity = NODE_CAPACITY - BASE_NODE_COUNT;
        let selected_used = budget.used();
        let installed_used = BASE_NODE_COUNT + installed_nodes.len();
        ui.add(
            sundial::investment::progress_bar(custom_used as f32 / custom_capacity as f32)
                .desired_width(220.0)
                .text(format!(
                    "{custom_used} / {custom_capacity} Custom Nodes Used"
                )),
        );
        let label = if selected_used > NODE_CAPACITY {
            egui::RichText::new(format!(
                "{} Selected Build Nodes Over Limit",
                selected_used - NODE_CAPACITY
            ))
            .color(ui.visuals().error_fg_color)
        } else if installed_used > NODE_CAPACITY {
            egui::RichText::new(format!(
                "{} Installed Nodes Over Limit",
                installed_used - NODE_CAPACITY
            ))
            .color(ui.visuals().error_fg_color)
        } else if used > NODE_CAPACITY {
            egui::RichText::new("Selected Build Fits After Replacement")
                .color(ui.visuals().weak_text_color())
        } else {
            egui::RichText::new(format!("{} Nodes Available", NODE_CAPACITY - used))
                .color(ui.visuals().weak_text_color())
        };
        let selected_new = selected_nodes.difference(&installed_nodes).count();
        let stock = crate::progression::STOCK_PRESENTATION_NODE_COUNT;
        ui.label(label).on_hover_text(format!(
            "{used} / {NODE_CAPACITY} nodes\nStock: {stock}\n{}: {}\nInstalled custom: {}\nNew in selected build: {selected_new}\nSelected build custom: {} ({} badge, {} page)",
            self.presentation_editor.branding().name(),
            BASE_NODE_COUNT - stock,
            installed_nodes.len(),
            selected_nodes.len(),
            budget.badges * 4,
            budget.pages + budget.gear_pages
        ));
        if !included {
            let mut with_draft = custom_node_hashes(members.into_iter().chain([current]));
            gear_entries.push(current_gear);
            with_draft.extend(gear_page_node_hashes(
                self.gear_collection_pages(&gear_entries),
            ));
            let with_draft = combined_custom_nodes(&installed_nodes, &with_draft).len();
            if with_draft > custom_used {
                ui.weak(format!(
                    "This recipe adds {} nodes.",
                    with_draft - custom_used
                ));
            }
        }
        if selected_used > NODE_CAPACITY {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Remove a custom badge or an added page before building.",
            );
        } else if installed_used > NODE_CAPACITY {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Installed Collections nodes exceed the limit.",
            );
        }
    }

    fn gear_collection_pages(
        &self,
        entries: &[(
            crate::ItemKind,
            u32,
            Option<crate::RecipeRarity>,
            Option<crate::ArmorClass>,
        )],
    ) -> BTreeSet<GearPage> {
        let mut pages = BTreeSet::new();
        let mut counts = [0usize; 3];
        for &(kind, donor_hash, rarity, selected) in entries {
            if kind == crate::ItemKind::Armor {
                if self.collection_is_exotic(rarity, donor_hash) {
                    continue;
                }
                let inherited = self
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.item_class_type(donor_hash))
                    .filter(|class| *class < 3);
                let class = selected.map_or(inherited, crate::ArmorClass::native_class);
                let classes = class.map_or(
                    crate::collection::Classes::ALL,
                    crate::collection::Classes::one,
                );
                for class in classes.iter() {
                    counts[usize::from(class)] += 1;
                }
            } else {
                pages.extend(GearPage::for_kind(kind));
            }
        }
        pages.extend(crate::collection::armor_pages(counts));
        pages
    }

    pub(super) fn draw_collection_destination(&mut self, ui: &mut egui::Ui) {
        if !self.recipe.kind.is_weapon() {
            ui.strong("Destination");
            ui.label(if self.recipe.kind == crate::ItemKind::Armor {
                if self.collection_is_exotic(self.recipe.overrides.rarity, self.recipe.donor.item_hash.parse_u32().unwrap_or_default()) {
                    "Exotic / Armor / Class.".to_owned()
                } else {
                    format!("{} category under Armor for each supported class. Numbered armor sets hold up to five items each.", self.presentation_editor.branding().name())
                }
            } else if GearPage::for_kind(self.recipe.kind).is_some() {
                format!(
                    "{} page under {}.",
                    self.presentation_editor.branding().name(),
                    self.recipe.kind.plural()
                )
            } else {
                format!("Beside its base {}.", self.recipe.kind.noun())
            });
            return;
        }
        let exotic = self.collection_is_exotic(
            self.recipe.overrides.rarity,
            self.recipe.donor.item_hash.parse_u32().unwrap_or_default(),
        );
        ui.strong("Destination");
        if exotic {
            ui.label("Exotic weapons appear under Exotics.");
            return;
        }
        let mut custom = self.recipe.overrides.collection_destination.is_some();
        let automatic = self.current_automatic_destination();
        egui::ComboBox::from_id_salt("collection_destination_mode")
            .selected_text(if custom { "Choose Page" } else { "Automatic" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut custom, false, "Automatic");
                ui.selectable_value(&mut custom, true, "Choose Page");
            });
        if !custom {
            self.recipe.overrides.collection_destination = None;
            if let Some(destination) = automatic {
                ui.weak(format!(
                    "Uses {} from the ammo type and weapon type.",
                    destination.label()
                ));
                if destination.stock_exemplar().is_none() {
                    ui.weak("Adds one shared Collections page.");
                }
            } else {
                ui.weak("Uses the ammo type and weapon type.");
            }
            return;
        }
        let destination =
            self.recipe
                .overrides
                .collection_destination
                .get_or_insert(automatic.unwrap_or(Destination {
                    ammo: Ammo::Primary,
                    family: Family::AutoRifles,
                }));
        ui.horizontal_wrapped(|ui| {
            ui.label("Weapons / ");
            egui::ComboBox::from_id_salt("collection_ammo")
                .selected_text(destination.ammo.label())
                .show_ui(ui, |ui| {
                    for ammo in Ammo::ALL {
                        ui.selectable_value(&mut destination.ammo, ammo, ammo.label());
                    }
                });
            ui.label(" / ");
            egui::ComboBox::from_id_salt("collection_family")
                .selected_text(destination.family.label())
                .show_ui(ui, |ui| {
                    for family in Family::ALL {
                        ui.selectable_value(&mut destination.family, family, family.label());
                    }
                });
        });
        if destination.stock_exemplar().is_none() {
            ui.weak("Adds one shared page for the selected weapon type.");
        } else {
            ui.weak("Uses an existing page. No additional nodes.");
        }
    }

    fn collection_is_exotic(&self, rarity: Option<crate::RecipeRarity>, donor_hash: u32) -> bool {
        rarity.map_or_else(
            || {
                self.donor_summaries
                    .iter()
                    .chain(self.gear_donors.values().flatten())
                    .find(|donor| donor.hash == donor_hash)
                    .is_some_and(|donor| donor.rarity == WeaponRarity::Exotic)
            },
            |rarity| rarity == crate::RecipeRarity::Exotic,
        )
    }
}

fn combined_custom_nodes(installed: &BTreeSet<u64>, selected: &BTreeSet<u64>) -> BTreeSet<u64> {
    installed.union(selected).copied().collect()
}

fn installed_custom_nodes(presentation_node_hashes: &[u64]) -> BTreeSet<u64> {
    presentation_node_hashes
        .iter()
        .skip(BASE_NODE_COUNT)
        .copied()
        .collect()
}

impl PackageAuthoringApp {
    /// Where the open recipe files when no page is chosen.
    fn current_automatic_destination(&self) -> Option<Destination> {
        let summary = |hash: u32| self.donor_summaries.iter().find(|d| d.hash == hash);
        automatic_destination(
            summary(self.recipe.donor.item_hash.parse_u32().unwrap_or_default()),
            summary(self.recipe.type_donor_hash()),
            self.recipe.overrides.ammo_type,
        )
    }
}

/// The page a weapon files under: the family of the type it shows (`shown`, falling back to the
/// base's when that type has no page) and the base weapon's ammo, unless the recipe sets one.
fn automatic_destination(
    donor: Option<&WeaponDonorSummary>,
    shown: Option<&WeaponDonorSummary>,
    authored_ammo: Option<crate::RecipeAmmoType>,
) -> Option<Destination> {
    let donor = donor?;
    let ammo = authored_ammo.map_or_else(
        || {
            donor.ammo_type.map(|ammo| match ammo {
                sundial::investment::WeaponAmmoType::Primary => Ammo::Primary,
                sundial::investment::WeaponAmmoType::Special => Ammo::Special,
                sundial::investment::WeaponAmmoType::Heavy => Ammo::Heavy,
            })
        },
        |ammo| {
            Some(match ammo {
                crate::RecipeAmmoType::Primary => Ammo::Primary,
                crate::RecipeAmmoType::Special => Ammo::Special,
                crate::RecipeAmmoType::Heavy => Ammo::Heavy,
            })
        },
    )?;
    Some(Destination {
        ammo,
        family: shown
            .and_then(|shown| Family::from_type_name(&shown.type_name))
            .or_else(|| Family::from_type_name(&donor.type_name))?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Ammo, Destination, Family, WeaponAmmoType, WeaponDonorSummary, automatic_destination,
    };

    #[test]
    fn automatic_destination_combines_authored_ammo_with_donor_weapon_type() {
        let donor = WeaponDonorSummary {
            ammo_type: Some(WeaponAmmoType::Heavy),
            ..crate::test_support::donor_summary(1, "Sword donor", "Sword")
        };
        assert_eq!(
            automatic_destination(Some(&donor), None, Some(crate::RecipeAmmoType::Special)),
            Some(Destination {
                ammo: Ammo::Special,
                family: Family::Swords,
            })
        );
    }
}
