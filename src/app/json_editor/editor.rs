use std::{collections::HashSet, sync::Arc};

use eframe::egui;

use super::fold_projection::{FoldId, fold_regions};

mod controls;
mod viewport;

pub(in crate::app) struct JsonEditorState {
    query: String,
    current_match: Option<usize>,
    folded: HashSet<FoldId>,
    modified: bool,
    pending_application: bool,
    reset_pending: bool,
    cursor_line: usize,
    cursor_column: usize,
    cursor_source_byte: usize,
    scroll_offset: egui::Vec2,
    restore_location: bool,
    history: egui::util::undoer::Undoer<String>,
    text_edit_id: Option<egui::Id>,
    validated_text: Option<String>,
    validation_error: Option<String>,
    application_error: Option<String>,
    parsed: Option<serde_json::Value>,
    locations: Vec<super::operations::Location>,
    regions: Arc<Vec<super::fold_projection::FoldRegion>>,
    error_position: Option<usize>,
    pending_jump: Option<(usize, usize)>,
    replacement: String,
    replace_open: bool,
    focus_replacement: bool,
    pointer: String,
    navigation_open: bool,
    completion_open: bool,
    completion_filter: String,
    completion_defaults: Option<serde_json::Value>,
    tools_notice: Option<String>,
    layout_cache: super::syntax::JsonLayoutCache,
}

impl Default for JsonEditorState {
    fn default() -> Self {
        Self {
            query: String::new(),
            current_match: None,
            folded: HashSet::new(),
            modified: false,
            pending_application: false,
            reset_pending: false,
            cursor_line: 1,
            cursor_column: 1,
            cursor_source_byte: 0,
            scroll_offset: egui::Vec2::ZERO,
            restore_location: false,
            history: egui::util::undoer::Undoer::with_settings(egui::util::undoer::Settings {
                max_undos: 30,
                ..Default::default()
            }),
            text_edit_id: None,
            validated_text: None,
            validation_error: None,
            application_error: None,
            parsed: None,
            locations: Vec::new(),
            regions: Arc::default(),
            error_position: None,
            pending_jump: None,
            replacement: String::new(),
            replace_open: false,
            focus_replacement: false,
            pointer: String::new(),
            navigation_open: false,
            completion_open: false,
            completion_filter: String::new(),
            completion_defaults: None,
            tools_notice: None,
            layout_cache: Default::default(),
        }
    }
}

impl JsonEditorState {
    pub(in crate::app) const fn has_unapplied_changes(&self) -> bool {
        self.modified
    }

    pub(in crate::app) fn mark_synced(&mut self) {
        self.modified = false;
        self.pending_application = false;
        self.reset_pending = false;
        self.application_error = None;
    }

    pub(in crate::app) fn mark_modified(&mut self) {
        self.modified = true;
        self.pending_application = true;
        self.reset_pending = false;
        self.application_error = None;
    }

    pub(in crate::app) fn reset_history(&mut self) {
        self.history = Self::default().history;
        self.completion_defaults = None;
    }

    pub(in crate::app) fn take_auto_apply_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_application)
    }

    fn restore_history(&mut self, text: &mut String, redo: bool) {
        let restored = if redo {
            self.history.redo(text)
        } else {
            self.history.undo(text)
        }
        .cloned();
        if let Some(restored) = restored {
            *text = restored;
            self.folded.clear();
            self.mark_modified();
            self.restore_location_next_draw();
        }
    }

    pub(in crate::app) fn set_application_error(&mut self, error: String) {
        self.application_error = Some(error);
    }

    fn refresh_analysis(&mut self, text: &str) {
        if self.validated_text.as_deref() == Some(text) {
            return;
        }
        self.regions = Arc::new(fold_regions(text));
        let valid_ids: HashSet<_> = self.regions.iter().map(|region| &region.id).collect();
        self.folded.retain(|id| valid_ids.contains(id));
        match crate::strict_json::from_str::<serde_json::Value>(text) {
            Ok(value) => {
                self.locations = super::operations::locations(text, &value);
                self.parsed = Some(value);
                self.validation_error = None;
                self.error_position = None;
            }
            Err(error) => {
                self.error_position = Some(super::operations::error_byte(
                    text,
                    error.line(),
                    error.column(),
                ));
                self.validation_error = Some(error.to_string());
                self.parsed = None;
                self.locations.clear();
            }
        }
        self.validated_text = Some(text.to_owned());
    }

    fn validate(&mut self, text: &str) -> Option<&str> {
        self.refresh_analysis(text);
        self.validation_error.as_deref()
    }

    pub(in crate::app) fn set_completion_defaults(
        &mut self,
        defaults: Result<serde_json::Value, String>,
    ) {
        match defaults {
            Ok(value) => {
                self.completion_defaults = Some(value);
                self.tools_notice = None;
            }
            Err(error) => {
                self.completion_defaults = None;
                self.tools_notice = Some(error);
            }
        }
    }

    pub(in crate::app) fn restore_location_next_draw(&mut self) {
        self.restore_location = true;
    }
}

#[derive(Default)]
pub(in crate::app) struct JsonEditorResponse {
    pub(in crate::app) save: bool,
    pub(in crate::app) reset: bool,
    pub(in crate::app) toggle_window: bool,
    pub(in crate::app) load_defaults: bool,
}

/// Keep source mutations and display projection in a fixed order within each frame.
pub(in crate::app) fn draw(
    ui: &mut egui::Ui,
    text: &mut String,
    state: &mut JsonEditorState,
    detached: bool,
    unsaved: bool,
) -> JsonEditorResponse {
    let mut response = JsonEditorResponse::default();
    let time = begin_frame(ui, text, state);
    state.refresh_analysis(text);
    controls::draw_toolbar(ui, text, state, detached, unsaved, &mut response);
    let navigation = controls::draw_search(ui, text, state);
    viewport::draw(ui, text, state, navigation, time);
    controls::draw_status(ui, text, state);
    response
}

fn begin_frame(ui: &mut egui::Ui, text: &mut String, state: &mut JsonEditorState) -> f64 {
    let time = ui.input(|input| input.time);
    state.history.feed_state(time, text);
    if state
        .text_edit_id
        .is_some_and(|id| ui.memory(|memory| memory.has_focus(id)))
    {
        let redo = ui.input_mut(|input| {
            input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
        });
        let undo = ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
        if undo || redo {
            state.restore_history(text, redo);
        }
    }
    time
}

#[cfg(test)]
mod tests;

mod tools;
