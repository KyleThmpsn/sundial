use super::*;

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_runtime_component_donors(&mut self, ui: &mut egui::Ui) {
        let graph = self.runtime_graph.as_ref().and_then(|(key, graph)| {
            (Some(key) == self.runtime_graph_target.as_ref()).then(|| Arc::clone(graph))
        });
        if let Some(donor_width) = runtime_workspace_donor_width(ui.available_width()) {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(donor_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(donor_width);
                        self.draw_runtime_component_donor_column(ui, graph.as_deref());
                    },
                );
                ui.separator();
                let value_width = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(value_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(value_width);
                        self.draw_runtime_value_column(ui, graph.as_deref());
                    },
                );
            });
        } else {
            self.draw_runtime_component_donor_column(ui, graph.as_deref());
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(5.0);
            self.draw_runtime_value_column(ui, graph.as_deref());
        }
    }

    pub(in crate::app) fn draw_runtime_component_donor_column(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&WeaponRuntimeGraph>,
    ) {
        self.draw_runtime_component_donor_pickers(ui, graph);
        if self.show_experimental_options {
            ui.add_space(8.0);
            self.draw_runtime_resource_patches(ui);
        }
    }

    fn draw_runtime_component_donor_pickers(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&WeaponRuntimeGraph>,
    ) {
        draw_donor_section_label(
            ui,
            "Runtime Component Donors",
            Some(
                "A selection replaces the complete shared owner partition for that resource, including every alias and any other component binding owned by the same partition. Unselected owners remain byte-identical to the baseline. Cross-family components can depend on different runtime data, so test new combinations in-game even when the native graph validates.",
            ),
        );
        self.draw_runtime_donor_undo(ui);
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Mixing component donors is highly experimental and has a high risk of crashes. Use with caution.",
        );
        if self.runtime_graph_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Reading the selected pattern's complete runtime graph…");
            });
        }
        if let Some((key, error)) = self.runtime_graph_error.as_ref()
            && Some(key) == self.runtime_graph_target.as_ref()
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Runtime graph could not be decoded: {error}"),
            );
            if ui.small_button("Retry Runtime Scan").clicked() {
                self.runtime_graph_error = None;
            }
        }
        let mut active = BTreeMap::<u32, String>::new();
        if let Some(graph) = graph {
            for binding in &graph.bindings {
                active
                    .entry(binding.binding_hash)
                    .or_insert_with(|| binding.binding_label.clone());
            }
        } else {
            // An experimental choice can prevent full field decoding. Keep repair and
            // compatibility-review controls available instead of trapping that saved choice.
            ui.weak("Runtime data is unavailable. Saved donors can still be reviewed or reset.");
            for control in PRIMARY_RUNTIME_COMPONENTS {
                active.insert(control.binding_hash, control.label.to_owned());
            }
            for component in &self.recipe.runtime_component_donors {
                if let Ok(hash) = component.binding_hash.parse_u32() {
                    active
                        .entry(hash)
                        .or_insert_with(|| format!("Binding 0x{hash:08X}"));
                }
            }
        }
        for control in PRIMARY_RUNTIME_COMPONENTS {
            if active.contains_key(&control.binding_hash) {
                self.draw_runtime_component_donor_picker(
                    ui,
                    control.binding_hash,
                    control.label,
                    control.tooltip,
                );
                ui.add_space(4.0);
            }
        }

        let additional_count = active
            .iter()
            .filter(|(hash, _)| {
                !PRIMARY_RUNTIME_COMPONENTS
                    .iter()
                    .any(|known| known.binding_hash == **hash)
            })
            .count();
        if ui
            .button(format!("Advanced Runtime Bindings… ({additional_count})"))
            .clicked()
        {
            self.runtime_bindings_open = true;
        }

        let stale = self
            .recipe
            .runtime_component_donors
            .iter()
            .filter_map(|component| component.binding_hash.parse_u32().ok())
            .filter(|binding_hash| !active.contains_key(binding_hash))
            .collect::<Vec<_>>();
        for binding_hash in stale {
            ui.horizontal(|ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Saved component binding 0x{binding_hash:08X} is not present in this pattern"),
                );
                if ui.small_button("Remove").clicked() {
                    self.recipe.set_runtime_component_donor(binding_hash, None);
                }
            });
        }
    }

    pub(in crate::app) fn draw_runtime_bindings_window(&mut self, ctx: &egui::Context) {
        if !self.runtime_bindings_open
            || !self.show_experimental_options
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
        {
            return;
        }
        let graph = self.runtime_graph.as_ref().and_then(|(key, graph)| {
            (Some(key) == self.runtime_graph_target.as_ref()).then(|| Arc::clone(graph))
        });
        let mut open = true;
        egui::Window::new("Advanced Runtime Bindings")
            .id(egui::Id::new("parhelion-runtime-bindings-window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(660.0)
            .default_height(600.0)
            .resizable(true)
            .show(ctx, |ui| {
            workbench_style(ui);
            ui.label(format!("{} · Component sources", self.recipe.name));
            let additional = if let Some(graph) = graph.as_deref() {
                graph.bindings.iter()
                    .filter(|binding| !PRIMARY_RUNTIME_COMPONENTS.iter().any(|known| known.binding_hash == binding.binding_hash))
                    .map(|binding| (binding.binding_hash, binding.binding_label.clone()))
                    .collect::<BTreeMap<_, _>>()
            } else {
                ui.weak("Runtime data is unavailable. Saved additional donors remain available for repair.");
                self.recipe.runtime_component_donors.iter()
                    .filter_map(|component| component.binding_hash.parse_u32().ok())
                    .filter(|hash| !PRIMARY_RUNTIME_COMPONENTS.iter().any(|known| known.binding_hash == *hash))
                    .map(|hash| (hash, format!("Binding 0x{hash:08X}")))
                    .collect::<BTreeMap<_, _>>()
            };
            ui.horizontal_wrapped(|ui| {
                ui.label("Filter");
                named_control(ui.add(
                    egui::TextEdit::singleline(&mut self.runtime_binding_filter)
                        .desired_width(260.0)
                        .hint_text("Name or 0x hash"),
                ), "Filter Runtime Bindings");
            });
            let query = self.runtime_binding_filter.trim().to_ascii_lowercase();
            let filtered = additional
                .into_iter()
                .filter(|(binding_hash, discovered_label)| {
                    let known = runtime_component_control(*binding_hash);
                    let label = known.map_or(discovered_label.as_str(), |control| control.label);
                    query.is_empty()
                        || label.to_ascii_lowercase().contains(&query)
                        || format!("0x{binding_hash:08x}").contains(&query)
                })
                .collect::<Vec<_>>();
            ui.weak(format!(
                "{} matching binding{}",
                filtered.len(),
                if filtered.len() == 1 { "" } else { "s" }
            ));
            egui::ScrollArea::vertical()
                .id_salt(("additional-runtime-binding-results", self.recipe_panel_scope()))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (binding_hash, discovered_label) in filtered {
                        let known = runtime_component_control(binding_hash);
                        let label =
                            known.map_or(discovered_label.as_str(), |control| control.label);
                        let tooltip = known.map_or(
                            "A binding discovered directly from the selected runtime entity. The compiler requires the selected donor to expose the same binding shape.",
                            |control| control.tooltip,
                        );
                        self.draw_runtime_component_donor_picker(
                            ui,
                            binding_hash,
                            label,
                            tooltip,
                        );
                        ui.add_space(4.0);
                    }
                });
        });

        self.runtime_bindings_open = open;
    }

    pub(in crate::app) fn runtime_component_baseline_hash(&self) -> Option<u32> {
        let current_key = self.runtime_graph_key();
        if let Some(hash) = self
            .runtime_graph
            .as_ref()
            .filter(|(key, _)| Some(key) == current_key.as_ref())
            .map(|(_, graph)| graph.item_hash)
            .filter(|hash| *hash != 0)
        {
            return Some(hash);
        }
        let Some(index) = self.recipe.overrides.weapon_pattern_index else {
            return self
                .recipe
                .donor
                .item_hash
                .parse_u32()
                .ok()
                .filter(|hash| *hash != 0);
        };
        // An explicit runtime row can be unrelated to the gameplay donor. Without a
        // current graph, only a representative verified against that row is a baseline.
        self.recipe
            .overrides
            .weapon_pattern_donor_hash
            .as_ref()
            .and_then(|hash| hash.parse_u32().ok())
            .filter(|hash| {
                self.donor_summaries
                    .iter()
                    .any(|donor| donor.hash == *hash && donor.weapon_pattern_index == Some(index))
            })
            .or_else(|| {
                self.donor_summaries
                    .iter()
                    .find(|donor| donor.weapon_pattern_index == Some(index))
                    .map(|donor| donor.hash)
            })
            .filter(|hash| *hash != 0)
    }

    pub(in crate::app) fn draw_runtime_component_donor_picker(
        &mut self,
        ui: &mut egui::Ui,
        binding_hash: u32,
        label: &str,
        tooltip: &str,
    ) {
        draw_donor_section_label(
            ui,
            label,
            Some(&format!("{tooltip} Native binding: 0x{binding_hash:08X}.")),
        );
        let pattern_hash = self.runtime_component_baseline_hash();
        let current_reference = self.recipe.runtime_component_donor(binding_hash).cloned();
        let current_hash = current_reference
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let selected_text = current_reference.as_ref().map_or_else(
            || {
                pattern_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || "Follow runtime baseline".to_owned(),
                        |donor| format!("Follow {} · 0x{:08X}", donor.name, donor.hash),
                    )
            },
            |reference| {
                current_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || {
                            format!(
                                "{} · {}",
                                reference
                                    .expected_name
                                    .as_deref()
                                    .unwrap_or("Unknown component donor"),
                                reference.item_hash
                            )
                        },
                        |donor| {
                            format!(
                                "{} · {} · 0x{:08X}",
                                donor.name, donor.type_name, donor.hash
                            )
                        },
                    )
            },
        );
        self.draw_checked_runtime_donor_header(
            ui,
            binding_hash,
            &selected_text,
            current_hash,
            pattern_hash,
        );
    }
}
