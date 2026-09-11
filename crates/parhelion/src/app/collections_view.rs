use super::*;
use crate::collection::{Ammo, BASE_NODE_COUNT, Destination, Family, NODE_CAPACITY, NodeBudget};

impl PackageAuthoringApp {
    pub(super) fn draw_collection_capacity(&self, ui: &mut egui::Ui) {
        let entries = self.recipe_entries.iter().filter(|entry| {
            self.enabled_recipe_paths.contains(&entry.path)
                && self.recipe_path.as_ref() != Some(&entry.path)
        });
        let mut members = entries
            .map(|entry| {
                (
                    entry.badge.as_ref().map(|badge| badge.name.as_str()),
                    entry
                        .collection_destination
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
        let used = budget.used();
        let custom_used = used.saturating_sub(BASE_NODE_COUNT);
        let custom_capacity = NODE_CAPACITY - BASE_NODE_COUNT;
        ui.add(
            sundial::investment::progress_bar(custom_used as f32 / custom_capacity as f32)
                .desired_width(220.0)
                .text(format!(
                    "{custom_used} / {custom_capacity} Custom Nodes Used"
                )),
        );
        let label = if used > NODE_CAPACITY {
            egui::RichText::new(format!("{} Nodes Over Limit", used - NODE_CAPACITY))
                .color(ui.visuals().error_fg_color)
        } else {
            egui::RichText::new(format!("{} Nodes Available", NODE_CAPACITY - used))
                .color(ui.visuals().weak_text_color())
        };
        ui.label(label).on_hover_text(format!(
            "{used} / {NODE_CAPACITY} nodes used.\n924 stock nodes + 4 Project Sunrise nodes + {} custom badge nodes + {} added pages. Recipes sharing a badge or page use the same nodes.", budget.badges * 4, budget.pages));
        if !included {
            let with_draft = NodeBudget::new(members.into_iter().chain([current]));
            if with_draft.used() > used {
                ui.weak(format!(
                    "This recipe adds {} nodes.",
                    with_draft.used() - used
                ));
            }
        }
        if used > NODE_CAPACITY {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Remove a custom badge or an added page before building.",
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
        egui::ComboBox::from_id_salt("collection_destination_mode")
            .selected_text(if custom { "Choose Page" } else { "Automatic" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut custom, false, "Automatic");
                ui.selectable_value(&mut custom, true, "Choose Page");
            });
        if !custom {
            self.recipe.overrides.collection_destination = None;
            ui.weak("Use the stock collection for the gameplay donor’s weapon family.");
            return;
        }
        let destination = self
            .recipe
            .overrides
            .collection_destination
            .get_or_insert(Destination {
                ammo: Ammo::Primary,
                family: Family::AutoRifles,
            });
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
