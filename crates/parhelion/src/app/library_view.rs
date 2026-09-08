//! Library navigation and transactional build selection are intentionally separate.
use super::*;

type LibraryIconResult = (
    PathBuf,
    AuthoredIconPreviewKey,
    Result<egui::ColorImage, String>,
);

#[derive(Default)]
pub(super) struct LibraryIcons {
    previews: BTreeMap<PathBuf, AuthoredIconPreview>,
    receiver: Option<Receiver<LibraryIconResult>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for LibraryIcons {
    fn drop(&mut self) {
        // Release every package reader before installation or a catalog replacement.
        self.receiver = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl LibraryIcons {
    fn update(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        catalog: &InvestmentCatalog,
        donors: &[WeaponDonorSummary],
        entries: &[RecipeLibraryEntry],
    ) {
        let finished = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished());
        if finished {
            let _ = self.worker.take().unwrap().join();
        }
        if let Some(receiver) = &self.receiver {
            while let Ok((path, key, result)) = receiver.try_recv() {
                let preview = match result {
                    Ok(image) => AuthoredIconPreview::Ready {
                        texture: ctx.load_texture(
                            format!("library-icon-{}", path.display()),
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                        key,
                    },
                    Err(error) => AuthoredIconPreview::Failed { key, error },
                };
                self.previews.insert(path, preview);
            }
        }
        if finished {
            self.receiver = None;
        }
        if self.worker.is_some() {
            return;
        }
        self.previews
            .retain(|path, _| entries.iter().any(|entry| &entry.path == path));
        let missing: Vec<_> = entries
            .iter()
            .filter_map(|entry| {
                let key = AuthoredIconPreviewKey {
                    item_hash: entry.icon_hash,
                    container_tag: catalog.weapon_icon_container(entry.icon_hash)?,
                    rarity: effective_icon_rarity(
                        entry.rarity,
                        donors
                            .iter()
                            .find(|donor| donor.hash == entry.donor_hash)
                            .map(|donor| donor.rarity),
                    )?,
                    edit: entry.icon_edit.clone(),
                };
                let cached = self.previews.get(&entry.path).map(|preview| match preview {
                    AuthoredIconPreview::Ready { key, .. }
                    | AuthoredIconPreview::Failed { key, .. } => key,
                });
                (cached != Some(&key)).then(|| (entry.path.clone(), key))
            })
            .collect();
        if missing.is_empty() {
            return;
        }
        let packages = packages.to_path_buf();
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.worker = Some(thread::spawn(move || {
            let manager = open_shadowkeep_package_manager(&packages);
            for (path, key) in missing {
                let result = manager.as_ref().map_err(Clone::clone).and_then(|manager| {
                    crate::icon_edit::render_weapon_icon_preview_from_manager(
                        manager,
                        TagHash(key.container_tag),
                        key.rarity,
                        &key.edit,
                    )
                });
                if sender.send((path, key, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            ctx.request_repaint();
        }));
    }
}

fn library_entry_details(entry: &RecipeLibraryEntry, donor: Option<&WeaponDonorSummary>) -> String {
    let type_name = entry
        .type_name
        .as_deref()
        .or_else(|| donor.map(|donor| donor.type_name.as_str()))
        .unwrap_or("Weapon");
    let rarity = entry
        .rarity
        .map(|value| format!("{value:?}"))
        .or_else(|| donor.map(|donor| donor.rarity.label().to_owned()));
    let damage = entry
        .damage_type
        .map(|value| format!("{value:?}"))
        .or_else(|| {
            donor
                .and_then(|donor| donor.damage_type)
                .map(|value| value.label().to_owned())
        });
    let ammo = entry
        .ammo_type
        .map(|value| format!("{value:?}"))
        .or_else(|| {
            donor
                .and_then(|donor| donor.ammo_type)
                .map(|value| value.label().to_owned())
        });
    std::iter::once(type_name.to_owned())
        .chain(rarity)
        .chain(damage)
        .chain(ammo)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Search terms may span fields: "arc special" matches "Arc · Special".
fn library_entry_matches(entry: &RecipeLibraryEntry, details: &str, query: &str) -> bool {
    let searchable = format!("{} {} {details}", entry.name, entry.namespace).to_lowercase();
    query
        .split_whitespace()
        .all(|term| searchable.contains(term))
}

/// Both library navigation and build selection use the authored weapon preview.
fn draw_library_row(
    ui: &mut egui::Ui,
    icons: &LibraryIcons,
    entry: &RecipeLibraryEntry,
    details: &str,
    current: bool,
    inclusion: Option<bool>,
) -> bool {
    ui.push_id(&entry.path, |ui| {
        let current = inclusion.unwrap_or(current);
        let mut checkbox_changed = false;
        let mut checkbox_focus = false;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 64.0), egui::Sense::hover());
        let response = ui.interact(
            rect,
            ui.make_persistent_id("weapon-row"),
            if inclusion.is_some() {
                egui::Sense::CLICK
            } else {
                egui::Sense::click()
            },
        );
        let visuals = ui.style().interact_selectable(&response, current);
        let fill = if current {
            ui.visuals().selection.bg_fill
        } else if response.hovered() || response.has_focus() {
            visuals.weak_bg_fill
        } else {
            ui.visuals().faint_bg_color
        };
        ui.painter().rect_filled(rect, visuals.corner_radius, fill);
        let icon_rect =
            egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(48.0, 48.0));
        match icons.previews.get(&entry.path) {
            Some(AuthoredIconPreview::Ready { texture, key })
                if key.edit == entry.icon_edit
                    && key.item_hash == entry.icon_hash
                    && entry.rarity.is_none_or(|rarity| {
                        crate::AuthoredWeaponRarity::from(rarity) == key.rarity
                    }) =>
            {
                egui::Image::new(texture).paint_at(ui, icon_rect);
            }
            Some(AuthoredIconPreview::Failed { error, .. }) => {
                ui.put(icon_rect, egui::Label::new("No icon").wrap())
                    .on_hover_text(error);
            }
            _ if icons.worker.is_some() => {
                ui.put(icon_rect, egui::Spinner::new());
            }
            _ => {
                ui.put(icon_rect, egui::Label::new("No icon").wrap());
            }
        }
        let text_rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(68.0, 8.0),
            rect.max - egui::vec2(8.0, 6.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(
                        (ui.available_width() - 76.0).max(40.0),
                        ui.spacing().interact_size.y,
                    ),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&entry.name).strong())
                                .truncate()
                                .selectable(false),
                        )
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(mut included) = inclusion {
                        let checkbox = ui.checkbox(&mut included, "");
                        checkbox.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                ui.is_enabled(),
                                included,
                                format!("Include {} in this build · {details}", entry.name),
                            )
                        });
                        checkbox_changed = checkbox.changed();
                        checkbox_focus = checkbox.has_focus();
                        if checkbox.gained_focus() {
                            checkbox.scroll_to_me(None);
                        }
                    } else if current {
                        ui.add(egui::Label::new("Open").selectable(false));
                    }
                    if entry.bundled {
                        ui.add(
                            egui::Label::new(egui::RichText::new("Built-in").weak())
                                .selectable(false),
                        );
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(details)
                        .color(ui.visuals().text_color().gamma_multiply(0.8)),
                )
                .truncate()
                .selectable(false),
            );
        });

        if response.has_focus() || checkbox_focus {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                2.0,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if response.gained_focus() {
            response.scroll_to_me(None);
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                current,
                format!("{} · {details}", entry.name),
            )
        });
        let action = if inclusion.is_some() {
            "Toggle inclusion in this build"
        } else {
            "Open recipe to edit"
        };
        let clicked = response
            .on_hover_text(format!("{}\n{details}\n{action}", entry.namespace))
            .clicked();
        ui.add_space(4.0);
        clicked || checkbox_changed
    })
    .inner
}

fn draw_bundled_recipe_selection(
    ui: &mut egui::Ui,
    entries: &[RecipeLibraryEntry],
    selected: &mut BTreeSet<PathBuf>,
) {
    let bundled: Vec<_> = entries.iter().filter(|entry| entry.bundled).collect();
    let count = bundled
        .iter()
        .filter(|entry| selected.contains(&entry.path))
        .count();
    let mut all = !bundled.is_empty() && count == bundled.len();
    if ui.add_enabled(
        !bundled.is_empty(),
        egui::Checkbox::new(&mut all, format!("Include default Parhelion weapons ({}/{})", count, bundled.len()))
            .indeterminate(count > 0 && count < bundled.len()),
    ).on_hover_text("Select or clear every bundled recipe, including recipes hidden by the search. Your custom weapons stay unchanged. Defaults are selected in a new library; Apply selection saves your choice.").changed() {
        for entry in bundled {
            if all {
                selected.insert(entry.path.clone());
            } else {
                selected.remove(&entry.path);
            }
        }
    }
}

impl PackageAuthoringApp {
    pub(super) fn current_recipe_is_in_build(&self) -> bool {
        self.recipe_path
            .as_ref()
            .is_some_and(|path| self.enabled_recipe_paths.contains(path))
    }

    pub(super) fn open_build_selection(&mut self) {
        if self.build_selection_draft.is_none() {
            self.build_selection_draft = Some(self.enabled_recipe_paths.clone());
            self.build_selection_error = None;
            self.build_selection_query.clear();
            self.recipe_search_focus_pending = true;
        }
        self.library_open = false;
    }

    fn draw_library_controls(&mut self, ui: &mut egui::Ui, busy: bool) -> bool {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Refresh").small())
                .clicked()
            {
                self.refresh_recipe_library();
            }
            if ui
                .add_enabled(
                    !busy && !self.recipe_dirty && self.recipe_library.is_some(),
                    egui::Button::new("Restore Default Recipes…").small(),
                )
                .on_hover_text(
                    "Restore bundled recipes from this version. Save or discard open edits first.",
                )
                .clicked()
            {
                match self
                    .recipe_library
                    .as_ref()
                    .unwrap()
                    .prepare_restore_defaults()
                {
                    Ok(preview) => self.restore_defaults_preview = Some(preview),
                    Err(error) => self.log.push(LogEntry::error(error)),
                }
            }
        });
        if self.restore_defaults_preview.is_none() {
            return false;
        }
        ui.group(|ui| {
            ui.label("Restore Default Recipes?");
            ui.label("This replaces saved edits to all bundled recipes and restores missing defaults. Changed files are backed up first. Custom recipes, build selection and installed game files stay unchanged.");
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy && !self.recipe_dirty, egui::Button::new("Restore Defaults")).clicked() {
                    let preview = self.restore_defaults_preview.take().unwrap();
                    let library = self.recipe_library.as_ref().unwrap();
                    match library.restore_defaults(&preview) {
                        Ok(backup) => {
                            self.log.push(LogEntry::info(match backup {
                                Some(path) => format!("Default recipes restored. Backup: {}", path.display()),
                                None => "Default recipes are already up to date.".into(),
                            }));
                            let reload = self.recipe_path.clone().filter(|path|
                                self.recipe_entries.iter().any(|entry| entry.bundled && &entry.path == path));
                            self.refresh_recipe_library();
                            if let Some(path) = reload { self.open_recipe_path(&path); }
                        }
                        Err(error) => self.log.push(LogEntry::error(error)),
                    }
                }
                if ui.button("Cancel").clicked() { self.restore_defaults_preview = None; }
            });
        });
        true
    }

    pub(super) fn draw_library_windows(&mut self, ctx: &egui::Context) {
        // Escape abandons an uncommitted selection before closing library navigation.
        if (self.build_selection_draft.is_some() || self.library_open)
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.build_selection_draft.is_some() {
                self.build_selection_draft = None;
            } else {
                self.library_open = false;
                self.restore_defaults_preview = None;
            }
        }
        let busy = self.has_background_work();
        if (self.library_open || self.build_selection_draft.is_some())
            && let Some(catalog) = &self.catalog
        {
            self.library_icons.update(
                ctx,
                &self.packages,
                catalog,
                &self.donor_summaries,
                &self.recipe_entries,
            );
        }
        let mut open = self.library_open;
        let mut selected = None;
        egui::Window::new("Recipe Library")
            .open(&mut open)
            .default_width(620.0)
            .default_height(720.0)
            .resizable(true)
            .show(ctx, |ui| {
                workbench_style(ui);
                if self.draw_library_controls(ui, busy) {
                    return;
                }
                let search = named_control(
                    ui.add(
                        egui::TextEdit::singleline(&mut self.library_query)
                            .hint_text("Search name, weapon type, element or ammo…")
                            .desired_width(f32::INFINITY),
                    ),
                    "Search recipes",
                );
                if std::mem::take(&mut self.recipe_search_focus_pending) {
                    search.request_focus();
                }
                ui.separator();
                let query = self.library_query.trim().to_lowercase();
                let mut matches = 0;
                egui::ScrollArea::vertical()
                    .id_salt("recipe-library-results")
                    .max_height((ui.available_height() - 45.0).max(120.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in &self.recipe_entries {
                            let donor = self
                                .donor_summaries
                                .iter()
                                .find(|donor| donor.hash == entry.donor_hash);
                            let details = library_entry_details(entry, donor);
                            if !library_entry_matches(entry, &details, &query) {
                                continue;
                            }
                            matches += 1;
                            let current = self.recipe_path.as_ref() == Some(&entry.path);
                            if ui
                                .add_enabled_ui(!busy, |ui| {
                                    draw_library_row(
                                        ui,
                                        &self.library_icons,
                                        entry,
                                        &details,
                                        current,
                                        None,
                                    )
                                })
                                .inner
                            {
                                selected = Some(entry.path.clone());
                            }
                        }
                    });
                if matches == 0 {
                    ui.label("No matching recipes. Try a different search.");
                }
                ui.separator();
                let noun = if matches == 1 { "recipe" } else { "recipes" };
                ui.weak(format!(
                    "{matches} {noun} · Opening a recipe keeps your build selection unchanged."
                ));
            });
        self.library_open = open;
        if !open {
            self.restore_defaults_preview = None;
        }
        if let Some(path) = selected {
            self.library_open = false;
            self.request_recipe_action(PendingRecipeAction::Open(path));
        }

        let Some(mut draft) = self.build_selection_draft.take() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Weapons in This Build")
            .open(&mut open)
            .default_width(620.0)
            .default_height(720.0)
            .resizable(true)
            .show(ctx, |ui| {
                workbench_style(ui);
                ui.label("Choose the weapons to build together.");
                draw_bundled_recipe_selection(ui, &self.recipe_entries, &mut draft);
                let search = named_control(
                    ui.add(
                        egui::TextEdit::singleline(&mut self.build_selection_query)
                            .hint_text("Search name, weapon type, element or ammo…")
                            .desired_width(f32::INFINITY),
                    ),
                    "Search weapons for this build",
                );
                if std::mem::take(&mut self.recipe_search_focus_pending) {
                    search.request_focus();
                }
                let query = self.build_selection_query.trim().to_lowercase();
                let shown: Vec<_> = self
                    .recipe_entries
                    .iter()
                    .filter_map(|entry| {
                        let donor = self
                            .donor_summaries
                            .iter()
                            .find(|donor| donor.hash == entry.donor_hash);
                        let details = library_entry_details(entry, donor);
                        library_entry_matches(entry, &details, &query).then_some((entry, details))
                    })
                    .collect();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} selected · {} shown", draft.len(), shown.len()));
                    let all_selected = shown.iter().all(|(entry, _)| draft.contains(&entry.path));
                    let any_selected = shown.iter().any(|(entry, _)| draft.contains(&entry.path));
                    if ui
                        .add_enabled(!all_selected, egui::Button::new("Select Shown"))
                        .clicked()
                    {
                        draft.extend(shown.iter().map(|(entry, _)| entry.path.clone()));
                    }
                    if ui
                        .add_enabled(any_selected, egui::Button::new("Clear Shown"))
                        .clicked()
                    {
                        for (entry, _) in &shown {
                            draft.remove(&entry.path);
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("build-selection-results")
                    .max_height((ui.available_height() - 75.0).max(120.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (entry, details) in &shown {
                            let included = draft.contains(&entry.path);
                            if draw_library_row(
                                ui,
                                &self.library_icons,
                                entry,
                                details,
                                false,
                                Some(included),
                            ) {
                                if included {
                                    draft.remove(&entry.path);
                                } else {
                                    draft.insert(entry.path.clone());
                                }
                            }
                        }
                        if shown.is_empty() {
                            ui.label("No matching recipes. Try a different search.");
                        }
                    });
                ui.separator();
                if let Some(error) = &self.build_selection_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(!busy, egui::Button::new("Apply Selection"))
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if apply {
            if let Err(error) = self.apply_build_selection(draft.clone()) {
                self.build_selection_error = Some(error.clone());
                self.log.push(LogEntry::error(error));
                self.build_selection_draft = Some(draft);
            } else {
                self.build_selection_error = None;
            }
        } else if open && !cancel {
            self.build_selection_draft = Some(draft);
        }
    }

    pub(super) fn apply_build_selection(
        &mut self,
        selected: BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        let library = self
            .recipe_library
            .as_ref()
            .ok_or("Recipe library is unavailable")?;
        // Commit storage first. A failed write must not change the active build in memory.
        library.save_enabled_paths(&selected, &self.recipe_entries)?;
        if self.enabled_recipe_paths != selected {
            self.enabled_recipe_paths = selected;
            self.invalidate_results();
        }
        Ok(())
    }
}
