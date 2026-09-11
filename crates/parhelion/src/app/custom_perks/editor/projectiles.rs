use super::*;

impl PerkEditor {
    pub(super) fn projectile_label(source: &projectile::catalog::Entry) -> String {
        source
            .native_paths
            .first()
            .map(|path| sundial::package_authoring::tft::asset_label(path))
            .or_else(|| source.native_name.clone())
            .unwrap_or_else(|| format!("Unidentified {}", source.kind.label()))
    }

    /// Carry mapped properties by meaning. Native field offsets are asset-specific.
    fn select_projectile(
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
        }
        let mut change = None;
        for (ordinal, &(source, _)) in loaded.projectile_slots.iter().enumerate() {
            if loaded.projectile_slots.len() > 1 {
                ui.label(format!("Effect {}", ordinal + 1));
            }
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
            let label = current
                .map(Self::projectile_label)
                .unwrap_or_else(|| format!("Missing Effect · 0x{tag:08X}"));
            let mut query_text = std::mem::take(&mut self.projectile_query);
            let picked = super::super::workbench::pickers::popup(
                ui,
                ("perk-projectile", source),
                &label,
                &mut query_text,
                |ui, query, reset, height| {
                    let filter_id = ui.make_persistent_id("projectile-kind");
                    let mut filter =
                        ui.data_mut(|state| state.get_temp::<u8>(filter_id).unwrap_or(0));
                    let before = filter;
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut filter, 0, "All Assets");
                        ui.selectable_value(&mut filter, 1, "Projectiles");
                        ui.selectable_value(&mut filter, 2, "Emitters");
                    });
                    ui.data_mut(|state| state.insert_temp(filter_id, filter));
                    let source_name = loaded
                        .projectile_catalog
                        .entries
                        .iter()
                        .find(|entry| entry.graph == source)
                        .map(Self::projectile_label)
                        .unwrap_or_else(|| format!("0x{source:08X}"));
                    if sundial::investment::draw_asset_choice_row(
                        ui,
                        &source_name,
                        "Original Effect",
                        original.is_none(),
                    )
                    .clicked()
                    {
                        return Some(None);
                    }
                    ui.separator();
                    let mut choices = loaded
                        .projectile_catalog
                        .entries
                        .iter()
                        .filter(|choice| choice.graph != source)
                        .filter(|choice| {
                            filter == 0
                                || (filter == 1 && choice.kind == projectile::Kind::Projectile)
                                || (filter == 2 && choice.kind == projectile::Kind::Emitter)
                        })
                        .filter_map(|choice| {
                            let label = Self::projectile_label(choice);
                            let paths = choice.native_paths.join("\n");
                            let contexts = choice
                                .contexts
                                .iter()
                                .map(|context| context.path.as_str())
                                .collect::<std::collections::BTreeSet<_>>()
                                .into_iter()
                                .collect::<Vec<_>>()
                                .join("\n");
                            let perks = choice
                                .perk_indices
                                .iter()
                                .filter_map(|index| self.projectile_labels.get(index))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ");
                            let search = format!(
                                "{label} {:08X} {} {} {paths} {contexts} {perks}",
                                choice.graph,
                                choice.package,
                                choice.kind.label()
                            );
                            super::super::workbench::pickers::matches(query, &search)
                                .then_some((choice, label, paths, contexts, perks))
                        })
                        .collect::<Vec<_>>();
                    choices.sort_by_cached_key(|(choice, label, _, _, _)| {
                        (
                            choice.native_paths.is_empty() && choice.native_name.is_none(),
                            label.clone(),
                            choice.graph,
                        )
                    });
                    super::super::workbench::pickers::results(
                        ui,
                        "projectile-options",
                        choices.len(),
                        (height - 92.0).max(60.0),
                        reset || filter != before,
                        sundial::investment::authoring_choice_row_height(ui),
                        |ui, index| {
                            let (choice, label, paths, contexts, perks) = &choices[index];
                            sundial::investment::draw_asset_choice_row(
                                ui,
                                label,
                                &format!(
                                    "{} · {} · 0x{:08X}",
                                    choice.kind.label(),
                                    choice.package,
                                    choice.graph
                                ),
                                original == Some(choice.graph),
                            )
                            .on_hover_ui(|ui| {
                                ui.label(&choice.package);
                                if !paths.is_empty() {
                                    ui.label(paths);
                                }
                                if !perks.is_empty() {
                                    ui.label(format!("Used By: {perks}"));
                                }
                                if !contexts.is_empty() {
                                    ui.label("Referenced By");
                                    ui.label(contexts);
                                }
                            })
                            .clicked()
                            .then_some(Some(choice.graph))
                        },
                    )
                },
            );
            self.projectile_query = query_text;
            if let Some(path) = current.and_then(|entry| entry.native_paths.first()) {
                ui.add(egui::Label::new(path).truncate())
                    .on_hover_text(path);
            }
            if let Some(picked) = picked {
                selected = picked;
            }
            if selected != original {
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
}
