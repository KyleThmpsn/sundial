//! Shared read-only presentation for weapon and perk-owned runtime graphs.

use eframe::egui;

use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimeOwner,
    WeaponRuntimeResource, WeaponRuntimeRoot,
};

#[derive(Debug, Default)]
pub struct RuntimeViewOptions {
    pub query: String,
    pub show_opaque: bool,
    /// Why the last Copy All Fields failed, until the next copy.
    copy_error: Option<String>,
    /// The search and opaque setting the cached matches were found with.
    filter: (String, bool),
    /// Matching fields per graph for that filter.
    matches: Vec<Matches>,
}

/// Matching field indices for each root of one graph, in drawing order.
#[derive(Debug)]
struct Matches {
    /// The graph's address, entity tag and field count.
    graph: (usize, u32, usize),
    roots: Vec<Vec<usize>>,
}

impl RuntimeViewOptions {
    /// Matching field indices for each root of `graph`, found again only when the filter or
    /// the graph changes.
    fn matching(&mut self, graph: &WeaponRuntimeGraph, query: &str) -> &[Vec<usize>] {
        if self.filter.0 != query || self.filter.1 != self.show_opaque {
            self.filter = (query.to_owned(), self.show_opaque);
            self.matches.clear();
        }
        let key = (
            std::ptr::from_ref(graph) as usize,
            graph.entity_tag,
            graph.field_count(),
        );
        let position = match self.matches.iter().position(|matches| matches.graph == key) {
            Some(position) => position,
            None => {
                if self.matches.len() >= 16 {
                    self.matches.clear();
                }
                self.matches.push(Matches {
                    graph: key,
                    roots: graph_matches(graph, query, self.show_opaque),
                });
                self.matches.len() - 1
            }
        };
        &self.matches[position].roots
    }
}

pub fn draw_graph(ui: &mut egui::Ui, graph: &WeaponRuntimeGraph, options: &mut RuntimeViewOptions) {
    ui.push_id(("runtime-graph", graph.entity_tag), |ui| {
        let opaque = graph.fields().filter(|field| is_opaque(field)).count();
        ui.horizontal_wrapped(|ui| {
            super::search(
                ui,
                &mut options.query,
                false,
                300.0,
                "Search Fields, Components, Schemas or Hashes",
            );
            ui.weak(format!(
                "{} named · {opaque} opaque",
                graph.field_count() - opaque
            ));
            if ui
                .button("Copy All Fields")
                .on_hover_text("Ignores the filter")
                .clicked()
            {
                let data = serde_json::json!({
                    "source": "installed_package_graph_not_live_state",
                    "item_hash": graph.item_hash,
                    "pattern_global_id_hash": graph.pattern_global_id_hash,
                    "entity_tag": graph.entity_tag,
                    "fields": graph.fields().map(export_field).collect::<Vec<_>>(),
                });
                options.copy_error = match serde_json::to_string_pretty(&data) {
                    Ok(json) => {
                        ui.ctx().copy_text(json);
                        None
                    }
                    Err(error) => Some(format!("Could not encode fields: {error}")),
                };
            }
        });
        if let Some(error) = &options.copy_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if opaque > 0 {
            ui.checkbox(&mut options.show_opaque, "Include Opaque Byte Ranges")
                .on_hover_text(
                    "Exact package bytes whose internal gameplay meaning is not decoded.",
                );
        }
        egui::CollapsingHeader::new("Package Provenance").show(ui, |ui| {
            if graph.item_hash != 0 {
                ui.monospace(format!(
                    "Pattern item 0x{:08X} · runtime key 0x{:08X}",
                    graph.item_hash, graph.pattern_global_id_hash
                ));
            }
            ui.monospace(format!(
                "Entity 0x{:08X} · {} bindings · {} owners",
                graph.entity_tag,
                graph.bindings.len(),
                graph.owners.len()
            ));
        });
        let query = options.query.trim().to_lowercase();
        let mut matches = options.matching(graph, &query).iter().map(Vec::as_slice);
        let mut visible = 0;
        for resource in &graph.resources {
            let roots = std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(|root| (root, matches.next().unwrap_or_default()))
                .collect::<Vec<_>>();
            let count = roots.iter().map(|(_, fields)| fields.len()).sum::<usize>();
            if count == 0 {
                continue;
            }
            visible += count;
            let label = if resource.resource_count > 1 {
                format!(
                    "{} · resource {} · {count} fields",
                    resource.binding_label,
                    resource.resource_index + 1
                )
            } else {
                format!("{} · {count} fields", resource.binding_label)
            };
            egui::CollapsingHeader::new(label)
                .id_salt(("resource", resource.binding_hash, resource.resource_index))
                .default_open(!query.is_empty())
                .show(ui, |ui| {
                    ui.weak(format!(
                        "Owner 0x{:08X} · class 0x{:08X}",
                        resource.owner_tag, resource.concrete_class
                    ));
                    if !resource.alias_bindings.is_empty() {
                        ui.label(format!(
                            "Shared by {} other binding(s)",
                            resource.alias_bindings.len()
                        ))
                        .on_hover_text(
                            resource
                                .alias_bindings
                                .iter()
                                .map(|(hash, index)| {
                                    format!("0x{hash:08X} · resource {}", index + 1)
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                    for (root, fields) in roots {
                        draw_root(ui, root, fields);
                    }
                });
        }
        for owner in &graph.owners {
            let roots = owner
                .roots
                .iter()
                .map(|root| (root, matches.next().unwrap_or_default()))
                .collect::<Vec<_>>();
            let count = roots.iter().map(|(_, fields)| fields.len()).sum::<usize>();
            if count == 0 {
                continue;
            }
            visible += count;
            let label = owner_label(graph, owner);
            egui::CollapsingHeader::new(format!("Shared Owner State · {label} · {count} fields"))
                .id_salt(("owner", owner.owner_tag))
                .default_open(!query.is_empty())
                .show(ui, |ui| {
                    ui.weak(format!("Owner 0x{:08X}", owner.owner_tag));
                    for (root, fields) in roots {
                        draw_root(ui, root, fields);
                    }
                });
        }
        if visible == 0 {
            ui.weak(if query.is_empty() {
                "No Named Fields"
            } else {
                "No Matching Fields"
            });
        }
    });
}

/// The binding that names a shared owner.
fn owner_label<'a>(graph: &'a WeaponRuntimeGraph, owner: &WeaponRuntimeOwner) -> &'a str {
    graph
        .bindings
        .iter()
        .find(|binding| binding.binding_hash == owner.anchor_binding_hash)
        .map_or("Unclassified Component", |binding| {
            binding.binding_label.as_str()
        })
}

/// Matching field indices for every root of `graph`, in drawing order.
fn graph_matches(graph: &WeaponRuntimeGraph, query: &str, show_opaque: bool) -> Vec<Vec<usize>> {
    let mut roots = Vec::new();
    for resource in &graph.resources {
        let group = resource_group(resource);
        for root in std::iter::once(&resource.instance).chain(resource.definition.iter()) {
            roots.push(matching_indices(root, &group, query, show_opaque));
        }
    }
    for owner in &graph.owners {
        let group = format!(
            "{} 0x{:08X} 0x{:08X}",
            owner_label(graph, owner),
            owner.owner_tag,
            owner.anchor_binding_hash
        );
        for root in &owner.roots {
            roots.push(matching_indices(root, &group, query, show_opaque));
        }
    }
    roots
}

/// The text a resource's fields also match on: its binding, owner and class.
fn resource_group(resource: &WeaponRuntimeResource) -> String {
    format!(
        "{} 0x{:08X} 0x{:08X} 0x{:08X}",
        resource.binding_label, resource.binding_hash, resource.owner_tag, resource.concrete_class
    )
}

fn draw_root(ui: &mut egui::Ui, root: &WeaponRuntimeRoot, fields: &[usize]) {
    let fields = fields
        .iter()
        .filter_map(|index| root.fields.get(*index))
        .collect::<Vec<_>>();
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
                                    crate::ui::cut_label_within(ui, name, width * 0.59),
                                )
                                .on_hover_text(&tooltip);
                                let text = value_text(field);
                                ui.add_sized(
                                    [width * 0.35, 24.0],
                                    crate::ui::cut_label_within(
                                        ui,
                                        egui::RichText::new(&text).monospace(),
                                        width * 0.35,
                                    ),
                                )
                                .on_hover_text(format!("{text}\n{tooltip}"))
                                .context_menu(|ui| {
                                    if ui.button("Copy Exact Value").clicked() {
                                        ui.ctx().copy_text(exact_value_text(field));
                                        ui.close();
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });
        });
}

/// Indices of the fields of `root` that the filter keeps.
pub(crate) fn matching_indices(
    root: &WeaponRuntimeRoot,
    group: &str,
    query: &str,
    show_opaque: bool,
) -> Vec<usize> {
    let group_matches = query.is_empty()
        || group.to_lowercase().contains(query)
        || format!("0x{:08x}", root.schema).contains(query);
    root.fields
        .iter()
        .enumerate()
        .filter(|(_, field)| {
            (show_opaque || !is_opaque(field))
                && (group_matches
                    || field.path_label.to_lowercase().contains(query)
                    || field.name.to_lowercase().contains(query)
                    || format!("0x{:08x}", field.locator.type_handle).contains(query))
        })
        .map(|(index, _)| index)
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

use crate::runtime::presentation::field_tooltip;
pub(crate) use crate::runtime::presentation::{exact_value_text, value_text};

#[cfg(test)]
mod tests;
