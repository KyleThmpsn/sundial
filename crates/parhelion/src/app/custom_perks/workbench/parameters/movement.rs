use super::*;
use entity::projectile::parameters::{self, Parameter};

pub(in crate::app::custom_perks) fn mapped(
    loaded: &PrivatePerkRuntimeGraph,
) -> Vec<(u32, Parameter)> {
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
    /// Draws every mapped projectile property.
    pub(super) fn draw_movement(&mut self, ui: &mut egui::Ui, loaded: &PrivatePerkRuntimeGraph) {
        let parameters = mapped(loaded);
        if parameters.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.strong("Projectile Properties");
        self.draw_parameter_rows(ui, loaded, parameters);
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
        let labels = (groups.len() > 1)
            .then(|| self.projectile_labels_for(ui.ctx(), &loaded.projectile_catalog));
        for ((tag, _), parameters) in groups {
            if let Some(labels) = &labels {
                let label = labels
                    .get(&tag)
                    .cloned()
                    .unwrap_or_else(|| format!("Projectile 0x{tag:08X}"));
                ui.add_space(4.0);
                ui.label(label);
            }
            crate::app::style::tiles(ui, |ui, width| {
                for parameter in parameters {
                    if let Some(result) =
                        draw_parameter(ui, width, loaded, tag, &parameter, &mut self.draft)
                    {
                        self.parameter_error = result.err();
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

/// The inline action card and full property editor share the same tile and byte edits.
pub(in crate::app::custom_perks) fn draw_parameter(
    ui: &mut egui::Ui,
    width: f32,
    loaded: &PrivatePerkRuntimeGraph,
    tag: u32,
    parameter: &Parameter,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
) -> Option<Result<(), String>> {
    let current = parameter.value(draft);
    let mut value = current.clone().unwrap_or(parameter.original());
    // The same bounds as `Kind::validate`, so a drag cannot leave an invalid draft.
    let range = match parameter.kind {
        parameters::Kind::Speed => 0.001..=f32::MAX,
        parameters::Kind::Gravity => 0.0..=10.0,
        parameters::Kind::TravelDistance => 0.0..=f32::MAX,
    };
    let hint = (parameter.kind == parameters::Kind::Speed)
        .then(|| guided::verified_speed_evidence(loaded, tag, parameter.owner_tag))
        .flatten()
        .unwrap_or(parameter.kind.description());
    let label = parameter.kind.label();
    let (changed, reset) = crate::app::style::tile(
        ui,
        width,
        ("projectile-property", tag, parameter.owner_tag, label),
        label,
        hint,
        parameter.is_modified(draft),
        |ui| {
            let response = ui
                .add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut value)
                        .speed(0.05)
                        .max_decimals(3)
                        .range(range)
                        .clamp_existing_to_range(false)
                        .suffix(parameter.kind.suffix()),
                )
                .on_hover_text(format!(
                    "Original: {}{}",
                    parameter.original(),
                    parameter.kind.suffix()
                ));
            let changed = response.changed();
            crate::app::style::named_control(response, label);
            if let Err(error) = &current {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            changed
        },
    );
    if reset {
        Some(parameter.reset(draft))
    } else if changed {
        Some(parameter.set(draft, value))
    } else {
        None
    }
}
