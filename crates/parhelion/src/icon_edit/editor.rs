//! Modal state and controls. Rendering/decoding and image transforms have separate owners.
use super::{
    IconColorReplacement, WeaponIconEdit, color_selection, import_ui,
    preview::{LoadedIconPreview, load_icon_preview},
};
use std::path::Path;
use sundial::package_authoring::open_shadowkeep_package_manager;
use tiger_pkg::TagHash;

#[cfg(test)]
mod integration_tests;

const ICON_EDITOR_MIN_WIDTH: f32 = 300.0;
const ICON_EDITOR_MAX_WIDTH: f32 = 840.0;
const ICON_EDITOR_VIEWPORT_MARGIN: f32 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct WeaponIconEditorLayout {
    modal_width: f32,
    preview_size: f32,
    side_preview: bool,
    content_height: f32,
    rules_per_page: usize,
}

impl WeaponIconEditorLayout {
    fn for_viewport(viewport: egui::Vec2) -> Self {
        let modal_width = (viewport.x - ICON_EDITOR_VIEWPORT_MARGIN)
            .clamp(ICON_EDITOR_MIN_WIDTH, ICON_EDITOR_MAX_WIDTH);
        let side_preview = modal_width >= 640.0 && viewport.y >= 520.0;
        let preview_size = if side_preview {
            ((viewport.y - 240.0) / 2.0).clamp(112.0, 176.0)
        } else {
            128.0
        };
        let content_height = if side_preview {
            preview_size * 2.0 + 68.0
        } else {
            (viewport.y - 150.0).clamp(240.0, 400.0)
        };
        Self {
            modal_width,
            preview_size,
            side_preview,
            content_height,
            rules_per_page: if side_preview { 3 } else { 1 },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum IconEditorTab {
    Preview,
    Recolor,
    #[default]
    Adjust,
    Image,
}

/// A terminal choice made in the weapon-icon editor modal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WeaponIconEditorAction {
    Cancel,
    Apply(WeaponIconEdit),
}

/// Modal state and package-backed preview data for editing only an icon's primary artwork layer.
pub(crate) struct WeaponIconEditor {
    donor_hash: u32,
    donor_name: String,
    draft: WeaponIconEdit,
    preview: Result<LoadedIconPreview, String>,
    source_texture: Option<egui::TextureHandle>,
    edited_texture: Option<egui::TextureHandle>,
    rendered_edit: Option<WeaponIconEdit>,
    image_import: import_ui::ImageImport,
    tab: IconEditorTab,
    color_page: usize,
}

impl WeaponIconEditor {
    /// Opens the donor packages synchronously and retains either the decoded preview or a displayable
    /// error. The supplied edit is copied into a private draft and is never changed on cancellation.
    pub(crate) fn open(
        package_directory: &Path,
        donor_hash: u32,
        donor_name: impl Into<String>,
        donor_container_tag: TagHash,
        rarity: crate::AuthoredWeaponRarity,
        current: WeaponIconEdit,
    ) -> Self {
        let preview = open_shadowkeep_package_manager(package_directory)
            .and_then(|manager| load_icon_preview(&manager, donor_container_tag, rarity));
        Self {
            donor_hash,
            donor_name: donor_name.into(),
            draft: current,
            preview,
            source_texture: None,
            edited_texture: None,
            rendered_edit: None,
            image_import: import_ui::ImageImport::default(),
            tab: IconEditorTab::default(),
            color_page: 0,
        }
    }

    /// Draws the modal and returns a value only when it should be dismissed.
    pub(crate) fn show(&mut self, context: &egui::Context) -> Option<WeaponIconEditorAction> {
        self.image_import.poll(&mut self.draft);
        self.sync_textures(context);
        let layout = WeaponIconEditorLayout::for_viewport(context.screen_rect().size());
        let modal = egui::Modal::new(egui::Id::new(("weapon-icon-editor", self.donor_hash))).show(
            context,
            |ui| {
                crate::app::workbench_style(ui);
                ui.set_width(layout.modal_width);
                ui.horizontal(|ui| {
                    ui.heading("Edit Weapon Icon");
                    ui.add(
                        egui::Label::new(egui::RichText::new(&self.donor_name).weak()).truncate(),
                    );
                });
                ui.separator();
                let mut changed = false;
                if layout.side_preview {
                    let preview_width = layout.preview_size + 8.0;
                    let controls_width = ui.available_width() - preview_width - 16.0;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(preview_width, layout.content_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(preview_width);
                                changed |= self.draw_preview_section(
                                    ui,
                                    false,
                                    layout.preview_size,
                                    layout.rules_per_page,
                                );
                            },
                        );
                        ui.add_space(8.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(controls_width, layout.content_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(controls_width);
                                changed |= self.draw_editor_tabs(ui, layout);
                            },
                        );
                    });
                } else {
                    ui.allocate_ui_with_layout(
                        egui::vec2(layout.modal_width, layout.content_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            changed |= self.draw_editor_tabs(ui, layout);
                        },
                    );
                }
                if changed {
                    self.rendered_edit = None;
                    context.request_repaint();
                }

                ui.add_space(5.0);
                ui.separator();
                ui.add_space(5.0);
                self.draw_footer(ui, context)
            },
        );

        if let Some(action) = modal.inner {
            Some(action)
        } else if modal.should_close() {
            Some(WeaponIconEditorAction::Cancel)
        } else {
            None
        }
    }

    fn draw_editor_tabs(&mut self, ui: &mut egui::Ui, layout: WeaponIconEditorLayout) -> bool {
        if layout.side_preview && self.tab == IconEditorTab::Preview {
            self.tab = IconEditorTab::Recolor;
        }
        ui.horizontal(|ui| {
            if !layout.side_preview {
                ui.selectable_value(&mut self.tab, IconEditorTab::Preview, "Preview");
            }
            ui.selectable_value(&mut self.tab, IconEditorTab::Adjust, "Adjust");
            ui.selectable_value(&mut self.tab, IconEditorTab::Recolor, "Replace Colors");
            ui.selectable_value(&mut self.tab, IconEditorTab::Image, "Image");
        });
        ui.separator();
        match self.tab {
            IconEditorTab::Preview => {
                self.draw_preview_section(ui, true, layout.preview_size, layout.rules_per_page)
            }
            IconEditorTab::Recolor => color_selection::draw_controls(
                ui,
                &mut self.draft.color_replacements,
                &mut self.color_page,
                layout.rules_per_page,
            ),
            IconEditorTab::Adjust => self.draw_color_controls(ui),
            IconEditorTab::Image => {
                ui.strong("Artwork source");
                let imported = self
                    .image_import
                    .draw(ui, &mut self.draft, self.preview.is_ok());
                ui.add_space(12.0);
                ui.separator();
                imported | self.draw_transform_controls(ui)
            }
        }
    }

    fn draw_preview_section(
        &mut self,
        ui: &mut egui::Ui,
        side_by_side: bool,
        size: f32,
        rules_per_page: usize,
    ) -> bool {
        ui.add_space(4.0);
        let mut picked = None;
        match &self.preview {
            Ok(preview) => {
                if side_by_side {
                    ui.columns(2, |columns| {
                        picked = draw_source_preview(
                            &mut columns[0],
                            self.source_texture.as_ref(),
                            preview,
                            &self.draft,
                            size,
                        );
                        draw_preview(
                            &mut columns[1],
                            "Final composited icon",
                            self.edited_texture.as_ref(),
                            size,
                        );
                    });
                } else {
                    picked = draw_source_preview(
                        ui,
                        self.source_texture.as_ref(),
                        preview,
                        &self.draft,
                        size,
                    );
                    ui.add_space(8.0);
                    draw_preview(ui, "Final icon", self.edited_texture.as_ref(), size);
                }
                for warning in &preview.warnings {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(warning).color(ui.visuals().warn_fg_color),
                        )
                        .truncate(),
                    )
                    .on_hover_text(warning);
                }
            }
            Err(error) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Could not load the donor icon preview: {error}"),
                );
            }
        }
        if let Some(color) = picked
            && self.draft.color_replacements.len() < color_selection::MAX_REPLACEMENTS
        {
            self.draft.color_replacements.push(IconColorReplacement {
                source: color,
                replacement: color,
                ..Default::default()
            });
            self.color_page = (self.draft.color_replacements.len() - 1) / rules_per_page;
            self.tab = IconEditorTab::Recolor;
            true
        } else {
            false
        }
    }

    fn draw_transform_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong("Transform");
            ui.label("Rotation");
            egui::ComboBox::from_id_salt("weapon-icon-rotation")
                .selected_text(match self.draft.rotation_quarter_turns {
                    0 => "None",
                    1 => "90° clockwise",
                    2 => "180°",
                    3 => "270° clockwise",
                    _ => "Invalid",
                })
                .show_ui(ui, |ui| {
                    for (turns, label) in [
                        (0, "None"),
                        (1, "90° clockwise"),
                        (2, "180°"),
                        (3, "270° clockwise"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut self.draft.rotation_quarter_turns, turns, label)
                            .changed();
                    }
                });
            changed |= ui
                .checkbox(&mut self.draft.flip_horizontal, "Flip horizontal")
                .changed();
            changed |= ui
                .checkbox(&mut self.draft.flip_vertical, "Flip vertical")
                .changed();
        });
        changed
    }

    fn draw_color_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong("All artwork colors");
            let grayscale = ui
                .add_enabled(
                    self.draft.saturation != WeaponIconEdit::MIN_COLOR_ADJUSTMENT,
                    egui::Button::new("Set grayscale"),
                )
                .on_hover_text(
                    "One-shot action: sets Saturation to -100%. Adjust or reset Saturation to restore color.",
                )
                .clicked();
            if grayscale {
                changed |= apply_grayscale_action(&mut self.draft);
            }
            if ui
                .add_enabled(
                    !self.draft.color_is_default(),
                    egui::Button::new("Reset color"),
                )
                .clicked()
            {
                self.draft.reset_color();
                changed = true;
            }
        });
        egui::Grid::new("weapon-icon-color-controls")
            .num_columns(3)
            .spacing([12.0, 5.0])
            .show(ui, |ui| {
                changed |= draw_icon_adjustment(
                    ui,
                    "Hue",
                    &mut self.draft.hue_shift_degrees,
                    WeaponIconEdit::MIN_HUE_SHIFT_DEGREES,
                    WeaponIconEdit::MAX_HUE_SHIFT_DEGREES,
                    "°",
                );
                ui.end_row();
                changed |= draw_icon_adjustment(
                    ui,
                    "Saturation",
                    &mut self.draft.saturation,
                    WeaponIconEdit::MIN_COLOR_ADJUSTMENT,
                    WeaponIconEdit::MAX_COLOR_ADJUSTMENT,
                    "%",
                );
                ui.end_row();
                changed |= draw_icon_adjustment(
                    ui,
                    "Brightness",
                    &mut self.draft.brightness,
                    WeaponIconEdit::MIN_COLOR_ADJUSTMENT,
                    WeaponIconEdit::MAX_COLOR_ADJUSTMENT,
                    "%",
                );
                ui.end_row();
                changed |= draw_icon_adjustment(
                    ui,
                    "Contrast",
                    &mut self.draft.contrast,
                    WeaponIconEdit::MIN_COLOR_ADJUSTMENT,
                    WeaponIconEdit::MAX_COLOR_ADJUSTMENT,
                    "%",
                );
                ui.end_row();
            });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            changed |= draw_compact_icon_adjustment(ui, "Red", &mut self.draft.red_balance);
            changed |= draw_compact_icon_adjustment(ui, "Green", &mut self.draft.green_balance);
            changed |= draw_compact_icon_adjustment(ui, "Blue", &mut self.draft.blue_balance);
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            changed |= ui
                .checkbox(&mut self.draft.invert, "Invert colors")
                .changed();
            ui.separator();
            ui.label("Opacity");
            changed |= ui
                .add(
                    egui::Slider::new(
                        &mut self.draft.opacity_percent,
                        WeaponIconEdit::MIN_OPACITY..=WeaponIconEdit::MAX_OPACITY,
                    )
                    .suffix("%")
                    .show_value(true),
                )
                .changed();
        });
        changed
    }

    fn draw_footer(
        &mut self,
        ui: &mut egui::Ui,
        context: &egui::Context,
    ) -> Option<WeaponIconEditorAction> {
        let compact = ui.available_width() < 380.0;
        let apply_enabled = self.preview.is_ok() && !self.image_import.is_pending();
        let reset_enabled =
            self.draft != WeaponIconEdit::default() && !self.image_import.is_pending();
        let mut apply = false;
        let mut cancel = false;
        let mut reset = false;
        if compact {
            ui.horizontal_wrapped(|ui| {
                reset = ui
                    .add_enabled(reset_enabled, egui::Button::new("Reset all"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
            apply = draw_primary_apply_button(ui, apply_enabled, true);
        } else {
            ui.horizontal(|ui| {
                reset = ui
                    .add_enabled(reset_enabled, egui::Button::new("Reset all"))
                    .clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    apply = draw_primary_apply_button(ui, apply_enabled, false);
                    cancel = ui.button("Cancel").clicked();
                });
            });
        }
        if reset {
            self.draft = WeaponIconEdit::default();
            self.rendered_edit = None;
            self.image_import = import_ui::ImageImport::default();
            self.color_page = 0;
            context.request_repaint();
        }
        if apply {
            Some(WeaponIconEditorAction::Apply(self.draft.clone()))
        } else if cancel {
            Some(WeaponIconEditorAction::Cancel)
        } else {
            None
        }
    }

    fn sync_textures(&mut self, context: &egui::Context) {
        let rendered = {
            let Ok(preview) = &self.preview else {
                return;
            };
            if self.rendered_edit.as_ref() == Some(&self.draft) {
                return;
            }
            let source = preview.source_primary(&self.draft);
            let image = egui::ColorImage::from_rgba_unmultiplied(source.size, &source.rgba);
            if let Some(texture) = self.source_texture.as_mut() {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.source_texture = Some(context.load_texture(
                    "weapon-icon-editor-source",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            preview.render(&self.draft)
        };
        match rendered {
            Ok(image) => {
                let name = format!("weapon-icon-editor-edited-{:08X}", self.donor_hash);
                if let Some(texture) = self.edited_texture.as_mut() {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.edited_texture =
                        Some(context.load_texture(name, image, egui::TextureOptions::LINEAR));
                }
                self.rendered_edit = Some(self.draft.clone());
            }
            Err(error) => self.preview = Err(error),
        }
    }
}

fn draw_primary_apply_button(ui: &mut egui::Ui, enabled: bool, fill_width: bool) -> bool {
    let fill = ui.visuals().selection.bg_fill;
    let button = || {
        egui::Button::new(egui::RichText::new("Apply icon changes").strong())
            .fill(fill)
            .min_size([150.0, 28.0].into())
    };
    if fill_width {
        let width = ui.available_width();
        ui.add_enabled_ui(enabled, |ui| {
            ui.add_sized([width, 28.0], button()).clicked()
        })
        .inner
    } else {
        ui.add_enabled(enabled, button()).clicked()
    }
}

fn apply_grayscale_action(edit: &mut WeaponIconEdit) -> bool {
    if edit.saturation == WeaponIconEdit::MIN_COLOR_ADJUSTMENT {
        false
    } else {
        edit.saturation = WeaponIconEdit::MIN_COLOR_ADJUSTMENT;
        true
    }
}

fn draw_source_preview(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    preview: &LoadedIconPreview,
    edit: &WeaponIconEdit,
    size: f32,
) -> Option<[u8; 3]> {
    let response = draw_preview(ui, "Source · click to pick", texture, size)?
        .on_hover_cursor(egui::CursorIcon::Crosshair);
    if !response.clicked() {
        return None;
    }
    let uv = (response.interact_pointer_pos()? - response.rect.min) / response.rect.size();
    color_selection::sample(&preview.source_primary(edit), uv)
}

fn draw_preview(
    ui: &mut egui::Ui,
    label: &str,
    texture: Option<&egui::TextureHandle>,
    size: f32,
) -> Option<egui::Response> {
    ui.vertical(|ui| {
        ui.strong(label);
        if let Some(texture) = texture {
            let response = ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(egui::vec2(size, size))
                    .texture_options(egui::TextureOptions::LINEAR)
                    .sense(egui::Sense::click()),
            );
            response
                .ctx
                .accesskit_node_builder(response.id, |node| node.set_label(label));
            Some(response)
        } else {
            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            None
        }
    })
    .inner
}

fn draw_icon_adjustment(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut i16,
    minimum: i16,
    maximum: i16,
    suffix: &str,
) -> bool {
    ui.label(label);
    let mut changed = ui
        .add(
            egui::Slider::new(value, minimum..=maximum)
                .suffix(suffix)
                .show_value(true),
        )
        .changed();
    if ui
        .add_enabled(*value != 0, egui::Button::new("Reset"))
        .clicked()
    {
        *value = 0;
        changed = true;
    }
    changed
}

fn draw_compact_icon_adjustment(ui: &mut egui::Ui, label: &str, value: &mut i16) -> bool {
    ui.label(label);
    ui.add(
        egui::DragValue::new(value)
            .range(WeaponIconEdit::MIN_COLOR_ADJUSTMENT..=WeaponIconEdit::MAX_COLOR_ADJUSTMENT)
            .suffix("%"),
    )
    .changed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_action_is_idempotent_instead_of_toggling() {
        let mut edit = WeaponIconEdit {
            saturation: 25,
            ..WeaponIconEdit::default()
        };
        assert!(apply_grayscale_action(&mut edit));
        assert_eq!(edit.saturation, WeaponIconEdit::MIN_COLOR_ADJUSTMENT);
        assert!(!apply_grayscale_action(&mut edit));
        assert_eq!(edit.saturation, WeaponIconEdit::MIN_COLOR_ADJUSTMENT);
    }
}
