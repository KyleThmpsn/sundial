use super::*;

impl JsonEditorState {
    fn commit_edit(&mut self, text: &mut String, updated: String) {
        if *text == updated {
            return;
        }
        self.history.add_undo(text);
        *text = updated;
        self.history.add_undo(text);
        self.folded.clear();
        self.mark_modified();
        self.restore_location_next_draw();
        let mut cursor = self.cursor_source_byte.min(text.len());
        while !text.is_char_boundary(cursor) {
            cursor -= 1;
        }
        self.pending_jump = Some((cursor, cursor));
        self.refresh_analysis(text);
    }

    fn format_document(&mut self, text: &mut String) {
        self.refresh_analysis(text);
        if self.parsed.is_some() {
            let formatted = super::super::operations::format_json(text);
            self.commit_edit(text, formatted);
        }
    }

    fn navigate(&mut self) {
        if let Some(location) = self
            .locations
            .iter()
            .find(|entry| entry.pointer == self.pointer)
        {
            self.pending_jump = Some(location.range);
            self.tools_notice = None;
        } else {
            self.tools_notice = Some("That JSON pointer does not exist in this document.".into());
        }
    }

    pub(super) fn draw_tools(
        &mut self,
        ui: &mut egui::Ui,
        text: &mut String,
        response: &mut JsonEditorResponse,
    ) {
        let (replace, navigate, format) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::H),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::G),
                input.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::F,
                ),
            )
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.parsed.is_some(), egui::Button::new("Format JSON"))
                .on_hover_text("Ctrl+Shift+F")
                .clicked()
                || format
            {
                self.format_document(text);
            }
            ui.toggle_value(&mut self.replace_open, "Replace")
                .on_hover_text("Ctrl+H");
            ui.toggle_value(&mut self.navigation_open, "Go to Path")
                .on_hover_text("Ctrl+G");
            if ui
                .toggle_value(&mut self.completion_open, "Add Setting")
                .clicked()
                && self.completion_open
            {
                response.load_defaults = true;
                self.navigation_open = true;
            }
            if ui
                .add_enabled(
                    self.error_position.is_some(),
                    egui::Button::new("Go to Error"),
                )
                .clicked()
            {
                let start = self.error_position.unwrap();
                let end = start + text[start..].chars().next().map_or(0, char::len_utf8);
                self.pending_jump = Some((start, end));
            }
            if ui.button("Copy All").clicked() {
                ui.ctx().copy_text(text.clone());
            }
        });
        if replace {
            self.replace_open = true;
            self.focus_replacement = true;
        }
        if navigate {
            self.navigation_open = true;
        }
        if self.navigation_open {
            ui.horizontal(|ui| {
                let label = ui.label("JSON pointer");
                let field = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.pointer)
                            .hint_text("/state/characters/0 (empty = root)")
                            .desired_width(270.0),
                    )
                    .labelled_by(label.id);
                if navigate {
                    field.request_focus();
                }
                if ui.button("Go").clicked()
                    || ((field.has_focus() || field.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    self.navigate();
                }
            });
        }
        if self.completion_open {
            self.draw_completion(ui, text);
        }
        if let Some(notice) = &self.tools_notice {
            ui.label(notice);
        }
    }

    fn draw_completion(&mut self, ui: &mut egui::Ui, text: &mut String) {
        let Some(defaults) = &self.completion_defaults else {
            return;
        };
        let Some(document) = &self.parsed else {
            ui.label("Fix JSON errors before adding settings.");
            return;
        };
        if document.get("version").is_none() || document.get("version") != defaults.get("version") {
            ui.label(
                "Setting suggestions require the same version as your installed Sunrise defaults.",
            );
            return;
        }
        let Some(current) = document
            .pointer(&self.pointer)
            .and_then(serde_json::Value::as_object)
        else {
            ui.label("Enter the JSON pointer of an object to add a setting.");
            return;
        };
        let Some(available) = defaults
            .pointer(&self.pointer)
            .and_then(serde_json::Value::as_object)
        else {
            ui.label("No installed defaults exist for this object.");
            return;
        };
        let empty = current.is_empty();
        let suggestions: Vec<_> = available
            .iter()
            .filter(|(key, _)| !current.contains_key(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        ui.horizontal(|ui| {
            let label = ui.label("Missing setting");
            ui.text_edit_singleline(&mut self.completion_filter)
                .labelled_by(label.id);
        });
        ui.small("Suggestions use your installed Sunrise defaults. Existing values are preserved.");
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for (key, value) in suggestions
                .iter()
                .filter(|(key, _)| key.contains(&self.completion_filter))
                .take(10)
            {
                if ui
                    .button(format!("Add {key}"))
                    .on_hover_text(value.to_string())
                    .clicked()
                {
                    chosen = Some((key, value));
                }
            }
        });
        if suggestions.is_empty() {
            ui.label("This object already contains every suggested setting.");
        }
        if let Some((key, value)) = chosen {
            if let Some(location) = self
                .locations
                .iter()
                .find(|entry| entry.pointer == self.pointer)
            {
                let updated =
                    super::super::operations::add_member(text, location.range, key, value, empty);
                self.commit_edit(text, updated);
            }
        }
    }

    pub(super) fn draw_replace(
        &mut self,
        ui: &mut egui::Ui,
        text: &mut String,
        matches: &[(usize, usize)],
    ) {
        if !self.replace_open {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Replace with");
            let field = ui.add(egui::TextEdit::singleline(&mut self.replacement).desired_width(220.0)).labelled_by(label.id);
            if std::mem::take(&mut self.focus_replacement) { field.request_focus(); }
            let selected = self.current_match.and_then(|index| matches.get(index)).copied();
            let one = ui.add_enabled(selected.is_some(), egui::Button::new("Replace selected")).clicked();
            let all = ui.add_enabled(!matches.is_empty(), egui::Button::new("Replace all"))
                .on_hover_text("Literal replacement throughout the full JSON, including collapsed content. Search ignores ASCII case.").clicked();
            if one || all {
                let ranges = if all { matches } else { std::slice::from_ref(selected.as_ref().unwrap()) };
                let updated = super::super::operations::replace_ranges(text, ranges, &self.replacement);
                self.commit_edit(text, updated);
                self.current_match = None;
            }
        });
    }
}
