//! File selection and decoding stay off the UI thread; only successful imports replace the draft.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::{ImportedIcon, WeaponIconEdit};

type ImportResult = Result<Option<ImportedIcon>, String>;

#[derive(Default)]
pub(super) struct ImageImport {
    pending: Option<Receiver<ImportResult>>,
    error: Option<String>,
}

impl ImageImport {
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn poll(&mut self, draft: &mut WeaponIconEdit) {
        let Some(receiver) = &self.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("Image import stopped unexpectedly. Please try again.".to_owned())
            }
        };
        self.pending = None;
        self.accept(result, draft);
    }

    fn accept(&mut self, result: ImportResult, draft: &mut WeaponIconEdit) {
        match result {
            Ok(Some(image)) => {
                draft.imported_image = Some(image);
                self.error = None;
            }
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut WeaponIconEdit,
        enabled: bool,
    ) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    enabled && !self.is_pending(),
                    egui::Button::new("Import image…"),
                )
                .clicked()
            {
                self.start(ui.ctx().clone());
            }
            if draft.imported_image.is_some()
                && ui
                    .add_enabled(!self.is_pending(), egui::Button::new("Use donor artwork"))
                    .clicked()
            {
                draft.imported_image = None;
                self.error = None;
                changed = true;
            }
            if self.is_pending() {
                ui.spinner();
                ui.label("Importing…");
            }
        });
        ui.small(if draft.imported_image.is_some() {
            "Imported image · saved in this recipe. Proportions and transparency are preserved."
        } else {
            "PNG or JPEG · up to 16 MiB / 4096×4096. Fitted without cropping. Transparent PNG recommended."
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        changed
    }

    fn start(&mut self, context: egui::Context) {
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        self.error = None;
        let spawned = std::thread::Builder::new()
            .name("weapon-icon-import".to_owned())
            .spawn(move || {
                let result = rfd::FileDialog::new()
                    .set_title("Import primary icon artwork")
                    .add_filter("PNG / JPEG images", &["png", "jpg", "jpeg"])
                    .pick_file()
                    .map(|path| ImportedIcon::from_path(&path))
                    .transpose();
                let _ = sender.send(result);
                context.request_repaint();
            });
        if let Err(error) = spawned {
            self.pending = None;
            self.error = Some(format!("Could not start image import: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_and_cancelled_imports_leave_draft_unchanged() {
        let mut draft = WeaponIconEdit {
            brightness: 25,
            ..Default::default()
        };
        let before = draft.clone();
        let mut importer = ImageImport::default();
        importer.accept(Err("bad file".to_owned()), &mut draft);
        assert_eq!(draft, before);
        assert!(importer.error.is_some());
        importer.accept(Ok(None), &mut draft);
        assert_eq!(draft, before);
    }

    #[test]
    fn import_controls_fit_a_narrow_editor_without_changing_the_draft() {
        let context = egui::Context::default();
        let mut importer = ImageImport::default();
        let mut draft = WeaponIconEdit::default();
        for _ in 0..2 {
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let right = ui.max_rect().right();
                        assert!(!importer.draw(ui, &mut draft, true));
                        assert!(ui.min_rect().right() <= right + 1.0);
                    });
                },
            );
        }
        assert!(draft.is_identity());
        assert!(!importer.is_pending());
    }
}
