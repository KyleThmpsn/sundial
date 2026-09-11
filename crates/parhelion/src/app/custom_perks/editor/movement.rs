use super::*;
use projectile::parameters::{self, Parameter};

pub(super) fn mapped(loaded: &PrivatePerkRuntimeGraph) -> Vec<(u32, Parameter)> {
    loaded
        .graphs
        .iter()
        .filter(|(tag, _)| {
            (loaded.action_tag == 0 && loaded.action_payload.is_empty() && loaded.graphs.len() == 1)
                || loaded
                    .projectile_slots
                    .iter()
                    .any(|(_, effective)| effective == tag)
        })
        .flat_map(|(tag, graph)| {
            parameters::discover(graph)
                .into_iter()
                .map(|parameter| (*tag, parameter))
        })
        .collect()
}

impl PerkEditor {
    pub(super) fn draw_movement(&mut self, ui: &mut egui::Ui, loaded: &PrivatePerkRuntimeGraph) {
        let parameters = mapped(loaded);
        ui.add_space(8.0);
        ui.strong("Projectile Properties");
        if parameters.is_empty() {
            ui.label("This asset has no mapped movement properties yet.");
            return;
        }
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        for (tag, parameter) in parameters {
            groups
                .entry((tag, parameter.owner_tag))
                .or_default()
                .push(parameter);
        }
        let multiple = groups.len() > 1;
        for ((tag, owner), parameters) in groups {
            if multiple {
                let label = loaded
                    .projectile_catalog
                    .entries
                    .iter()
                    .find(|entry| entry.graph == tag)
                    .map(Self::projectile_label)
                    .unwrap_or_else(|| format!("Projectile 0x{tag:08X}"));
                ui.add_space(4.0);
                ui.label(label);
            }
            egui::Grid::new(("projectile-properties", tag, owner))
                .num_columns(3)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    for parameter in parameters {
                        ui.push_id((parameter.kind.label(), "label"), |ui| {
                            if parameter.kind != parameters::Kind::Speed
                                || !self.draw_verified_projectile_speed(
                                    ui,
                                    loaded,
                                    tag,
                                    parameter.owner_tag,
                                )
                            {
                                ui.label(parameter.kind.label())
                                    .on_hover_text(parameter.kind.description());
                            }
                        });
                        let current = parameter.value(&self.draft);
                        let mut value = current.clone().unwrap_or(parameter.original());
                        let mut changed = false;
                        ui.push_id((parameter.kind.label(), "value"), |ui| {
                            let response = ui
                                .add_sized(
                                    [100.0, ui.spacing().interact_size.y],
                                    egui::DragValue::new(&mut value)
                                        .speed(0.05)
                                        .max_decimals(3)
                                        .suffix(parameter.kind.suffix()),
                                )
                                .on_hover_text(format!(
                                    "Original: {}{}\n{}",
                                    parameter.original(),
                                    parameter.kind.suffix(),
                                    parameter.kind.description()
                                ));
                            let changed_value = response.changed();
                            crate::app::style::named_control(response, parameter.kind.label());
                            if changed_value {
                                self.parameter_error = parameter.set(&mut self.draft, value).err();
                                changed = true;
                            }
                        });
                        ui.push_id((parameter.kind.label(), "reset"), |ui| {
                            if ui
                                .add_enabled(
                                    parameter.is_modified(&self.draft),
                                    egui::Button::new("Reset"),
                                )
                                .on_hover_text("Restore this property's original value.")
                                .clicked()
                            {
                                self.parameter_error = parameter.reset(&mut self.draft).err();
                                changed = true;
                            }
                        });
                        ui.end_row();
                        if let Err(error) = current {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                            ui.end_row();
                        }
                        if changed {
                            self.value_text
                                .retain(|(locator, _), _| !parameter.contains(locator));
                        }
                    }
                });
        }
    }

    pub(super) fn carry_movement(&mut self, loaded: &PrivatePerkRuntimeGraph) {
        let Some((target, values)) = self.pending_movement.take() else {
            return;
        };
        let Some((_, graph)) = loaded.graphs.iter().find(|(tag, _)| *tag == target) else {
            return;
        };
        let parameters = parameters::discover(graph);
        for (kind, bits) in values {
            let value = f32::from_bits(bits);
            let matching = parameters
                .iter()
                .filter(|parameter| parameter.kind == kind)
                .collect::<Vec<_>>();
            if matching.len() == 1
                && let Err(error) = matching[0].set(&mut self.draft, value)
            {
                self.parameter_error = Some(error);
            }
        }
    }
}
