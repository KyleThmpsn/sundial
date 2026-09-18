//! Component editing, including values reached through linked native records.
use super::super::workbench::Workbench;
use super::*;
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimePathElement, decode_weapon_runtime_field_value, encode_weapon_runtime_field_value,
};

#[cfg(test)]
mod tests;

impl PerkEditor {
    /// Source-named values use the same checked editing path as the complete native view.
    /// Paired movement controls already occupy the main form and are not repeated here.
    pub(super) fn draw_component_properties(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) -> bool {
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
                // Reflection can name a field without identifying its component's role.
                // Keep those records in Advanced, using the same checked writer.
                if sundial::package_authoring::weapon_runtime::native_type_name(root.schema)
                    .is_none()
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
                            && seen.insert((
                                *tag,
                                owner,
                                field.owner_offset,
                                field.locator.byte_size,
                            ))
                    })
                    .collect::<Vec<_>>();
                if !fields.is_empty() {
                    groups.push((*tag, graph, owner, binding, root, fields));
                }
            }
        }
        if groups.is_empty() {
            return false;
        }
        ui.add_space(10.0);
        ui.strong("Component Properties");
        let query_id = ui.make_persistent_id("component-property-search");
        let mut query = ui.data(|data| data.get_temp::<String>(query_id).unwrap_or_default());
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 58.0).max(100.0);
            ui.add_sized(
                [width, 26.0],
                egui::TextEdit::singleline(&mut query).hint_text("Search Component Properties"),
            );
            if ui.button("Clear").clicked() {
                query.clear();
            }
        });
        ui.data_mut(|data| data.insert_temp(query_id, query.clone()));
        let query = query.trim().to_lowercase();
        let mut visible = false;
        for (tag, graph, owner, binding, root, fields) in groups {
            let root_name = match root.kind {
                sundial::package_authoring::weapon_runtime::WeaponRuntimeRootKind::ComponentInstance => "Initial Values",
                _ => "Configuration",
            };
            let binding = sundial::package_authoring::weapon_runtime::native_type_name(root.schema)
                .unwrap_or(binding);
            let fields = fields
                .into_iter()
                .filter(|field| matches_query(field, binding, &query))
                .collect::<Vec<_>>();
            if fields.is_empty() {
                continue;
            }
            visible = true;
            let title = format!("{binding} · {root_name}");
            egui::CollapsingHeader::new(title)
                .id_salt((
                    "component-properties",
                    tag,
                    owner,
                    root.kind,
                    root.owner_offset,
                ))
                .default_open(true)
                .open((!query.is_empty()).then_some(true))
                .show(ui, |ui| {
                    for field in &fields[page(ui, "Properties", fields.len())] {
                        self.draw_native_value(
                            ui,
                            loaded,
                            field,
                            carrier(graph, owner, field),
                            None,
                            true,
                        );
                    }
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
                    .default_open(!query.is_empty())
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
                    .default_open(expanded)
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
            self.draw_native_value(ui, loaded, field, carrier, parameter, false);
        }
    }

    fn draw_native_value(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        field: &WeaponRuntimeField,
        carrier: Option<&WeaponRuntimeField>,
        parameter: Option<&projectile::parameters::Parameter>,
        compact: bool,
    ) {
        let effective = current_value(loaded, field, carrier, &self.draft);
        let mut edits = Vec::new();
        match effective {
            Ok(value) if value != field.value => edits.push(WeaponRuntimeValueOverride {
                locator: field.locator.clone(),
                value,
            }),
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                let target = carrier
                    .filter(|carrier| saved(loaded, carrier, &self.draft).ok().flatten().is_some())
                    .unwrap_or(field);
                let label = if std::ptr::eq(target, field) {
                    "Remove Field Edit"
                } else {
                    "Remove Byte Range Edit"
                };
                if ui.small_button(label).clicked() {
                    self.draft
                        .retain(|edit| !guided::equivalent(loaded, &target.locator, &edit.locator));
                    self.value_text.retain(|(locator, _), _| {
                        !guided::equivalent(loaded, &target.locator, locator)
                    });
                    self.parameter_error = None;
                }
                return;
            }
            _ => {}
        }
        let previous = edits.clone();
        let had_text = self
            .value_text
            .keys()
            .any(|(locator, _)| locator == &field.locator);
        ui.push_id(&field.locator, |ui| {
            if compact {
                draw_property_value(ui, field, &mut edits, &mut self.value_text);
            } else {
                draw_runtime_value_override_field(ui, field, &mut edits, &mut self.value_text);
            }
        });
        if edits == previous {
            if had_text
                && !self
                    .value_text
                    .keys()
                    .any(|(locator, _)| locator == &field.locator)
            {
                self.parameter_error = None;
            }
            return;
        }
        let value = edits.first().map(|edit| &edit.value);
        if value.is_some_and(|value| !validation::finite(value)) {
            self.parameter_error = Some("Enter a finite value before applying this field.".into());
            return;
        }
        let result = if let Some(parameter) = parameter {
            write_parameter(parameter, &mut self.draft, value)
        } else {
            write_value(
                loaded,
                field,
                carrier,
                &mut self.draft,
                value.unwrap_or(&field.value),
            )
        };
        self.parameter_error = result.err();
        self.value_text.retain(|(locator, _), _| {
            locator != &field.locator
                && carrier.is_none_or(|carrier| locator != &carrier.locator)
                && parameter.is_none_or(|parameter| !parameter.contains(locator))
        });
    }
}

/// The named view shares the writer and validation with Advanced Fields, while
/// showing the value in its natural representation instead of its stored bits.
fn draw_property_value(
    ui: &mut egui::Ui,
    field: &WeaponRuntimeField,
    edits: &mut Vec<WeaponRuntimeValueOverride>,
    text: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) {
    let current = edits.first().map_or(&field.value, |edit| &edit.value);
    let hint = format!(
        "{}\nOriginal: {}",
        field.path_label,
        value_text(&field.value)
    );
    let (next, reset) = Workbench::property_row(ui, &field.name, &hint, |ui| {
        let next = match current {
            WeaponRuntimeValue::Float32Bits(bits) if f32::from_bits(*bits).is_finite() => {
                let mut value = f32::from_bits(*bits);
                ui.add_sized(
                    [100.0, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut value).speed(0.01),
                )
                .changed()
                .then(|| WeaponRuntimeValue::Float32Bits(value.to_bits()))
            }
            WeaponRuntimeValue::Float64Bits(bits) if f64::from_bits(*bits).is_finite() => {
                let mut value = f64::from_bits(*bits);
                ui.add_sized(
                    [100.0, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut value).speed(0.01),
                )
                .changed()
                .then(|| WeaponRuntimeValue::Float64Bits(value.to_bits()))
            }
            _ => draw_runtime_value_editor(ui, &field.locator, &field.kind, current, text),
        };
        let modified =
            !edits.is_empty() || text.keys().any(|(locator, _)| locator == &field.locator);
        let reset = ui
            .add_enabled(modified, egui::Button::new("Reset"))
            .clicked();
        (next, reset)
    });
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

/// Read the effective edits through the same adapters as the controls. In particular,
/// changing one value inside a byte carrier does not report every lane as modified.
pub(in crate::app::custom_perks) fn property_changes(
    loaded: &PrivatePerkRuntimeGraph,
    draft: &[WeaponRuntimeValueOverride],
) -> Result<Vec<String>, String> {
    for edit in draft {
        if validation::fields_for(loaded, &edit.locator).len() != 1 {
            return Err(
                "Open Properties to resolve an unavailable or ambiguous saved edit.".into(),
            );
        }
    }
    let mut lines = Vec::new();
    let mut covered =
        BTreeMap::<WeaponRuntimeFieldLocator, std::collections::BTreeSet<usize>>::new();
    for (_, graph) in &loaded.graphs {
        let parameters = projectile::parameters::discover(graph);
        for parameter in &parameters {
            if parameter.is_modified(draft) {
                let name = match parameter.kind {
                    projectile::parameters::Kind::Speed => "Projectile Speed",
                    projectile::parameters::Kind::Gravity => "Gravity",
                    projectile::parameters::Kind::TravelDistance => "Travel Distance",
                };
                lines.push(format!(
                    "{name}: {}{}",
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

use sundial::package_authoring::weapon_runtime::presentation::summary_value as value_text;

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
