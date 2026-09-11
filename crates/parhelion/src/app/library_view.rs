//! Library navigation and transactional build selection are intentionally separate.
use super::*;

mod actions;
mod browser;
mod restore;
mod sharing;
mod state;
use actions::{EntryAction, LibraryAction};
pub(crate) use state::LibraryState;
use state::SortOrder;

struct LibraryRowResponse {
    activated: bool,
    action: Option<EntryAction>,
}

#[derive(Clone, Copy, Default)]
struct LibraryRowState {
    current: bool,
    inclusion: Option<bool>,
    highlighted: bool,
    reveal: bool,
    can_restore: bool,
}

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
    pending: BTreeMap<PathBuf, AuthoredIconPreviewKey>,
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
                self.pending.remove(&path);
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
            for (path, key) in std::mem::take(&mut self.pending) {
                self.previews.insert(
                    path,
                    AuthoredIconPreview::Failed {
                        key,
                        error: "Library icon loading stopped unexpectedly".into(),
                    },
                );
            }
        }
        if self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let paths: BTreeSet<_> = entries.iter().map(|entry| &entry.path).collect();
        self.previews.retain(|path, _| paths.contains(path));
        let missing: Vec<_> = entries
            .iter()
            .filter_map(|entry| {
                let key = AuthoredIconPreviewKey {
                    corner_icon: entry.corner_icon.clone(),
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
        self.pending = missing.iter().cloned().collect();
        self.worker = Some(thread::spawn(move || {
            let manager = open_shadowkeep_package_manager(&packages);
            for (path, key) in missing {
                let result = manager.as_ref().map_err(Clone::clone).and_then(|manager| {
                    crate::icon_edit::render_weapon_icon_preview_from_manager(
                        manager,
                        TagHash(key.container_tag),
                        key.rarity,
                        &key.edit,
                        key.corner_icon.as_ref(),
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

fn matching_library_entries<'a>(
    entries: &'a [RecipeLibraryEntry],
    donors: &[WeaponDonorSummary],
    query: &str,
) -> Vec<(&'a RecipeLibraryEntry, String)> {
    let mut donors_by_hash = BTreeMap::new();
    for donor in donors {
        donors_by_hash.entry(donor.hash).or_insert(donor);
    }
    entries
        .iter()
        .filter_map(|entry| {
            let donor = donors_by_hash.get(&entry.donor_hash).copied();
            let details = library_entry_details(entry, donor);
            library_entry_matches(entry, &details, query).then_some((entry, details))
        })
        .collect()
}

fn library_entry_type<'a>(
    entry: &'a RecipeLibraryEntry,
    donor: Option<&'a WeaponDonorSummary>,
) -> &'a str {
    entry
        .type_name
        .as_deref()
        .or_else(|| donor.map(|donor| donor.type_name.as_str()))
        .unwrap_or("Weapon")
}

fn library_entry_details(entry: &RecipeLibraryEntry, donor: Option<&WeaponDonorSummary>) -> String {
    let type_name = library_entry_type(entry, donor);
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
    state: LibraryRowState,
) -> LibraryRowResponse {
    ui.push_id(&entry.path, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let inclusion = state.inclusion;
        let current = inclusion.unwrap_or(state.current);
        let mut checkbox_changed = false;
        let mut checkbox_focus = false;
        let row_height =
            (ui.spacing().interact_size.y + ui.text_style_height(&egui::TextStyle::Small) + 10.0)
                .max(52.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_height),
            egui::Sense::hover(),
        );
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
        } else if state.highlighted {
            ui.visuals().selection.bg_fill.gamma_multiply(0.35)
        } else if response.hovered() || response.has_focus() {
            visuals.weak_bg_fill
        } else {
            ui.visuals().faint_bg_color
        };
        ui.painter().rect_filled(rect, visuals.corner_radius, fill);
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 6.0, rect.center().y - 22.0),
            egui::vec2(44.0, 44.0),
        );
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
            rect.min + egui::vec2(58.0, 4.0),
            rect.max - egui::vec2(8.0, 4.0),
        );
        let mut menu_action = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
            ui.horizontal(|ui| {
                let trailing_width = 34.0
                    + if entry.bundled { 54.0 } else { 0.0 }
                    + if inclusion.is_none() && (current || state.highlighted) {
                        40.0
                    } else {
                        0.0
                    };
                ui.allocate_ui_with_layout(
                    egui::vec2(
                        (ui.available_width() - trailing_width).max(40.0),
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
                    if inclusion.is_none() {
                        let menu = ui.menu_button("⋯", |ui| {
                            menu_action = actions::entry_menu(ui, entry, state.can_restore);
                        });
                        named_control(menu.response, format!("Recipe Actions for {}", entry.name))
                            .on_hover_text("Recipe actions");
                    }
                    if let Some(mut included) = inclusion {
                        let checkbox = ui.checkbox(&mut included, "");
                        checkbox.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                ui.is_enabled(),
                                included,
                                format!("Select {} · {details}", entry.name),
                            )
                        });
                        checkbox_changed = checkbox.changed();
                        checkbox_focus = checkbox.has_focus();
                        if checkbox.gained_focus() {
                            checkbox.scroll_to_me(None);
                        }
                    } else if current {
                        ui.add(egui::Label::new("Open").selectable(false));
                    } else if state.highlighted {
                        ui.add(
                            egui::Label::new(egui::RichText::new("New").small()).selectable(false),
                        );
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
                        .small()
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
        if state.reveal {
            response.scroll_to_me(Some(egui::Align::Center));
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
            "Toggle selection"
        } else {
            "Open recipe to edit"
        };
        if inclusion.is_none() {
            response.context_menu(|ui| {
                menu_action = actions::entry_menu(ui, entry, state.can_restore);
            });
        }
        let clicked = response
            .on_hover_ui(|ui| {
                sundial::investment::tooltip_title(ui, &entry.namespace);
                ui.label(details);
                ui.label(action);
            })
            .clicked();
        LibraryRowResponse {
            activated: clicked || checkbox_changed,
            action: menu_action,
        }
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

    fn draw_library_controls(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        action: &mut Option<LibraryAction>,
    ) -> bool {
        let busy = busy || self.library_state.export_selection.is_some();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !busy && self.recipe_library.is_some(),
                    egui::Button::new("Import…"),
                )
                .clicked()
            {
                *action = Some(LibraryAction::Import);
            }
            ui.add_enabled_ui(!busy && self.recipe_library.is_some(), |ui| {
                ui.menu_button("Export", |ui| {
                    if ui
                        .add_enabled(
                            self.invalid_weapon_name.is_none(),
                            egui::Button::new("Open Recipe…"),
                        )
                        .clicked()
                    {
                        *action = Some(LibraryAction::ExportOpen);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.recipe_entries.is_empty(),
                            egui::Button::new("Choose Recipes…"),
                        )
                        .clicked()
                    {
                        *action = Some(LibraryAction::ChooseExport);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.recipe_entries.is_empty(),
                            egui::Button::new("All Recipes…"),
                        )
                        .clicked()
                    {
                        *action = Some(LibraryAction::ExportAll);
                        ui.close_menu();
                    }
                });
            });
            ui.separator();
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
        self.poll_library_transfer();
        if self.library_state.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // Escape abandons an uncommitted selection before closing library navigation.
        if (self.build_selection_draft.is_some() || self.library_open)
            && self.library_state.restore.is_none()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.library_state.export_selection.is_some() && !self.library_state.busy() {
                self.library_state.export_selection = None;
            } else if self.build_selection_draft.is_some() {
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
        self.draw_library_browser(ctx, busy);
        self.draw_recipe_restore(ctx, busy);

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
                let shown =
                    matching_library_entries(&self.recipe_entries, &self.donor_summaries, &query);
                ui.horizontal_wrapped(|ui| {
                    let all_selected = shown.iter().all(|(entry, _)| draft.contains(&entry.path));
                    let any_selected = shown.iter().any(|(entry, _)| draft.contains(&entry.path));
                    if ui
                        .add_enabled(!all_selected, egui::Button::new("Select All"))
                        .on_hover_text("Select all recipes shown by this search.")
                        .clicked()
                    {
                        draft.extend(shown.iter().map(|(entry, _)| entry.path.clone()));
                    }
                    if ui
                        .add_enabled(any_selected, egui::Button::new("Clear All"))
                        .on_hover_text("Clear all recipes shown by this search.")
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
                                LibraryRowState {
                                    inclusion: Some(included),
                                    ..Default::default()
                                },
                            )
                            .activated
                            {
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
                let footer_height = ui.spacing().interact_size.y
                    + ui.spacing().item_spacing.y * 2.0
                    + 1.0
                    + if self.build_selection_error.is_some() {
                        ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y
                    } else {
                        0.0
                    };
                ui.add_space((ui.available_height() - footer_height).max(0.0));
                ui.separator();
                if let Some(error) = &self.build_selection_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(!busy, egui::Button::new("Apply Selection"))
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(format!("{} selected · {} shown", draft.len(), shown.len()));
                    });
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
