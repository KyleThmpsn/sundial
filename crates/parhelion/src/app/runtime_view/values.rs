use super::*;

const RUNTIME_VALUES_HELP: &str =
    "Raw fields from the runtime and component donors. Patches apply at build time.";

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_runtime_value_column(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&Arc<WeaponRuntimeGraph>>,
    ) {
        if let Some(graph) = graph {
            self.draw_runtime_values(ui, graph);
        } else {
            draw_donor_section_label(ui, "Runtime Values", Some(RUNTIME_VALUES_HELP));
            ui.weak("Not loaded yet.");
        }
    }

    pub(in crate::app) fn draw_runtime_values(
        &mut self,
        ui: &mut egui::Ui,
        graph: &Arc<WeaponRuntimeGraph>,
    ) {
        // Taken out while the list draws, because drawing a field edits the recipe.
        let mut cache = match self.runtime_values_cache.take() {
            Some(cache) if cache.is_for(graph) => cache,
            _ => RuntimeValuesCache::new(graph),
        };
        let resolved_count = cache.resolved;
        let technical_count = cache.technical;
        draw_donor_section_label(ui, "Runtime Values", Some(RUNTIME_VALUES_HELP));
        ui.horizontal(|ui| {
            ui.label("Filter");
            named_control(
                ui.add(
                    egui::TextEdit::singleline(&mut self.runtime_value_query)
                        .desired_width(ui.available_width())
                        .hint_text("Field, component, schema, type, or 0x hash"),
                ),
                "Filter Runtime Values",
            );
        });
        ui.horizontal_wrapped(|ui| {
            if self.show_experimental_options {
                ui.checkbox(
                    &mut self.show_technical_runtime_values,
                    format!("Show All Native Values ({technical_count})"),
                )
                .on_hover_text("Adds unnamed byte ranges, which can hold pointers.");
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{resolved_count} resolved · {} customized",
                        self.recipe.overrides.runtime_values.len()
                    ))
                    .weak(),
                )
                .wrap_mode(egui::TextWrapMode::Extend),
            );
        });

        let list_height = (ui.ctx().screen_rect().height() * 0.52).clamp(300.0, 540.0);
        egui::ScrollArea::vertical()
            .id_salt(("parhelion-runtime-values", self.recipe_panel_scope()))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list_height)
            // This sits inside a split layout and an outer page scroll area. A maximum
            // alone lets egui collapse it to its 64px minimum during height measurement.
            .min_scrolled_height(list_height)
            .auto_shrink([false, true])
            .show(ui, |ui| self.draw_runtime_value_list(ui, graph, &mut cache));
        self.runtime_values_cache = Some(cache);
    }

    fn runtime_values_key(&self, query: &str) -> RuntimeValuesKey {
        RuntimeValuesKey {
            query: query.to_owned(),
            show_experimental_options: self.show_experimental_options,
            show_all_native_values: self.show_technical_runtime_values,
            customized: self
                .recipe
                .overrides
                .runtime_values
                .iter()
                .map(|value| value.locator.clone())
                .collect(),
        }
    }

    fn draw_runtime_value_list(
        &mut self,
        ui: &mut egui::Ui,
        graph: &WeaponRuntimeGraph,
        cache: &mut RuntimeValuesCache,
    ) {
        let query = self.runtime_value_query.trim().to_ascii_lowercase();
        let stale = cache
            .view(graph, self.runtime_values_key(&query))
            .stale
            .iter()
            .map(|&index| {
                (
                    index,
                    self.recipe.overrides.runtime_values[index].locator.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut remove_stale = None;
        for (index, locator) in stale {
            ui.horizontal(|ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "Customized binding 0x{:08X}, schema 0x{:08X}, root offset 0x{:X} is not present in the effective graph",
                        locator.binding_hash, locator.root_schema, locator.value_offset
                    ),
                );
                if ui.small_button("Remove Stale Value").clicked() {
                    remove_stale = Some(index);
                }
            });
        }
        if let Some(index) = remove_stale {
            let removed = self.recipe.overrides.runtime_values.remove(index);
            self.runtime_value_text
                .retain(|(locator, _), _| locator != &removed.locator);
        }

        let view = cache.view(graph, self.runtime_values_key(&query));
        // Headers are forced open while a filter is active.
        let open = (!query.is_empty()).then_some(true);
        for (resource, group) in graph.resources.iter().zip(&view.resources) {
            let field_count = group.count;
            if field_count == 0 {
                continue;
            }
            let roots = std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .collect::<Vec<_>>();
            let resource_suffix = if resource.resource_count > 1 {
                format!(
                    " · resource {}/{}",
                    resource.resource_index + 1,
                    resource.resource_count
                )
            } else {
                String::new()
            };
            egui::CollapsingHeader::new(format!(
                "{}{} · class 0x{:08X} · {} value{}",
                resource.binding_label,
                resource_suffix,
                resource.concrete_class,
                field_count,
                if field_count == 1 { "" } else { "s" }
            ))
            .id_salt((
                "runtime-resource",
                resource.binding_hash,
                resource.resource_index,
                resource.concrete_class,
            ))
            .open(open)
            .show(ui, |ui| {
                ui.weak(format!(
                    "Owner 0x{:08X}{}",
                    resource.owner_tag,
                    if resource.alias_bindings.is_empty() {
                        String::new()
                    } else {
                        format!(" · {} alias bindings", resource.alias_bindings.len())
                    },
                ));
                for (root_position, fields) in &group.roots {
                    let root = roots[*root_position];
                    egui::CollapsingHeader::new(format!(
                        "{} · schema 0x{:08X} · owner offset 0x{:X} · 0x{:X} bytes · {}",
                        root.kind.label(),
                        root.schema,
                        root.owner_offset,
                        root.byte_size,
                        if root.generated_schema {
                            "generated"
                        } else {
                            "native"
                        }
                    ))
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
                            self.draw_runtime_value_field(ui, &root.fields[field]);
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
            egui::CollapsingHeader::new(format!(
                "Shared Owner State · {} · 0x{:08X} · {} value{}",
                binding_label,
                owner.owner_tag,
                owner_field_count,
                if owner_field_count == 1 { "" } else { "s" }
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
                    egui::CollapsingHeader::new(format!(
                        "{} · schema 0x{:08X} · {}",
                        root.kind.label(),
                        root.schema,
                        if root.generated_schema {
                            "generated"
                        } else {
                            "native"
                        }
                    ))
                    .id_salt(("runtime-root", owner.owner_tag, root.kind, root.schema))
                    .open(open)
                    .show(ui, |ui| {
                        for &field in root_fields {
                            self.draw_runtime_value_field(ui, &root.fields[field]);
                        }
                    });
                }
            });
        }
        if view.visible == 0 {
            ui.weak("No runtime values match the current filter.");
        }
    }

    pub(in crate::app) fn draw_runtime_value_field(
        &mut self,
        ui: &mut egui::Ui,
        field: &WeaponRuntimeField,
    ) {
        draw_runtime_value_override_field(
            ui,
            field,
            &mut self.recipe.overrides.runtime_values,
            &mut self.runtime_value_text,
        );
    }
}
