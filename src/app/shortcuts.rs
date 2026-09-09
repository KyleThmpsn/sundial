//! Workspace shortcuts run after focused controls have handled their input.
use super::{SaveAction, SundialApp};
use eframe::egui;

impl SundialApp {
    pub(super) fn handle_workspace_shortcuts(&mut self, ctx: &egui::Context) {
        if self.confirmation.is_some()
            || ctx.memory(|memory| memory.top_modal_layer().is_some() || memory.any_popup_open())
        {
            return;
        }

        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.request_save(ctx, SaveAction::Save);
        }

        let editing_text = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
        if editing_text || self.json_editor.has_unapplied_changes() {
            return;
        }

        // egui's logical modifier matching accepts extra Shift for Command+Z.
        // Consume the more specific redo combination before checking undo.
        let redo = ctx.input_mut(|input| {
            input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
        });
        if redo {
            self.redo();
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.undo();
        }
    }
}
