//! Component editing, including values reached through linked native records.
use super::*;
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimePathElement, decode_weapon_runtime_field_value, encode_weapon_runtime_field_value,
};

mod records;
#[cfg(test)]
mod tests;
use records::{Drawn, Entry, Layout};

impl PerkEditor {
    /// Source-named values use the same checked editing path as the complete native view.
    /// Paired movement controls already occupy the main form and are not repeated here.
    pub(super) fn draw_component_properties(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) -> bool {
        let groups = component_groups(loaded);
        if groups.is_empty() {
            return false;
        }
        ui.add_space(10.0);
        ui.strong("Component Properties");
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 58.0).max(100.0);
            ui.add_sized(
                [width, 26.0],
                egui::TextEdit::singleline(&mut self.property_query)
                    .hint_text("Search Component Properties"),
            );
            if ui.button("Clear").clicked() {
                self.property_query.clear();
            }
        });
        let query = self.property_query.trim().to_lowercase();
        let mut visible = false;
        for group in groups {
            let component = group.component();
            let fields = group
                .fields
                .iter()
                .filter(|field| matches_query(field, component, &query))
                .collect::<Vec<_>>();
            if fields.is_empty() {
                continue;
            }
            visible = true;
            egui::CollapsingHeader::new(group.title())
                .id_salt((
                    "component-properties",
                    group.tag,
                    group.owner,
                    group.root.kind,
                    group.root.owner_offset,
                ))
                .default_open(true)
                .open((!query.is_empty()).then_some(true))
                .show(ui, |ui| {
                    let range = page(ui, "Properties", fields.len());
                    let entries = fields[range]
                        .iter()
                        .map(|field| Entry { group: 0, field })
                        .collect::<Vec<_>>();
                    records::arrange(&entries, |_| group.source()).draw(ui, |ui, entry, layout| {
                        self.draw_native_value(
                            ui,
                            loaded,
                            entry.field,
                            carrier(group.graph, group.owner, entry.field),
                            None,
                            layout,
                        )
                    });
                });
        }
        if !visible {
            ui.label("No matching component properties.");
        }
        true
    }

    pub(super) fn draw_native_fields(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        query: &str,
    ) -> usize {
        let mut occurrences = BTreeMap::new();
        for field in loaded.graphs.iter().flat_map(|(_, graph)| graph.fields()) {
            *occurrences.entry(&field.locator).or_insert(0usize) += 1;
        }
        let mut visible = 0;
        for (tag, graph) in &loaded.graphs {
            let roots = graph
                .resources
                .iter()
                .flat_map(|resource| {
                    std::iter::once(&resource.instance)
                        .chain(resource.definition.iter())
                        .map(move |root| {
                            (resource.owner_tag, resource.binding_label.as_str(), root)
                        })
                })
                .chain(graph.owners.iter().flat_map(|owner| {
                    owner
                        .roots
                        .iter()
                        .map(move |root| (owner.owner_tag, "Shared Component", root))
                }));
            for (owner, binding, root) in roots {
                let fields = root
                    .fields
                    .iter()
                    .filter(|field| {
                        field.source != WeaponRuntimeFieldSource::OpaqueNativeType
                            && occurrences[&field.locator] == 1
                            && matches_query(field, binding, query)
                    })
                    .collect::<Vec<_>>();
                if fields.is_empty() {
                    continue;
                }
                visible += fields.len();
                let asset = if loaded.graphs.len() > 1 {
                    format!("Asset 0x{tag:08X} / ")
                } else {
                    String::new()
                };
                let title = format!(
                    "{asset}{binding} / {} / {} Fields",
                    root.kind.label(),
                    fields.len()
                );
                egui::CollapsingHeader::new(title)
                    .id_salt((
                        "editable-component",
                        tag,
                        owner,
                        root.kind,
                        root.owner_offset,
                    ))
                    .open((!query.is_empty()).then_some(true))
                    .show(ui, |ui| {
                        self.draw_native_records(
                            ui,
                            loaded,
                            graph,
                            owner,
                            &fields,
                            !query.is_empty(),
                        )
                    });
            }
        }
        visible
    }

    fn draw_native_records(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        graph: &WeaponRuntimeGraph,
        owner: u32,
        fields: &[&WeaponRuntimeField],
        expanded: bool,
    ) {
        let mut records = BTreeMap::new();
        for field in fields {
            let mut path = field.locator.path.clone();
            path.pop();
            records.entry(path).or_insert_with(Vec::new).push(*field);
        }
        let multiple = records.len() > 1;
        let range = page(ui, "Records", records.len());
        for (path, fields) in records.into_iter().skip(range.start).take(range.len()) {
            if multiple {
                let title = record_label(&path, fields[0].locator.type_handle);
                egui::CollapsingHeader::new(title)
                    .id_salt(&path)
                    .open(expanded.then_some(true))
                    .show(ui, |ui| {
                        self.draw_native_values(ui, loaded, graph, owner, &fields)
                    });
            } else {
                self.draw_native_values(ui, loaded, graph, owner, &fields);
            }
        }
    }

    fn draw_native_values(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        graph: &WeaponRuntimeGraph,
        owner: u32,
        fields: &[&WeaponRuntimeField],
    ) {
        let parameters = projectile::parameters::discover(graph);
        for field in &fields[page(ui, "Fields", fields.len())] {
            let carrier = carrier(graph, owner, field);
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.targets_field(owner, field));
            self.draw_native_value(ui, loaded, field, carrier, parameter, Layout::Row);
        }
    }

    /// Draws a value, or only reads it for `Layout::Peek`, and reports it.
    fn draw_native_value(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        field: &WeaponRuntimeField,
        carrier: Option<&WeaponRuntimeField>,
        parameter: Option<&projectile::parameters::Parameter>,
        layout: Layout,
    ) -> Drawn {
        if !matches!(layout, Layout::Peek) {
            let edited = edit_value(
                ui,
                loaded,
                field,
                carrier,
                parameter,
                layout,
                &mut self.draft,
                &mut self.value_text,
            );
            if let Some(result) = edited {
                self.parameter_error = result.err();
            }
        }
        Drawn::of(
            current_value(loaded, field, carrier, &self.draft).ok(),
            &field.value,
        )
    }
}

/// One value's control, with a change written through the checked writer. A tile or a cell
/// draws the named value, and a row draws the complete native field. `Some` carries the error
/// state the edit leaves, and `None` means nothing was attempted.
#[allow(clippy::too_many_arguments)]
fn edit_value(
    ui: &mut egui::Ui,
    loaded: &PrivatePerkRuntimeGraph,
    field: &WeaponRuntimeField,
    carrier: Option<&WeaponRuntimeField>,
    parameter: Option<&projectile::parameters::Parameter>,
    layout: Layout,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) -> Option<Result<(), String>> {
    let effective = current_value(loaded, field, carrier, draft);
    let mut edits = Vec::new();
    match effective {
        Ok(value) if value != field.value => edits.push(WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value,
        }),
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            let target = carrier
                .filter(|carrier| saved(loaded, carrier, draft).ok().flatten().is_some())
                .unwrap_or(field);
            let label = if std::ptr::eq(target, field) {
                "Remove Field Edit"
            } else {
                "Remove Byte Range Edit"
            };
            if ui.small_button(label).clicked() {
                draft.retain(|edit| !guided::equivalent(loaded, &target.locator, &edit.locator));
                text.retain(|(locator, _), _| {
                    !guided::equivalent(loaded, &target.locator, locator)
                });
                return Some(Ok(()));
            }
            return None;
        }
        _ => {}
    }
    let previous = edits.clone();
    let had_text = text.keys().any(|(locator, _)| locator == &field.locator);
    match layout {
        Layout::Tile(width, choices) => {
            draw_property_value(ui, width, field, &mut edits, text, choices);
        }
        Layout::Cell {
            width,
            reset,
            choices,
        } => draw_property_cell(ui, width, reset, choices, field, &mut edits, text),
        Layout::Peek => {}
        Layout::Row => {
            ui.push_id(&field.locator, |ui| {
                draw_runtime_value_override_field(ui, field, &mut edits, text);
            });
        }
    }
    if edits == previous {
        let cleared = had_text && !text.keys().any(|(locator, _)| locator == &field.locator);
        return cleared.then_some(Ok(()));
    }
    let value = edits.first().map(|edit| &edit.value);
    if value.is_some_and(|value| !validation::finite(value)) {
        return Some(Err(
            "Enter a finite value before applying this field.".into()
        ));
    }
    let result = if let Some(parameter) = parameter {
        write_parameter(parameter, draft, value)
    } else {
        write_value(loaded, field, carrier, draft, value.unwrap_or(&field.value))
    };
    text.retain(|(locator, _), _| {
        locator != &field.locator
            && carrier.is_none_or(|carrier| locator != &carrier.locator)
            && parameter.is_none_or(|parameter| !parameter.contains(locator))
    });
    Some(result)
}

/// An asset's named values on its effect card, so none of them needs a trip to Edit
/// Components. Proven values lead, and the rest wait one click away under More Properties,
/// by component. Returns the error state the last edit leaves, and whether the fold is open.
pub(in crate::app::custom_perks) fn draw_card_values(
    ui: &mut egui::Ui,
    loaded: &PrivatePerkRuntimeGraph,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) -> (Option<Result<(), String>>, bool) {
    let groups = component_groups(loaded);
    let mut result = None;
    let entries = groups
        .iter()
        .enumerate()
        .flat_map(|(group, found)| {
            found
                .fields
                .iter()
                .filter(|field| proven(field))
                .map(move |field| Entry { group, field })
        })
        .collect::<Vec<_>>();
    records::arrange(&entries, |group| groups[group].source()).draw(ui, |ui, entry, layout| {
        let group = &groups[entry.group];
        let carrier = carrier(group.graph, group.owner, entry.field);
        if !matches!(layout, Layout::Peek) {
            result = edit_value(ui, loaded, entry.field, carrier, None, layout, draft, text)
                .or(result.take());
        }
        Drawn::of(
            current_value(loaded, entry.field, carrier, draft).ok(),
            &entry.field.value,
        )
    });
    let more = groups
        .iter()
        .flat_map(|group| &group.fields)
        .filter(|field| !proven(field))
        .count();
    if more == 0 {
        return (result, false);
    }
    let fold = egui::CollapsingHeader::new(format!("More Properties ({more})"))
        .id_salt("more-properties")
        .show(ui, |ui| {
            for group in &groups {
                let fields = group
                    .fields
                    .iter()
                    .filter(|field| !proven(field))
                    .collect::<Vec<_>>();
                if fields.is_empty() {
                    continue;
                }
                ui.push_id((group.tag, group.owner, group.root.owner_offset), |ui| {
                    ui.add_space(4.0);
                    ui.strong(group.title());
                    let range = page(ui, "Properties", fields.len());
                    let entries = fields[range]
                        .iter()
                        .map(|field| Entry { group: 0, field })
                        .collect::<Vec<_>>();
                    records::arrange(&entries, |_| group.source()).draw(ui, |ui, entry, layout| {
                        let carrier = carrier(group.graph, group.owner, entry.field);
                        if !matches!(layout, Layout::Peek) {
                            result = edit_value(
                                ui,
                                loaded,
                                entry.field,
                                carrier,
                                None,
                                layout,
                                draft,
                                text,
                            )
                            .or(result.take());
                        }
                        Drawn::of(
                            current_value(loaded, entry.field, carrier, draft).ok(),
                            &entry.field.value,
                        )
                    });
                });
            }
        });
    (result, fold.body_returned.is_some())
}

/// One component root's source-named values.
struct Group<'a> {
    tag: u32,
    graph: &'a WeaponRuntimeGraph,
    owner: u32,
    binding: &'a str,
    root: &'a sundial::package_authoring::weapon_runtime::WeaponRuntimeRoot,
    fields: Vec<&'a WeaponRuntimeField>,
}

impl Group<'_> {
    /// The component's native type name, or the binding that reaches it.
    fn component(&self) -> &str {
        sundial::package_authoring::weapon_runtime::native_type_name(self.root.schema)
            .unwrap_or(self.binding)
    }

    /// Which of the component's roots holds the values.
    fn root_label(&self) -> &'static str {
        match self.root.kind {
            sundial::package_authoring::weapon_runtime::WeaponRuntimeRootKind::ComponentInstance => {
                "Initial Values"
            }
            _ => "Configuration",
        }
    }

    fn title(&self) -> String {
        format!("{} · {}", self.component(), self.root_label())
    }

    /// Where a table row's values live.
    fn source(&self) -> records::Source {
        records::Source {
            component: self.component().to_owned(),
            root: self.root_label(),
        }
    }
}

/// Source-named values by component root. Paired movement controls already occupy the main
/// form and are not repeated.
fn component_groups(loaded: &PrivatePerkRuntimeGraph) -> Vec<Group<'_>> {
    let mut occurrences = BTreeMap::new();
    for field in loaded.graphs.iter().flat_map(|(_, graph)| graph.fields()) {
        *occurrences.entry(&field.locator).or_insert(0usize) += 1;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut groups = Vec::new();
    for (tag, graph) in &loaded.graphs {
        let movement = projectile::parameters::discover(graph);
        let roots = graph
            .resources
            .iter()
            .flat_map(|resource| {
                std::iter::once(&resource.instance)
                    .chain(resource.definition.iter())
                    .map(move |root| (resource.owner_tag, resource.binding_label.as_str(), root))
            })
            .chain(graph.owners.iter().flat_map(|owner| {
                owner
                    .roots
                    .iter()
                    .map(move |root| (owner.owner_tag, "Shared Component", root))
            }));
        for (owner, binding, root) in roots {
            // Reflection can name a field without identifying its component's role.
            // Keep those records in Advanced, using the same checked writer.
            if sundial::package_authoring::weapon_runtime::native_type_name(root.schema).is_none()
                && (binding.starts_with("Binding 0x") || binding == "Shared Component")
            {
                continue;
            }
            let fields = root
                .fields
                .iter()
                .filter(|field| {
                    named_property(field)
                        && occurrences[&field.locator] == 1
                        && !movement
                            .iter()
                            .any(|parameter| parameter.targets_field(owner, field))
                        && seen.insert((*tag, owner, field.owner_offset, field.locator.byte_size))
                })
                .collect::<Vec<_>>();
            if !fields.is_empty() {
                groups.push(Group {
                    tag: *tag,
                    graph,
                    owner,
                    binding,
                    root,
                    fields,
                });
            }
        }
    }
    groups
}

/// The named view shares the writer and validation with Advanced Fields, while
/// showing the value in its natural representation instead of its stored bits.
fn draw_property_value(
    ui: &mut egui::Ui,
    width: f32,
    field: &WeaponRuntimeField,
    edits: &mut Vec<WeaponRuntimeValueOverride>,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    choices: Option<&'static [(i64, &'static str)]>,
) {
    let current = edits.first().map_or(&field.value, |edit| &edit.value);
    // Only a value that differs from the original is modified. A 64-bit field keeps its text in
    // `text` from the first frame, so that alone would mark every one of them.
    let modified = !edits.is_empty();
    let (next, reset) = crate::app::style::tile(
        ui,
        width,
        &field.locator,
        &field.name,
        &property_hint(field),
        modified,
        |ui| property_control(ui, field, current, text, choices),
    );
    apply_property(field, edits, text, next, reset);
}

/// A value as a table cell. Its row resets it, since the column names it.
fn draw_property_cell(
    ui: &mut egui::Ui,
    width: f32,
    reset: bool,
    choices: Option<&'static [(i64, &'static str)]>,
    field: &WeaponRuntimeField,
    edits: &mut Vec<WeaponRuntimeValueOverride>,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) {
    let current = edits.first().map_or(&field.value, |edit| &edit.value);
    let next = ui
        .push_id(&field.locator, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    property_control(ui, field, current, text, choices)
                },
            )
        })
        .inner;
    next.response.on_hover_text(property_hint(field));
    apply_property(field, edits, text, next.inner, reset);
}

/// Where a value lives, what it started as, and what it does when that is known.
fn property_hint(field: &WeaponRuntimeField) -> String {
    let mut hint = format!(
        "{}\nOriginal: {}",
        field.path_label,
        value_text(&field.value)
    );
    if let Some(help) = sundial::package_authoring::weapon_runtime::presentation::field_help(field)
    {
        hint.push('\n');
        hint.push_str(help);
    }
    hint
}

/// Writes a new value or a reset into the value's pending edits.
fn apply_property(
    field: &WeaponRuntimeField,
    edits: &mut Vec<WeaponRuntimeValueOverride>,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    next: Option<WeaponRuntimeValue>,
    reset: bool,
) {
    if reset {
        edits.clear();
        text.retain(|(locator, _), _| locator != &field.locator);
    } else if let Some(value) = next {
        edits.clear();
        edits.push(WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value,
        });
        text.retain(|(locator, _), _| locator != &field.locator);
    }
}

/// A value's control, filling the width it is given. Returns a changed value.
fn property_control(
    ui: &mut egui::Ui,
    field: &WeaponRuntimeField,
    current: &WeaponRuntimeValue,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    choices: Option<&'static [(i64, &'static str)]>,
) -> Option<WeaponRuntimeValue> {
    let size = [ui.available_width(), ui.spacing().interact_size.y];
    match current {
        WeaponRuntimeValue::Float32Bits(bits) if f32::from_bits(*bits).is_finite() => {
            let mut value = f32::from_bits(*bits);
            let response = ui.add_sized(size, egui::DragValue::new(&mut value).speed(0.01));
            let changed = response.changed();
            crate::app::style::named_control(response, &field.name);
            changed.then(|| WeaponRuntimeValue::Float32Bits(value.to_bits()))
        }
        WeaponRuntimeValue::Float64Bits(bits) if f64::from_bits(*bits).is_finite() => {
            let mut value = f64::from_bits(*bits);
            let response = ui.add_sized(size, egui::DragValue::new(&mut value).speed(0.01));
            let changed = response.changed();
            crate::app::style::named_control(response, &field.name);
            changed.then(|| WeaponRuntimeValue::Float64Bits(value.to_bits()))
        }
        _ => {
            ui.horizontal_wrapped(|ui| {
                // Integers fill the tile as the numbers do, beside their hex reading.
                let room = if matches!(current, WeaponRuntimeValue::Unsigned(_)) {
                    60.0
                } else {
                    0.0
                };
                ui.spacing_mut().interact_size.x = (ui.available_width() - room).max(40.0);
                ui.spacing_mut().combo_width = ui.available_width();
                crate::app::runtime_fields::draw_runtime_value_editor_with(
                    ui,
                    &field.locator,
                    &field.kind,
                    current,
                    text,
                    choices,
                )
            })
            .inner
        }
    }
}

pub(in crate::app::custom_perks) fn named_property(field: &WeaponRuntimeField) -> bool {
    !field.name_inferred
        && field.source != WeaponRuntimeFieldSource::OpaqueNativeType
        && !matches!(field.value, WeaponRuntimeValue::Bytes(_))
        // Native references and keys belong with the complete structural view,
        // even when reflection supplies a name such as m_elements.
        && !matches!(field.kind, WeaponRuntimeValueKind::HexIdentifier { .. })
        && !field.name.starts_with("Unnamed ")
        && !field.name.starts_with("Member 0x")
        && !field.name.starts_with("Field 0x")
        && !matches!(
            field.name.as_str(),
            "Array Element Count"
                | "Header Offset"
                | "Attachment Designator"
                | "Curve Distance Scale"
                | "Curve Travel Distance"
                | "Distance Curve Enabled"
        )
        && !field.name.starts_with("Padding")
        && !field.name.starts_with("Reserved ")
}

#[cfg(test)]
fn property_changes(
    loaded: &PrivatePerkRuntimeGraph,
    draft: &[WeaponRuntimeValueOverride],
) -> Result<Vec<String>, String> {
    changes(loaded, draft, &std::collections::BTreeSet::new())
}

/// The changes an effect card does not already show as rows. `named` says every named value
/// is in view, not only the proven ones.
pub(in crate::app::custom_perks) fn unlisted_changes(
    loaded: &PrivatePerkRuntimeGraph,
    draft: &[WeaponRuntimeValueOverride],
    named: bool,
) -> Result<Vec<String>, String> {
    let listed = component_groups(loaded)
        .into_iter()
        .flat_map(|group| group.fields)
        .filter(|field| named || proven(field))
        .map(|field| &field.locator)
        .collect();
    changes(loaded, draft, &listed)
}

/// Read the effective edits through the same adapters as the controls. In particular,
/// changing one value inside a byte carrier does not report every lane as modified.
fn changes(
    loaded: &PrivatePerkRuntimeGraph,
    draft: &[WeaponRuntimeValueOverride],
    listed: &std::collections::BTreeSet<&WeaponRuntimeFieldLocator>,
) -> Result<Vec<String>, String> {
    for edit in draft {
        if validation::fields_for(loaded, &edit.locator).len() != 1 {
            return Err("Open Edit Components… to resolve an ambiguous edit.".into());
        }
    }
    let mut lines = Vec::new();
    let mut covered =
        BTreeMap::<WeaponRuntimeFieldLocator, std::collections::BTreeSet<usize>>::new();
    for (_, graph) in &loaded.graphs {
        let parameters = projectile::parameters::discover(graph);
        for parameter in &parameters {
            if parameter.is_modified(draft) {
                lines.push(format!(
                    "{}: {}{}",
                    parameter.kind.label(),
                    parameter.value(draft)?,
                    parameter.kind.suffix()
                ));
            }
        }
        let roots = graph
            .resources
            .iter()
            .flat_map(|resource| {
                std::iter::once(&resource.instance)
                    .chain(resource.definition.iter())
                    .map(move |root| (resource.owner_tag, root))
            })
            .chain(
                graph
                    .owners
                    .iter()
                    .flat_map(|owner| owner.roots.iter().map(move |root| (owner.owner_tag, root))),
            );
        for (owner, root) in roots {
            for field in root.fields.iter().filter(|field| {
                field.source != WeaponRuntimeFieldSource::OpaqueNativeType
                    && !matches!(field.value, WeaponRuntimeValue::Bytes(_))
            }) {
                let carrier = carrier(graph, owner, field);
                let value = current_value(loaded, field, carrier, draft)?;
                if value == field.value {
                    continue;
                }
                if let Some(carrier) = carrier {
                    let start = (field.owner_offset - carrier.owner_offset) as usize;
                    for edit in draft
                        .iter()
                        .filter(|edit| guided::equivalent(loaded, &carrier.locator, &edit.locator))
                    {
                        covered
                            .entry(edit.locator.clone())
                            .or_default()
                            .extend(start..start + field.locator.byte_size as usize);
                    }
                }
                if parameters
                    .iter()
                    .any(|parameter| parameter.targets_field(owner, field))
                    || listed.contains(&field.locator)
                {
                    continue;
                }
                let name = if named_property(field) {
                    field.name.clone()
                } else {
                    format!(
                        "Type 0x{:08X} +0x{:X}",
                        field.locator.type_handle, field.locator.value_offset
                    )
                };
                let line = format!("{name}: {}", value_text(&value));
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
        }
        append_unmapped_changes(graph, loaded, draft, &covered, &mut lines);
    }
    Ok(lines)
}

fn append_unmapped_changes(
    graph: &WeaponRuntimeGraph,
    loaded: &PrivatePerkRuntimeGraph,
    draft: &[WeaponRuntimeValueOverride],
    covered: &BTreeMap<WeaponRuntimeFieldLocator, std::collections::BTreeSet<usize>>,
    lines: &mut Vec<String>,
) {
    for edit in draft {
        let WeaponRuntimeValue::Bytes(bytes) = &edit.value else {
            continue;
        };
        let Some(field) = graph
            .fields()
            .find(|field| guided::equivalent(loaded, &field.locator, &edit.locator))
        else {
            continue;
        };
        let WeaponRuntimeValue::Bytes(original) = &field.value else {
            continue;
        };
        let unmapped = bytes
            .iter()
            .zip(original)
            .enumerate()
            .filter(|(offset, (after, before))| {
                after != before
                    && !covered
                        .get(&edit.locator)
                        .is_some_and(|offsets| offsets.contains(offset))
            })
            .map(|(offset, (value, _))| format!("+0x{offset:X}={value:02X}"))
            .collect::<Vec<_>>();
        if !unmapped.is_empty() {
            lines.push(format!("Native Bytes: {}", unmapped.join(", ")));
        }
    }
}

use sundial::package_authoring::weapon_runtime::presentation::{
    proven, summary_value as value_text,
};

fn write_parameter(
    parameter: &projectile::parameters::Parameter,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    value: Option<&WeaponRuntimeValue>,
) -> Result<(), String> {
    match value {
        Some(WeaponRuntimeValue::Float32Bits(bits)) => parameter.set(draft, f32::from_bits(*bits)),
        None => parameter.reset(draft),
        _ => Err("This parameter requires a floating-point value.".into()),
    }
}

fn page(ui: &mut egui::Ui, label: &'static str, count: usize) -> std::ops::Range<usize> {
    const SIZE: usize = 64;
    if count <= SIZE {
        return 0..count;
    }
    let id = ui.make_persistent_id(("component-page", label));
    let mut index = ui
        .data(|data| data.get_temp::<usize>(id))
        .unwrap_or(0)
        .min((count - 1) / SIZE);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(index > 0, egui::Button::new("Previous"))
            .clicked()
        {
            index -= 1;
        }
        if ui
            .add_enabled((index + 1) * SIZE < count, egui::Button::new("Next"))
            .clicked()
        {
            index += 1;
        }
        ui.label(format!(
            "{label} {} to {} of {count}",
            index * SIZE + 1,
            ((index + 1) * SIZE).min(count)
        ));
    });
    ui.data_mut(|data| data.insert_temp(id, index));
    index * SIZE..((index + 1) * SIZE).min(count)
}

fn matches_query(field: &WeaponRuntimeField, binding: &str, query: &str) -> bool {
    query.is_empty()
        || format!(
            "{binding} {} {} {} 0x{:08X}",
            field.name,
            field.path_label,
            runtime_value_kind_label(&field.kind),
            field.locator.type_handle
        )
        .to_lowercase()
        .contains(query)
}

/// Typed controls inside legacy byte ranges keep the same saved source as convenience controls.
fn carrier<'a>(
    graph: &'a WeaponRuntimeGraph,
    owner: u32,
    field: &WeaponRuntimeField,
) -> Option<&'a WeaponRuntimeField> {
    if field.source != WeaponRuntimeFieldSource::NativeDeclaration {
        return None;
    }
    let roots = graph
        .resources
        .iter()
        .filter(|r| r.owner_tag == owner)
        .flat_map(|r| std::iter::once(&r.instance).chain(r.definition.iter()))
        .chain(
            graph
                .owners
                .iter()
                .filter(|o| o.owner_tag == owner)
                .flat_map(|o| &o.roots),
        );
    let mut candidates = roots.flat_map(|root| &root.fields).filter(|other| {
        other.source == WeaponRuntimeFieldSource::OpaqueNativeType
            && other.owner_offset <= field.owner_offset
            && u64::from(other.owner_offset) + u64::from(other.locator.byte_size)
                >= u64::from(field.owner_offset) + u64::from(field.locator.byte_size)
    });
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

fn saved<'a>(
    loaded: &PrivatePerkRuntimeGraph,
    field: &WeaponRuntimeField,
    draft: &'a [WeaponRuntimeValueOverride],
) -> Result<Option<&'a WeaponRuntimeValueOverride>, String> {
    let mut matching = draft
        .iter()
        .filter(|edit| guided::equivalent(loaded, &field.locator, &edit.locator));
    let first = matching.next();
    if matching.next().is_some() {
        return Err(format!("{} has duplicate saved edits.", field.name));
    }
    Ok(first)
}

fn current_value(
    loaded: &PrivatePerkRuntimeGraph,
    field: &WeaponRuntimeField,
    carrier: Option<&WeaponRuntimeField>,
    draft: &[WeaponRuntimeValueOverride],
) -> Result<WeaponRuntimeValue, String> {
    let direct = saved(loaded, field, draft)?;
    let Some(carrier) = carrier else {
        return Ok(direct.map_or(&field.value, |edit| &edit.value).clone());
    };
    let carried = saved(loaded, carrier, draft)?;
    if direct.is_some() && carried.is_some() {
        return Err(format!(
            "{} overlaps a saved byte edit. Remove one edit before continuing.",
            field.name
        ));
    }
    if let Some(direct) = direct {
        return Ok(direct.value.clone());
    }
    let bytes = encode_weapon_runtime_field_value(
        carrier,
        carried.map_or(&carrier.value, |edit| &edit.value),
    )?;
    let start = (field.owner_offset - carrier.owner_offset) as usize;
    decode_weapon_runtime_field_value(
        field,
        bytes
            .get(start..start + field.locator.byte_size as usize)
            .ok_or("Saved component data is truncated.")?,
    )
}

fn write_value(
    loaded: &PrivatePerkRuntimeGraph,
    field: &WeaponRuntimeField,
    carrier: Option<&WeaponRuntimeField>,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    value: &WeaponRuntimeValue,
) -> Result<(), String> {
    current_value(loaded, field, carrier, draft)?;
    let storage = carrier.unwrap_or(field);
    let value = if let Some(carrier) = carrier {
        let old = saved(loaded, carrier, draft)?.map_or(&carrier.value, |edit| &edit.value);
        let mut bytes = encode_weapon_runtime_field_value(carrier, old)?;
        let replacement = encode_weapon_runtime_field_value(field, value)?;
        let start = (field.owner_offset - carrier.owner_offset) as usize;
        bytes
            .get_mut(start..start + replacement.len())
            .ok_or("Saved component data is truncated.")?
            .copy_from_slice(&replacement);
        decode_weapon_runtime_field_value(carrier, &bytes)?
    } else {
        value.clone()
    };
    draft.retain(|edit| {
        !guided::equivalent(loaded, &storage.locator, &edit.locator)
            && !guided::equivalent(loaded, &field.locator, &edit.locator)
    });
    if value != storage.value {
        draft.push(WeaponRuntimeValueOverride {
            locator: storage.locator.clone(),
            value,
        });
    }
    Ok(())
}

fn record_label(path: &[WeaponRuntimePathElement], schema: u32) -> String {
    let route = path
        .iter()
        .skip(1)
        .map(|step| match step.name_hash {
            0x504E_4100 => format!("Entry {}", u64::from(step.byte_offset) + 1),
            0x504E_5000 => format!("Link +0x{:X}", step.byte_offset),
            _ => format!("Record +0x{:X}", step.byte_offset),
        })
        .collect::<Vec<_>>()
        .join(" / ");
    if route.is_empty() {
        format!("Type 0x{schema:08X}")
    } else {
        format!("{route} / Type 0x{schema:08X}")
    }
}
