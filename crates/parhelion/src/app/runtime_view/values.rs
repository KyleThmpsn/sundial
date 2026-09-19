use super::*;

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_runtime_value_column(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&WeaponRuntimeGraph>,
    ) {
        if let Some(graph) = graph {
            self.draw_runtime_values(ui, graph);
        } else {
            draw_donor_section_label(
                ui,
                "Runtime Values",
                Some(
                    "Raw fields decoded from the selected runtime and component donors. Saved field edits are shown here, but binary patches, automatic ammo and HUD edits, and raw entity patches are applied only during compilation. Field names and types do not establish final in-game behavior or units.",
                ),
            );
            ui.weak("Runtime values appear after the selected runtime row has been decoded.");
        }
    }

    pub(in crate::app) fn draw_runtime_values(
        &mut self,
        ui: &mut egui::Ui,
        graph: &WeaponRuntimeGraph,
    ) {
        let resolved_count = graph
            .fields()
            .filter(|field| {
                field.source != WeaponRuntimeFieldSource::OpaqueNativeType
                    && runtime_field_is_editable(field)
            })
            .count();
        let technical_count = graph
            .fields()
            .filter(|field| {
                field.source == WeaponRuntimeFieldSource::OpaqueNativeType
                    && runtime_field_is_editable(field)
            })
            .count();
        draw_donor_section_label(
            ui,
            "Runtime Values",
            Some(
                "Raw fields decoded from the selected runtime and component donors. Saved field edits are shown here, but binary patches, automatic ammo and HUD edits, and raw entity patches are applied only during compilation. Field names and types do not establish final in-game behavior or units.",
            ),
        );
        ui.weak("Source: Selected runtime and component donors, with saved field edits.");
        ui.weak("Binary, ammo, HUD and raw entity patches are applied at build time, not shown here. These are raw package fields, not final in-game stats.");
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
                    format!("Show all native values ({technical_count} additional)"),
                )
                .on_hover_text(
                    "Includes unreflected byte ranges from the selected weapon's concrete runtime resources. Some ranges may contain pointers, descriptors, or coupled state.",
                );
            }
            ui.add(egui::Label::new(egui::RichText::new(format!(
                "{resolved_count} resolved · {} customized",
                self.recipe.overrides.runtime_values.len()
            )).weak()).wrap_mode(egui::TextWrapMode::Extend));
        });
        if self.show_experimental_options && self.show_technical_runtime_values {
            ui.weak(
                "Technical ranges are exact package bytes, not guessed gameplay properties. Invalid pointer, descriptor, or coupled values can make the client reject or crash on the weapon.",
            );
        }

        let list_height = (ui.ctx().screen_rect().height() * 0.52).clamp(300.0, 540.0);
        egui::ScrollArea::vertical()
            .id_salt(("parhelion-runtime-values", self.recipe_panel_scope()))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list_height)
            // This sits inside a split layout and an outer page scroll area. A maximum
            // alone lets egui collapse it to its 64px minimum during height measurement.
            .min_scrolled_height(list_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
        let live_locators = graph
            .fields()
            .map(|field| &field.locator)
            .collect::<BTreeSet<_>>();
        let stale = self
            .recipe
            .overrides
            .runtime_values
            .iter()
            .enumerate()
            .filter(|(_, value)| !live_locators.contains(&value.locator))
            .map(|(index, value)| (index, value.locator.clone()))
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

        let query = self.runtime_value_query.trim().to_ascii_lowercase();
        let mut visible_count = 0usize;
        for resource in &graph.resources {
            let resource_matches = query.is_empty()
                || resource.binding_label.to_ascii_lowercase().contains(&query)
                || format!("0x{:08x}", resource.binding_hash).contains(&query)
                || format!("0x{:08x}", resource.owner_tag).contains(&query)
                || format!("0x{:08x}", resource.concrete_class).contains(&query)
                || resource.definition.as_ref().is_some_and(|definition| {
                    format!("0x{:08x}", definition.schema).contains(&query)
                });
            let roots = std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(|root| {
                    let fields = root
                        .fields
                        .iter()
                        .filter(|field| {
                            self.runtime_field_is_visible(field, &query, resource_matches)
                        })
                        .collect::<Vec<_>>();
                    (root, fields)
                })
                .filter(|(_, fields)| !fields.is_empty())
                .collect::<Vec<_>>();
            let field_count = roots.iter().map(|(_, fields)| fields.len()).sum::<usize>();
            if field_count == 0 {
                continue;
            }
            visible_count += field_count;
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
            .default_open(!query.is_empty())
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
                for (root, fields) in roots {
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
                    .default_open(!query.is_empty())
                    .show(ui, |ui| {
                        for field in fields {
                            self.draw_runtime_value_field(ui, field);
                        }
                    });
                }
            });
        }
        for owner in &graph.owners {
            let binding_label = graph
                .bindings
                .iter()
                .find(|binding| binding.binding_hash == owner.anchor_binding_hash)
                .map_or_else(
                    || format!("Binding 0x{:08X}", owner.anchor_binding_hash),
                    |binding| binding.binding_label.clone(),
                );
            let owner_matches = query.is_empty()
                || binding_label.to_ascii_lowercase().contains(&query)
                || format!("0x{:08x}", owner.owner_tag).contains(&query)
                || format!("0x{:08x}", owner.anchor_binding_hash).contains(&query);
            let owner_visible = owner.roots.iter().any(|root| {
                root.fields
                    .iter()
                    .any(|field| self.runtime_field_is_visible(field, &query, owner_matches))
            });
            if !owner_visible {
                continue;
            }
            let owner_field_count = owner
                .roots
                .iter()
                .flat_map(|root| &root.fields)
                .filter(|field| self.runtime_field_is_visible(field, &query, owner_matches))
                .count();
            visible_count += owner_field_count;
            egui::CollapsingHeader::new(format!(
                "Shared owner state · {} · 0x{:08X} · {} value{}",
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
            .default_open(!query.is_empty())
            .show(ui, |ui| {
                for root in &owner.roots {
                    let root_fields = root
                        .fields
                        .iter()
                        .filter(|field| self.runtime_field_is_visible(field, &query, owner_matches))
                        .collect::<Vec<_>>();
                    if root_fields.is_empty() {
                        continue;
                    }
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
                    .default_open(!query.is_empty())
                    .show(ui, |ui| {
                        for field in root_fields {
                            self.draw_runtime_value_field(ui, field);
                        }
                    });
                }
            });
        }
        if visible_count == 0 {
            ui.weak("No runtime values match the current filter.");
        }
            });
    }

    pub(in crate::app) fn runtime_field_is_visible(
        &self,
        field: &WeaponRuntimeField,
        query: &str,
        owner_matches: bool,
    ) -> bool {
        let customized = self
            .recipe
            .overrides
            .runtime_values
            .iter()
            .any(|value| value.locator == field.locator);
        if !runtime_field_is_in_editor_scope(
            field.source,
            runtime_field_is_editable(field),
            customized,
            self.show_experimental_options,
            self.show_technical_runtime_values,
        ) {
            return false;
        }
        if query.is_empty() || owner_matches {
            return true;
        }
        field.name.to_ascii_lowercase().contains(query)
            || field.path_label.to_ascii_lowercase().contains(query)
            || runtime_value_kind_label(&field.kind)
                .to_ascii_lowercase()
                .contains(query)
            || format!("0x{:08x}", field.locator.root_schema).contains(query)
            || format!("0x{:08x}", field.locator.type_handle).contains(query)
            || format!("0x{:x}", field.owner_offset).contains(query)
            || format!("0x{:x}", field.locator.value_offset).contains(query)
            || field.locator.path.iter().any(|element| {
                format!("0x{:08x}", element.name_hash).contains(query)
                    || format!("0x{:08x}", element.type_handle).contains(query)
            })
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
