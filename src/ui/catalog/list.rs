//! Virtualized native choices with stable identity and independent inspection.
use eframe::egui;
pub struct BrowserList<'a> {
    /// Stable native identity prevents a filter change from selecting a different row.
    pub keys: &'a [u64],
    pub height: f32,
    pub reset: bool,
    pub row_height: f32,
    /// A key to select and reveal this frame, when a link elsewhere chose it.
    pub select: Option<u64>,
}

impl BrowserList<'_> {
    pub fn draw<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        detail: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
    ) -> Option<T> {
        ui.label(format!("{} Results", self.keys.len()));
        self.draw_body(ui, row, detail)
    }

    pub fn draw_body<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        detail: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
    ) -> Option<T> {
        self.draw_layout(ui, row, detail, false)
    }

    /// Full-width choices with a compact action area, for previews hosted in another window.
    pub fn draw_with_actions<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        actions: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
    ) -> Option<T> {
        ui.label(format!("{} Results", self.keys.len()));
        self.draw_layout(ui, row, actions, true)
    }

    fn draw_layout<T>(
        &self,
        ui: &mut egui::Ui,
        mut row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        mut detail: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
        actions: bool,
    ) -> Option<T> {
        if self.keys.is_empty() {
            ui.allocate_ui(egui::vec2(ui.available_width(), self.height), |ui| {
                ui.set_min_height(self.height);
                ui.strong("No Matching Results");
                ui.label("Clear the search or change a filter.");
            });
            return None;
        }
        let id = ui.make_persistent_id("inspected-choice");
        let mut selected = ui
            .data(|data| data.get_temp::<u64>(id))
            .filter(|key| self.keys.contains(key))
            .unwrap_or(self.keys[0]);
        let keyboard_step = if ui.memory(eframe::egui::Memory::any_popup_open) {
            0
        } else {
            ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    1
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            })
        };
        let mut reveal = None;
        if let Some(key) = self.select
            && let Some(index) = self.keys.iter().position(|other| *other == key)
        {
            selected = key;
            reveal = Some(index);
        }
        if keyboard_step != 0 {
            let index = self
                .keys
                .iter()
                .position(|key| *key == selected)
                .unwrap_or(0);
            let next = index
                .saturating_add_signed(keyboard_step)
                .min(self.keys.len() - 1);
            selected = self.keys[next];
            reveal = Some(next);
        }
        let narrow = actions || ui.available_width() < 700.0;
        let list_height = if actions {
            (self.height - 76.0).max(80.0)
        } else if narrow {
            self.height * 0.52
        } else {
            self.height
        };
        let list_width = if narrow {
            ui.available_width()
        } else {
            ui.available_width() * 0.52
        };
        let mut picked = None;
        let layout = if narrow {
            egui::Layout::top_down(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        };
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), self.height),
            layout,
            |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(list_width, list_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let mut scroll = egui::ScrollArea::vertical()
                            .id_salt("choices")
                            .max_height(list_height)
                            .min_scrolled_height(list_height)
                            .auto_shrink([false, false]);
                        if let Some(index) = reveal {
                            scroll = scroll.vertical_scroll_offset(
                                (index as f32 * (self.row_height + ui.spacing().item_spacing.y)
                                    - list_height * 0.5)
                                    .max(0.0),
                            );
                        } else if self.reset {
                            scroll = scroll.vertical_scroll_offset(0.0);
                        }
                        scroll.show_rows(ui, self.row_height, self.keys.len(), |ui, indices| {
                            for index in indices {
                                // Keep stripes tied to result indices, not the first visible row.
                                // Selection and hover paint over this quiet background.
                                if index % 2 == 1 {
                                    let rect = egui::Rect::from_min_size(
                                        ui.cursor().min,
                                        egui::vec2(ui.available_width(), self.row_height),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        ui.visuals().faint_bg_color,
                                    );
                                }
                                if row(ui, index, self.keys[index] == selected).clicked() {
                                    selected = self.keys[index];
                                }
                            }
                        });
                    },
                );
                ui.separator();
                let detail_height = if actions {
                    64.0
                } else if narrow {
                    (self.height - list_height - 12.0).max(100.0)
                } else {
                    self.height
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), detail_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("choice-details", selected))
                            .max_height(detail_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let index = self
                                    .keys
                                    .iter()
                                    .position(|key| *key == selected)
                                    .expect("selected visible choice");
                                picked = detail(ui, index);
                            });
                    },
                );
            },
        );
        ui.data_mut(|data| data.insert_temp(id, selected));
        picked
    }
}

#[cfg(test)]
mod tests;
