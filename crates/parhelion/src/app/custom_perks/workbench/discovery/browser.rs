use super::*;

impl Discovery {
    pub fn show(&mut self, ctx: &egui::Context, choices: &[WeaponSandboxPerkChoice]) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Native Asset Browser")
            .id(egui::Id::new("native-asset-browser"))
            .open(&mut open)
            .collapsible(false)
            .default_size(egui::vec2(1000.0, 700.0))
            .min_width(580.0)
            .min_height(420.0)
            .max_width((ctx.screen_rect().width() - 40.0).max(580.0))
            .max_height((ctx.screen_rect().height() - 64.0).max(420.0))
            .show(ctx, |ui| {
                crate::app::style::perk_style(ui);
                self.draw(ui, choices);
            });
        self.open = open;
    }

    fn draw(&mut self, ui: &mut egui::Ui, choices: &[WeaponSandboxPerkChoice]) {
        ui.label(
            "Inspect installed assets and their references. This browser does not change a perk.",
        );
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading Native Content…");
            });
            if let Some((current, total)) = self.progress {
                ui.small(format!("{current} Of {total} Resources"));
            }
            return;
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let Some(data) = &self.data else {
            return;
        };
        ui.small(format!(
            "{} Effect Entities · {} Perk Records · {} TFT References",
            data.effects.entries.len(),
            data.perks.perks.len(),
            data.names.references.len()
        ));
        let previous = self.view;
        ui.horizontal_wrapped(|ui| {
            for (value, label) in [
                (View::Projectiles, "Projectiles"),
                (View::Emitters, "Emitters"),
                (View::Perks, "Perks"),
                (View::AllPaths, "TFT Paths"),
                (View::AllReferences, "TFT References"),
            ] {
                ui.selectable_value(&mut self.view, value, label);
            }
        });
        if self.view != previous {
            self.selected = None;
            self.filter_query = None;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("Search Names, Paths, or Tags")
                .desired_width(f32::INFINITY),
        );
        let query = self
            .query
            .trim()
            .trim_start_matches("0x")
            .to_ascii_lowercase();
        if self.row_key != Some((self.view, choices.len())) {
            let labels = choices
                .iter()
                .map(|choice| {
                    (
                        usize::from(choice.perk_index),
                        choice.representative_name.as_str(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let mut rows = match self.view {
                View::Projectiles | View::Emitters => data
                    .effects
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| {
                        entry.kind
                            == if self.view == View::Projectiles {
                                projectile::Kind::Projectile
                            } else {
                                projectile::Kind::Emitter
                            }
                    })
                    .map(|(index, entry)| {
                        let name = entry
                            .native_paths
                            .first()
                            .map(|path| tft::asset_label(path))
                            .or_else(|| entry.native_name.clone())
                            .unwrap_or_else(|| format!("Unidentified {}", entry.kind.label()));
                        (
                            index,
                            format!("{name} · 0x{:08X}", entry.graph),
                            format!(
                                "{name} {:08X} {} {}",
                                entry.graph,
                                entry.package,
                                entry.native_paths.join(" ")
                            ),
                        )
                    })
                    .collect::<Vec<_>>(),
                View::Perks => data
                    .perks
                    .perks
                    .iter()
                    .enumerate()
                    .map(|(index, perk)| {
                        let name = labels.get(&perk.index).map_or_else(
                            || format!("Effect {}", perk.index),
                            |name| (*name).to_owned(),
                        );
                        let paths = u16::try_from(perk.index)
                            .ok()
                            .and_then(|index| data.perk_search.get(&index))
                            .map_or("", String::as_str);
                        (
                            index,
                            format!("{name} · Effect {}", perk.index),
                            format!("{name} {} {:08X} {paths}", perk.index, perk.hash),
                        )
                    })
                    .collect(),
                View::AllReferences => data
                    .names
                    .references
                    .iter()
                    .enumerate()
                    .map(|(index, reference)| {
                        (
                            index,
                            format!(
                                "{} · 0x{:08X}",
                                tft::asset_label(&reference.path),
                                reference.target
                            ),
                            format!(
                                "{} {:08X} {:08X}",
                                reference.path, reference.source, reference.target
                            ),
                        )
                    })
                    .collect(),
                View::AllPaths => data
                    .names
                    .paths
                    .iter()
                    .enumerate()
                    .map(|(index, path)| {
                        (
                            index,
                            format!("{} · 0x{:08X}", tft::asset_label(&path.path), path.source),
                            format!("{} {:08X}", path.path, path.source),
                        )
                    })
                    .collect(),
            };
            rows.sort_by_cached_key(|(index, label, _)| {
                let unnamed = match self.view {
                    View::Projectiles | View::Emitters => {
                        data.effects.entries[*index].native_paths.is_empty()
                            && data.effects.entries[*index].native_name.is_none()
                    }
                    View::Perks => !labels.contains_key(&data.perks.perks[*index].index),
                    View::AllPaths | View::AllReferences => false,
                };
                (unnamed, label.to_ascii_lowercase())
            });
            self.rows = rows
                .into_iter()
                .map(|(index, label, search)| Row {
                    index,
                    label,
                    search: search.to_ascii_lowercase(),
                })
                .collect();
            self.row_key = Some((self.view, choices.len()));
            self.filter_query = None;
        }
        let reset = self.filter_query.as_ref() != Some(&query);
        if reset {
            self.filtered = self
                .rows
                .iter()
                .enumerate()
                .filter_map(|(index, row)| row.search.contains(&query).then_some(index))
                .collect();
            self.filter_query = Some(query);
        }
        ui.label(format!("{} Results", self.filtered.len()));
        let available = (ui.available_height() - 52.0).max(180.0);
        let details_height = if self.selected.is_some() {
            (available * 0.35).clamp(100.0, 220.0)
        } else {
            0.0
        };
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("native-asset-list", self.view))
            .auto_shrink([false, false])
            .max_height((available - details_height - 16.0).max(80.0));
        if reset {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show_rows(
            ui,
            crate::app::style::list_row_height(ui),
            self.filtered.len(),
            |ui, visible| {
                for position in visible {
                    let row = &self.rows[self.filtered[position]];
                    if crate::app::style::list_row(ui, self.selected == Some(row.index), &row.label)
                        .clicked()
                    {
                        self.selected = Some(row.index);
                    }
                }
            },
        );
        if let Some(selected) = self.selected {
            ui.separator();
            egui::ScrollArea::vertical().id_salt("native-asset-details")
                .max_height(details_height.max(100.0)).auto_shrink([false, false]).show(ui, |ui| {
            match self.view {
                View::Projectiles | View::Emitters => {
                    if let Some(entry) = data.effects.entries.get(selected) {
                        ui.strong(format!("{} · 0x{:08X}", entry.kind.label(), entry.graph));
                        ui.label(&entry.package);
                        if entry.native_paths.is_empty() {
                            ui.label("No TFT path names this asset in the recovered map.");
                        }
                        for path in &entry.native_paths {
                            draw_path(ui, path);
                        }
                        egui::CollapsingHeader::new("Referenced By").show(ui, |ui| {
                            ui.small("These paths name resources that refer to this asset.");
                            for context in &entry.contexts {
                                ui.label(&context.path);
                                ui.small(format!(
                                    "Graph 0x{:08X} · Owner 0x{:08X} + 0x{:X}",
                                    context.graph, context.owner, context.offset
                                ));
                            }
                        });
                    }
                }
                View::Perks => {
                    if let Some(perk) = data.perks.perks.get(selected) {
                        ui.strong(perk.status());
                        if let Some(action) = perk.action {
                            ui.label(format!("Action 0x{action:08X}"));
                        }
                        if let Some(error) = &perk.error {
                            ui.colored_label(ui.visuals().warn_fg_color, error);
                        }
                        if let Some(assets) = data.perk_assets.get(selected) {
                            for (label, references) in [
                                ("Action References", &assets.action),
                                ("Graph References", &assets.graphs),
                                ("Component References", &assets.components),
                            ] {
                                if references.is_empty() {
                                    continue;
                                }
                                egui::CollapsingHeader::new(format!(
                                    "{label} ({})",
                                    references.len()
                                ))
                                .default_open(true)
                                .show(ui, |ui| {
                                    for &index in references {
                                        draw_reference(ui, &data.names.references[index]);
                                    }
                                });
                            }
                        }
                        for graph in &perk.graphs {
                            ui.small(format!(
                                "Graph 0x{:08X} · {} Components",
                                graph.tag,
                                graph.components.len()
                            ));
                        }
                    }
                }
                View::AllReferences => {
                    if let Some(reference) = data.names.references.get(selected) {
                        draw_reference(ui, reference);
                    }
                }
                View::AllPaths => {
                    if let Some(path) = data.names.paths.get(selected) {
                        draw_path(ui, &path.path);
                        ui.small(format!(
                            "Stored In 0x{:08X} + 0x{:X}",
                            path.source, path.offset
                        ));
                        ui.small("A path stored here may refer to another resource. TFT References shows the resolved links.");
                    }
                }
            }
            });
        }
        ui.separator();
        if ui.button("Export Native Map…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("native-perk-map.json")
                .save_file()
            {
                let value = serde_json::json!({"tft":&*data.names,"effects":&*data.effects,"perks":&*data.perks,"perk_assets":&data.perk_assets});
                let result = serde_json::to_vec_pretty(&value)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        sundial::package_authoring::replace_authoring_file(&path, &bytes)
                            .map_err(|error| error.to_string())
                    });
                if let Err(error) = result {
                    self.error = Some(error);
                }
            }
        }
    }
}

fn draw_reference(ui: &mut egui::Ui, reference: &tft::Reference) {
    draw_path(ui, &reference.path);
    ui.small(format!(
        "0x{:08X} + 0x{:X} → 0x{:08X} · Class 0x{:08X}",
        reference.source, reference.offset, reference.target, reference.target_class
    ));
}

fn draw_path(ui: &mut egui::Ui, path: &str) {
    ui.add(egui::Label::new(path).wrap());
    if ui.button("Copy Path").clicked() {
        ui.ctx().copy_text(path.to_owned());
    }
}
