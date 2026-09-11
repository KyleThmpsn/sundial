use super::{
    HudImage,
    image::{HEIGHT, WIDTH},
};
mod preview;
#[cfg(test)]
mod tests;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(crate) struct Appearance<'a> {
    pub packages: &'a Path,
    pub pattern_index: Option<u16>,
    pub name: &'a str,
}

#[derive(Default)]
pub(crate) struct Editor {
    pending: Option<Receiver<Result<Option<HudImage>, String>>>,
    error: Option<String>,
    preview: Option<(HudImage, egui::TextureHandle)>,
    inherited: preview::Preview,
}
impl Editor {
    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut Option<HudImage>,
        appearance: Appearance<'_>,
    ) {
        if ui.is_enabled()
            && let Some(rx) = &self.pending
        {
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
        self.inherited.update(
            ui.ctx(),
            appearance.packages,
            draft
                .is_none()
                .then_some(appearance.pattern_index)
                .flatten(),
        );
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
        }
        let texture = if draft.is_some() {
            self.preview.as_ref().map(|(_, texture)| texture.clone())
        } else {
            self.inherited.texture().cloned()
        };
        ui.horizontal_top(|ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_gray(35))
                .inner_margin(4.0)
                .show(ui, |ui| {
                    let size = egui::vec2(WIDTH as f32, HEIGHT as f32);
                    if let Some(texture) = texture {
                        ui.add(egui::Image::new(&texture).fit_to_exact_size(size).maintain_aspect_ratio(true));
                    } else {
                        ui.allocate_ui_with_layout(size, egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                            ui.weak(self.inherited.status());
                        });
                    }
                });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.strong("Ammo HUD Icon");
                    sundial::investment::draw_authoring_info_icon(ui, "Weapon silhouette beside the ammunition count. Imported PNGs fit within 137 × 76 and preserve transparency. The preview uses the selected appearance unless you import an image.");
                });
                if draft.is_some() {
                    ui.label("Custom PNG");
                } else {
                    ui.label(format!("From {}", appearance.name));
                }
                self.draw_actions(ui, draft);
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                } else if draft.is_none() && let Some(error) = self.inherited.error() {
                    ui.weak("Preview unavailable").on_hover_text(error);
                }
            });
        });
    }

    fn draw_actions(&mut self, ui: &mut egui::Ui, draft: &mut Option<HudImage>) {
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
                        .set_title("Import Ammo HUD Icon")
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
                    egui::Button::new("Use Appearance"),
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
    }
}
