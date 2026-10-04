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
    fn receive(&mut self, draft: &mut Option<HudImage>) {
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
    }

    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut Option<HudImage>,
        appearance: Appearance<'_>,
    ) {
        if ui.is_enabled() {
            self.receive(draft);
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
                    squared(egui::ColorImage::from_rgba_unmultiplied(
                        [WIDTH as usize, HEIGHT as usize],
                        image.rgba(),
                    )),
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
        ui.horizontal(|ui| {
            ui.strong("Ammo HUD Icon");
            sundial::investment::draw_authoring_info_icon(
                ui,
                "The weapon's silhouette beside the ammunition count. An imported PNG fits within \
                 137 × 76 and keeps its transparency.",
            );
        });
        let custom = draft.is_some();
        let name = if custom {
            "Custom Image".to_owned()
        } else {
            appearance.name.to_owned()
        };
        let detail = if custom {
            "Imported PNG"
        } else if texture.is_none() {
            self.inherited.status()
        } else {
            "From Appearance"
        };
        let idle = self.pending.is_none();
        let mut actions = vec![("Import PNG…", idle)];
        if custom {
            actions.push(("Use Appearance", idle));
        }
        match sundial::investment::draw_authoring_tile(
            ui,
            texture.as_ref(),
            &name,
            detail,
            &actions,
        ) {
            Some(0) => self.import(ui.ctx()),
            Some(_) => {
                *draft = None;
                self.preview = None;
                self.error = None;
            }
            None => {}
        }
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Importing\u{2026}");
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        } else if !custom && let Some(error) = self.inherited.error() {
            ui.weak("Preview unavailable").on_hover_text(error);
        }
    }

    /// Asks for a PNG on a worker thread, so the file dialog never blocks the window.
    fn import(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
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
}

/// `image` centered in a square of its longer side, so a card's square thumbnail shows the whole
/// wide silhouette rather than squeezing it.
pub(super) fn squared(image: egui::ColorImage) -> egui::ColorImage {
    let [width, height] = image.size;
    let side = width.max(height);
    let mut square = egui::ColorImage::new([side, side], egui::Color32::TRANSPARENT);
    let (left, top) = ((side - width) / 2, (side - height) / 2);
    for y in 0..height {
        for x in 0..width {
            square.pixels[(top + y) * side + left + x] = image.pixels[y * width + x];
        }
    }
    square
}
