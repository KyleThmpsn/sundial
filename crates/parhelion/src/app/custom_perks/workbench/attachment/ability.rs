//! A subclass ability or path node as a custom perk's destination. The perk goes on the entry
//! whole, as one of its custom perks, in place of the one with its id. A copy of one of the
//! entry's stock perks takes that perk's place once applied.
use super::*;
use crate::subclass::Place;

/// Where a custom perk goes: an ability or node, and the stock perk it replaces there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::custom_perks::workbench) struct Target {
    place: Place,
    replaces: Option<u16>,
}

pub(in crate::app::custom_perks::workbench) struct Change {
    target: Target,
    perk: PerkRecipe,
}

impl Change {
    pub(in crate::app::custom_perks::workbench) fn apply(
        &self,
        recipe: &mut WeaponRecipe,
    ) -> Result<(), String> {
        if let Some(issue) = crate::perk::preflight::check(&self.perk)
            .into_iter()
            .find(|issue| issue.blocking)
        {
            return Err(issue.message);
        }
        let base = recipe
            .donor
            .item_hash
            .parse_u32()
            .map_err(|error| error.to_string())?;
        let mut abilities = recipe
            .overrides
            .subclass_abilities
            .clone()
            .unwrap_or_default();
        let place = self.target.place;
        let mut edits = abilities.edits(base, place);
        if let Some(stock) = self.target.replaces {
            edits.remove_perk(stock);
        }
        edits.set_custom_perk(self.perk.clone());
        abilities.set_edits(base, place, edits);
        abilities.validate()?;
        recipe.overrides.subclass_abilities = (!abilities.is_empty()).then_some(abilities);
        Ok(())
    }
}

impl Workbench {
    /// Opens a custom perk of the ability or node at `place`: a new one, one of its own, or
    /// `copy`, the host's copy of a stock perk.
    pub(in crate::app::custom_perks::workbench) fn open_ability(
        &mut self,
        recipe: &WeaponRecipe,
        (place, perk): (Place, AbilityPerk),
        copy: Option<PerkRecipe>,
    ) {
        self.picker = None;
        self.initialize();
        self.message = None;
        self.message_path = None;
        let target = Target {
            place,
            replaces: match perk {
                AbilityPerk::Stock(index) => Some(index),
                AbilityPerk::New | AbilityPerk::Custom(_) => None,
            },
        };
        let own = match perk {
            AbilityPerk::Custom(index) => {
                recipe.donor.item_hash.parse_u32().ok().and_then(|base| {
                    recipe
                        .overrides
                        .subclass_abilities
                        .as_ref()?
                        .edits(base, place)
                        .custom_perks
                        .get(index)
                        .cloned()
                })
            }
            AbilityPerk::New | AbilityPerk::Stock(_) => None,
        };
        // The copy already open for this perk, or for this stock perk of this entry.
        let open = self.documents.iter().position(|document| match &own {
            Some(own) => document.recipe.id == own.id,
            None => target.replaces.is_some() && document.ability == Some(target),
        });
        if let Some(index) = open {
            self.documents[index].ability = Some(target);
            self.select_document(index);
        } else {
            let mut document = Document::new(own.or(copy).unwrap_or_default(), None);
            document.ability = Some(target);
            self.add_document(document);
        }
        self.open = true;
    }

    /// The footer of a subclass recipe: the ability or node the perk goes on, and the apply.
    pub(in crate::app::custom_perks::workbench) fn draw_ability_attachment(
        &mut self,
        ui: &mut egui::Ui,
    ) -> Option<Change> {
        self.documents.get(self.selected)?;
        let issue = self.selected_issue();
        if let Some(issue) = &issue {
            self.draw_issue(ui, issue);
        }
        let edit_issue = self.edit_issue();
        let editing = self.editor.is_some();
        let recipe_unsaved = self.recipe_unsaved;
        let mut save_recipe = false;
        let places = &self.ability_places;
        let document = self.documents.get_mut(self.selected)?;
        let mut result = None;
        ui.add_enabled_ui(!editing, |ui| {
            ui.horizontal(|ui| {
                ui.label("Ability");
                let width = (ui.available_width() * 0.45).clamp(200.0, 340.0);
                let destination = document
                    .ability
                    .and_then(|target| places.iter().find(|(place, _)| *place == target.place))
                    .map_or_else(|| "Select Ability".to_owned(), |(_, label)| label.clone());
                controls::sized(ui, width, |ui| {
                    egui::ComboBox::from_id_salt("perk-ability-destination")
                        .width(width)
                        .truncate()
                        .selected_text(destination.clone())
                        .show_ui(ui, |ui| {
                            for (place, label) in places {
                                let current = document.ability.map(|target| target.place);
                                if ui
                                    .selectable_label(current == Some(*place), label)
                                    .clicked()
                                    && current != Some(*place)
                                {
                                    // A replaced stock perk belongs to the entry it came from.
                                    document.ability = Some(Target {
                                        place: *place,
                                        replaces: None,
                                    });
                                }
                            }
                        })
                        .response
                        .on_hover_text(destination);
                    pickers::name_combo(ui, "perk-ability-destination", "Destination Ability");
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(recipe_unsaved, egui::Button::new("Save Recipe"))
                        .on_disabled_hover_text("Recipe saved.")
                        .clicked()
                    {
                        save_recipe = true;
                    }
                    ui.separator();
                    // A warning is shown above but does not hold the perk back.
                    let blocking = issue.as_ref().filter(|issue| issue.blocking);
                    let ready =
                        document.ability.is_some() && blocking.is_none() && edit_issue.is_none();
                    let apply = crate::app::style::primary(ui, "Apply to Subclass");
                    if ui
                        .add_enabled(ready, apply)
                        .on_disabled_hover_text(
                            edit_issue
                                .or_else(|| blocking.map(|issue| issue.message.as_str()))
                                .unwrap_or("Choose an ability."),
                        )
                        .clicked()
                    {
                        result = document.ability.map(|target| Change {
                            target,
                            perk: document.recipe.clone(),
                        });
                    }
                });
            });
        });
        self.save_recipe_requested |= save_recipe;
        result
    }

    pub(in crate::app::custom_perks::workbench) fn applied_to_ability(&mut self, change: &Change) {
        self.error = None;
        self.message_path = None;
        self.message = Some(format!("Applied to {}.", change.target.place.label()));
        self.persist_drafts();
    }
}
