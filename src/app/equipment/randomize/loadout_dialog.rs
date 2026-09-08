//! Full-loadout confirmation and execution dialog.

use crate::app::account_workspace as account;

use super::*;

pub(super) fn draw_loadout_confirmation(
    app: &mut SundialApp,
    context: &egui::Context,
    character_index: usize,
    open_requested: bool,
) {
    let dialog_id = egui::Id::new(("equipment-randomize-loadout", character_index));
    let options_id = dialog_id.with("options");
    if open_requested {
        context.data_mut(|data| data.insert_temp(dialog_id, true));
        context.data_mut(|data| data.insert_temp(options_id, LoadoutOptions::default()));
    }
    let mut open = context
        .data_mut(|data| data.get_temp::<bool>(dialog_id))
        .unwrap_or(false);
    if !open {
        return;
    }

    let mut cancel_requested = false;
    let mut randomize_requested = false;
    let mut options = context
        .data_mut(|data| data.get_temp::<LoadoutOptions>(options_id))
        .unwrap_or_default();
    let character_name = account::character_metadata(&app.document, character_index)
        .ok()
        .map(|metadata| u64::from(metadata.class_type))
        .map_or_else(
            || format!("Character {}", character_index + 1),
            |class_type| {
                format!(
                    "Character {} · {}",
                    character_index + 1,
                    class_name(class_type)
                )
            },
        );
    egui::Window::new("Randomize Loadout")
        .id(dialog_id.with("window"))
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(560.0)
        .open(&mut open)
        .show(context, |ui| {
            ui.heading(character_name);
            ui.add_space(4.0);
            ui.label("Choose which parts of this character to regenerate.");
            ui.add_space(8.0);
            egui::Grid::new(dialog_id.with("scopes"))
                .num_columns(2)
                .spacing(egui::vec2(18.0, 6.0))
                .show(ui, |ui| {
                    ui.checkbox(&mut options.weapons, "Weapons")
                        .on_hover_text("Equipped weapons and held weapon inventory");
                    ui.checkbox(&mut options.armor, "Armor")
                        .on_hover_text("Equipped armor and held armor inventory");
                    ui.end_row();
                    ui.checkbox(&mut options.equipment_flair, "Equipment / Flair")
                        .on_hover_text("Ghosts, vehicles, ships, banners, emblems, emotes, and finishers");
                    ui.checkbox(&mut options.subclass, "Subclass")
                        .on_hover_text("A class-compatible subclass with valid default abilities");
                    ui.end_row();
                });
            ui.checkbox(
                &mut options.replace_held_inventory,
                "Replace existing inventory items",
            )
            .on_hover_text(
                "Off by default. When enabled, held items in the selected sections are removed and regenerated.",
            );
            ui.checkbox(&mut options.keep_locked_items, "Keep locked items")
                .on_hover_text(
                    "Preserves locked equipped and held items while randomizing the rest.",
                );
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.group(|ui| {
                ui.strong("Perk Safety");
                app.draw_plug_safety_controls(ui);
                ui.label("Applies to equipped and inventory weapon and armor rolls. Compatible uses each item's native perk pool.");
            });
            ui.label(
                egui::RichText::new(
                    "Checked sections regenerate equipped items immediately. Held inventory is preserved unless its replacement option is enabled. One equipped exotic is kept per weapon and armor set.",
                )
                .color(crate::app::ui::secondary_text_color(ui)),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(options.any(), egui::Button::new("Randomize Loadout"))
                        .on_disabled_hover_text("Select at least one section")
                        .clicked()
                    {
                        randomize_requested = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_requested = true;
                    }
                });
            });
        });

    if randomize_requested {
        match randomize_full_loadout(
            &mut app.document,
            &app.manifest,
            character_index,
            app.plug_selection_mode,
            app.show_dummy_items,
            options,
        ) {
            Ok(message) => {
                open = false;
                app.dirty = true;
                app.set_status(
                    format!(
                        "{message}. Perk safety: {}. Click Save to write it",
                        app.plug_selection_mode.label()
                    ),
                    false,
                );
            }
            Err(error) => {
                app.set_status(format!("Loadout not randomized: {error}"), true);
            }
        }
    } else if cancel_requested {
        open = false;
    }
    context.data_mut(|data| {
        data.insert_temp(dialog_id, open);
        data.insert_temp(options_id, options);
    });
}
