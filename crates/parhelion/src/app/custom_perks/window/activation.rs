//! Trigger controls belong to the mapped effect, not the entire custom perk.
use super::*;
use sundial::package_authoring::sandbox_perk::activation::{PerkActivation, supports_activation};

pub(super) fn draw(
    ui: &mut egui::Ui,
    variant: &mut WeaponSocketPlugVariantRecipe,
    experimental: bool,
) {
    let saved = variant
        .sandbox_perks
        .iter()
        .any(|perk| perk.activation.is_some());
    if !experimental && !saved {
        return;
    }
    ui.strong("Outlaw Reload Bonus — Trigger (Experimental)").on_hover_text(
        "Changes the mapped Outlaw kill filter in this custom perk only. The original reload effect and duration remain. This does not control Stat Bonuses While Equipped or Additional Effects. In-game trigger behavior still needs testing.",
    );
    ui.label("Only controls Outlaw's reload bonus. Stat bonuses and additional effects keep their own behavior.");
    let mut supported = false;
    for perk in &mut variant.sandbox_perks {
        if !supports_activation(perk.source_perk_index) && perk.activation.is_none() {
            continue;
        }
        supported = true;
        ui.push_id(
            (
                "activation",
                variant.socket_index,
                variant.choice_index,
                perk.source_perk_index,
            ),
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(if supports_activation(perk.source_perk_index) {
                        "Outlaw Reload Bonus"
                    } else {
                        "Unsupported Source"
                    });
                    ui.add_enabled_ui(
                        experimental && supports_activation(perk.source_perk_index),
                        |ui| {
                            egui::ComboBox::from_id_salt("condition")
                                .selected_text(perk.activation.map_or(
                                    "Original (Precision Weapon Kill)",
                                    PerkActivation::label,
                                ))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut perk.activation,
                                        None,
                                        "Original (Precision Weapon Kill)",
                                    );
                                    for condition in PerkActivation::ALL {
                                        ui.selectable_value(
                                            &mut perk.activation,
                                            Some(condition),
                                            condition.label(),
                                        );
                                    }
                                });
                        },
                    );
                    if perk.activation.is_some() && ui.small_button("Reset").clicked() {
                        perk.activation = None;
                    }
                });
            },
        );
    }
    if supported {
        if !experimental {
            ui.label("Enable advanced technical controls in Preferences to edit this saved condition, or Reset to restore the original.");
        }
    } else {
        ui.label("This perk has no Outlaw trigger to edit. Choose Outlaw on the Weapon tab to try these conditions.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_controls_fit_and_preserve_saved_conditions_when_hidden() {
        let mut variant: WeaponSocketPlugVariantRecipe = serde_json::from_str(
            r#"{"socket_index":3,"choice_index":0,"source_plug_hash":"0x45A0BDD7","sandbox_perks":[{"source_perk_index":421,"activation":"grenade_kill","runtime_values":[]}]}"#,
        ).unwrap();
        let before = variant.clone();
        for width in [360.0, 480.0, 620.0] {
            for experimental in [false, true] {
                for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
                    let ctx = egui::Context::default();
                    ctx.set_visuals(visuals);
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 1000.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                workbench_style(ui);
                                let right = ui.max_rect().right();
                                draw(ui, &mut variant, experimental);
                                assert!(
                                    ui.min_rect().right() <= right + 1.0,
                                    "activation overflow at {width}"
                                );
                            });
                        },
                    );
                    assert_eq!(variant, before);
                }
            }
        }
    }
}
