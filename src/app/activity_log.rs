//! Recent activity UI, backed by the shared rotating activity log.
use std::collections::VecDeque;

use eframe::egui;

use super::SundialApp;
use crate::activity_log::{Entry, FileLog, Product};

const CAPACITY: usize = 100;

#[derive(Default)]
pub(super) struct ActivityLog {
    entries: VecDeque<Entry>,
    file: FileLog,
}

impl ActivityLog {
    pub(super) fn enable_file(&mut self) {
        self.file.enable(Product::Sundial);
        self.push(
            format!("Sundial {} session started", env!("CARGO_PKG_VERSION")),
            false,
        );
    }

    pub(super) fn push(&mut self, message: String, is_error: bool) {
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        let entry = Entry::new(message, is_error);
        self.file.append(&entry);
        self.entries.push_back(entry);
    }

    pub(super) fn text(&self) -> String {
        self.entries
            .iter()
            .rev()
            .map(Entry::formatted)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl SundialApp {
    pub(super) fn draw_activity_log_window(&mut self, ctx: &egui::Context) {
        if !self.activity_log_open {
            return;
        }
        egui::Window::new("Activity Log")
            .id(egui::Id::new("sundial-activity-log"))
            .open(&mut self.activity_log_open)
            .collapsible(false)
            .resizable(true)
            .default_size([720.0, 480.0])
            .min_width(320.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "Events: {} · newest first",
                        self.activity_log.entries.len()
                    ));
                    if ui.button("Copy Log").clicked() {
                        ui.ctx().copy_text(self.activity_log.text());
                    }
                    if ui.button("Open Log Folder").clicked() {
                        self.activity_log.file.open_folder();
                    }
                });
                ui.label(format!("Latest {CAPACITY} events. Times in UTC."));
                if let Some(error) = self.activity_log.file.error() {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("sundial-activity-entries")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in self.activity_log.entries.iter().rev() {
                            let color = if entry.error {
                                ui.visuals().error_fg_color
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(entry.formatted()).color(color),
                                )
                                .wrap()
                                .selectable(true),
                            );
                            ui.separator();
                        }
                    });
            });
    }
}
