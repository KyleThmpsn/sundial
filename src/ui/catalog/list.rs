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
        ui.label(result_count(self.keys.len()));
        self.draw_body(ui, row, detail)
    }

    /// [`Self::draw`], with the detail told whether its row was double-clicked this frame.
    /// A picker takes a double-click as its Use button.
    pub fn draw_activating<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        detail: impl FnMut(&mut egui::Ui, usize, bool) -> Option<T>,
    ) -> Option<T> {
        ui.label(result_count(self.keys.len()));
        self.draw_layout(ui, row, detail, false, 0)
    }

    pub fn draw_body<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        mut detail: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
    ) -> Option<T> {
        self.draw_layout(ui, row, |ui, index, _| detail(ui, index), false, 0)
    }

    /// The same, saying how many rows a wider search would add.
    ///
    /// A list can come back empty because the default listing hides what the game never
    /// names, which is not something the reader can see from an empty box.
    pub fn draw_body_reporting_hidden<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        mut detail: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
        hidden: usize,
    ) -> Option<T> {
        self.draw_layout(ui, row, |ui, index, _| detail(ui, index), false, hidden)
    }

    /// [`Self::draw_body_reporting_hidden`], with the detail told whether its row was
    /// double-clicked this frame. A picker takes a double-click as its Use button.
    pub fn draw_body_activating<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        detail: impl FnMut(&mut egui::Ui, usize, bool) -> Option<T>,
        hidden: usize,
    ) -> Option<T> {
        self.draw_layout(ui, row, detail, false, hidden)
    }

    /// Full-width choices with a compact action area, for previews hosted in another window.
    pub fn draw_with_actions<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        mut actions: impl FnMut(&mut egui::Ui, usize) -> Option<T>,
    ) -> Option<T> {
        ui.label(result_count(self.keys.len()));
        self.draw_layout(ui, row, |ui, index, _| actions(ui, index), true, 0)
    }

    /// [`Self::draw_with_actions`], with the actions told whether their row was
    /// double-clicked this frame. A picker takes a double-click as its Use button.
    pub fn draw_with_actions_activating<T>(
        &self,
        ui: &mut egui::Ui,
        row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        actions: impl FnMut(&mut egui::Ui, usize, bool) -> Option<T>,
    ) -> Option<T> {
        ui.label(result_count(self.keys.len()));
        self.draw_layout(ui, row, actions, true, 0)
    }

    fn draw_layout<T>(
        &self,
        ui: &mut egui::Ui,
        mut row: impl FnMut(&mut egui::Ui, usize, bool) -> egui::Response,
        mut detail: impl FnMut(&mut egui::Ui, usize, bool) -> Option<T>,
        actions: bool,
        hidden: usize,
    ) -> Option<T> {
        if self.keys.is_empty() {
            ui.allocate_ui(egui::vec2(ui.available_width(), self.height), |ui| {
                ui.set_min_height(self.height);
                ui.strong("No Matching Results");
                if hidden > 0 {
                    ui.label(format!("{hidden} Unidentified Hidden"));
                }
            });
            return None;
        }
        let id = ui.make_persistent_id("inspected-choice");
        // A row stays selected through a filter change while it is still listed. A listing
        // it is not in starts on its first row, so a search followed by Enter takes its first
        // result.
        let mut selected = ui
            .data(|data| data.get_temp::<u64>(id))
            .filter(|key| self.keys.contains(key))
            .unwrap_or(self.keys[0]);
        // The list in the top window takes the keys, so a catalog list open behind a picker
        // does not step with it. A tooltip over a row is not a window above the list.
        let top = ui.ctx().memory(|memory| {
            memory
                .layer_ids()
                .filter(|layer| {
                    layer.order != egui::Order::Tooltip && memory.areas().is_visible(layer)
                })
                .last()
        });
        let keyboard_owner =
            !ui.memory(eframe::egui::Memory::any_popup_open) && top == Some(ui.layer_id());
        let keyboard_step = if keyboard_owner {
            ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    1
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            })
        } else {
            0
        };
        // Enter uses the selected row as a double-click does. The search box above the list
        // gives up its focus on the same key, so typing a search and pressing Enter picks its
        // first result.
        let entered = keyboard_owner
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let mut reveal = None;
        if let Some(key) = self.select
            && let Some(index) = self.keys.iter().position(|other| *other == key)
        {
            selected = key;
            reveal = Some(index);
        } else if self.reset {
            // A new listing scrolls to the row it keeps selected, which is the top when the
            // selection is new, so the detail never shows a row the list scrolled away from.
            reveal = self.keys.iter().position(|key| *key == selected);
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
        // Whether the selected row was double-clicked or entered this frame, which asks its
        // detail to act.
        let mut activated = entered;
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
                                let was_selected = self.keys[index] == selected;
                                let response = row(ui, index, was_selected);
                                if response.clicked() {
                                    // egui counts a double-click by time alone, wherever the
                                    // clicks land. The first click of a real one selects this
                                    // row, so only a row already selected is double-clicked.
                                    activated |= response.double_clicked() && was_selected;
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
                                picked = if actions {
                                    ui.horizontal_wrapped(|ui| detail(ui, index, activated))
                                        .inner
                                } else {
                                    detail(ui, index, activated)
                                };
                            });
                    },
                );
            },
        );
        ui.data_mut(|data| data.insert_temp(id, selected));
        picked
    }
}

/// "1 Result" or "N Results".
fn result_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "Result" } else { "Results" })
}

#[cfg(test)]
mod tests;
