use super::{
    HudImage,
    image::{HEIGHT, WIDTH},
};
use std::sync::mpsc::{self, Receiver, TryRecvError};
#[derive(Default)]
pub(crate) struct Editor {
    pending: Option<Receiver<Result<Option<HudImage>, String>>>,
    error: Option<String>,
    preview: Option<(HudImage, egui::TextureHandle)>,
}
impl Editor {
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, draft: &mut Option<HudImage>) {
        if let Some(rx) = &self.pending {
            let result = match rx.try_recv() {
                Ok(value) => Some(value),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("HUD image import stopped unexpectedly".into()))
                }
            };
            if let Some(result) = result {
                self.pending = None;
                match result {
                    Ok(Some(image)) => {
                        *draft = Some(image);
                        self.error = None;
                    }
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
        }
        ui.heading("Ammo HUD icon");
        ui.label("Weapon silhouette beside the ammunition count. PNGs fit within 137 × 76 and preserve transparency.");
        if let Some(image) = draft.as_ref() {
            if self
                .preview
                .as_ref()
                .is_none_or(|(cached, _)| cached != image)
            {
                let texture = ui.ctx().load_texture(
                    "ammo-hud-preview",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [WIDTH as usize, HEIGHT as usize],
                        image.rgba(),
                    ),
                    egui::TextureOptions::LINEAR,
                );
                self.preview = Some((image.clone(), texture));
            }
            if let Some((_, texture)) = &self.preview {
                egui::Frame::new()
                    .fill(egui::Color32::from_gray(35))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.image((texture.id(), egui::vec2(WIDTH as f32, HEIGHT as f32)));
                    });
            }
        } else {
            ui.weak("Following the selected appearance donor's HUD icon.");
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Import HUD PNG…"))
                .clicked()
            {
                let (tx, rx) = mpsc::channel();
                self.pending = Some(rx);
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = rfd::FileDialog::new()
                        .set_title("Import ammo HUD icon")
                        .add_filter("PNG image", &["png"])
                        .pick_file()
                        .map(|path| HudImage::from_path(&path))
                        .transpose();
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            }
            if ui
                .add_enabled(
                    self.pending.is_none() && draft.is_some(),
                    egui::Button::new("Use appearance donor"),
                )
                .clicked()
            {
                *draft = None;
                self.preview = None;
                self.error = None;
            }
            if self.pending.is_some() {
                ui.spinner();
                ui.label("Importing…");
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
}
