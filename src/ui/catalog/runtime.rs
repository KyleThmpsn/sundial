//! Shared read-only presentation for weapon and perk-owned runtime graphs.

use eframe::egui;

use crate::weapon_runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimeRoot,
};

#[derive(Debug, Default)]
pub struct RuntimeViewOptions {
    pub query: String,
    pub show_opaque: bool,
}

pub fn draw_graph(ui: &mut egui::Ui, graph: &WeaponRuntimeGraph, options: &mut RuntimeViewOptions) {
    ui.push_id(("runtime-graph", graph.entity_tag), |ui| {
        let opaque = graph.fields().filter(|field| is_opaque(field)).count();
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut options.query)
                .hint_text("Find a field, component, schema, or hash")
                .desired_width(300.0));
            ui.weak(format!("{} named · {opaque} opaque", graph.field_count() - opaque));
            if ui.button("Copy All Fields").on_hover_text("Copies every decoded field and opaque range, independent of the current filter. Package data only, not live runtime state.").clicked() {
                let data = serde_json::json!({
                    "source": "installed_package_graph_not_live_state",
                    "item_hash": graph.item_hash,
                    "pattern_global_id_hash": graph.pattern_global_id_hash,
                    "entity_tag": graph.entity_tag,
                    "fields": graph.fields().map(export_field).collect::<Vec<_>>(),
                });
                match serde_json::to_string_pretty(&data) {
                    Ok(json) => ui.ctx().copy_text(json),
                    Err(error) => { ui.colored_label(ui.visuals().error_fg_color, format!("Could not encode fields: {error}")); },
                }
            }
        });
        if opaque > 0 {
            ui.checkbox(&mut options.show_opaque, "Include Opaque Byte Ranges")
                .on_hover_text("Exact package bytes whose internal gameplay meaning is not decoded.");
        }
        if options.show_opaque {
            ui.weak("Opaque ranges may include pointers or coupled state. They are not identified gameplay properties.");
        }
        egui::CollapsingHeader::new("Package Provenance").show(ui, |ui| {
            if graph.item_hash != 0 {
                ui.monospace(format!("Pattern item 0x{:08X} · runtime key 0x{:08X}", graph.item_hash, graph.pattern_global_id_hash));
            }
            ui.monospace(format!("Entity 0x{:08X} · {} bindings · {} owners", graph.entity_tag, graph.bindings.len(), graph.owners.len()));
            ui.weak("A pattern can be shared by several items. Package resolution does not prove native memory residency.");
        });
        let query = options.query.trim().to_lowercase();
        let mut visible = 0;
        for resource in &graph.resources {
            let group = format!("{} 0x{:08X} 0x{:08X} 0x{:08X}", resource.binding_label, resource.binding_hash, resource.owner_tag, resource.concrete_class);
            let roots = std::iter::once(&resource.instance).chain(resource.definition.iter()).collect::<Vec<_>>();
            let count = roots.iter().map(|root| matching_fields(root, &group, &query, options.show_opaque).len()).sum::<usize>();
            if count == 0 { continue; }
            visible += count;
            let label = if resource.resource_count > 1 {
                format!("{} · resource {} · {count} fields", resource.binding_label, resource.resource_index + 1)
            } else {
                format!("{} · {count} fields", resource.binding_label)
            };
            egui::CollapsingHeader::new(label)
                .id_salt(("resource", resource.binding_hash, resource.resource_index))
                .default_open(!query.is_empty())
                .show(ui, |ui| {
                    ui.weak(format!("Owner 0x{:08X} · class 0x{:08X}", resource.owner_tag, resource.concrete_class));
                    if !resource.alias_bindings.is_empty() {
                        ui.label(format!("Shared by {} other binding(s)", resource.alias_bindings.len()))
                            .on_hover_text(resource.alias_bindings.iter().map(|(hash, index)| format!("0x{hash:08X} · resource {}", index + 1)).collect::<Vec<_>>().join("\n"));
                    }
                    for root in roots {
                        draw_root(ui, root, &group, &query, options.show_opaque);
                    }
                });
        }
        for owner in &graph.owners {
            let label = graph.bindings.iter().find(|binding| binding.binding_hash == owner.anchor_binding_hash)
                .map_or("Unclassified component", |binding| binding.binding_label.as_str());
            let group = format!("{label} 0x{:08X} 0x{:08X}", owner.owner_tag, owner.anchor_binding_hash);
            let count = owner.roots.iter().map(|root| matching_fields(root, &group, &query, options.show_opaque).len()).sum::<usize>();
            if count == 0 { continue; }
            visible += count;
            egui::CollapsingHeader::new(format!("Shared Owner State · {label} · {count} fields"))
                .id_salt(("owner", owner.owner_tag))
                .default_open(!query.is_empty())
                .show(ui, |ui| {
                    ui.weak(format!("Owner 0x{:08X}", owner.owner_tag));
                    for root in &owner.roots {
                        draw_root(ui, root, &group, &query, options.show_opaque);
                    }
                });
        }
        if visible == 0 {
            ui.weak(if query.is_empty() { "No named runtime fields are available in this graph." } else { "No fields match this filter." });
        }
    });
}

fn draw_root(
    ui: &mut egui::Ui,
    root: &WeaponRuntimeRoot,
    group: &str,
    query: &str,
    show_opaque: bool,
) {
    let fields = matching_fields(root, group, query, show_opaque);
    if fields.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("{} Values ({})", root.kind.label(), fields.len()))
        .id_salt((root.kind, root.schema, root.owner_offset))
        .default_open(true)
        .show(ui, |ui| {
            ui.weak(format!(
                "Schema 0x{:08X} · {} bytes · {}",
                root.schema,
                root.byte_size,
                if root.generated_schema {
                    "generated schema"
                } else {
                    "native registry"
                }
            ));
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .auto_shrink([false, true])
                .id_salt("fields")
                .show_rows(ui, 24.0, fields.len(), |ui, rows| {
                    let width = ui.available_width();
                    egui::Grid::new("values")
                        .num_columns(2)
                        .striped(true)
                        .min_col_width(0.0)
                        .max_col_width(width * 0.62)
                        .show(ui, |ui| {
                            for index in rows {
                                let field = fields[index];
                                let tooltip = field_tooltip(field);
                                let name = if is_opaque(field) {
                                    format!("{} [opaque]", field.path_label)
                                } else {
                                    field.path_label.clone()
                                };
                                ui.add_sized(
                                    [width * 0.59, 24.0],
                                    egui::Label::new(name).truncate(),
                                )
                                .on_hover_text(&tooltip);
                                let text = value_text(field);
                                ui.add_sized(
                                    [width * 0.35, 24.0],
                                    egui::Label::new(egui::RichText::new(&text).monospace())
                                        .truncate(),
                                )
                                .on_hover_text(format!("{text}\n{tooltip}"))
                                .context_menu(|ui| {
                                    if ui.button("Copy Exact Value").clicked() {
                                        ui.ctx().copy_text(exact_value_text(field));
                                        ui.close_menu();
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });
        });
}

pub(crate) fn matching_fields<'a>(
    root: &'a WeaponRuntimeRoot,
    group: &str,
    query: &str,
    show_opaque: bool,
) -> Vec<&'a WeaponRuntimeField> {
    let group_matches = query.is_empty()
        || group.to_lowercase().contains(query)
        || format!("0x{:08x}", root.schema).contains(query);
    root.fields
        .iter()
        .filter(|field| {
            (show_opaque || !is_opaque(field))
                && (group_matches
                    || field.path_label.to_lowercase().contains(query)
                    || field.name.to_lowercase().contains(query)
                    || format!("0x{:08x}", field.locator.type_handle).contains(query))
        })
        .collect()
}

fn is_opaque(field: &WeaponRuntimeField) -> bool {
    field.source == WeaponRuntimeFieldSource::OpaqueNativeType
}

pub(crate) fn export_field(field: &WeaponRuntimeField) -> serde_json::Value {
    serde_json::json!({
        "locator": field.locator,
        "owner_offset": field.owner_offset,
        "name": field.name,
        "path": field.path_label,
        "kind": field.kind,
        "value": field.value,
        "display": exact_value_text(field),
        "source": match field.source {
            WeaponRuntimeFieldSource::GeneratedSchema => "generated_schema",
            WeaponRuntimeFieldSource::NativeMember => "native_member",
            WeaponRuntimeFieldSource::OpaqueNativeType => "opaque_semantics_unknown",
            WeaponRuntimeFieldSource::NativeDeclaration => "native_storage_declaration",
        },
        "generated_kind": field.generated_kind,
    })
}

use crate::weapon_runtime::presentation::field_tooltip;
pub(crate) use crate::weapon_runtime::presentation::{exact_value_text, value_text};

#[cfg(test)]
mod tests;
