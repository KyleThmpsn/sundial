//! Editor commands, search navigation, and validation status.
use super::super::syntax::find_matches;
use super::{JsonEditorResponse, JsonEditorState};
use eframe::egui;

pub(super) struct SearchNavigation {
    pub matches: Vec<(usize, usize)>,
    pub jump_to_match: bool,
}

pub(super) fn draw_toolbar(
    ui: &mut egui::Ui,
    text: &mut String,
    state: &mut JsonEditorState,
    detached: bool,
    unsaved: bool,
    response: &mut JsonEditorResponse,
) {
    ui.horizontal_wrapped(|ui| {
        ui.heading("All Settings");
        ui.menu_button("Editor", |ui| {
            state.draw_tool_menu(ui, text, response);
            ui.separator();
            draw_folding_menu(ui, state);
            ui.separator();
            if ui
                .add_enabled(state.modified, egui::Button::new("Reset Editor"))
                .clicked()
            {
                state.reset_pending = true;
                ui.close_menu();
            }
        });
        response.toggle_window = ui
            .button(if detached {
                "Dock in Main Window"
            } else {
                "Open in Window"
            })
            .clicked();
        if detached {
            response.save |= ui.button("Save").clicked();
            if state.modified || unsaved {
                ui.label(egui::RichText::new("Unsaved changes").color(ui.visuals().warn_fg_color));
            }
        }
        crate::ui_help::info(ui, "Edit JSON directly. Save validates and writes the account file.\n\nCtrl+S: Save\nCtrl+F: Find\nCtrl+H: Replace\nF3 / Shift+F3: Next / previous match\nCtrl+G: Go to path\nCtrl+Shift+F: Format JSON\nCtrl+Z / Ctrl+Y: Undo / redo text edits");
    });
    if state.reset_pending {
        ui.horizontal_wrapped(|ui| {
            ui.label("Discard the JSON draft and restore the current account?");
            response.reset = ui
                .button(egui::RichText::new("Discard Edits").color(ui.visuals().error_fg_color))
                .clicked();
            if ui.button("Cancel").clicked() {
                state.reset_pending = false;
            }
        });
    }
    state.draw_tools(ui, text);
    response.save |=
        ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
}

fn draw_folding_menu(ui: &mut egui::Ui, state: &mut JsonEditorState) {
    let regions = &state.regions;
    let can_collapse = regions
        .iter()
        .any(|region| region.id.len() > 1 && !state.folded.contains(&region.id))
        || state.folded.iter().any(|id| id.len() == 1);
    if ui
        .add_enabled(can_collapse, egui::Button::new("Collapse All"))
        .clicked()
    {
        state.folded = regions
            .iter()
            .filter(|region| region.id.len() > 1)
            .map(|region| region.id.clone())
            .collect();
        ui.close_menu();
    }
    if ui
        .add_enabled(!state.folded.is_empty(), egui::Button::new("Expand All"))
        .clicked()
    {
        state.folded.clear();
        ui.close_menu();
    }
}

pub(super) fn draw_search(
    ui: &mut egui::Ui,
    text: &str,
    state: &mut JsonEditorState,
) -> SearchNavigation {
    let focus_find =
        ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
    let mut jump_to_match = false;
    let mut source_matches = Vec::new();
    let find_width = (ui.available_width() - 250.0).clamp(120.0, 320.0);
    ui.horizontal_wrapped(|ui| {
        let find_label = ui.label("Find");
        let find_response = ui
            .add(
                egui::TextEdit::singleline(&mut state.query)
                    .id_salt("json_editor_find")
                    .hint_text("Search JSON…")
                    .desired_width(find_width),
            )
            .labelled_by(find_label.id);
        if focus_find {
            find_response.request_focus();
        }
        ui.toggle_value(&mut state.replace_open, "Replace")
            .on_hover_text("Ctrl+H");

        let query_changed = find_response.changed();
        source_matches = find_matches(text, &state.query);
        if source_matches.is_empty() {
            state.current_match = None;
        } else if query_changed
            || state.current_match.is_none()
            || state
                .current_match
                .is_some_and(|index| index >= source_matches.len())
        {
            state.current_match = Some(0);
            jump_to_match = true;
        }

        let enter = (find_response.has_focus() || find_response.lost_focus())
            && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if enter {
            find_response.request_focus();
        }
        let f3 = ui.input(|input| input.key_pressed(egui::Key::F3));
        let reverse = ui.input(|input| input.modifiers.shift);
        let previous = ui
            .add_enabled(!source_matches.is_empty(), egui::Button::new("Previous"))
            .clicked()
            || (!source_matches.is_empty() && (enter || f3) && reverse);
        let next = ui
            .add_enabled(!source_matches.is_empty(), egui::Button::new("Next"))
            .clicked()
            || (!source_matches.is_empty() && (enter || f3) && !reverse);
        if previous {
            let current = state.current_match.unwrap_or(0);
            state.current_match = Some((current + source_matches.len() - 1) % source_matches.len());
            jump_to_match = true;
        } else if next {
            let current = state.current_match.unwrap_or(0);
            state.current_match = Some((current + 1) % source_matches.len());
            jump_to_match = true;
        }

        match state.current_match {
            Some(current) => {
                ui.label(format!("{} of {}", current + 1, source_matches.len()));
            }
            None if !state.query.is_empty() => {
                ui.label("No matches");
            }
            None => {}
        }
        if ui
            .add_enabled(!state.query.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            state.query.clear();
            state.current_match = None;
            source_matches.clear();
        }
    });

    SearchNavigation {
        matches: source_matches,
        jump_to_match,
    }
}

pub(super) fn draw_status(ui: &mut egui::Ui, text: &str, state: &mut JsonEditorState) {
    let line_count = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let previous_item_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let error = state.validate(text).map(str::to_owned);
    match error {
        None if state.application_error.is_some() => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!(
                    "Not applied: {}",
                    state.application_error.as_deref().unwrap_or_default()
                ),
            );
        }
        None => {
            ui.label(
                egui::RichText::new(format!(
                    "Valid JSON · {line_count} lines · {:.1} KiB · Ln {}, Col {}",
                    text.len() as f64 / 1024.0,
                    state.cursor_line,
                    state.cursor_column
                ))
                .weak(),
            );
        }
        Some(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Invalid JSON: {error}"),
            );
        }
    }
    ui.spacing_mut().item_spacing.y = previous_item_spacing;
}
