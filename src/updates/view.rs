use super::{InstallState, UpdateCheck, UpdateStatus};
use eframe::egui;
use std::{sync::atomic::Ordering, time::Duration};

pub(crate) enum Action {
    None,
    Save,
    Prepare,
    Commit,
}

impl UpdateCheck {
    pub(crate) fn draw(
        &mut self,
        ctx: &egui::Context,
        blocker: Option<&str>,
        can_save: bool,
    ) -> Action {
        if matches!(
            self.install,
            InstallState::Downloading | InstallState::Preparing
        ) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !self.window_open {
            if matches!(self.install, InstallState::Waiting(_)) {
                self.install = InstallState::Idle;
            }
            return Action::None;
        }
        let mut open = true;
        let mut download = false;
        let mut cancel_restart = false;
        let mut action = Action::None;
        egui::Window::new("Update Sundial")
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                let UpdateStatus::Available(release) = &self.status else {
                    ui.label(
                        self.check_error
                            .as_deref()
                            .unwrap_or("No update is available."),
                    );
                    return;
                };
                ui.heading(format!("Sundial {}", release.version));
                ui.label(format!("Installed version: v{}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.strong("Release Notes");
                egui::ScrollArea::vertical()
                    .id_salt("update_release_notes")
                    .max_height(300.0)
                    .show(ui, |ui| {
                        ui.style_mut().url_in_tooltip = true;
                        egui_commonmark::CommonMarkViewer::new().show(
                            ui,
                            &mut self.notes_cache,
                            &release.notes,
                        );
                    });
                ui.separator();
                if let Err(error) = &release.asset {
                    ui.colored_label(ui.visuals().warn_fg_color, error);
                    ui.hyperlink_to(
                        "Open This Release",
                        format!("{}/tag/{}", super::RELEASES_URL, release.version),
                    );
                    return;
                }
                match &self.install {
                    InstallState::Idle | InstallState::Failed(_) => {
                        if let InstallState::Failed(error) = &self.install {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        download = ui
                            .add(
                                egui::Button::new("Download Update")
                                    .fill(ui.visuals().selection.bg_fill),
                            )
                            .clicked();
                    }
                    InstallState::Downloading => {
                        let received = self.progress.received.load(Ordering::Relaxed);
                        let total = release.asset.as_ref().map_or(1, |asset| asset.size);
                        ui.add(
                            crate::investment::progress_bar(received as f32 / total as f32).text(
                                format!(
                                    "{:.1} / {:.1} MiB",
                                    received as f64 / 1_048_576.0,
                                    total as f64 / 1_048_576.0,
                                ),
                            ),
                        );
                        ui.label(if received >= total {
                            "Verifying and extracting the update…"
                        } else {
                            "Downloading the update…"
                        });
                        if ui.button("Cancel Download").clicked() {
                            self.progress.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    InstallState::Ready(_) => {
                        ui.label("The update is downloaded and verified.");
                        restart_blocker(ui, blocker, can_save, &mut action);
                        if ui
                            .add_enabled(
                                blocker.is_none(),
                                egui::Button::new("Restart and Update")
                                    .fill(ui.visuals().selection.bg_fill),
                            )
                            .clicked()
                        {
                            action = Action::Prepare;
                        }
                    }
                    InstallState::Preparing => {
                        ui.spinner();
                        ui.label("Preparing a safe restart…");
                    }
                    InstallState::Waiting(_) => {
                        restart_blocker(ui, blocker, can_save, &mut action);
                        if blocker.is_none() {
                            action = Action::Commit;
                        }
                        cancel_restart = ui.button("Cancel Restart").clicked();
                    }
                    InstallState::Restarting => {
                        ui.spinner();
                        ui.label("Restarting Sundial…");
                    }
                }
            });
        self.window_open = open;
        if download {
            self.download(ctx);
        }
        if cancel_restart {
            self.install = InstallState::Idle;
            action = Action::None;
        }
        if !open && matches!(self.install, InstallState::Waiting(_)) {
            self.install = InstallState::Idle;
            action = Action::None;
        }
        action
    }
}

fn restart_blocker(ui: &mut egui::Ui, blocker: Option<&str>, can_save: bool, action: &mut Action) {
    if let Some(reason) = blocker {
        ui.colored_label(ui.visuals().warn_fg_color, reason);
        if can_save && ui.button("Save Changes").clicked() {
            *action = Action::Save;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_and_download_action_render_without_starting_work() {
        for width in [640.0, 1280.0] {
            let mut check = UpdateCheck::default();
            let (name, _, _) = crate::updates::release::package("v99.0").unwrap();
            let response = serde_json::json!({"tag_name":"v99.0", "body":"# Changes\n- Account updates\nKeep your settings.",
                "assets":[{"name":name,"browser_download_url":format!("https://github.com/kylethmpsn/sundial/releases/download/v99.0/{name}"),
                "size":1234, "digest":format!("sha256:{}", "a".repeat(64))}]});
            check.status = UpdateStatus::Available(
                crate::updates::release::parse(&serde_json::to_vec(&response).unwrap(), "0.4.1")
                    .unwrap()
                    .unwrap(),
            );
            check.window_open = true;
            let ctx = egui::Context::default();
            let mut text = String::new();
            for _ in 0..2 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 720.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        assert!(matches!(check.draw(ctx, None, false), Action::None));
                    },
                );
                for shape in output.shapes {
                    if let egui::Shape::Text(text_shape) = shape.shape {
                        text.push_str(&text_shape.galley.job.text);
                    }
                }
            }
            for expected in [
                "Sundial v99.0",
                "Release Notes",
                "Account updates",
                "Download Update",
            ] {
                assert!(text.contains(expected), "missing {expected} at {width}");
            }
            assert!(check.worker.is_none());
            assert!(matches!(check.install, InstallState::Idle));
        }
    }
}
