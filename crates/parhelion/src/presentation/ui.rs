use super::{Artwork, Badge};
mod lore;
use super::editor::{self, Kind};

#[derive(Default)]
pub(crate) struct Editor {
    branding: crate::branding::Branding,
    badge: ImageEditor,
    corner: ImageEditor,
    lore: lore::Preview,
    artwork: Option<editor::Editor>,
    nameplate_source: Option<crate::HexHash>,
}
impl Editor {
    /// Discard recipe-specific previews and dialogs without forgetting the active runtime.
    pub(crate) fn reset(&mut self) {
        let branding = self.branding;
        *self = Self::default();
        self.set_branding(branding);
    }

    pub(crate) fn branding(&self) -> crate::branding::Branding {
        self.branding
    }

    pub(crate) fn set_branding(&mut self, branding: crate::branding::Branding) {
        if self.branding != branding {
            self.branding = branding;
            self.badge = ImageEditor {
                branding,
                ..Default::default()
            };
            self.corner = ImageEditor {
                branding,
                ..Default::default()
            };
            self.artwork = None;
            self.nameplate_source = None;
        }
    }
    /// Whether the base item's lore has finished loading, so a capture shows it.
    #[cfg(test)]
    pub(crate) fn lore_loaded(&self) -> bool {
        self.lore.loaded()
    }

    pub(crate) fn editing(&self) -> bool {
        self.artwork.is_some()
    }

    pub(crate) fn edit_nameplate(
        &mut self,
        part: crate::emblem::NameplatePart,
        size: (u32, u32),
        artwork: Artwork,
        source_emblem: Option<crate::HexHash>,
    ) {
        self.nameplate_source = source_emblem;
        self.artwork = Some(editor::Editor::with_branding(
            Kind::Nameplate { part, size },
            Some(artwork),
            self.branding,
        ));
    }

    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        draft: &mut crate::WeaponRecipeOverrides,
        packages: &std::path::Path,
        catalog: Option<&sundial::investment::InvestmentCatalog>,
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
        match editor.show_with_sources(ctx, packages, catalog) {
            Some(editor::Action::Apply(artwork)) => {
                if let Kind::Nameplate { part, .. } = kind {
                    let image = crate::emblem::NameplateImage::Artwork {
                        artwork,
                        source_emblem: self.nameplate_source.take(),
                    };
                    let nameplate = draft.nameplate.get_or_insert_with(Default::default);
                    let changed = nameplate.part(part) != Some(&image);
                    nameplate.set(part, Some(image));
                    self.artwork = None;
                    return changed;
                }
                let target = match kind {
                    Kind::Badge => draft.badge.as_mut().map(|badge| &mut badge.icon),
                    Kind::Watermark => Some(&mut draft.corner_icon),
                    Kind::Nameplate { .. } => unreachable!(),
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
                self.nameplate_source = None;
                false
            }
            None => false,
        }
    }

    /// `badges` are the library's, each with the recipe it is read from when picked.
    /// Armor's badge membership follows the classes its equipment supports.
    pub(crate) fn draw_badge(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut crate::WeaponRecipeOverrides,
        badges: &[(crate::recipe_library::LibraryBadge, std::path::PathBuf)],
        class_armor: bool,
    ) {
        ui.horizontal(|ui| {
            ui.strong("Collections Badge");
            sundial::investment::draw_authoring_info_icon(ui, "Group items in a custom badge. Use the same badge settings for each member. Normal Collections placement is unchanged.");
        });
        let mut enabled = draft.badge.is_some();
        if ui.checkbox(&mut enabled, "Custom Badge").changed() {
            draft.badge = enabled.then(Badge::default);
            self.badge = ImageEditor {
                branding: self.branding,
                ..Default::default()
            };
        }
        let mut include = !draft.exclude_from_sunrise_badge;
        if ui
            .checkbox(
                &mut include,
                format!("Include in {} Badge", self.branding.name()),
            )
            .changed()
        {
            draft.exclude_from_sunrise_badge = !include;
        }
        if class_armor {
            ui.weak("Armor joins the badge for its supported classes.");
        }
        if let Some(badge) = &mut draft.badge {
            if !badges.is_empty() {
                egui::ComboBox::from_id_salt("existing-badge")
                    .selected_text("Choose from Library")
                    .show_ui(ui, |ui| {
                        let icon = badge.icon.as_ref().map(Artwork::fingerprint);
                        for (existing, path) in badges {
                            let selected = badge.name == existing.name
                                && badge.description == existing.description
                                && icon == existing.icon;
                            if !ui.selectable_label(selected, &existing.name).clicked() {
                                continue;
                            }
                            // The library holds no artwork, so the badge is read whole from
                            // the recipe that carries it.
                            let error = match crate::recipe_library::load_badge(path) {
                                Ok(Some(picked)) => {
                                    *badge = picked;
                                    None
                                }
                                Ok(None) => Some("That recipe no longer has a badge.".to_owned()),
                                Err(error) => Some(error),
                            };
                            self.badge = ImageEditor {
                                branding: self.branding,
                                error,
                                ..Default::default()
                            };
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
                if self.branding == crate::branding::Branding::Dawn {
                    "Use Dawn Artwork"
                } else {
                    "Use Sunrise Artwork"
                },
            ) {
                self.artwork = Some(editor::Editor::with_branding(
                    Kind::Badge,
                    badge
                        .icon
                        .clone()
                        .or_else(|| self.branding.badge().ok().flatten()),
                    self.branding,
                ));
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
            if self.branding == crate::branding::Branding::Dawn {
                "Use Dawn Watermark"
            } else {
                "Use Sunrise Watermark"
            },
        ) {
            self.artwork = Some(editor::Editor::with_branding(
                Kind::Watermark,
                draft
                    .corner_icon
                    .clone()
                    .or_else(|| self.branding.corner().ok().flatten()),
                self.branding,
            ));
        }
    }

    /// The lore tab: the base item's, none, or a story of its own for an item of `kind`.
    pub(crate) fn draw_lore(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut crate::WeaponRecipeOverrides,
        packages: &std::path::Path,
        (item_hash, kind): (Option<u32>, crate::ItemKind),
    ) {
        self.lore.update(ui.ctx(), packages, item_hash);
        ui.strong("Lore Tab");
        if ui.checkbox(&mut draft.remove_lore, "No Lore Tab").changed() && draft.remove_lore {
            draft.lore = None;
        }

        let mut lore = draft.lore.is_some();
        if ui.checkbox(&mut lore, "Custom Lore Tab").changed() {
            if lore {
                draft.remove_lore = false;
            }
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
                    .hint_text(format!("Write this {}’s story…", kind.noun())),
            );
            ui.weak(format!("{} / 16,384 bytes", text.len()));
        } else if !draft.remove_lore {
            self.lore.draw(ui, kind);
        }
    }
}

#[derive(Default)]
struct ImageEditor {
    branding: crate::branding::Branding,
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
                    Kind::Badge => self
                        .branding
                        .badge()
                        .and_then(|default| {
                            crate::badge_icon::preview_with_branding(
                                draft.as_ref().or(default.as_ref()),
                                None,
                                self.branding,
                            )
                        })
                        .map(|image| editor::color_image(&image))
                        .map_err(|e| e.to_string()),
                    Kind::Watermark => match draft.as_ref() {
                        Some(artwork) => crate::watermark::render_custom_corner_preview(artwork)
                            .map(|image| editor::color_image(&image)),
                        None => self
                            .branding
                            .watermark()
                            .map(|image| editor::color_image(&image)),
                    }
                    .map_err(|e| e.to_string()),
                    Kind::Nameplate {
                        size: (width, height),
                        ..
                    } => draft
                        .as_ref()
                        .map(|artwork| editor::color_image(&artwork.render(width, height)))
                        .ok_or_else(|| "Choose nameplate artwork first.".to_owned()),
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
            if kind == Kind::Watermark {
                ui.horizontal(|ui| {
                    ui.strong(label);
                    sundial::investment::draw_authoring_info_icon(
                        ui,
                        "Marks the inventory icon's corner. Uses the image's silhouette, so a \
                         transparent PNG works best.",
                    );
                });
                let custom = draft.is_some();
                let name = if custom {
                    "Custom Artwork"
                } else {
                    reset.trim_start_matches("Use ")
                };
                let mut actions = vec![("Edit Artwork…", true)];
                if custom {
                    actions.push(("Use Default", true));
                }
                let clicked = sundial::investment::draw_authoring_tile(
                    ui,
                    self.preview.as_ref().map(|(_, texture)| texture),
                    name,
                    "Icon Corner",
                    &actions,
                );
                if clicked == Some(1) {
                    *draft = None;
                    self.preview = None;
                    self.error = None;
                }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                return clicked == Some(0);
            }
            ui.horizontal_top(|ui| {
                if let Some((_, texture)) = &self.preview {
                    let size = match kind {
                        Kind::Badge => egui::vec2(110.0, 67.0),
                        Kind::Watermark => egui::vec2(64.0, 64.0),
                        Kind::Nameplate {
                            size: (width, height),
                            ..
                        } => egui::vec2(110.0, 110.0 * height as f32 / width as f32),
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
