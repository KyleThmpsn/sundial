use super::{Artwork, Badge};
mod lore;
use super::editor::{self, Kind};

#[derive(Default)]
pub(crate) struct Editor {
    badge: ImageEditor,
    corner: ImageEditor,
    lore: lore::Preview,
    artwork: Option<editor::Editor>,
}
impl Editor {
    pub(crate) fn editing(&self) -> bool {
        self.artwork.is_some()
    }

    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        draft: &mut crate::WeaponRecipeOverrides,
        packages: &std::path::Path,
        icon: Option<(
            tiger_pkg::TagHash,
            crate::AuthoredWeaponRarity,
            crate::WeaponIconEdit,
        )>,
    ) -> bool {
        let Some(editor) = &mut self.artwork else {
            return false;
        };
        editor.load_context(ctx, packages, icon);
        let kind = editor.kind;
        match editor.show(ctx) {
            Some(editor::Action::Apply(artwork)) => {
                let target = match kind {
                    Kind::Badge => draft.badge.as_mut().map(|badge| &mut badge.icon),
                    Kind::Watermark => Some(&mut draft.corner_icon),
                };
                let mut changed = false;
                if let Some(target) = target {
                    changed = target.as_ref() != Some(&artwork);
                    *target = Some(artwork);
                }
                self.artwork = None;
                changed
            }
            Some(editor::Action::Cancel) => {
                self.artwork = None;
                false
            }
            None => false,
        }
    }

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
            if self.badge.draw(
                ui,
                &mut badge.icon,
                "Badge Artwork",
                Kind::Badge,
                "Use Sunrise Artwork",
            ) {
                self.artwork = Some(editor::Editor::new(Kind::Badge, badge.icon.clone()));
            }
        }
    }

    pub(crate) fn draw_corner(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut crate::WeaponRecipeOverrides,
    ) {
        if self.corner.draw(
            ui,
            &mut draft.corner_icon,
            "Release Watermark",
            Kind::Watermark,
            "Use Sunrise Watermark",
        ) {
            self.artwork = Some(editor::Editor::new(
                Kind::Watermark,
                draft.corner_icon.clone(),
            ));
        }
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
    preview: Option<(Option<Artwork>, egui::TextureHandle)>,
    error: Option<String>,
}
impl ImageEditor {
    fn draw(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut Option<Artwork>,
        label: &str,
        kind: Kind,
        reset: &str,
    ) -> bool {
        ui.push_id(label, |ui| {
            if self
                .preview
                .as_ref()
                .is_none_or(|(cached, _)| cached != draft)
            {
                let rendered = match kind {
                    Kind::Badge => crate::badge_icon::preview(draft.as_ref(), None)
                        .map(|image| editor::color_image(&image))
                        .map_err(|e| e.to_string()),
                    Kind::Watermark => match draft.as_ref() {
                        Some(artwork) => crate::watermark::render_custom_corner_preview(artwork)
                            .map(|image| editor::color_image(&image)),
                        None => crate::watermark::render_output_texture(0)
                            .map(|image| editor::color_image(&image)),
                    }
                    .map_err(|e| e.to_string()),
                };
                match rendered {
                    Ok(image) => {
                        self.preview = Some((
                            draft.clone(),
                            ui.ctx()
                                .load_texture(label, image, egui::TextureOptions::LINEAR),
                        ));
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            ui.horizontal_top(|ui| {
                if let Some((_, texture)) = &self.preview {
                    let size = match kind {
                        Kind::Badge => egui::vec2(110.0, 67.0),
                        Kind::Watermark => egui::vec2(64.0, 64.0),
                    };
                    ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                }
                ui.vertical(|ui| {
                    ui.strong(label);
                    let edit = ui.button("Edit Artwork…").clicked();
                    if ui
                        .add_enabled(draft.is_some(), egui::Button::new(reset))
                        .clicked()
                    {
                        *draft = None;
                        self.preview = None;
                        self.error = None;
                    }
                    if let Some(error) = &self.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    edit
                })
                .inner
            })
            .inner
        })
        .inner
    }
}
