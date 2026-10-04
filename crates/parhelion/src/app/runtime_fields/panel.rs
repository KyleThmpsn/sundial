//! A runtime graph's values as a filterable list of resources and shared owners, each field
//! editing one list of value overrides. The weapon's Runtime Values and an authored ability's
//! values both draw it.
use super::*;

/// What one panel reads and writes.
pub(in crate::app) struct ValuePanel<'a> {
    pub(in crate::app) title: &'a str,
    pub(in crate::app) help: &'a str,
    /// Keeps the panel's scroll position apart from another panel's.
    pub(in crate::app) scope: egui::Id,
    pub(in crate::app) query: &'a mut String,
    pub(in crate::app) cache: &'a mut Option<RuntimeValuesCache>,
    pub(in crate::app) text: &'a mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    pub(in crate::app) overrides: &'a mut Vec<WeaponRuntimeValueOverride>,
    pub(in crate::app) show_experimental: bool,
    pub(in crate::app) show_technical: &'a mut bool,
    /// Leads with the values that read as plain words, outside their groups.
    pub(in crate::app) readable_first: bool,
}

impl ValuePanel<'_> {
    fn key(&self, query: &str) -> RuntimeValuesKey {
        RuntimeValuesKey {
            query: query.to_owned(),
            show_experimental_options: self.show_experimental,
            show_all_native_values: *self.show_technical,
            customized: self
                .overrides
                .iter()
                .map(|value| value.locator.clone())
                .collect(),
        }
    }

    fn field(&mut self, ui: &mut egui::Ui, field: &WeaponRuntimeField) {
        draw_runtime_value_override_field(ui, field, &mut *self.overrides, &mut *self.text);
    }
}

/// Draws `graph`'s values under the panel's title, with its search and counts. The list scrolls
/// in a box of its own, so what follows the panel stays in reach.
pub(in crate::app) fn draw_value_panel(
    ui: &mut egui::Ui,
    graph: &Arc<WeaponRuntimeGraph>,
    panel: ValuePanel<'_>,
) {
    draw_panel(ui, graph, panel, true);
}

/// The same panel where it ends the page. Its list flows in the page's own scroll rather than
/// in a second one inside it.
pub(in crate::app) fn draw_page_value_panel(
    ui: &mut egui::Ui,
    graph: &Arc<WeaponRuntimeGraph>,
    panel: ValuePanel<'_>,
) {
    draw_panel(ui, graph, panel, false);
}

fn draw_panel(
    ui: &mut egui::Ui,
    graph: &Arc<WeaponRuntimeGraph>,
    mut panel: ValuePanel<'_>,
    boxed: bool,
) {
    // Taken out while the list draws, because drawing a field edits the overrides.
    let mut cache = match panel.cache.take() {
        Some(cache) if cache.is_for(graph) => cache,
        _ => RuntimeValuesCache::new(graph),
    };
    let resolved_count = cache.resolved;
    let technical_count = cache.technical;
    // A panel under a tab of the same name leaves its heading to the tab.
    if !panel.title.is_empty() {
        draw_donor_section_label(ui, panel.title, Some(panel.help));
    }
    // Native values show only while Experimental Features are on, so only then do they count.
    let rows = resolved_count
        + if panel.show_experimental && *panel.show_technical {
            technical_count
        } else {
            0
        };
    let long = crate::app::pickers::wants_filter(rows);
    if long {
        ui.horizontal(|ui| {
            let width = (ui.available_width() - crate::app::pickers::CLEAR_WIDTH).max(160.0);
            sundial::ui::catalog::search(ui, &mut *panel.query, false, width, "Search Values");
        });
    }
    // A short list shows whole. A search typed over a longer one waits for it.
    let set_aside = (!long).then(|| std::mem::take(&mut *panel.query));
    ui.horizontal_wrapped(|ui| {
        if panel.show_experimental {
            ui.checkbox(
                &mut *panel.show_technical,
                format!("Show All Native Values ({technical_count})"),
            )
            .on_hover_text("Adds unnamed byte ranges, which can hold pointers.");
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "{resolved_count} Values · {} Changed",
                    panel.overrides.len()
                ))
                .color(crate::app::style::secondary(ui.visuals())),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
    if boxed {
        let list_height = (ui.ctx().screen_rect().height() * 0.52).clamp(300.0, 540.0);
        egui::ScrollArea::vertical()
            .id_salt(panel.scope)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list_height)
            // This can sit inside a split layout and an outer page scroll area. A maximum alone
            // lets egui collapse it to its 64px minimum during height measurement.
            .min_scrolled_height(list_height)
            .auto_shrink([false, true])
            .show(ui, |ui| draw_value_list(ui, graph, &mut panel, &mut cache));
    } else {
        ui.push_id(panel.scope, |ui| {
            draw_value_list(ui, graph, &mut panel, &mut cache);
        });
    }
    if let Some(query) = set_aside {
        *panel.query = query;
    }
    *panel.cache = Some(cache);
}

fn draw_value_list(
    ui: &mut egui::Ui,
    graph: &WeaponRuntimeGraph,
    panel: &mut ValuePanel<'_>,
    cache: &mut RuntimeValuesCache,
) {
    let query = panel.query.trim().to_ascii_lowercase();
    let stale = cache
        .view(graph, panel.key(&query))
        .stale
        .iter()
        .map(|&index| (index, panel.overrides[index].locator.clone()))
        .collect::<Vec<_>>();
    let mut remove_stale = None;
    for (index, locator) in stale {
        if crate::app::style::missing(
            ui,
            &format!(
                "Missing Value 0x{:08X} +0x{:X}",
                locator.binding_hash, locator.value_offset
            ),
            &format!("Schema 0x{:08X}. Not in this runtime.", locator.root_schema),
        ) {
            remove_stale = Some(index);
        }
    }
    if let Some(index) = remove_stale {
        let removed = panel.overrides.remove(index);
        panel
            .text
            .retain(|(locator, _), _| locator != &removed.locator);
    }

    let view = cache.view(graph, panel.key(&query));
    // Headers are forced open while a filter is active.
    let open = (!query.is_empty()).then_some(true);
    if panel.readable_first && query.is_empty() {
        draw_readable(ui, graph, view, panel);
    }
    for (resource, group) in graph.resources.iter().zip(&view.resources) {
        let field_count = group.count;
        if field_count == 0 {
            continue;
        }
        let roots = std::iter::once(&resource.instance)
            .chain(resource.definition.iter())
            .collect::<Vec<_>>();
        // The name, which part of it this is, and how many values. Its class shows once it opens,
        // since an unnamed one already names itself by a hash.
        let resource_suffix = if resource.resource_count > 1 {
            format!(
                " · {} of {}",
                resource.resource_index + 1,
                resource.resource_count
            )
        } else {
            String::new()
        };
        let facts = format!(
            "{resource_suffix} · {field_count} Value{}",
            if field_count == 1 { "" } else { "s" }
        );
        egui::CollapsingHeader::new(header_text(ui, &resource.binding_label, &facts))
            .id_salt((
                "runtime-resource",
                resource.binding_hash,
                resource.resource_index,
                resource.concrete_class,
            ))
            .open(open)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Class 0x{:08X} · Owner 0x{:08X}{}",
                        resource.concrete_class,
                        resource.owner_tag,
                        if resource.alias_bindings.is_empty() {
                            String::new()
                        } else {
                            format!(" · {} alias bindings", resource.alias_bindings.len())
                        },
                    ))
                    .color(crate::app::style::secondary(ui.visuals())),
                );
                for (root_position, fields) in &group.roots {
                    let root = roots[*root_position];
                    let facts = format!(
                        " · schema 0x{:08X} · owner offset 0x{:X} · 0x{:X} bytes · {}",
                        root.schema,
                        root.owner_offset,
                        root.byte_size,
                        if root.generated_schema {
                            "generated"
                        } else {
                            "native"
                        }
                    );
                    egui::CollapsingHeader::new(header_text(ui, root.kind.label(), &facts))
                        .id_salt((
                            "runtime-component-root",
                            resource.binding_hash,
                            resource.resource_index,
                            root.kind,
                            root.schema,
                        ))
                        .open(open)
                        .show(ui, |ui| {
                            for &field in fields {
                                panel.field(ui, &root.fields[field]);
                            }
                        });
                }
            });
    }
    for (owner, group) in graph.owners.iter().zip(&view.owners) {
        let owner_field_count = group.count;
        if owner_field_count == 0 {
            continue;
        }
        let binding_label = runtime_owner_label(graph, owner);
        let facts = format!(
            " · 0x{:08X} · {owner_field_count} value{}",
            owner.owner_tag,
            if owner_field_count == 1 { "" } else { "s" }
        );
        egui::CollapsingHeader::new(header_text(
            ui,
            &format!("Shared Owner State · {binding_label}"),
            &facts,
        ))
        .id_salt((
            "runtime-owner",
            owner.owner_tag,
            owner.anchor_binding_hash,
            owner.anchor_resource_index,
        ))
        .open(open)
        .show(ui, |ui| {
            for (root_position, root_fields) in &group.roots {
                let root = &owner.roots[*root_position];
                let facts = format!(
                    " · schema 0x{:08X} · {}",
                    root.schema,
                    if root.generated_schema {
                        "generated"
                    } else {
                        "native"
                    }
                );
                egui::CollapsingHeader::new(header_text(ui, root.kind.label(), &facts))
                    .id_salt(("runtime-root", owner.owner_tag, root.kind, root.schema))
                    .open(open)
                    .show(ui, |ui| {
                        for &field in root_fields {
                            panel.field(ui, &root.fields[field]);
                        }
                    });
            }
        });
    }
    if view.visible == 0 {
        ui.label(if query.is_empty() {
            "No Values"
        } else {
            "No Matching Results"
        });
    }
}

/// The values that read as plain words, ahead of the groups that hold them. Each reads by its own
/// name, and a name two fields share by its group, then its root. The full path stays in the
/// group and the hover.
fn draw_readable(
    ui: &mut egui::Ui,
    graph: &WeaponRuntimeGraph,
    view: &RuntimeValuesView,
    panel: &mut ValuePanel<'_>,
) {
    let mut readable = Vec::new();
    for (resource, group) in graph.resources.iter().zip(&view.resources) {
        let roots = std::iter::once(&resource.instance)
            .chain(resource.definition.iter())
            .collect::<Vec<_>>();
        let name = if resource.resource_count > 1 {
            format!("{} {}", resource.binding_label, resource.resource_index + 1)
        } else {
            resource.binding_label.clone()
        };
        for (position, fields) in &group.roots {
            let root = roots[*position];
            for field in fields {
                readable.push((name.clone(), root, &root.fields[*field]));
            }
        }
    }
    for (owner, group) in graph.owners.iter().zip(&view.owners) {
        let name = runtime_owner_label(graph, owner);
        for (position, fields) in &group.roots {
            let root = &owner.roots[*position];
            for field in fields {
                readable.push((name.clone(), root, &root.fields[*field]));
            }
        }
    }
    readable.retain(|(_, _, field)| readable_field(field));
    if readable.is_empty() {
        return;
    }
    let count = |same: &dyn Fn(&str, &str) -> bool| {
        readable
            .iter()
            .filter(|(group, _, field)| same(group, &field.name))
            .count()
    };
    let labels = readable
        .iter()
        .map(|(group, root, field)| {
            if count(&|_, name| name == field.name) == 1 {
                field.name.clone()
            } else if count(&|other, name| name == field.name && other == group) == 1 {
                format!("{} · {group}", field.name)
            } else {
                format!("{} · {group} · {}", field.name, root.kind.label())
            }
        })
        .collect::<Vec<_>>();
    ui.push_id("readable-values", |ui| {
        for ((_, _, field), label) in readable.iter().zip(labels) {
            let mut named = (*field).clone();
            named.path_label = label;
            panel.field(ui, &named);
        }
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("All Values").color(crate::app::style::secondary(ui.visuals())));
}

/// Whether a field reads as plain words: a verified name of its own, holding a number or a
/// switch. Unnamed and inferred fields, identifiers, flags and raw bytes stay in groups.
fn readable_field(field: &WeaponRuntimeField) -> bool {
    !field.name_inferred
        && field.source != WeaponRuntimeFieldSource::OpaqueNativeType
        && matches!(
            field.kind,
            WeaponRuntimeValueKind::Boolean
                | WeaponRuntimeValueKind::SignedInteger { .. }
                | WeaponRuntimeValueKind::UnsignedInteger { .. }
                | WeaponRuntimeValueKind::Float32
                | WeaponRuntimeValueKind::Float64
        )
        && ![
            "Unnamed Field",
            "Member 0x",
            "Value 0x",
            "Unreflected byte",
            "native value",
        ]
        .iter()
        .any(|unnamed| field.name.starts_with(unnamed))
        && !field.name.contains("(inferred)")
        // Engine bookkeeping reads as words but sets nothing a player would see.
        && !field.name.split_whitespace().any(|word| {
            PLUMBING
                .iter()
                .any(|plumbing| word.eq_ignore_ascii_case(plumbing))
        })
}

/// Words that name engine bookkeeping: where records sit and how they are found.
const PLUMBING: &[&str] = &[
    "Offset",
    "Id",
    "Index",
    "Designator",
    "Hack",
    "Hash",
    "Handle",
    "Pointer",
    "Padding",
    "Header",
    "Sobject",
];

/// A header's name in the header's own colour, then its facts in the secondary colour.
fn header_text(ui: &egui::Ui, name: &str, facts: &str) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        name,
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    job.append(
        facts,
        0.0,
        egui::TextFormat {
            font_id,
            color: crate::app::style::secondary(ui.visuals()),
            ..Default::default()
        },
    );
    job
}
