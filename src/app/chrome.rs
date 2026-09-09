//! Application navigation, status chrome, and about/progress windows.
use super::account_workspace::AccountSourceKind;
use super::background_tasks::CatalogTaskKind;
use super::platform::load_logo_texture;
use super::save_support::SaveAction;
use super::{
    CREDITS_URL, ConfirmationDialog, DISPLAY_VERSION, MAIN_SIDEBAR_WIDTH, PROJECT_URL, SUNRISE_URL,
    SundialApp, TIGER_PKG_URL, ViewMode, persistence_compatibility,
};
use crate::updates::{RELEASES_URL, UpdateStatus};
use eframe::egui;

impl SundialApp {
    pub(super) fn draw_app_chrome(&mut self, ctx: &egui::Context, available_update: Option<&str>) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            if let Some(warning) = self.preferences_load_warning.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(ui.visuals().warn_fg_color, warning);
                    if ui.small_button("Dismiss").clicked() {
                        self.preferences_load_warning = None;
                    }
                });
            }
            ui.horizontal(|ui| {
                if self.has_unsaved_changes() {
                    ui.label(
                        egui::RichText::new("Unsaved changes").color(ui.visuals().warn_fg_color),
                    );
                }
                let undo_label = self.undo_history.last().map(|entry| entry.label.clone());
                let redo_label = self.redo_history.last().map(|entry| entry.label.clone());
                let draft_pending = self.json_editor.has_unapplied_changes();
                let draft_notice = "Finish or reset the JSON draft before undoing account changes. Use Ctrl+Z in the editor to undo text edits.";
                let undo = ui
                    .add_enabled(!draft_pending && undo_label.is_some(), egui::Button::new("Undo"))
                    .on_disabled_hover_text(if draft_pending { draft_notice } else { "Nothing to undo" });
                let undo = if let Some(label) = undo_label.as_deref() {
                    undo.on_hover_text(format!("Undo: {label}"))
                } else {
                    undo
                };
                if undo.clicked() {
                    self.undo();
                }
                let redo = ui
                    .add_enabled(!draft_pending && redo_label.is_some(), egui::Button::new("Redo"))
                    .on_disabled_hover_text(if draft_pending { draft_notice } else { "Nothing to redo" });
                let redo = if let Some(label) = redo_label.as_deref() {
                    redo.on_hover_text(format!("Redo: {label}"))
                } else {
                    redo
                };
                if redo.clicked() {
                    self.redo();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.has_unsaved_changes(), egui::Button::new("Save"))
                        .clicked()
                    {
                        self.request_save(ctx, SaveAction::Save);
                    }
                    if ui.button("Reload").clicked() {
                        if self.has_unsaved_changes() {
                            self.confirmation = Some(ConfirmationDialog::Reload);
                        } else {
                            self.reload();
                        }
                    }
                });
            });
        });

        if self.persistence_compatibility.detected() {
            egui::TopBottomPanel::top("persistence_compatibility_warning").show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new("Persistence compatibility warning:")
                                    .strong()
                                    .color(ui.visuals().warn_fg_color),
                            );
                            ui.label(persistence_compatibility::WARNING_MESSAGE);
                        });
                    });
            });
        }

        egui::SidePanel::left("characters")
            .resizable(false)
            .exact_width(MAIN_SIDEBAR_WIDTH)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (view, label) in [
                    (ViewMode::Characters, "Characters & Loadouts"),
                    (ViewMode::ProfileInventory, "Profile Inventory"),
                    (ViewMode::CharacterInventory, "Character Inventory"),
                    (ViewMode::GameSettings, "Game Settings"),
                ] {
                    if ui.selectable_label(self.view_mode == view, label).clicked() {
                        self.select_view(view);
                    }
                }
                if ui
                    .selectable_label(self.view_mode == ViewMode::Progression, "Progression")
                    .clicked()
                {
                    self.select_view(ViewMode::Progression);
                }
                for (view, label) in [
                    (ViewMode::AdvancedJson, "All Settings (JSON)"),
                    (ViewMode::Preferences, "Preferences"),
                ] {
                    if ui.selectable_label(self.view_mode == view, label).clicked() {
                        self.select_view(view);
                    }
                }
                if self.preferences.experimental_package_authoring
                    && ui.button("Open Parhelion").clicked()
                {
                    self.open_package_authoring(ctx);
                }
                let footer = sidebar_footer(ui, available_update);
                self.about_open |= footer.about.clicked();
                self.activity_log_open |= footer.activity_log.clicked();
                if footer.update.is_some_and(|response| response.clicked()) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(RELEASES_URL));
                }
            });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let color = if self.status_is_error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let account_source = self.document.source_info();
                if account_source.kind != AccountSourceKind::Json {
                    let source_color = if account_source.kind == AccountSourceKind::Blocked {
                        ui.visuals().error_fg_color
                    } else if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(105, 156, 118)
                    } else {
                        egui::Color32::from_rgb(64, 122, 80)
                    };
                    ui.label(
                        egui::RichText::new(egui_phosphor::regular::DATABASE)
                            .size(16.0)
                            .color(source_color),
                    )
                    .on_hover_text(format!(
                        "{}\n\n{}\n\nPath: {}\nContract: {}",
                        account_source.label,
                        account_source.detail,
                        account_source.database_path.display(),
                        account_source.contract,
                    ));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&self.status).color(color)).truncate(),
                    )
                    .on_hover_text(&self.status);
                });
            });
        });
    }

    pub(super) fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let logo = self
            .logo
            .get_or_insert_with(|| load_logo_texture(ctx))
            .clone();
        let update_status = self.update_check.status().clone();
        let mut retry_update_check = false;
        egui::Window::new("About Sundial")
            .open(&mut self.about_open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_width(430.0);
                ui.vertical_centered(|ui| {
                    ui.image((logo.id(), egui::vec2(64.0, 64.0)));
                    ui.heading("Sundial");
                    ui.label(egui::RichText::new(DISPLAY_VERSION).weak());
                    ui.add_space(8.0);
                    ui.label("A simple Project Sunrise settings editor.");
                    ui.hyperlink_to("github.com/kylethmpsn/sundial", PROJECT_URL);
                    ui.add_space(8.0);
                    match &update_status {
                        UpdateStatus::NotStarted => {
                            retry_update_check = ui.button("Check for updates").clicked();
                        }
                        UpdateStatus::Checking => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Checking for updates...");
                            });
                        }
                        UpdateStatus::Current => {
                            ui.label(egui::RichText::new("Sundial is up to date.").weak());
                        }
                        UpdateStatus::Available(version) => {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                format!("Sundial {version} is available."),
                            );
                            ui.hyperlink_to("Open GitHub Releases", RELEASES_URL);
                        }
                        UpdateStatus::Failed => {
                            ui.label(
                                egui::RichText::new("Could not check for updates.").weak(),
                            );
                            retry_update_check = ui.button("Try again").clicked();
                        }
                    }
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label("Built for Project Sunrise 0.1 through 0.4.0.");
                ui.hyperlink_to("Project Sunrise on GitHub", SUNRISE_URL);
                ui.add_space(6.0);
                ui.label("Local Destiny package parsing is powered by tiger-pkg.");
                ui.hyperlink_to("tiger-pkg on GitHub", TIGER_PKG_URL);
                ui.add_space(6.0);
                ui.hyperlink_to("For additional credits, see the project README.", CREDITS_URL);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "This project is not affiliated with or endorsed by Bungie Inc. or Sony Interactive Entertainment. Destiny and related intellectual property are owned by Bungie Inc. and their respective rights holders.",
                    )
                    .weak(),
                );
            });
        if retry_update_check {
            self.update_check.retry(ctx);
        }
    }

    pub(super) fn draw_catalog_progress(&self, ctx: &egui::Context) {
        let Some(task) = &self.catalog_task else {
            return;
        };
        let progress = task.progress;
        let title = task.kind.title();
        let path = match &task.kind {
            CatalogTaskKind::LoadInstall(pending) => &pending.install_path,
            CatalogTaskKind::Rebuild => &self.install_path,
        };
        egui::Modal::new("catalog_task_progress".into()).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.heading(title);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong(progress.message);
            });
            ui.add_space(10.0);
            let mut bar = crate::investment::progress_bar(progress.fraction()).desired_width(480.0);
            if progress.total > 0 {
                bar = bar.show_percentage();
            } else {
                bar = bar.animate(true);
            }
            ui.add(bar);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(path.display().to_string())
                    .weak()
                    .small(),
            );
        });
    }
}

struct SidebarFooter {
    about: egui::Response,
    activity_log: egui::Response,
    update: Option<egui::Response>,
}

fn sidebar_footer(ui: &mut egui::Ui, available_update: Option<&str>) -> SidebarFooter {
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        let (about, activity_log) = ui
            .horizontal(|ui| (ui.small_button("About"), ui.small_button("Activity Log…")))
            .inner;
        // A separate row keeps the update action above the fixed footer instead
        // of wrapping downward into the window edge in a bottom-up layout.
        let update = available_update.map(|version| {
            ui.add(
                egui::Button::new(
                    egui::RichText::new("Update Available").color(ui.visuals().hyperlink_color),
                )
                .small(),
            )
            .on_hover_text(format!(
                "Sundial {version} is available. Open GitHub Releases."
            ))
        });
        SidebarFooter {
            about,
            activity_log,
            update,
        }
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_action_is_visible_above_footer_without_overlap() {
        for height in [360.0, 600.0, 900.0] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, height),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::SidePanel::left("test_sidebar")
                    .exact_width(MAIN_SIDEBAR_WIDTH)
                    .show(ctx, |ui| {
                        ui.label("Character Inventory");
                        let clip = ui.clip_rect();
                        let footer = sidebar_footer(ui, Some("99.0.0"));
                        let update = footer.update.unwrap();
                        for response in [&footer.about, &footer.activity_log, &update] {
                            assert!(
                                clip.contains_rect(response.rect),
                                "Clipped footer: {:?}",
                                response.rect
                            );
                        }
                        assert!(update.rect.bottom() < footer.about.rect.top());
                        assert!(!footer.about.rect.intersects(footer.activity_log.rect));
                    });
            });
        }
    }
}
