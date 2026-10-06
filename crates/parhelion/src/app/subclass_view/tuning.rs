//! An ability's bank script parameters, as tiles of its Ability card, and its raw values in a
//! closed Technical part: those of its entity and of the graphs it spawns, reached through a
//! trail of the graphs above them.
use super::modifiers::{ordered, parameter_name, parameter_value};
use super::*;
use sundial::investment::AbilityParameter;
use sundial::package_authoring::ability_bank::{
    ParameterKind, parameter_kind, parameter_label, parameter_meaning,
};

/// Levels of spawned graphs the build copies below an ability.
pub(super) const SPAWN_DEPTH: usize = crate::subclass::SPAWN_DEPTH;

/// The keys an entry's own pool applies to its own ability, whose bank rows Properties shows.
pub(super) fn own_keys(summary: Option<&SubclassSummary>, entry: u8) -> Vec<u32> {
    summary
        .and_then(|summary| {
            let row = summary.entry_rows.get(&entry)?;
            let applied = summary.entry_modifiers.get(&entry)?;
            Some(
                applied
                    .iter()
                    .filter(|(_, target)| target == row)
                    .map(|(key, _)| *key)
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default()
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

/// The parameter filter's width, beside the Ability card's title.
const PARAMETER_FILTER_WIDTH: f32 = 200.0;

/// A bank's script parameters, named ones apart from the ones with only a hash. The Ability card
/// shows the named, and Technical the rest, whose role is unknown.
pub(super) fn split_parameters(
    row: &sundial::investment::AbilityRowSummary,
) -> (Vec<&AbilityParameter>, Vec<&AbilityParameter>) {
    ordered(row)
        .into_iter()
        .partition(|parameter| parameter_label(parameter.name).is_some())
}

/// The parameter filter, where `count` parameters are enough to want one. Returns the lowercased
/// query, empty when there is none.
pub(super) fn parameter_filter(ui: &mut egui::Ui, count: usize, page: &mut PageState) -> String {
    if !crate::app::pickers::wants_filter(count) {
        return String::new();
    }
    ui.add(
        egui::TextEdit::singleline(&mut page.parameter_query)
            .hint_text(format!(
                "{} Filter",
                egui_phosphor::regular::MAGNIFYING_GLASS
            ))
            .desired_width(PARAMETER_FILTER_WIDTH),
    );
    page.parameter_query.trim().to_lowercase()
}

/// The script parameters `query` matches as tiles.
pub(super) fn parameter_tiles(
    ui: &mut egui::Ui,
    width: f32,
    (parameters, query): (&[&AbilityParameter], &str),
    edits: &mut EntryEdits,
) {
    for parameter in parameters {
        let name = parameter_name(parameter.name);
        if query.is_empty()
            || name.to_lowercase().contains(query)
            || format!("{:08x}", parameter.name).contains(query)
        {
            parameter_tile(ui, width, (parameter, &name), edits);
        }
    }
}

/// One parameter: its name over its field, what it does and its stock value in the name's
/// tooltip.
fn parameter_tile(
    ui: &mut egui::Ui,
    width: f32,
    (parameter, name): (&AbilityParameter, &str),
    edits: &mut EntryEdits,
) {
    let own = edits.parameter(parameter.name);
    let mut value = own.unwrap_or(parameter.reset);
    let kind = shown_kind(parameter.name, parameter.reset, value);
    let stock = format!("Stock {}", reading(kind, parameter.reset));
    let hint = parameter_meaning(parameter.name)
        .map_or_else(|| stock.clone(), |meaning| format!("{meaning}\n{stock}"));
    let (edited, reset) = style::tile(
        ui,
        width,
        parameter.name,
        name,
        &hint,
        own.is_some(),
        |ui| {
            // A number field fills its tile.
            ui.spacing_mut().interact_size.x = width;
            let field = style::named_control(value_field(ui, kind, &mut value), name);
            (field.changed() && value.is_finite()).then_some(value)
        },
    );
    if let Some(value) = edited {
        edits.set_parameter(parameter.name, Some(value));
    } else if reset {
        edits.set_parameter(parameter.name, None);
    }
}

/// The Raw Values trail: the graphs opened below an ability's entity, outermost first.
#[derive(Clone, Debug, Default)]
pub(super) struct Trail {
    entity: u32,
    graphs: Vec<u32>,
}

impl PackageAuthoringApp {
    /// The Technical part of an ability's Gameplay, closed until opened: its bank's parameters
    /// with only a hash, then the raw values of its entity and of the graphs it spawns. Returns
    /// the entry's edits once they change.
    pub(super) fn draw_technical(
        &self,
        ui: &mut egui::Ui,
        (summary, entry, place): (Option<&SubclassSummary>, u8, Place),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let entity = summary.and_then(|summary| summary.entry_entities.get(&entry).copied());
        let row = summary
            .and_then(|summary| summary.entry_rows.get(&entry))
            .and_then(|row| self.catalog.as_ref()?.ability_row(*row));
        let unnamed = row.map(|row| split_parameters(row).1).unwrap_or_default();
        let marked = !edits.ability_values.is_empty()
            || unnamed
                .iter()
                .any(|parameter| edits.parameter(parameter.name).is_some());
        let title = if marked { "Technical •" } else { "Technical" };
        let technical = egui::CollapsingHeader::new(title)
            .id_salt(("subclass-technical", place))
            .default_open(false)
            .show(ui, |ui| {
                let mut next = edits.clone();
                if !unnamed.is_empty() {
                    ui.label(quiet(ui, "Unnamed Parameters"));
                    style::tiles(ui, |ui, width| {
                        parameter_tiles(ui, width, (&unnamed, ""), &mut next);
                    });
                    ui.add_space(6.0);
                }
                let values = match entity {
                    Some(entity) => self.draw_spawns(ui, entity, place, edits, page),
                    None => {
                        ui.weak("No values.");
                        None
                    }
                };
                values.or_else(|| (next != *edits).then_some(next))
            });
        // A screen reader hears the edit dot as "Changed".
        if marked {
            style::named_control(technical.header_response, "Technical, Changed");
        }
        technical.body_returned.flatten()
    }

    /// Raw Values: a trail back to the ability, then the open graph's values, the ability's own
    /// at the trail's root, and the graphs it spawns in turn, each marked once its values change.
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
        let changed = self
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
