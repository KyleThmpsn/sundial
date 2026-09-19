use super::*;

impl PerkEditor {
    pub(super) fn projectile_label(&self, source: &projectile::catalog::Entry) -> String {
        source.discovery_label_with(
            |index| self.projectile_labels.get(&index).cloned(),
            |item| self.item_names.get(&item).cloned(),
        )
    }

    /// Carry mapped properties by meaning. Native field offsets are asset-specific.
    pub(super) fn select_projectile(
        &mut self,
        loaded: &PrivatePerkRuntimeGraph,
        source: u32,
        selected: Option<u32>,
    ) {
        if let Some((_, effective)) = loaded
            .projectile_slots
            .iter()
            .find(|(tag, _)| *tag == source)
        {
            let parameters = movement::mapped(loaded)
                .into_iter()
                .filter(|(tag, _)| tag == effective)
                .map(|(_, parameter)| parameter)
                .collect::<Vec<_>>();
            let carry = parameters
                .iter()
                .filter(|parameter| {
                    parameter.is_modified(&self.draft)
                        && parameters
                            .iter()
                            .filter(|other| other.kind == parameter.kind)
                            .count()
                            == 1
                })
                .filter_map(|parameter| {
                    parameter
                        .value(&self.draft)
                        .ok()
                        .map(|value| (parameter.kind, value.to_bits()))
                })
                .collect();
            self.pending_movement = Some((selected.unwrap_or(source), carry));
            let belongs = |locator: &WeaponRuntimeFieldLocator| {
                loaded
                    .graphs
                    .iter()
                    .filter(|(tag, _)| tag == effective)
                    .flat_map(|(_, graph)| graph.fields())
                    .any(|field| guided::equivalent(loaded, &field.locator, locator))
            };
            self.draft.retain(|value| !belongs(&value.locator));
            self.value_text.retain(|(locator, _), _| !belongs(locator));
        }
        self.projectile_draft
            .retain(|selection| selection.source_graph != source);
        if let Some(selected) = selected {
            self.projectile_draft.push(ProjectileSelection {
                source_graph: source,
                donor_graph: selected,
            });
        }
        self.projectile_draft
            .sort_by_key(|selection| selection.source_graph);
        self.parameter_error = None;
    }

    /// Draws every projectile slot in one list. Used when the action has no readable summary
    /// to place the slots on. Returns whether a selection changed.
    pub(super) fn draw_projectiles(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) -> bool {
        ui.strong("Projectiles and Emitters").on_hover_text(
            "The effect keeps its original trigger when you choose a different asset.",
        );
        if loaded.projectile_slots.is_empty() {
            ui.label("This effect has no projectile or emitter that can be selected here.");
            return false;
        }
        self.draw_projectile_notes(ui, loaded);
        let mut change = None;
        for (ordinal, &(source, _)) in loaded.projectile_slots.iter().enumerate() {
            if loaded.projectile_slots.len() > 1 {
                ui.label(format!("Effect {}", ordinal + 1));
            }
            if let Some(selected) = self.draw_projectile_slot(ui, loaded, source) {
                change = Some((source, selected));
            }
        }
        if let Some((source, selected)) = change {
            self.select_projectile(loaded, source, selected);
            return true;
        }
        ui.add_space(8.0);
        false
    }

    /// The guidance shown once above the projectile pickers.
    pub(super) fn draw_projectile_notes(
        &self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) {
        ui.small(
            "Changing the projectile or emitter keeps your speed, gravity, and travel distance settings where supported. Other edits to that asset reset to its defaults.",
        );
        if !loaded.projectile_catalog.errors.is_empty() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "{} resources could not be read. The effect catalog may be incomplete.",
                    loaded.projectile_catalog.errors.len()
                ),
            );
            egui::CollapsingHeader::new("Catalog Read Errors").show(ui, |ui| {
                for error in &loaded.projectile_catalog.errors {
                    ui.label(error);
                }
            });
        }
    }

    /// One projectile or emitter picker. Returns a new selection when the user picked one,
    /// with `None` inside meaning the original asset.
    pub(super) fn draw_projectile_slot(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        source: u32,
    ) -> Option<Option<u32>> {
        {
            let labels_id = ui.make_persistent_id((
                "projectile-display-names",
                Arc::as_ptr(&loaded.projectile_catalog) as usize,
                self.projectile_labels.len(),
                self.item_names.len(),
            ));
            let labels = ui.data_mut(|data| {
                data.get_temp_mut_or_insert_with(labels_id, || {
                    Arc::new(loaded.projectile_catalog.discovery_labels_with(
                        |index| self.projectile_labels.get(&index).cloned(),
                        |item| self.item_names.get(&item).cloned(),
                    ))
                })
                .clone()
            });
            let mut query_text = std::mem::take(&mut self.projectile_query);
            let label_for = |entry: &projectile::catalog::Entry| {
                labels
                    .get(&entry.graph)
                    .cloned()
                    .unwrap_or_else(|| entry.discovery_label_with(|_| None, |_| None))
            };
            let original = self
                .projectile_draft
                .iter()
                .find(|selection| selection.source_graph == source)
                .map(|selection| selection.donor_graph);
            let mut selected = original;
            let tag = original.unwrap_or(source);
            let current = loaded
                .projectile_catalog
                .entries
                .iter()
                .find(|choice| choice.graph == tag);
            let label = current.map_or_else(|| format!("Missing Effect · 0x{tag:08X}"), label_for);
            use super::super::workbench::{assets, pickers};
            let picked = pickers::browser(
                ui,
                ("perk-projectile", source),
                &label,
                "Choose a Projectile or Emitter",
                &mut query_text,
                |ui, query, reset, height| {
                    let filter_id = ui.make_persistent_id("projectile-kind");
                    let mut filter = ui.data(|state| state.get_temp::<u8>(filter_id).unwrap_or(0));
                    let before = filter;
                    let mut visibility = (false, false);
                    let mut use_original = false;
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut filter, 0, "All Types");
                        ui.selectable_value(&mut filter, 1, "Projectiles");
                        ui.selectable_value(&mut filter, 2, "Emitters");
                        ui.separator();
                        visibility = pickers::show_all(ui);
                        use_original = ui.button("Use Original").clicked();
                    });
                    if use_original {
                        return Some(None);
                    }
                    ui.data_mut(|state| state.insert_temp(filter_id, filter));
                    let mut choices = loaded
                        .projectile_catalog
                        .entries
                        .iter()
                        .filter(|choice| {
                            matches!(
                                choice.kind,
                                projectile::Kind::Projectile | projectile::Kind::Emitter
                            )
                        })
                        .filter(|choice| {
                            visibility.0
                                || choice.has_discovery_identity_with(
                                    |index| self.projectile_labels.get(&index).cloned(),
                                    |item| self.item_names.get(&item).cloned(),
                                )
                        })
                        .filter(|choice| {
                            filter == 0
                                || (filter == 1 && choice.kind == projectile::Kind::Projectile)
                                || (filter == 2 && choice.kind == projectile::Kind::Emitter)
                        })
                        .filter_map(|choice| {
                            let label = label_for(choice);
                            let roles = choice.source_hint.as_deref().unwrap_or_default();
                            let contexts = choice
                                .contexts
                                .iter()
                                .map(|context| context.path.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                            let search = format!(
                                "{label} {:08X} {} {roles} {contexts}",
                                choice.graph,
                                choice
                                    .native_paths
                                    .iter()
                                    .chain(choice.native_name.iter())
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                            pickers::matches(&query.replace("0x", ""), &search)
                                .then_some((choice, label))
                        })
                        .collect::<Vec<_>>();
                    choices.sort_by_cached_key(|(choice, label)| {
                        (choice.label_rank(), label.to_lowercase(), choice.graph)
                    });
                    let keys = choices
                        .iter()
                        .map(|(choice, _)| u64::from(choice.graph))
                        .collect::<Vec<_>>();
                    pickers::BrowserList {
                        keys: &keys,
                        height,
                        reset: reset || visibility.1 || filter != before,
                        row_height: sundial::investment::authoring_choice_row_height(ui),
                    }
                    .draw(
                        ui,
                        |ui, index, selected| {
                            let (choice, label) = &choices[index];
                            sundial::investment::draw_asset_choice_row(
                                ui,
                                label,
                                &assets::technical_name(choice),
                                selected,
                            )
                        },
                        |ui, index| {
                            let (choice, label) = &choices[index];
                            ui.heading(label);
                            if ui
                                .add(crate::app::style::primary(ui, "Use Effect"))
                                .clicked()
                            {
                                return Some((choice.graph != source).then_some(choice.graph));
                            }
                            assets::asset_details(ui, choice, &loaded.projectile_catalog, "");
                            None
                        },
                    )
                },
            );
            self.projectile_query = query_text;
            if let Some(picked) = picked {
                selected = picked;
            }
            (selected != original).then_some(selected)
        }
    }
}
