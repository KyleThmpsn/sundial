use super::super::workbench::Workbench;
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
    /// Draws every mapped projectile property. Used for an independently opened entity and
    /// for an action with no readable summary to place the properties on.
    pub(super) fn draw_movement(&mut self, ui: &mut egui::Ui, loaded: &PrivatePerkRuntimeGraph) {
        let parameters = mapped(loaded);
        if parameters.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.strong("Projectile Properties");
        self.draw_parameter_rows(ui, loaded, parameters);
    }

    /// Draws the mapped properties of the graphs listed, for placement on an effect block.
    /// Returns whether anything was drawn.
    pub(super) fn draw_movement_for(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        graphs: &[u32],
    ) -> bool {
        let parameters = mapped(loaded)
            .into_iter()
            .filter(|(tag, _)| graphs.contains(tag))
            .collect::<Vec<_>>();
        if parameters.is_empty() {
            return false;
        }
        ui.label(
            egui::RichText::new("Projectile Properties")
                .small()
                .strong(),
        );
        self.draw_parameter_rows(ui, loaded, parameters);
        true
    }

    fn draw_parameter_rows(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        parameters: Vec<(u32, Parameter)>,
    ) {
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
                    .map(|entry| self.projectile_label(entry))
                    .unwrap_or_else(|| format!("Projectile 0x{tag:08X}"));
                ui.add_space(4.0);
                ui.label(label);
            }
            for parameter in parameters {
                ui.push_id(
                    ("projectile-property", tag, owner, parameter.kind.label()),
                    |ui| {
                        let current = parameter.value(&self.draft);
                        let mut value = current.clone().unwrap_or(parameter.original());
                        let modified = parameter.is_modified(&self.draft);
                        let verified = parameter.kind == parameters::Kind::Speed;
                        let (changed_value, reset) = Workbench::property_row_with(
                            ui,
                            parameter.kind.label(),
                            parameter.kind.description(),
                            |ui| {
                                verified
                                    && self.draw_verified_projectile_speed(
                                        ui,
                                        loaded,
                                        tag,
                                        parameter.owner_tag,
                                    )
                            },
                            |ui| {
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
                                // This frame's edit is written below, so the reset stays live on it.
                                let reset = ui
                                    .add_enabled(
                                        modified || changed_value,
                                        egui::Button::new("Reset"),
                                    )
                                    .on_hover_text("Restore this property's original value.")
                                    .clicked();
                                (changed_value, reset)
                            },
                        );
                        let mut changed = false;
                        if changed_value {
                            self.parameter_error = parameter.set(&mut self.draft, value).err();
                            changed = true;
                        }
                        if reset {
                            self.parameter_error = parameter.reset(&mut self.draft).err();
                            changed = true;
                        }
                        if let Err(error) = current {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        if changed {
                            self.value_text
                                .retain(|(locator, _), _| !parameter.contains(locator));
                        }
                    },
                );
            }
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
