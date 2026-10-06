//! Sparrow motion and experimental whole-vehicle summoning controls.
use super::*;
use crate::vehicle::{Sparrow, Summon};

impl PackageAuthoringApp {
    pub(super) fn draw_vehicle_controls(&mut self, ui: &mut egui::Ui) {
        let original = self.recipe.overrides.sparrow.clone();
        let mut edited = original.clone().unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Vehicle");
            if ui
                .add_enabled(
                    original.as_ref().is_some_and(Sparrow::has_changes),
                    egui::Button::new("Reset Vehicle"),
                )
                .clicked()
            {
                edited = Sparrow::default();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Summon Vehicle");
            egui::ComboBox::from_id_salt("sparrow-summon")
                .selected_text(edited.summon.label())
                .show_ui(ui, |ui| {
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
                        .selectable_label(
                            matches!(edited.summon, Summon::Other { .. }),
                            "Other Vehicle",
                        )
                        .clicked()
                        && !matches!(edited.summon, Summon::Other { .. })
                    {
                        edited.summon = Summon::Other {
                            entity: HexHash::new(0x80C0_D9FA),
                        };
                    }
                });
        });
        if edited.summon != Summon::Sparrow {
            ui.label("Experimental. Summons the selected vehicle with its own seats and weapons. Driver attachment, weapon fire and cleanup still need an in-game test.");
        }
        if let Summon::Other { entity } = &mut edited.summon {
            egui::CollapsingHeader::new("Technical").id_salt("sparrow-vehicle-technical").show(ui, |ui| {
                ui.label("Vehicle Entity Tag");
                let mut value = entity.as_str().to_owned();
                if ui.add(egui::TextEdit::singleline(&mut value).desired_width(140.0)).changed() {
                    entity.set_text(value);
                }
                ui.label("Use a native Shadowkeep vehicle entity with entry markers and supported vehicle motion.");
                if let Err(error) = entity.parse_u32() {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
            });
        }
        #[cfg(feature = "d2-model-importer")]
        if self.recipe.overrides.imported_graph.is_some() && edited.summon != Summon::Sparrow {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Alternate summoning requires a native base without an imported appearance.",
            );
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Driving Speed").strong());
        let hover = edited.summon != Summon::Tank;
        if !hover
            && edited.summon
                != original
                    .as_ref()
                    .map_or(Summon::Sparrow, |s| s.summon.clone())
        {
            edited.speed_percent = 100;
        }
        ui.add_enabled_ui(hover, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::Slider::new(&mut edited.speed_percent, 1..=1000).suffix("%"));
                for (percent, label) in [(50, "0.5×"), (100, "1×"), (200, "2×"), (300, "3×")] {
                    if ui
                        .selectable_label(edited.speed_percent == percent, label)
                        .clicked()
                    {
                        edited.speed_percent = percent;
                    }
                }
            });
        });
        ui.label(if hover {
            "Multiplies forward and reverse motion. 100% keeps the selected vehicle's speed. 200% requests twice that speed, including fixed-speed Sparrows. The Speed stat below only changes the tooltip."
        } else {
            "Tank keeps its native driving speed. Its motion does not support this multiplier."
        });
        if edited != original.clone().unwrap_or_default() {
            self.recipe.overrides.sparrow = edited.has_changes().then_some(edited);
        }
        ui.add_space(8.0);
    }
}
