use super::*;
use crate::app::runtime_donors::ComponentRow;

impl PackageAuthoringApp {
    /// The runtime graph scanned for the recipe on screen, if it is current.
    fn current_runtime_graph(&self) -> Option<Arc<WeaponRuntimeGraph>> {
        self.runtime_graph.as_ref().and_then(|(key, graph)| {
            (Some(key) == self.runtime_graph_target.as_ref()).then(|| Arc::clone(graph))
        })
    }

    /// Gameplay's Technical fold: the runtime, perk and inventory controls few weapons need, all
    /// behind Experimental Features and closed until asked for.
    pub(in crate::app) fn draw_gameplay_technical(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        let graph = self.current_runtime_graph();
        egui::CollapsingHeader::new("Technical")
            .id_salt(("gameplay-technical", self.recipe_panel_scope()))
            .default_open(false)
            .show(ui, |ui| {
                if self.recipe.kind.is_weapon() && donor.is_some() {
                    self.draw_type_marker_part(ui);
                    ui.add_space(6.0);
                }
                let active = self.runtime_component_bindings(graph.as_deref());
                let additional = active
                    .keys()
                    .filter(|hash| {
                        !PRIMARY_RUNTIME_COMPONENTS
                            .iter()
                            .any(|known| known.binding_hash == **hash)
                    })
                    .count();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(format!("Advanced Runtime Bindings… ({additional})"))
                        .clicked()
                    {
                        self.runtime_bindings_open = true;
                    }
                    if ui.button("Perks & Patterns…").clicked() {
                        crate::app::runtime_dependencies::request(ui.ctx(), None);
                    }
                });
                ui.add_space(4.0);
                self.draw_runtime_resource_patches(ui);
                for draw in [
                    Self::draw_base_sandbox_perks
                        as fn(&mut Self, &mut egui::Ui, Option<&WeaponDonor>),
                    Self::draw_item_traits,
                    Self::draw_native_inventory_fields,
                ] {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(5.0);
                    draw(self, ui, donor);
                }
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(5.0);
                self.draw_raw_payload_patches(ui);
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(5.0);
                self.draw_runtime_value_column(ui, graph.as_ref());
            });
    }

    fn runtime_component_bindings(
        &self,
        graph: Option<&WeaponRuntimeGraph>,
    ) -> BTreeMap<u32, String> {
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
        active
    }

    /// Gameplay's Parts card: every part of the weapon another weapon can supply, on one label
    /// column. Runtime leads, since the rows under it follow it. Behavior, Type Markers, Firing
    /// Behavior, Barrel and Magazine are always offered: each copies one part into the weapon's own
    /// runtime. Reload swaps a whole runtime component, which can crash the game, so it needs
    /// Experimental Features.
    pub(in crate::app) fn draw_gameplay_parts(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        crate::app::style::card(ui, |ui| self.draw_part_rows(ui, donor));
    }

    fn draw_part_rows(&mut self, ui: &mut egui::Ui, donor: Option<&WeaponDonor>) {
        let graph = self.current_runtime_graph();
        let graph = graph.as_deref();
        let current_key = self.runtime_graph_key();
        let baseline_hash = self.runtime_component_baseline_hash(current_key.as_ref());
        let experimental = self.show_experimental_options;
        // The crash warning shows only while another weapon's runtime component is swapped in.
        let mixed = !self.recipe.runtime_component_donors.is_empty();
        ui.horizontal(|ui| {
            draw_donor_section_label_with_warning(
                ui,
                "Parts",
                Some("Parts taken from other weapons."),
                mixed.then_some("Mixing runtime components can crash the game."),
            );
            if experimental {
                self.draw_runtime_donor_undo(ui);
            }
        });
        ui.add_space(2.0);
        self.draw_runtime_part(ui, donor);
        self.draw_behavior_part(ui, donor);
        if self.recipe.kind.is_weapon() && donor.is_some() {
            // Type markers are the runtime's internal type names, so they sit in Technical. One
            // already chosen stays here without Experimental Features, where it can be reset.
            if !experimental && self.recipe.overrides.type_marker_donor.is_some() {
                self.draw_type_marker_part(ui);
            }
            self.draw_component_splices(ui);
        }
        if !experimental {
            return;
        }
        if self.runtime_graph_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Reading runtime components…");
            });
        }
        if let Some((key, error)) = self.runtime_graph_error.as_ref()
            && Some(key) == self.runtime_graph_target.as_ref()
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Could not read the runtime: {error}"),
            );
            if ui.small_button("Retry Runtime Scan").clicked() {
                self.runtime_graph_error = None;
            }
        }
        if graph.is_none() && self.runtime_graph_job.is_none() {
            ui.weak("Runtime data unavailable.");
        }
        let active = self.runtime_component_bindings(graph);
        for control in PRIMARY_RUNTIME_COMPONENTS {
            // Firing Behavior, Barrel and Magazine are the part rows above.
            let spliced = [
                WEAPON_TRIGGER_COMPONENT_KEY,
                WEAPON_BARREL_COMPONENT_KEY,
                WEAPON_MAGAZINE_COMPONENT_KEY,
            ]
            .contains(&control.binding_hash);
            if !spliced && active.contains_key(&control.binding_hash) {
                self.draw_runtime_component_row(
                    ui,
                    &ComponentRow {
                        binding_hash: control.binding_hash,
                        // Reload's row sits among the Parts, named as short as they are.
                        label: if control.binding_hash == WEAPON_RELOAD_COMPONENT_KEY {
                            "Reload"
                        } else {
                            control.label
                        },
                        tooltip: control.tooltip,
                        baseline_hash,
                        current_key: current_key.as_ref(),
                    },
                );
            }
        }

        let stale = self
            .recipe
            .runtime_component_donors
            .iter()
            .filter_map(|component| component.binding_hash.parse_u32().ok())
            .filter(|binding_hash| !active.contains_key(binding_hash))
            .collect::<Vec<_>>();
        for binding_hash in stale {
            if crate::app::style::missing(
                ui,
                &format!("Missing Binding 0x{binding_hash:08X}"),
                "Not in this runtime.",
            ) {
                self.recipe.set_runtime_component_donor(binding_hash, None);
            }
        }
    }

    /// Every binding beyond the four main components, as a list beside the selected one's
    /// source and actions.
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
        let current_key = self.runtime_graph_key();
        let baseline_hash = self.runtime_component_baseline_hash(current_key.as_ref());
        let mut open = true;
        let screen = ctx.content_rect();
        let width = (screen.width() - 40.0).clamp(280.0, 820.0);
        let height = (screen.height() - 64.0).clamp(240.0, 640.0);
        egui::Window::new("Advanced Runtime Bindings")
            .id(egui::Id::new("parhelion-runtime-bindings-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(width)
            .default_height(height)
            .min_width(width.min(480.0))
            .min_height(height.min(320.0))
            .show(ctx, |ui| {
                workbench_style(ui);
                let additional = if let Some(graph) = graph.as_deref() {
                    graph
                        .bindings
                        .iter()
                        .filter(|binding| {
                            !PRIMARY_RUNTIME_COMPONENTS
                                .iter()
                                .any(|known| known.binding_hash == binding.binding_hash)
                        })
                        .map(|binding| (binding.binding_hash, binding.binding_label.clone()))
                        .collect::<BTreeMap<_, _>>()
                } else {
                    if self.runtime_graph_job.is_some() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Reading runtime components…");
                        });
                    } else {
                        ui.weak("Runtime data unavailable.");
                    }
                    self.recipe
                        .runtime_component_donors
                        .iter()
                        .filter_map(|component| component.binding_hash.parse_u32().ok())
                        .filter(|hash| {
                            !PRIMARY_RUNTIME_COMPONENTS
                                .iter()
                                .any(|known| known.binding_hash == *hash)
                        })
                        .map(|hash| (hash, format!("Binding 0x{hash:08X}")))
                        .collect::<BTreeMap<_, _>>()
                };
                // Nothing to list is not a search that failed, so it gets its own title.
                if additional.is_empty() {
                    ui.label("No Other Bindings");
                    return;
                }
                let long = crate::app::pickers::wants_filter(additional.len());
                let mut changed = false;
                if long {
                    changed = ui
                        .horizontal(|ui| {
                            let width = (ui.available_width() - crate::app::pickers::CLEAR_WIDTH)
                                .clamp(160.0, 320.0);
                            sundial::ui::catalog::search(
                                ui,
                                &mut self.runtime_binding_filter,
                                false,
                                width,
                                "Search Bindings",
                            )
                        })
                        .inner;
                }
                let query = if long {
                    self.runtime_binding_filter.trim().to_ascii_lowercase()
                } else {
                    String::new()
                };
                let rows = additional
                    .into_iter()
                    .map(|(hash, discovered)| {
                        let label = runtime_component_control(hash)
                            .map_or(discovered, |control| control.label.to_owned());
                        (hash, label)
                    })
                    .filter(|(hash, label)| {
                        query.is_empty()
                            || label.to_ascii_lowercase().contains(&query)
                            || format!("0x{hash:08x}").contains(&query)
                    })
                    .collect::<Vec<_>>();
                // Read before the list draws, since the selected binding's actions edit the
                // recipe the rows read.
                let sources = rows
                    .iter()
                    .map(|(hash, _)| {
                        self.component_source_text(*hash, baseline_hash, current_key.as_ref())
                    })
                    .collect::<Vec<_>>();
                let keys = rows
                    .iter()
                    .map(|(hash, _)| u64::from(*hash))
                    .collect::<Vec<_>>();
                let list = crate::app::pickers::BrowserList {
                    keys: &keys,
                    // The count line above the list takes a row of its own.
                    height: (ui.available_height() - if long { 24.0 } else { 4.0 }).max(160.0),
                    reset: changed,
                    row_height: sundial::investment::authoring_choice_row_height(ui),
                    select: None,
                };
                let row = |ui: &mut egui::Ui, index: usize, selected: bool| {
                    sundial::investment::draw_asset_choice_row_plain(
                        ui,
                        &rows[index].1,
                        &sources[index],
                        selected,
                    )
                };
                let detail = |ui: &mut egui::Ui, index: usize| {
                    let (hash, label) = &rows[index];
                    self.draw_runtime_component_detail(
                        ui,
                        &ComponentRow {
                            binding_hash: *hash,
                            label,
                            tooltip: runtime_component_control(*hash)
                                .map_or("", |control| control.tooltip),
                            baseline_hash,
                            current_key: current_key.as_ref(),
                        },
                    );
                    None::<()>
                };
                if long {
                    list.draw(ui, row, detail);
                } else {
                    list.draw_body(ui, row, detail);
                }
            });

        self.runtime_bindings_open = open;
    }

    pub(in crate::app) fn runtime_component_baseline_hash(
        &self,
        current_key: Option<&RuntimeGraphKey>,
    ) -> Option<u32> {
        if let Some(hash) = self
            .runtime_graph
            .as_ref()
            .filter(|(key, _)| Some(key) == current_key)
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
}
