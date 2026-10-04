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
        let columns = ((ui.available_width() + ui.spacing().item_spacing.x)
            / (78.0 + ui.spacing().item_spacing.x))
            .floor()
            .max(1.0) as usize;
        let grid_height = ui.available_height().max(40.0);
        let mut on_screen = HashSet::new();
        let mut requested = Vec::new();
        let mut clicked = None;
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
                        "No Matching Results"
                    });
                }
                for row_index in range {
                    ui.horizontal(|ui| {
                        for &index in choices.iter().skip(row_index * columns).take(columns) {
                            let row = &self.rows[index];
                            on_screen.insert(row.origin.clone());
                            let thumbnail = self
                                .thumbnails
                                .entry(row.origin.clone())
                                .or_insert_with(|| {
                                    requested.push(row.origin.clone());
                                    Thumbnail::Pending
                                });
                            // A tile keeps its size while its thumbnail loads.
                            let tile = match thumbnail.texture(ui.ctx(), index) {
                                Some(texture) => egui::Button::image(
                                    egui::Image::new(texture).fit_to_exact_size(row.extent),
                                ),
                                None => egui::Button::new(""),
                            };
                            let response = ui
                                .add(
                                    tile.min_size(egui::vec2(78.0, 78.0))
                                        .selected(row.origin.is(current)),
                                )
                                .on_hover_text(&row.label);
                            pickers::name_response(ui, &response, &row.label);
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
                }
            },
        );
        self.thumbnails
            .retain(|origin, _| on_screen.contains(origin));
        self.request_thumbnails(ui.ctx(), on_screen, requested);
        clicked.and_then(|index| self.pick(index))
    }

    /// What picking row `index` selects. A local file becomes a perk's icon only when it is
    /// picked, so the library holds no icon per file.
    fn pick(&mut self, index: usize) -> Option<Selection> {
        match &self.rows[index].origin {
            Origin::Texture(tag) => Some(Selection::Icon(Icon::Texture { tag: (*tag).into() })),
            Origin::File { path, .. } if self.purpose != Purpose::Perk => {
                Some(Selection::Local(path.clone()))
            }
            Origin::File { path, name } => match library::icon(path, name) {
                Ok(icon) => Some(Selection::Icon(icon)),
                Err(error) => {
                    self.error = Some(error);
                    None
                }
            },
        }
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
                if let Some(hash) = pickers::browser_with_toolbar(
                    ui,
                    "existing-perk-icon",
                    "Select Existing Perk",
                    "Select Existing Perk",
                    &mut perk_query,
                    |ui, query, opened, _| {
                        let catalog = catalog?;
                        let searched = ui
                            .horizontal(|ui| {
                                let width =
                                    (ui.available_width() - pickers::CLEAR_WIDTH).max(160.0);
                                sundial::ui::catalog::search(
                                    ui,
                                    query,
                                    opened,
                                    width,
                                    "Search Perks",
                                )
                            })
                            .inner;
                        let words = query.trim().to_lowercase();
                        let choices: Vec<_> = catalog
                            .perk_template_choices_from(
                                crate::package_profile::is_stock_item_definition,
                            )
                            .into_iter()
                            .filter(|choice| pickers::matches(&words, &choice.representative_name))
                            .collect();
                        let keys = choices
                            .iter()
                            .map(|choice| u64::from(choice.representative_hash))
                            .collect::<Vec<_>>();
                        pickers::BrowserList {
                            keys: &keys,
                            // The result count takes a line above the list.
                            height: (ui.available_height() - 24.0).max(160.0),
                            reset: opened || searched,
                            row_height: sundial::investment::authoring_choice_row_height(ui),
                            select: None,
                        }
                        .draw_activating(
                            ui,
                            |ui, index, selected| {
                                let choice = &choices[index];
                                catalog.draw_authoring_choice_row(
                                    ui,
                                    Some(choice.representative_hash),
                                    &choice.representative_name,
                                    Some(&choice.representative_type_name),
                                    selected,
                                )
                            },
                            |ui, index, activated| {
                                let choice = &choices[index];
                                ui.heading(&choice.representative_name);
                                // A double-click takes the icon, as Use Icon does.
                                let use_icon = ui.button("Use Icon").clicked() || activated;
                                ui.label(&choice.representative_type_name);
                                catalog.draw_perk_icon(ui, choice.representative_hash, 96.0);
                                use_icon.then_some(choice.representative_hash)
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
