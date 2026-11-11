//! Retained build and installation messages, their displayed rows and clipboard output.
use super::*;
use std::collections::{BTreeMap, VecDeque};

mod events;

const MAX_EVENTS: usize = 100_000;

#[derive(Default)]
pub(in crate::app) struct Activity {
    lines: VecDeque<String>,
    /// Absolute row numbers of ordinary messages. Keeping this index avoids filtering the entire
    /// history on every frame when runtime details are hidden.
    summary_rows: VecDeque<u64>,
    operation: Option<events::Entry>,
    first_line: u64,
    packages: BTreeMap<(u16, u32), (u64, crate::workflow::OperationStatus)>,
}

impl Activity {
    fn update(&mut self, id: u64, elapsed: Duration, message: String) {
        if let Some(line) = id
            .checked_sub(self.first_line)
            .and_then(|index| self.lines.get_mut(index as usize))
        {
            *line = format!("[{}] {message}", format_elapsed(elapsed));
        }
    }

    pub(in crate::app) fn push(&mut self, elapsed: Duration, message: String) {
        self.operation = None;
        self.append(elapsed, message);
    }

    fn append(&mut self, elapsed: Duration, message: String) -> u64 {
        self.store(elapsed, message, false)
    }

    fn runtime_detail(&mut self, elapsed: Duration, message: String) {
        self.store(elapsed, message, true);
    }

    fn store(&mut self, elapsed: Duration, message: String, runtime_detail: bool) -> u64 {
        if self.lines.len() == MAX_EVENTS {
            self.lines.pop_front();
            if self.summary_rows.front() == Some(&self.first_line) {
                self.summary_rows.pop_front();
            }
            self.first_line += 1;
            self.packages
                .retain(|_, (line, _)| *line >= self.first_line);
        }
        let row = self.first_line + self.lines.len() as u64;
        self.lines
            .push_back(format!("[{}] {message}", format_elapsed(elapsed)));
        if !runtime_detail {
            self.summary_rows.push_back(row);
        }
        row
    }

    fn row_count(&self, details: bool) -> usize {
        if details {
            self.lines.len()
        } else {
            self.summary_rows.len()
        }
    }

    fn row(&self, index: usize, details: bool) -> &str {
        let index = if details {
            index
        } else {
            (self.summary_rows[index] - self.first_line) as usize
        };
        &self.lines[index]
    }

    fn copied_text(&self, details: bool) -> String {
        (0..self.row_count(details))
            .map(|index| self.row(index, details))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The list and Copy Progress use the same selected rows. A row cut off at the edge shows its
    /// full text on hover, as egui does for an elided label, and every row's full text is in the
    /// copied output. Fixed-height rows let the scroll area paint only what fits.
    pub(in crate::app::build_status) fn draw(
        &self,
        ui: &mut egui::Ui,
        id: &'static str,
        footer_height: Option<f32>,
    ) {
        let toggle = egui::Id::new(id).with("runtime-details");
        let mut details = ui
            .ctx()
            .data_mut(|data| data.get_temp::<bool>(toggle).unwrap_or(false));
        ui.horizontal_wrapped(|ui| {
            ui.strong("Activity");
            if self.lines.len() != self.summary_rows.len() {
                ui.checkbox(&mut details, "Show Detailed Runtime Output").on_hover_text(
                    "Include cache diagnostics, detailed timings and resource ranges in the list and copied progress.",
                );
            }
            ui.weak(format!("{} events retained", self.lines.len()));
            if ui.add_enabled(self.row_count(details) > 0, egui::Button::new("Copy Progress")).clicked() {
                ui.ctx().copy_text(self.copied_text(details));
            }
        });
        ui.ctx().data_mut(|data| data.insert_temp(toggle, details));
        egui::Frame::group(ui.style())
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let row_height = ui.text_style_height(&egui::TextStyle::Body);
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .max_height(
                        footer_height
                            .map_or(180.0, |footer| (ui.available_height() - footer).max(40.0)),
                    )
                    .auto_shrink([false, footer_height.is_none()])
                    .stick_to_bottom(true)
                    .show_rows(ui, row_height, self.row_count(details), |ui, rows| {
                        for index in rows {
                            let line = self.row(index, details);
                            ui.add(egui::Label::new(line.replace('\n', " ")).truncate());
                        }
                    });
                if self.lines.is_empty() {
                    ui.weak("Waiting for the worker to report its first operation.");
                }
            });
    }
}

fn message(label: &str, item: Option<&str>, completed: usize, total: usize, unit: &str) -> String {
    let count = if total > 0 {
        format!(" ({completed}/{total} {unit} complete)")
    } else {
        String::new()
    };
    format!(
        "{label}{count}{}",
        item.map_or_else(String::new, |item| format!(": {item}"))
    )
}
