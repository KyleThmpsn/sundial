use super::*;

pub(crate) struct Browser<'a> {
    pub packages: Option<&'a Path>,
    pub catalog: Option<&'a InvestmentCatalog>,
    pub current: Option<&'a Icon>,
}

impl Picker {
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        query: &mut String,
        opened: bool,
        height: f32,
        browser: Browser<'_>,
    ) -> Option<Selection> {
        let Browser {
            packages,
            catalog,
            current,
        } = browser;
        self.start(packages, ui.ctx());
        let top = ui.cursor().top();
        self.poll();
        let mut reset = opened || self.clear_query;
        if self.clear_query {
            query.clear();
            self.clear_query = false;
        }
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(query)
                    .hint_text("Search Icon Names, Packages, or Tags")
                    .desired_width((ui.available_width() - 170.0).max(120.0)),
            );
            pickers::name_response(ui, &response, "Search Icons");
            if opened {
                response.request_focus();
            }
            reset |= response.changed();
            egui::ComboBox::from_id_salt("icon-source")
                .selected_text(
                    ["All Icons", "Packages", "My Icons", "destiny-icons"]
                        [usize::from(self.source)],
                )
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (0, "All Icons"),
                        (1, "Packages"),
                        (2, "My Icons"),
                        (3, "destiny-icons"),
                    ] {
                        reset |= ui
                            .selectable_value(&mut self.source, value, label)
                            .changed();
                    }
                });
            pickers::name_combo(ui, "icon-source", "Icon Source");
        });
        if self.purpose == Purpose::Perk {
            reset |= ui.checkbox(&mut self.all_colors, "Show All Colors")
                .on_hover_text("Include colored artwork that meets the same size and transparency requirements.")
                .changed();
        }
        let query = query.trim().to_lowercase();
        let choices: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                (self.purpose != Purpose::Perk || self.all_colors || row.white)
                    && (self.source == 0 || row.source == self.source)
                    && query
                        .split_whitespace()
                        .all(|word| row.search.contains(word.strip_prefix("0x").unwrap_or(word)))
            })
            .map(|(i, _)| i)
            .collect();
        ui.horizontal(|ui| {
            ui.label(format!("{} Icons", choices.len()));
            if self.busy() {
                ui.spinner();
                ui.label("Reading icons…");
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            if let Some((done, total)) = self.progress {
                ui.weak(format!("{done} / {total} textures"));
            }
        });
        if self.purpose == Purpose::Perk {
            ui.weak("White perk glyphs with transparency. Native textures must be 96 × 96.");
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.skipped > 0 {
            ui.weak(format!(
                "{} local files do not meet this browser's artwork requirements.",
                self.skipped
            ));
        }
        ui.separator();
        let remaining = (height - (ui.cursor().top() - top)).max(100.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), remaining),
            egui::Layout::bottom_up(egui::Align::LEFT),
            |ui| {
                let footer = self.footer(ui, catalog);
                ui.separator();
                let grid = ui
                    .allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| self.grid(ui, &choices, reset, current),
                    )
                    .inner;
                footer.or(grid)
            },
        )
        .inner
    }

    fn grid(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[usize],
        reset: bool,
        current: Option<&Icon>,
    ) -> Option<Selection> {
        let mut picked = None;
        let columns = ((ui.available_width() + ui.spacing().item_spacing.x)
            / (78.0 + ui.spacing().item_spacing.x))
            .floor()
            .max(1.0) as usize;
        let grid_height = ui.available_height().max(40.0);
        let mut visible = BTreeSet::new();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("icon-grid")
            .auto_shrink([false, false])
            .max_height(grid_height)
            .min_scrolled_height(grid_height);
        if reset {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show_rows(
            ui,
            78.0,
            choices.len().div_ceil(columns).max(1),
            |ui, range| {
                if choices.is_empty() {
                    ui.weak(if self.busy() {
                        "Looking for transparent icons…"
                    } else {
                        "No matching icons. Clear the search or add artwork below."
                    });
                }
                for row_index in range {
                    ui.horizontal(|ui| {
                        for &index in choices.iter().skip(row_index * columns).take(columns) {
                            visible.insert(index);
                            let row = &self.rows[index];
                            let texture = self.textures.entry(index).or_insert_with(|| {
                                ui.ctx().load_texture(
                                    format!("perk-icon-{index}"),
                                    row.image.clone(),
                                    egui::TextureOptions::LINEAR,
                                )
                            });
                            let size =
                                egui::vec2(row.image.size[0] as f32, row.image.size[1] as f32);
                            let response = ui
                                .add(
                                    egui::Button::image(
                                        egui::Image::new(&*texture).fit_to_exact_size(size),
                                    )
                                    .min_size(egui::vec2(78.0, 78.0))
                                    .selected(current == Some(&row.icon)),
                                )
                                .on_hover_text(&row.label);
                            pickers::name_response(ui, &response, &row.label);
                            if response.clicked() {
                                picked = Some(if self.purpose != Purpose::Perk {
                                    row.local
                                        .as_ref()
                                        .map(|path| Selection::Local(path.clone()))
                                        .unwrap_or_else(|| Selection::Icon(row.icon.clone()))
                                } else {
                                    Selection::Icon(row.icon.clone())
                                });
                            }
                        }
                    });
                }
            },
        );
        self.textures.retain(|index, _| visible.contains(index));
        picked
    }

    fn footer(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
    ) -> Option<Selection> {
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(catalog.is_some(), |ui| {
                let mut perk_query = String::new();
                if let Some(hash) = pickers::popup(
                    ui,
                    "existing-perk-icon",
                    "Select Existing Perk",
                    &mut perk_query,
                    |ui, query, reset, height| {
                        let catalog = catalog?;
                        let choices: Vec<_> = catalog
                            .perk_template_choices_from(
                                crate::package_profile::is_stock_item_definition,
                            )
                            .into_iter()
                            .filter(|choice| pickers::matches(query, &choice.representative_name))
                            .collect();
                        pickers::results(
                            ui,
                            "existing-icons",
                            choices.len(),
                            height,
                            reset,
                            sundial::investment::authoring_choice_row_height(ui),
                            |ui, index| {
                                let choice = &choices[index];
                                catalog
                                    .draw_authoring_choice_row(
                                        ui,
                                        Some(choice.representative_hash),
                                        &choice.representative_name,
                                        Some(&choice.representative_type_name),
                                        false,
                                    )
                                    .clicked()
                                    .then_some(choice.representative_hash)
                            },
                        )
                    },
                ) {
                    picked = Some(Selection::Perk(hash));
                }
            });
            if !self.downloaded
                && ui
                    .add_enabled(
                        !self.local_loading && self.local_workers.is_empty(),
                        egui::Button::new("Download destiny-icons"),
                    )
                    .on_hover_text("Download the optional icon collection by justrealmilk.")
                    .clicked()
            {
                self.local_job(ui.ctx(), true);
            }
            if ui
                .add_enabled(
                    !self.local_loading && self.local_workers.is_empty(),
                    egui::Button::new("Add Icon…"),
                )
                .on_hover_text(self.purpose.guidance())
                .clicked()
            {
                self.local_job(ui.ctx(), false);
            }
            if self.downloaded {
                ui.hyperlink_to("destiny-icons by justrealmilk", library::REPOSITORY);
            }
        });
        picked
    }
}
