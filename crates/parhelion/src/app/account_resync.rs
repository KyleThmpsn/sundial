//! Visible progress and results for the account worker, independent of its busy lifetime.
use super::*;

#[derive(Default)]
pub(super) struct Status {
    open: bool,
    result: Option<Result<AccountResyncReport, String>>,
}

impl Status {
    pub(super) fn begin(&mut self) {
        self.open = true;
        self.result = None;
    }

    pub(super) fn finish(&mut self, result: Result<AccountResyncReport, String>) {
        self.open = true;
        self.result = Some(result);
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_account_resync_window(&mut self, ctx: &egui::Context) {
        if !self.account_resync.open {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut show_log = false;
        let running = self.account_resync_receiver.is_some();
        egui::Window::new("Account Resync")
            .id(egui::Id::new("parhelion-account-resync"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                workbench_style(ui);
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| match &self.account_resync.result {
                        None => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.heading("Resyncing Account");
                            });
                            ui.label("Updating Collections and installed items…");
                        }
                        Some(Ok(report)) => draw_report(ui, report),
                        Some(Err(error)) => {
                            ui.heading("Resync Failed");
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(error).color(ui.visuals().error_fg_color),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    close = ui.button(if running { "Hide" } else { "OK" }).clicked();
                    show_log = ui.button("View Activity Log").clicked();
                });
            });
        self.account_resync.open = open && !close && !show_log;
        if show_log {
            self.activity_log_open = true;
        }
    }
}

fn count(value: usize, noun: &str) -> String {
    format!("{value} {noun}{}", if value == 1 { "" } else { "s" })
}

fn draw_report(ui: &mut egui::Ui, report: &AccountResyncReport) {
    let grants = report
        .item_grants
        .as_ref()
        .and_then(|result| result.as_ref().ok());
    let incomplete = report.profile_sync.is_err()
        || report.item_grants.as_ref().is_some_and(Result::is_err)
        || grants.is_some_and(|grants| !grants.full.is_empty());
    let changed = report
        .profile_sync
        .as_ref()
        .is_ok_and(|profile| profile.newly_set_unlocks > 0)
        || grants.is_some_and(|grants| !grants.added.is_empty() || !grants.equipped.is_empty());
    ui.heading(if incomplete {
        "Resync Needs Attention"
    } else if changed {
        "Account Resynced"
    } else {
        "Account Is Up to Date"
    });
    if !incomplete && !changed {
        ui.label("No changes were needed.");
    }
    match &report.profile_sync {
        Ok(profile) if profile.newly_set_unlocks > 0 => {
            ui.label(format!(
                "Added {}.",
                count(profile.newly_set_unlocks, "Collections unlock")
            ));
        }
        Ok(profile) if profile.total_unlocks > 0 => {
            ui.label(format!(
                "{} already available.",
                count(profile.total_unlocks, "Collections unlock")
            ));
        }
        Ok(_) => {}
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Collections could not be updated: {error}"),
            );
        }
    }
    match &report.item_grants {
        Some(Ok(grants)) => {
            if !grants.added.is_empty() {
                ui.label(format!("Added {}.", count(grants.added.len(), "item")));
            }
            if !grants.equipped.is_empty() {
                ui.label(format!(
                    "Equipped {}.",
                    count(grants.equipped.len(), "item")
                ));
            }
            if !grants.full.is_empty() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "Could not add {} because the inventory is full.",
                        count(grants.full.len(), "item")
                    ),
                );
            }
        }
        Some(Err(error)) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Items could not be added: {error}"),
            );
        }
        None => {}
    }
}
