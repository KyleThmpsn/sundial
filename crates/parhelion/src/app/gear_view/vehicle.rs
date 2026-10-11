//! Vehicle tuning and whole-vehicle summoning controls for authored Sparrows.
use super::*;
use crate::vehicle::{Driving, Projectile, Sparrow, Summon, Weapons};
mod controls;
mod picker;
mod sections;

/// The Summon Vehicle field, under the item's text beside Rarity: the vehicle the Sparrow
/// summons, its own by default.
pub(super) fn draw_summon(ui: &mut egui::Ui, app: &mut PackageAuthoringApp) {
    let original = app.recipe.overrides.sparrow.clone().unwrap_or_default();
    let mut edited = original.clone();
    if matches!(edited.summon, Summon::Other { .. })
        || matches!(edited.weapons.projectile, Projectile::Other { .. })
    {
        picker::prepare(app, ui);
    }
    let mut choose_other = false;
    let (label, reset) = style::field_name(
        ui,
        "Summon Vehicle",
        "The vehicle it summons, with its own seats and weapons. Untested in game",
        edited.summon != Summon::default(),
    );
    if reset {
        edited.summon = Summon::default();
    }
    egui::ComboBox::from_id_salt("sparrow-summon")
        .selected_text(picker::vehicle_label(app, &edited.summon))
        .width(ui.available_width())
        .truncate()
        .show_ui(ui, |ui| {
            workbench_style(ui);
            for choice in [
                Summon::Sparrow,
                Summon::Pike,
                Summon::HeavyPike,
                Summon::Interceptor,
                Summon::SuperInterceptor,
                Summon::Tank,
            ] {
                let label = choice.label();
                ui.selectable_value(&mut edited.summon, choice, label);
            }
            if ui
                .add_enabled(
                    app.build_receiver.is_none() && app.install_receiver.is_none(),
                    egui::Button::selectable(false, "Choose Other Vehicle…"),
                )
                .on_hover_text("Search and preview vehicles")
                .clicked()
            {
                choose_other = true;
                ui.close();
            }
        })
        .response
        .labelled_by(label.id);
    picker::vehicle(app, ui, &mut edited, choose_other);
    // The vehicle's silhouette takes the icon's art unless the icon has an image of its own.
    if edited.summon != Summon::Sparrow {
        let own_image = app.recipe.overrides.icon_edit.imported_image.is_some();
        ui.add_enabled(
            !own_image,
            egui::Checkbox::new(&mut edited.vehicle_icon, "Vehicle Icon"),
        )
        .on_hover_text("The icon shows the vehicle's HUD silhouette")
        .on_disabled_hover_text("The icon has an image of its own");
        let (label, reset) = style::field_name(
            ui,
            "Inventory Model",
            "What the inventory and inspect screens show",
            edited.inventory_model != crate::vehicle::InventoryModel::Sparrow,
        );
        if reset {
            edited.inventory_model = crate::vehicle::InventoryModel::Sparrow;
        }
        egui::ComboBox::from_id_salt("sparrow-inventory-model")
            .selected_text(edited.inventory_model.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                workbench_style(ui);
                for choice in crate::vehicle::InventoryModel::ALL {
                    ui.selectable_value(&mut edited.inventory_model, choice, choice.label());
                }
            })
            .response
            .labelled_by(label.id);
    }
    // Tank keeps its native driving speed, which takes no multiplier.
    if edited.summon == Summon::Tank && original.summon != Summon::Tank {
        edited.speed_percent = 100;
        edited.driving = Driving::default();
    }
    if edited.summon != original.summon {
        if edited.summon != Summon::Sparrow {
            edited.handling.side_dodges = false;
            edited.handling.air_control = false;
            edited.handling.roll_tricks = false;
        } else {
            // A Sparrow summons itself, so these settings return to their defaults.
            edited.vehicle_icon = true;
            edited.inventory_model = crate::vehicle::InventoryModel::Sparrow;
        }
        if !picker::capabilities(app, &edited.summon).armed {
            edited.weapons = Weapons::default();
        }
    }
    if edited != original {
        app.recipe.overrides.sparrow = edited.has_changes().then_some(edited);
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_vehicle_controls(&mut self, ui: &mut egui::Ui) {
        let original = self.recipe.overrides.sparrow.clone().unwrap_or_default();
        let mut edited = original.clone();
        ui.push_id("vehicle-tuning", |ui| {
            ui.horizontal(|ui| {
                // Marked and restorable as the stats' heading is, once something here changes.
                style::heading(ui, "Vehicle", original.has_changes());
                controls::info(
                    ui,
                    "Vehicle",
                    "Percent of the summoned vehicle's own values. Driving, repair and firing are untested in game.",
                );
                if original.has_changes() && style::reset_icon(ui, "Reset Vehicle") {
                    edited = Sparrow::default();
                }
            });
            let capabilities = picker::capabilities(self, &edited.summon);
            let projectile_label = picker::projectile_label(self, &edited.weapons.projectile);
            let choose_projectile = cards(
                ui,
                &mut edited,
                (capabilities.hover, capabilities.armed),
                &projectile_label,
            );
            picker::projectile(self, ui, &mut edited, choose_projectile);
            for entity in [
                match &edited.summon { Summon::Other { entity } => Some(entity), _ => None },
                match &edited.weapons.projectile { Projectile::Other { entity } => Some(entity), _ => None },
            ].into_iter().flatten() {
                if let Err(error) = entity.parse_u32() {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
            }
            #[cfg(feature = "d2-model-importer")]
            if let Some(graph) = &self.recipe.overrides.imported_graph
                && let Ok(Some(entity)) = edited.summon.entity()
                && let Err(error) = graph.validate_vehicle(entity) {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        });
        if edited != original {
            self.recipe.overrides.sparrow = edited.has_changes().then_some(edited);
        }
        ui.add_space(8.0);
    }
}

/// The page width from which the four cards share one line, each wide enough for two tiles.
const ONE_LINE: f32 = 1360.0;
/// The page width from which the cards go two to a line.
const TWO_LINES: f32 = 700.0;

/// The Driving, Handling, Durability and Weapons cards, as many to a line as the page holds, each
/// line's cards as tall as its tallest. `(hover, armed)` say what the summoned vehicle can take.
/// Returns whether Choose Other Projectile… was picked.
fn cards(
    ui: &mut egui::Ui,
    settings: &mut Sparrow,
    (hover, armed): (bool, bool),
    projectile_label: &str,
) -> bool {
    let width = ui.available_width();
    let count = if width >= ONE_LINE {
        4
    } else if width >= TWO_LINES {
        2
    } else {
        1
    };
    let mut choose_projectile = false;
    for (line, kinds) in [0, 1, 2, 3].chunks(count).enumerate() {
        let mut cards = style::CardLine::new(ui, ("vehicle-card-line", count, line));
        ui.columns(count, |columns| {
            for (&kind, column) in kinds.iter().zip(columns.iter_mut()) {
                cards.card(column, |ui| match kind {
                    0 => sections::driving(ui, settings, hover),
                    1 => sections::handling(ui, settings),
                    2 => sections::durability(ui, settings),
                    _ => {
                        choose_projectile =
                            sections::weapons(ui, settings, armed, projectile_label);
                    }
                });
            }
        });
        cards.finish(ui.ctx());
    }
    choose_projectile
}
