//! Advanced JSON editing and detached-editor coordination.
use super::save_support::SaveAction;
use super::{
    SundialApp, ViewMode, draw_json_account_source_notice, encode_settings_for_editor, json_editor,
    update_detached_window_state,
};
use eframe::egui;
use serde_json::Value;

fn check_json_edit_baseline(baseline: &Value, current: &Value) -> Result<(), String> {
    if baseline != current {
        return Err("The workspace changed after this JSON draft was created. Copy your draft before resetting the editor, then reapply your changes to the current document.".into());
    }
    Ok(())
}

impl SundialApp {
    pub(super) fn sync_raw_json(&mut self) {
        if let Ok(raw_json) = encode_settings_for_editor(self.document.json()) {
            self.raw_json = raw_json;
            self.raw_json_document = self.document.json().clone();
            self.json_editor.mark_synced();
            self.json_editor.reset_history();
            self.json_editor.restore_location_next_draw();
        }
    }

    pub(super) fn sync_raw_json_if_stale(&mut self) {
        if !self.json_editor.has_unapplied_changes()
            && self.raw_json_document != *self.document.json()
        {
            self.sync_raw_json();
        }
    }

    pub(super) fn apply_raw_json(&mut self) -> bool {
        self.apply_raw_json_with_status(true)
    }

    pub(super) fn apply_raw_json_silently(&mut self) -> bool {
        self.apply_raw_json_with_status(false)
    }

    pub(super) fn apply_raw_json_with_status(&mut self, report_status: bool) -> bool {
        if let Err(error) = check_json_edit_baseline(&self.raw_json_document, self.document.json())
        {
            self.json_editor.set_application_error(error.clone());
            if report_status {
                self.set_status(error, true);
            }
            return false;
        }
        match crate::strict_json::from_str::<Value>(&self.raw_json) {
            Ok(document) => {
                let mut candidate = self.document.clone();
                candidate.replace_json(document.clone());
                let compatibility_warning = match self.validation_warning_for_write(&candidate) {
                    Ok(warning) => warning,
                    Err(error) => {
                        self.json_editor.set_application_error(error.clone());
                        if report_status {
                            self.set_status(format!("Advanced JSON not applied: {error}"), true);
                        }
                        return false;
                    }
                };
                let changed = document != *self.document.json();
                self.raw_json_document = document.clone();
                self.document.replace_json(document);
                self.progression_ui.invalidate_document();
                self.selected_character = self
                    .selected_character
                    .min(self.character_count().saturating_sub(1));
                self.clear_picker_state();
                self.dirty |= changed;
                if report_status {
                    if let Some(warning) = compatibility_warning {
                        self.set_status(
                            format!(
                                "Advanced JSON applied with the existing compatibility warning: {warning}. Click Save to write it"
                            ),
                            true,
                        );
                    } else {
                        self.set_status("Advanced JSON applied; click Save to write it", false);
                    }
                }
                self.json_editor.mark_synced();
                true
            }
            Err(error) => {
                if report_status {
                    self.set_status(
                        format!(
                            "JSON syntax error at line {}, column {}: {error}",
                            error.line(),
                            error.column()
                        ),
                        true,
                    );
                }
                false
            }
        }
    }

    pub(super) fn set_json_editor_window_open(&mut self, open: bool) {
        update_detached_window_state(
            &mut self.json_editor_window_open,
            &mut self.json_editor_window_generation,
            open,
        );
    }

    pub(super) fn handle_json_editor_response(
        &mut self,
        ctx: &egui::Context,
        response: json_editor::JsonEditorResponse,
    ) {
        if response.load_defaults {
            self.json_editor.set_completion_defaults(
                super::settings::load_installed_sunrise_defaults(&self.install_path),
            );
        }
        if response.save {
            self.request_save(ctx, SaveAction::Save);
        }
        if response.reset {
            self.sync_raw_json();
            self.set_status("JSON editor reset to current settings", false);
        }
        if response.toggle_window {
            self.set_json_editor_window_open(!self.json_editor_window_open);
            self.json_editor.restore_location_next_draw();
            if !self.json_editor_window_open {
                self.view_mode = ViewMode::AdvancedJson;
            }
        }
    }

    pub(super) fn draw_json_editor_window(&mut self, ctx: &egui::Context) {
        if !self.json_editor_window_open {
            return;
        }

        self.sync_raw_json_if_stale();
        let account_source_kind = self.document.source_info().kind;
        let (response, close_requested) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of((
                "sundial_json_editor",
                self.json_editor_window_generation,
            )),
            egui::ViewportBuilder::default()
                .with_title("Sundial: All settings (JSON)")
                .with_inner_size([960.0, 720.0])
                .with_min_inner_size([640.0, 420.0]),
            |child_ctx, class| {
                let close_requested = child_ctx.input(|input| input.viewport().close_requested());
                let mut response = json_editor::JsonEditorResponse::default();
                if class == egui::ViewportClass::Embedded {
                    egui::Window::new("All Settings (JSON)")
                        .id(egui::Id::new("embedded_json_editor_window"))
                        .default_size([960.0, 720.0])
                        .show(child_ctx, |ui| {
                            draw_json_account_source_notice(ui, account_source_kind);
                            response = json_editor::draw(
                                ui,
                                &mut self.raw_json,
                                &mut self.json_editor,
                                true,
                                self.dirty,
                            );
                        });
                } else {
                    egui::CentralPanel::default().show(child_ctx, |ui| {
                        draw_json_account_source_notice(ui, account_source_kind);
                        response = json_editor::draw(
                            ui,
                            &mut self.raw_json,
                            &mut self.json_editor,
                            true,
                            self.dirty,
                        );
                    });
                }
                (response, close_requested)
            },
        );

        self.handle_json_editor_response(ctx, response);
        if self.json_editor.take_auto_apply_request() {
            let _ = self.apply_raw_json_silently();
        }
        if close_requested {
            self.set_json_editor_window_open(false);
            self.json_editor.restore_location_next_draw();
            if self.json_editor.has_unapplied_changes() {
                self.view_mode = ViewMode::AdvancedJson;
            }
        }
    }
}
