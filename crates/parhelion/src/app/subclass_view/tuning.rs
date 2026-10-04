//! An ability's tuning in one place, one tab at a time: its bank's script parameters, its
//! entity's values, and the graphs it spawns with theirs, reached through a trail of the graphs
//! above them.
use super::modifiers::{ordered, parameter_name, parameter_value};
use super::values::values_of;
use super::*;
use sundial::package_authoring::ability_bank::{ParameterKind, parameter_kind, parameter_meaning};

/// Levels of spawned graphs the build copies below an ability.
pub(super) const SPAWN_DEPTH: usize = 3;

/// The tab the Tuning field shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Tab {
    #[default]
    Parameters,
    Values,
    Spawns,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Parameters, Self::Values, Self::Spawns];

    const fn label(self) -> &'static str {
        match self {
            Self::Parameters => "Parameters",
            Self::Values => "Values",
            Self::Spawns => "Spawns",
        }
    }
}

/// How the table shows a parameter: as its kind, unless its values contradict it, such as a
/// switch written something other than 0 or 1, which then shows as the number it is.
pub(super) fn shown_kind(parameter: u32, stock: f32, value: f32) -> ParameterKind {
    let whole = |value: f32| value.fract() == 0.0;
    match parameter_kind(parameter) {
        ParameterKind::Switch if [stock, value].iter().all(|v| *v == 0.0 || *v == 1.0) => {
            ParameterKind::Switch
        }
        ParameterKind::Count if whole(stock) && whole(value) => ParameterKind::Count,
        ParameterKind::Multiplier => ParameterKind::Multiplier,
        _ => ParameterKind::Number,
    }
}

/// A value as its kind reads: Off or On, a whole count, a multiplier, or a number.
pub(super) fn reading(kind: ParameterKind, value: f32) -> String {
    match kind {
        ParameterKind::Switch => if value == 0.0 { "Off" } else { "On" }.to_owned(),
        ParameterKind::Count => format!("{value:.0}"),
        ParameterKind::Multiplier => format!("×{}", parameter_value(value)),
        ParameterKind::Number => parameter_value(value),
    }
}

/// The control for a value of `kind`: a checkbox for a switch, a whole-number field for a count,
/// and a number field otherwise, a multiplier's marked with ×.
pub(super) fn value_field(
    ui: &mut egui::Ui,
    kind: ParameterKind,
    value: &mut f32,
) -> egui::Response {
    match kind {
        ParameterKind::Switch => {
            let mut on = *value != 0.0;
            let label = if on { "On" } else { "Off" };
            let response = ui.checkbox(&mut on, label);
            if response.changed() {
                *value = if on { 1.0 } else { 0.0 };
            }
            response
        }
        // Stock values are shown as they are, never clamped by drawing.
        ParameterKind::Count => ui.add(
            egui::DragValue::new(value)
                .speed(0.05)
                .fixed_decimals(0)
                .range(0.0..=99.0)
                .clamp_existing_to_range(false),
        ),
        ParameterKind::Multiplier => ui.add(
            egui::DragValue::new(value)
                .speed(0.01)
                .max_decimals(4)
                .prefix("×"),
        ),
        ParameterKind::Number => ui.add(egui::DragValue::new(value).speed(0.01).max_decimals(4)),
    }
}

/// The Spawns tab's trail: the graphs opened below an ability's entity, outermost first.
#[derive(Clone, Debug, Default)]
pub(super) struct Trail {
    entity: u32,
    graphs: Vec<u32>,
}

impl PackageAuthoringApp {
    /// The Tuning field of an ability with an entity: its tabs, then the open one. Returns the
    /// entry's edits once they change.
    pub(super) fn draw_tuning(
        &self,
        ui: &mut egui::Ui,
        (summary, entry, place): (Option<&SubclassSummary>, u8, Place),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let entity = *summary?.entry_entities.get(&entry)?;
        let row = summary
            .and_then(|summary| summary.entry_rows.get(&entry))
            .and_then(|row| self.catalog.as_ref()?.ability_row(*row));
        let entity_values = values_of(&edits.ability_values, entity, entity);
        let edited = [
            !edits.parameters.is_empty(),
            !entity_values.is_empty(),
            entity_values.len() != edits.ability_values.len(),
        ];
        let modified = edited.iter().any(|edited| *edited);
        let (changed, reset) = detail::field(ui, "Tuning", modified, |ui| {
            ui.horizontal(|ui| {
                for (tab, edited) in Tab::ALL.into_iter().zip(edited) {
                    if detail::marked_tab(ui, page.tuning == tab, tab.label(), edited).clicked() {
                        page.tuning = tab;
                    }
                }
            });
            ui.add_space(4.0);
            match page.tuning {
                Tab::Parameters => self.draw_parameters(ui, row, edits, page),
                Tab::Values => self.draw_spawn_values(ui, (entity, entity), place, edits, page),
                Tab::Spawns => self.draw_spawns(ui, entity, place, edits, page),
            }
        });
        changed.or_else(|| {
            reset.then(|| EntryEdits {
                parameters: Vec::new(),
                ability_values: Vec::new(),
                ..edits.clone()
            })
        })
    }

    /// The bank's script parameters, named ones first: each one's stock value and its own.
    fn draw_parameters(
        &self,
        ui: &mut egui::Ui,
        row: Option<&sundial::investment::AbilityRowSummary>,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let Some(row) = row.filter(|row| !row.parameters.is_empty()) else {
            ui.weak("No parameters.");
            return None;
        };
        let mut changed = None;
        let long = crate::app::pickers::wants_filter(row.parameters.len());
        if long {
            ui.add(
                egui::TextEdit::singleline(&mut page.parameter_query)
                    .hint_text(format!(
                        "{} Filter",
                        egui_phosphor::regular::MAGNIFYING_GLASS
                    ))
                    .desired_width(f32::INFINITY),
            );
        }
        let query = if long {
            page.parameter_query.trim().to_lowercase()
        } else {
            String::new()
        };
        egui::Grid::new("subclass-parameters")
            .num_columns(4)
            .striped(true)
            .spacing(egui::vec2(12.0, 4.0))
            .show(ui, |ui| {
                ui.label(quiet(ui, "Parameter"));
                ui.label(quiet(ui, "Stock"));
                ui.label(quiet(ui, "Value"));
                ui.label("");
                ui.end_row();
                for parameter in ordered(row) {
                    let name = parameter_name(parameter.name);
                    if !query.is_empty()
                        && !name.to_lowercase().contains(&query)
                        && !format!("{:08x}", parameter.name).contains(&query)
                    {
                        continue;
                    }
                    let own = edits.parameter(parameter.name);
                    let text = egui::RichText::new(&name);
                    let label = ui.label(if own.is_some() { text.strong() } else { text });
                    if let Some(meaning) = parameter_meaning(parameter.name) {
                        label.on_hover_text(meaning);
                    }
                    let mut value = own.unwrap_or(parameter.reset);
                    let kind = shown_kind(parameter.name, parameter.reset, value);
                    ui.label(quiet(ui, reading(kind, parameter.reset)));
                    let field = value_field(ui, kind, &mut value);
                    let field = style::named_control(field, &name);
                    if field.changed() && value.is_finite() {
                        let mut edited = edits.clone();
                        edited.set_parameter(parameter.name, Some(value));
                        changed = Some(edited);
                    }
                    if own.is_some() && detail::reset_icon(ui) {
                        let mut edited = edits.clone();
                        edited.set_parameter(parameter.name, None);
                        changed = Some(edited);
                    }
                    ui.end_row();
                }
            });
        changed
    }

    /// The values of one graph, `(graph, entity)`, once it loads.
    fn draw_spawn_values(
        &self,
        ui: &mut egui::Ui,
        (graph, entity): (u32, u32),
        place: Place,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let Some(loaded) = page.values.poll(ui.ctx(), &self.packages, graph) else {
            ui.weak("Loading…");
            return None;
        };
        match loaded {
            Ok(loaded) => self
                .draw_graph_values(
                    ui,
                    (&*loaded, entity),
                    place,
                    &edits.ability_values,
                    &mut page.values,
                )
                .map(|ability_values| EntryEdits {
                    ability_values,
                    ..edits.clone()
                }),
            Err(error) => {
                ui.colored_label(ui.visuals().warn_fg_color, "Values unavailable.")
                    .on_hover_text(error);
                None
            }
        }
    }

    /// The graphs the ability spawns: a trail back to the ability, then the open graph's values
    /// and the graphs it spawns in turn, each marked once its values change.
    fn draw_spawns(
        &self,
        ui: &mut egui::Ui,
        entity: u32,
        place: Place,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        if page.trail.entity != entity {
            page.trail = Trail {
                entity,
                graphs: Vec::new(),
            };
        }
        let open = page.trail.graphs.last().copied().unwrap_or(entity);
        let edited = |graph: u32| {
            edits
                .ability_values
                .iter()
                .any(|value| value.locator.graph_tag.map(|tag| tag.get()) == Some(graph))
        };
        // The trail: the ability, then each graph opened below it.
        let mut back = None;
        ui.horizontal_wrapped(|ui| {
            // The trail's root reads as the place, and links back only from below it.
            if page.trail.graphs.is_empty() {
                ui.label(egui::RichText::new("Ability").strong());
            } else if ui.link("Ability").clicked() {
                back = Some(0);
            }
            let mut parent = entity;
            for (depth, graph) in page.trail.graphs.iter().enumerate() {
                ui.label(quiet(ui, "›"));
                let name = page
                    .values
                    .spawn_name(parent, *graph)
                    .map_or_else(|| format!("Graph 0x{graph:08X}"), str::to_owned);
                if depth + 1 == page.trail.graphs.len() {
                    ui.label(egui::RichText::new(name).strong());
                } else if ui.link(name).clicked() {
                    back = Some(depth + 1);
                }
                parent = *graph;
            }
        });
        if let Some(depth) = back {
            page.trail.graphs.truncate(depth);
            return None;
        }
        let Some(loaded) = page.values.poll(ui.ctx(), &self.packages, open) else {
            ui.weak("Loading…");
            return None;
        };
        let loaded = match loaded {
            Ok(loaded) => loaded,
            Err(error) => {
                ui.colored_label(ui.visuals().warn_fg_color, "Spawns unavailable.")
                    .on_hover_text(error);
                return None;
            }
        };
        let mut changed = None;
        if open != entity {
            changed = self
                .draw_graph_values(
                    ui,
                    (&*loaded, entity),
                    place,
                    &edits.ability_values,
                    &mut page.values,
                )
                .map(|ability_values| EntryEdits {
                    ability_values,
                    ..edits.clone()
                });
            ui.add_space(6.0);
        }
        if page.trail.graphs.len() >= SPAWN_DEPTH {
            return changed;
        }
        if loaded.spawns.is_empty() {
            ui.weak("Spawns nothing.");
            return changed;
        }
        ui.label(quiet(ui, "Spawns"));
        let mut opened = None;
        for (graph, name) in &loaded.spawns {
            let (label, hover) = if edited(*graph) {
                (format!("{name} •"), format!("Changed · 0x{graph:08X}"))
            } else {
                (name.clone(), format!("0x{graph:08X}"))
            };
            if style::list_row(ui, false, &label)
                .on_hover_text(hover)
                .clicked()
            {
                opened = Some(*graph);
            }
        }
        if let Some(graph) = opened {
            page.trail.graphs.push(graph);
        }
        changed
    }
}
