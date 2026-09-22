use super::*;
use crate::collection::{
    Ammo, BASE_NODE_COUNT, Destination, Family, NODE_CAPACITY, NodeBudget, custom_node_hashes,
};
use std::collections::BTreeSet;

impl PackageAuthoringApp {
    pub(super) fn draw_collection_capacity(&self, ui: &mut egui::Ui) {
        let entries = self.recipe_entries.iter().filter(|entry| {
            self.enabled_recipe_paths.contains(&entry.path)
                && self.recipe_path.as_ref() != Some(&entry.path)
        });
        let mut members = entries
            .map(|entry| {
                let donor = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == entry.donor_hash);
                (
                    entry.badge.as_ref().map(|badge| badge.name.as_str()),
                    entry
                        .collection_destination
                        .or_else(|| automatic_destination(donor, entry.ammo_type))
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
                .or_else(|| {
                    automatic_destination(
                        self.donor_summaries.iter().find(|donor| {
                            donor.hash
                                == self.recipe.donor.item_hash.parse_u32().unwrap_or_default()
                        }),
                        self.recipe.overrides.ammo_type,
                    )
                })
                .filter(|_| !exotic),
        );
        let included = self
            .recipe_path
            .as_ref()
            .is_some_and(|path| self.enabled_recipe_paths.contains(path));
        if included {
            members.push(current);
        }
        let budget = NodeBudget::new(members.iter().copied());
        let selected_nodes = custom_node_hashes(members.iter().copied());
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
        let selected_existing = selected_nodes.intersection(&installed_nodes).count();
        ui.label(label).on_hover_text(format!(
            "{used} / {NODE_CAPACITY} distinct nodes accounted for.\n924 stock nodes + 4 {} nodes + {} installed custom nodes + {selected_new} additional selected-build nodes. The selected build uses {} custom nodes, including {} badge nodes and {} added pages. {selected_existing} selected nodes already exist and are counted once. Installing the selected build replaces the installed custom set.", self.presentation_editor.branding().name(), installed_nodes.len(), selected_nodes.len(), budget.badges * 4, budget.pages));
        if !included {
            let with_draft = custom_node_hashes(members.into_iter().chain([current]));
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
                "The installed presentation-node table exceeds its native capacity.",
            );
        }
    }

    pub(super) fn draw_collection_destination(&mut self, ui: &mut egui::Ui) {
        let exotic = self.collection_is_exotic(
            self.recipe.overrides.rarity,
            self.recipe.donor.item_hash.parse_u32().unwrap_or_default(),
        );
        ui.strong("Destination");
        if exotic {
            ui.label("Exotic weapons use the Exotics collection for their inventory slot.");
            return;
        }
        let mut custom = self.recipe.overrides.collection_destination.is_some();
        let automatic = automatic_destination(
            self.donor_summaries.iter().find(|donor| {
                donor.hash == self.recipe.donor.item_hash.parse_u32().unwrap_or_default()
            }),
            self.recipe.overrides.ammo_type,
        );
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
                    "Uses {} from the authored ammo type and gameplay donor weapon type.",
                    destination.label()
                ));
                if destination.stock_exemplar().is_none() {
                    ui.weak("This combination adds one shared Collections page.");
                }
            } else {
                ui.weak("Uses the authored ammo type and gameplay donor weapon type.");
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
            ui.weak("Adds one shared page using the stock weapon-type name and icon.");
        } else {
            ui.weak("Uses an existing page. No additional nodes.");
        }
        ui.weak("Collection placement does not change ammo or gameplay.");
    }

    fn collection_is_exotic(&self, rarity: Option<crate::RecipeRarity>, donor_hash: u32) -> bool {
        rarity.map_or_else(
            || {
                self.donor_summaries
                    .iter()
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

fn automatic_destination(
    donor: Option<&WeaponDonorSummary>,
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
        family: Family::from_type_name(&donor.type_name)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Ammo, BASE_NODE_COUNT, Destination, Family, WeaponAmmoType, WeaponDamageProfile,
        WeaponDonorSummary, WeaponRarity, automatic_destination, combined_custom_nodes,
        installed_custom_nodes,
    };
    use std::collections::BTreeSet;

    #[test]
    fn installed_and_selected_custom_nodes_share_matching_identities() {
        let installed = BTreeSet::from([10, 20, 30]);
        let selected = BTreeSet::from([20, 30, 40]);
        assert_eq!(
            combined_custom_nodes(&installed, &selected),
            BTreeSet::from([10, 20, 30, 40])
        );
    }

    #[test]
    fn installed_custom_nodes_begin_after_stock_and_runtime_rows() {
        let mut hashes = vec![1; BASE_NODE_COUNT];
        hashes.extend([10, 20, 30]);
        assert_eq!(
            installed_custom_nodes(&hashes),
            BTreeSet::from([10, 20, 30])
        );
    }

    #[test]
    fn automatic_destination_combines_authored_ammo_with_donor_weapon_type() {
        let donor = WeaponDonorSummary {
            hash: 1,
            name: "Sword donor".into(),
            type_name: "Sword".into(),
            bucket_hash: 0,
            collection_backed: true,
            power_cap: None,
            damage_type: None,
            inventory_slot: None,
            ammo_type: Some(WeaponAmmoType::Heavy),
            weapon_pattern_index: None,
            weapon_translation_group: None,
            stat_group_index: None,
            damage_profile: WeaponDamageProfile::Unknown,
            rarity: WeaponRarity::Legendary,
        };
        assert_eq!(
            automatic_destination(Some(&donor), Some(crate::RecipeAmmoType::Special)),
            Some(Destination {
                ammo: Ammo::Special,
                family: Family::Swords,
            })
        );
    }
}
