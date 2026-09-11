use super::{Artwork, Badge};
mod lore;
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Default)]
pub(crate) struct Editor {
    badge: ImageEditor,
    corner: ImageEditor,
    lore: lore::Preview,
}
impl Editor {
    pub(crate) fn draw_badge(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut crate::WeaponRecipeOverrides,
        badges: &[Badge],
    ) {
        ui.horizontal(|ui| {
            ui.strong("Collections Badge");
            sundial::investment::draw_authoring_info_icon(ui, "Group weapons in a custom badge. Use the same badge settings for each member. Normal Collections placement is unchanged.");
        });
        let mut enabled = draft.badge.is_some();
        if ui.checkbox(&mut enabled, "Custom Badge").changed() {
            draft.badge = enabled.then(Badge::default);
            self.badge = ImageEditor::default();
        }
        let mut include = !draft.exclude_from_sunrise_badge;
        if ui
            .checkbox(&mut include, "Include in Project Sunrise Badge")
            .changed()
        {
            draft.exclude_from_sunrise_badge = !include;
        }
        if let Some(badge) = &mut draft.badge {
            if !badges.is_empty() {
                egui::ComboBox::from_id_salt("existing-badge")
                    .selected_text("Choose From Library")
                    .show_ui(ui, |ui| {
                        for existing in badges {
                            if ui
                                .selectable_label(badge == existing, &existing.name)
                                .clicked()
                            {
                                *badge = existing.clone();
                                self.badge = ImageEditor::default();
                            }
                        }
                    });
            }
            ui.add(
                egui::TextEdit::singleline(&mut badge.name)
                    .hint_text("Badge name")
                    .desired_width(f32::INFINITY),
            );
            ui.add(
                egui::TextEdit::multiline(&mut badge.description)
                    .hint_text("Set description")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            self.badge.draw(
                ui,
                &mut badge.icon,
                &format!(
                    "Badge Artwork ({} × {} px)",
                    super::artwork::WIDTH,
                    super::artwork::HEIGHT
                ),
                "Use Sunrise Artwork",
            );
        }
    }

    pub(crate) fn draw_corner(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut crate::WeaponRecipeOverrides,
    ) {
        self.corner.draw(
            ui,
            &mut draft.corner_icon,
            "Release Watermark",
            "Use Sunrise Watermark",
        );
        ui.weak("Release watermarks use the image silhouette. A transparent PNG works best.");
    }

    pub(crate) fn draw_lore(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut crate::WeaponRecipeOverrides,
        packages: &std::path::Path,
        item_hash: Option<u32>,
    ) {
        self.lore.update(ui.ctx(), packages, item_hash);
        ui.strong("Lore Tab");
        let mut lore = draft.lore.is_some();
        if ui.checkbox(&mut lore, "Custom Lore Tab").changed() {
            draft.lore = lore.then(|| {
                self.lore
                    .entry()
                    .map(|entry| entry.text.clone())
                    .unwrap_or_default()
            });
        }
        if let Some(text) = &mut draft.lore {
            ui.add(
                egui::TextEdit::multiline(text)
                    .desired_rows(8)
                    .desired_width(f32::INFINITY)
                    .hint_text("Write this weapon’s story…"),
            );
            ui.weak(format!("{} / 16,384 bytes", text.len()));
        } else {
            self.lore.draw(ui);
        }
    }
}

#[derive(Default)]
struct ImageEditor {
    pending: Option<Receiver<Result<Option<Artwork>, String>>>,
    preview: Option<(Artwork, egui::TextureHandle)>,
    error: Option<String>,
}
impl ImageEditor {
    fn draw(&mut self, ui: &mut egui::Ui, draft: &mut Option<Artwork>, label: &str, reset: &str) {
        ui.push_id(label, |ui| {
            if let Some(rx) = &self.pending {
                let result = match rx.try_recv() {
                    Ok(value) => Some(value),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        Some(Err("Image import stopped unexpectedly.".into()))
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
            if let Some(image) = draft.as_ref() {
                if self
                    .preview
                    .as_ref()
                    .is_none_or(|(cached, _)| cached != image)
                {
                    self.preview = Some((
                        image.clone(),
                        ui.ctx().load_texture(
                            label,
                            egui::ColorImage::from_rgba_unmultiplied(
                                [
                                    super::artwork::WIDTH as usize,
                                    super::artwork::HEIGHT as usize,
                                ],
                                image.rgba(),
                            ),
                            egui::TextureOptions::LINEAR,
                        ),
                    ));
                }
            } else {
                self.preview = None;
            }
            ui.horizontal_top(|ui| {
                if let Some((_, texture)) = &self.preview {
                    ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(56.0, 56.0)));
                }
                ui.vertical(|ui| {
                    ui.strong(label);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(self.pending.is_none(), egui::Button::new("Import PNG…"))
                            .clicked()
                        {
                            let (tx, rx) = mpsc::channel();
                            self.pending = Some(rx);
                            let ctx = ui.ctx().clone();
                            std::thread::spawn(move || {
                                let result = rfd::FileDialog::new()
                                    .set_title("Import Artwork")
                                    .add_filter("PNG Image", &["png"])
                                    .pick_file()
                                    .map(|path| Artwork::from_path(&path))
                                    .transpose();
                                let _ = tx.send(result);
                                ctx.request_repaint();
                            });
                        }
                        if ui
                            .add_enabled(
                                self.pending.is_none() && draft.is_some(),
                                egui::Button::new(reset),
                            )
                            .clicked()
                        {
                            *draft = None;
                            self.preview = None;
                            self.error = None;
                        }
                        if self.pending.is_some() {
                            ui.spinner();
                        }
                    });
                    if let Some(error) = &self.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                });
            });
        });
    }
}
