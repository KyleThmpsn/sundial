//! The workbench frame around the pages: tabs, the recipe editor and library panels, the
//! tools menu, actions, notices and confirmations.
use super::*;

impl PackageAuthoringApp {
    pub(super) fn draw_workbench_tabs(&mut self, ui: &mut egui::Ui) {
        let kind = self.recipe.kind;
        let pages = WorkbenchPage::for_kind(kind);
        if !pages.contains(&self.workbench_page) {
            self.workbench_page = WorkbenchPage::Weapon;
        }
        ui.horizontal_wrapped(|ui| {
            for &page in pages {
                ui.selectable_value(&mut self.workbench_page, page, page.label_for(kind));
            }
        });
    }

    pub(super) fn draw_recipe_editor(&mut self, ui: &mut egui::Ui) {
        if self.catalog.is_none() {
            if self.install_receiver.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Installing…");
                });
                return;
            }
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "Weapon catalog unavailable.");
                if ui
                    .add_enabled(
                        !self.has_background_work(),
                        egui::Button::new("Reload Catalog"),
                    )
                    .clicked()
                {
                    self.reset_catalog_load();
                }
            });
        }
        if !self.show_experimental_options {
            let hidden_features = technical_recipe_features(&self.recipe);
            if !hidden_features.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "Additional Overrides ({})",
                    hidden_features.len()
                ))
                .id_salt("hidden_recipe_overrides")
                .show(ui, |ui| {
                    for feature in hidden_features {
                        ui.label(feature);
                    }
                    if ui.button("Show Technical Controls").clicked() {
                        self.set_show_experimental_options(true);
                    }
                });
            }
        }
        match self.workbench_page {
            WorkbenchPage::Weapon if !self.recipe.kind.is_weapon() => self.draw_gear_editor(ui),
            WorkbenchPage::Weapon => self.draw_core_recipe_editor(ui),
            WorkbenchPage::Appearance if self.recipe.kind == ItemKind::Subclass => {
                self.draw_subclass_appearance(ui);
            }
            WorkbenchPage::Appearance => self.draw_appearance_workspace(ui),
            WorkbenchPage::Collections => self.draw_collections_workspace(ui),
            WorkbenchPage::Advanced => {
                let donor = self.current_donor();
                self.draw_gameplay_workspace(ui, donor.as_ref());
                // A behavior chosen here claims its sockets in the same frame, so the choice and
                // the perks it places are one undo step. Undo then never lands between them,
                // where the next sync would take the perks straight back.
                if self.recipe.kind.is_weapon()
                    && let Some(donor) = &donor
                {
                    super::socket_editor::sync_behavior_socket_pins(
                        &mut self.recipe,
                        &mut self.behavior_pins,
                        donor,
                    );
                }
            }
            WorkbenchPage::Identity => self.draw_identity_workspace(ui),
        }
    }

    pub(super) fn recipe_panel_scope(&self) -> String {
        self.recipe_path.as_ref().map_or_else(
            || format!("unsaved:{}", self.recipe.identity.item_hash),
            |path| path.display().to_string(),
        )
    }

    pub(super) fn draw_recipe_library(&mut self, ui: &mut egui::Ui) -> bool {
        let mut replaced = false;
        let Some(library) = self.recipe_library.clone() else {
            draw_authoring_toolbar(ui, |ui| {
                self.draw_new_item_button(ui, &mut replaced);
                ui.separator();
                if ui.button("Custom Perk Workbench…").clicked() {
                    self.perk_workbench.open = true;
                }
                self.draw_tools_menu(ui);
                ui.separator();
                ui.colored_label(ui.visuals().warn_fg_color, "Recipe library unavailable.");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Preferences…").clicked() {
                        self.preferences_open = true;
                    }
                });
            });
            return replaced;
        };

        draw_authoring_toolbar(ui, |ui| {
            self.draw_recipe_library_primary(ui, &library, &mut replaced);
            ui.menu_button("Recipe…", |ui| {
                self.draw_recipe_library_actions(ui, &mut replaced);
            });
            ui.separator();
            if ui.button("Custom Perk Workbench…").clicked() {
                self.perk_workbench.open = true;
            }
            self.draw_tools_menu(ui);
            ui.separator();
            if ui.button("Preferences…").clicked() {
                self.preferences_open = true;
            }
        });
        replaced
    }

    /// New makes another of the kind open now in one click, so it reads New Armor on an armor
    /// page. The caret beside it lists every kind.
    pub(super) fn draw_new_item_button(&mut self, ui: &mut egui::Ui, replaced: &mut bool) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let kind = self.recipe.kind;
            if ui.button(format!("New {}", kind.label())).clicked() {
                *replaced |= self.request_recipe_action(PendingRecipeAction::New(kind));
            }
            let menu = ui.menu_button(
                crate::app::style::icon(ui, egui_phosphor::regular::CARET_DOWN),
                |ui| {
                    for kind in ItemKind::ALL {
                        // Weapons and armor, then the rest of the loadout.
                        if kind == ItemKind::Sparrow {
                            ui.separator();
                        }
                        if ui.button(kind.label()).clicked() {
                            *replaced |= self.request_recipe_action(PendingRecipeAction::New(kind));
                            ui.close();
                        }
                    }
                },
            );
            named_control(menu.response, "New Item").on_hover_text("New Item");
        });
    }

    pub(super) fn draw_tools_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Tools", |ui| {
            #[cfg(feature = "d2-model-importer")]
            if self.importer.enabled && ui.button("D2 Importer…").clicked() {
                self.importer.open = true;
                ui.close();
            }
            if self.show_experimental_options && ui.button("Engine Catalog…").clicked() {
                self.perk_workbench.open_engine_catalog();
                ui.close();
            }
            if ui
                .button("Technical Build…")
                .on_hover_text("What the next build assigns, or what the staged build produced.")
                .clicked()
            {
                self.technical_build_open = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    self.account_resync_receiver.is_none()
                        && self.install_receiver.is_none()
                        && !self.uninstall.open,
                    egui::Button::new("Resync Account"),
                )
                .on_hover_text(
                    "Apply the installed unlocks and items to the current account again.",
                )
                .clicked()
            {
                self.start_account_resync();
                ui.close();
            }
        });
    }

    pub(super) fn draw_recipe_library_primary(
        &mut self,
        ui: &mut egui::Ui,
        library: &RecipeLibrary,
        replaced: &mut bool,
    ) {
        if ui
            .add_enabled(
                self.build_selection_draft.is_none(),
                egui::Button::new("Library…"),
            )
            .clicked()
        {
            self.library_open = true;
            self.recipe_search_focus_pending = true;
        }
        self.draw_new_item_button(ui, replaced);
        ui.separator();
        ui.allocate_ui_with_layout(
            egui::vec2(
                ui.available_width().min(220.0),
                ui.spacing().interact_size.y,
            ),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&self.recipe.name).strong()).truncate(),
                );
            },
        );
        let can_save_existing = self
            .recipe_path
            .as_ref()
            .is_some_and(|path| path.starts_with(library.root()));
        let save_status = RecipeSaveStatus::derive(can_save_existing, self.recipe_dirty);
        let status_color = match save_status {
            RecipeSaveStatus::NotSavedYet => ui.visuals().text_color(),
            RecipeSaveStatus::UnsavedChanges => ui.visuals().warn_fg_color,
            RecipeSaveStatus::Saved => style::success_color(ui.visuals()),
        };
        ui.label(egui::RichText::new(save_status.label()).color(status_color));
        ui.separator();
        let save_label = if can_save_existing {
            "Save Changes"
        } else {
            "Save Recipe"
        };
        let can_save =
            (self.recipe_dirty || !can_save_existing) && self.invalid_weapon_name.is_none();
        if ui
            .add_enabled(can_save, egui::Button::new(save_label))
            .on_disabled_hover_text(if self.invalid_weapon_name.is_some() {
                "Enter a valid weapon name before saving"
            } else {
                "This recipe has no unsaved changes"
            })
            .clicked()
        {
            if can_save_existing {
                self.save_library_recipe();
            } else {
                self.save_recipe_copy();
            }
        }
    }

    pub(super) fn draw_recipe_library_actions(&mut self, ui: &mut egui::Ui, replaced: &mut bool) {
        for (label, redo, key) in [("Undo", false, egui::Key::Z), ("Redo", true, egui::Key::Y)] {
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);
            if ui
                .add_enabled(
                    self.recipe_history_available(redo),
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut)),
                )
                .clicked()
            {
                self.restore_recipe_history(ui.ctx(), redo);
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button("Duplicate")
            .on_hover_text(
                "Copy this weapon and its custom perks as a new weapon. Save to keep it.",
            )
            .clicked()
        {
            *replaced |= self.duplicate_recipe();
            ui.close();
        }
        if ui.button("Export…").clicked() {
            self.export_recipe();
            ui.close();
        }
        if ui.button("Import…").clicked() {
            *replaced |= self.request_recipe_action(PendingRecipeAction::Import);
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(self.recipe_dirty, egui::Button::new("Discard Changes"))
            .on_hover_text("Revert to the last saved version")
            .clicked()
        {
            self.discard_recipe_changes();
            *replaced = true;
            ui.close();
        }
    }

    pub(super) fn draw_discard_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_recipe_action.clone() else {
            return;
        };
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let response = egui::Modal::new("parhelion_discard_recipe".into()).show(ctx, |ui| {
            workbench_style(ui);
            ui.set_width(420.0_f32.min((ctx.content_rect().width() - 48.0).max(180.0)));
            ui.heading("Unsaved Recipe Changes");
            ui.add_space(6.0);
            ui.label(match &action {
                PendingRecipeAction::Close => {
                    "Save this recipe before closing Parhelion, or discard its unsaved changes."
                }
                PendingRecipeAction::New(_) => {
                    "Save this recipe before creating a new one, or discard its unsaved changes."
                }
                PendingRecipeAction::Open(_) => {
                    "Save this recipe before opening another, or discard its unsaved changes."
                }
                PendingRecipeAction::Import => {
                    "Save this recipe before importing another, or discard its unsaved changes."
                }
            });
            if let Some(error) = self.pending_recipe_error.as_deref().or_else(|| {
                self.invalid_weapon_name
                    .as_ref()
                    .map(|(_, error)| error.as_str())
            }) {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                        .wrap(),
                );
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                save = ui
                    .add_enabled(
                        self.invalid_weapon_name.is_none(),
                        egui::Button::new(match action {
                            PendingRecipeAction::Close => "Save and Close",
                            _ => "Save and Continue",
                        })
                        .fill(ui.visuals().selection.bg_fill),
                    )
                    .clicked();
                if ui
                    .button(match action {
                        PendingRecipeAction::Close => "Discard and Close",
                        _ => "Discard and Continue",
                    })
                    .clicked()
                {
                    discard = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        cancel |= response.should_close();
        if save {
            match self.try_save_open_recipe() {
                Ok(()) => {
                    self.pending_recipe_action = None;
                    self.pending_recipe_error = None;
                    self.execute_recipe_action(action);
                }
                Err(error) => {
                    self.log.push(LogEntry::error(&error));
                    self.pending_recipe_error = Some(error);
                }
            }
        } else if discard {
            self.pending_recipe_action = None;
            self.pending_recipe_error = None;
            self.execute_recipe_action(action);
        } else if cancel {
            self.pending_recipe_action = None;
            self.pending_recipe_error = None;
        }
    }

    pub(super) fn draw_actions(&mut self, ui: &mut egui::Ui) {
        let building = self.build_receiver.is_some();
        let installing = self.install_receiver.is_some();
        let running =
            building || installing || self.perk_workbench.editing() || self.library_state.busy();
        let included_count = self.enabled_recipe_paths.len();
        let catalog_ready = self.catalog.is_some()
            && self.catalog_receiver.is_none()
            && self.catalog_worker.is_none()
            && !self.catalog_reload_pending;
        let project_ready = included_count > 0 && catalog_ready;
        let maps = self
            .enabled_recipe_paths
            .iter()
            .filter(|path| self.map_entries.iter().any(|map| &map.path == *path))
            .count();
        let noun = match (maps, included_count) {
            // One pass over the library each frame. Finding each included recipe in it took
            // millions of path comparisons a frame for a library of 2,000 included recipes.
            (0, _) => ItemKind::count_noun(
                self.recipe_entries
                    .iter()
                    .filter(|entry| self.enabled_recipe_paths.contains(&entry.path))
                    .map(|entry| entry.kind),
                included_count,
            ),
            (1, 1) => "Map",
            (maps, count) if maps == count => "Maps",
            (_, 1) => "Item",
            _ => "Items",
        };
        ui.horizontal_wrapped(|ui| {
            let primary_fill = ui.visuals().selection.bg_fill;
            if ui
                .add_enabled(
                    !running,
                    egui::Button::new(format!("{included_count} {noun} in Build…"))
                        .min_size([0.0, ui.spacing().interact_size.y].into()),
                )
                .clicked()
            {
                self.open_build_selection();
            }
            let checking = self.build_check.busy();
            if ui
                .add_enabled(
                    !running && !checking && project_ready && self.build_selection_draft.is_none(),
                    egui::Button::new(
                        egui::RichText::new(if checking {
                            "Checking Installed Items…"
                        } else {
                            "Build & Stage"
                        })
                        .strong(),
                    )
                    .fill(primary_fill)
                    .min_size([160.0, ui.spacing().interact_size.y].into()),
                )
                .clicked()
            {
                self.start_build_checked(ui.ctx());
            }
            if (building || installing || self.latest_build.is_some())
                && ui.button("Build & Install Status…").clicked()
            {
                self.build_status_open = true;
            }
            self.draw_activity(ui);
            self.draw_build_notice(ui);
        });
        if included_count == 0 {
            ui.colored_label(ui.visuals().warn_fg_color, "Nothing selected to build.");
        } else if !catalog_ready && !installing {
            ui.colored_label(ui.visuals().warn_fg_color, "Weapon catalog unavailable.");
        }
    }

    /// Running background work, each with the window that shows it and the lines its hover adds.
    fn activities(&self) -> Vec<(String, Option<ActivityWindow>, Vec<String>)> {
        let mut tasks = Vec::new();
        if self.build_receiver.is_some() {
            tasks.push((
                "Building…".to_owned(),
                Some(ActivityWindow::BuildStatus),
                Vec::new(),
            ));
        }
        if self.install_receiver.is_some() {
            tasks.push((
                "Installing…".to_owned(),
                Some(ActivityWindow::BuildStatus),
                Vec::new(),
            ));
        }
        if self.build_check.busy() {
            tasks.push(("Checking Installed Items…".to_owned(), None, Vec::new()));
        }
        if self.catalog_receiver.is_some() || self.catalog_worker.is_some() {
            tasks.push(("Loading Catalog…".to_owned(), None, Vec::new()));
        }
        #[cfg(feature = "d2-model-importer")]
        tasks.extend(
            self.importer
                .activities()
                .into_iter()
                .map(|(label, shown, details)| {
                    (label, shown.then_some(ActivityWindow::Importer), details)
                }),
        );
        tasks
    }

    /// A spinner and the first running background task, with every task on hover. A click
    /// opens the task's window. Nothing shows while all is idle.
    fn draw_activity(&mut self, ui: &mut egui::Ui) {
        let tasks = self.activities();
        let Some((first, window, _)) = tasks.first().cloned() else {
            return;
        };
        let color = ui.visuals().weak_text_color();
        let text = match tasks.len() {
            1 => first,
            count => format!("{first} +{}", count - 1),
        };
        let shown = ui
            .horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0).color(color));
                ui.label(egui::RichText::new(text).color(color));
            })
            .response;
        let mut response = ui
            .interact(
                shown.rect,
                ui.id().with("background-activity"),
                egui::Sense::click(),
            )
            .on_hover_ui(|ui| {
                for (label, _, details) in &tasks {
                    ui.label(label);
                    for line in details {
                        ui.weak(line);
                    }
                }
            });
        if window.is_some() {
            response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            match window {
                Some(ActivityWindow::BuildStatus) => self.build_status_open = true,
                #[cfg(feature = "d2-model-importer")]
                Some(ActivityWindow::Importer) => self.importer.open = true,
                None => {}
            }
        }
    }

    pub(super) fn draw_build_notice(&self, ui: &mut egui::Ui) {
        if self.current_recipe_is_in_build() {
            return;
        }
        let message = if self.recipe_path.is_none() {
            "Recipe not saved or in build."
        } else {
            "Open recipe not in build."
        };
        let color = ui.visuals().warn_fg_color;
        let width = ui
            .available_size_before_wrap()
            .x
            .max(260.0)
            .min(ui.max_rect().width());
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(message).color(color))
                        .wrap()
                        .halign(egui::Align::RIGHT),
                );
            },
        );
    }

    pub(super) fn draw_action_error(&mut self, ui: &mut egui::Ui) {
        let diagnostic = self
            .latest_install
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|report| {
                [
                    report
                        .profile_sync
                        .as_ref()
                        .and_then(|result| result.as_ref().err()),
                    report
                        .item_grants
                        .as_ref()
                        .and_then(|result| result.as_ref().err()),
                ]
                .into_iter()
                .flatten()
                .next()
            })
            .map(|error| (ActionDiagnostic::AccountSync, error.as_str()))
            .or_else(|| {
                self.latest_install
                    .as_ref()
                    .and_then(|report| report.as_ref().err())
                    .map(|error| (ActionDiagnostic::Installation, error.as_str()))
            })
            .or_else(|| {
                self.latest_build
                    .as_ref()
                    .and_then(|report| report.as_ref().err())
                    .map(|error| (ActionDiagnostic::Build, error.as_str()))
            })
            .or_else(|| {
                self.log
                    .notice
                    .as_deref()
                    .map(|error| (ActionDiagnostic::Notice, error))
            });
        let Some((kind, error)) = diagnostic else {
            return;
        };

        let mut dismiss = false;
        egui::ScrollArea::vertical()
            .id_salt("parhelion_action_error")
            .max_height(72.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(safe_content_width(ui.available_width()));
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(kind.title())
                            .strong()
                            .color(ui.visuals().error_fg_color),
                    );
                    if kind == ActionDiagnostic::Notice {
                        dismiss = ui.button("Dismiss").clicked();
                    }
                });
                ui.add(
                    egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                        .wrap(),
                );
            });
        if dismiss {
            self.log.notice = None;
        }
        ui.add_space(5.0);
    }
}

/// The window that shows a background task.
#[derive(Clone, Copy)]
enum ActivityWindow {
    BuildStatus,
    #[cfg(feature = "d2-model-importer")]
    Importer,
}
