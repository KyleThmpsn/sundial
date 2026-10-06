//! The emblem page's Nameplate section: a tile for each of its three images, the banner, the
//! overlay drawn on it and the wide background, and an editor for the selected one. Each image is
//! the base emblem's, another emblem's or a picture of the recipe's own, and exports as a PNG.
use super::image_files::{self, ImageFiles, draw_contained};
use super::*;
use crate::emblem::{Nameplate, NameplateImage, NameplatePart};
use crate::image_import::EmbeddedImage;
use std::path::Path;

/// Thumbnail height inside a tile.
const THUMBNAIL_HEIGHT: f32 = 48.0;
/// Space around a tile's thumbnail and text.
const TILE_PADDING: f32 = 8.0;
/// Below this width the tiles stack.
const TILE_MIN_WIDTH: f32 = 220.0;
/// Space between tiles, and between the tiles and the editor.
const TILE_GAP: f32 = 8.0;
/// The tallest the editor draws an image.
const PREVIEW_HEIGHT: f32 = 160.0;
/// The widest the editor's source picker gets.
const SOURCE_WIDTH: f32 = 420.0;

/// What an export writes: an image read from an emblem's nameplate container, or a picture at the
/// size it builds at.
pub(super) enum Export {
    Layer(u32),
    Picture(EmbeddedImage, (u32, u32)),
}

/// The selected image, the picker's search, a running import or export and how the last one went.
#[derive(Default)]
pub(super) struct EmblemPage {
    pub(super) selected: NameplatePart,
    query: String,
    files: ImageFiles<NameplatePart>,
    trackers: Option<Result<Vec<sundial::investment::EmblemTrackerCategory>, String>>,
}

impl EmblemPage {
    /// Takes a finished import into the recipe, for the image it was started for.
    fn poll(&mut self, recipe: &mut WeaponRecipe) {
        if let Some((part, image)) = self.files.poll() {
            set_part(recipe, part, Some(NameplateImage::Image { image }));
        }
    }
}

/// The pixels an export writes.
fn export_pixels(
    packages: &Path,
    part: NameplatePart,
    source: Export,
) -> Result<image::RgbaImage, String> {
    match source {
        Export::Layer(container) => {
            let manager = sundial::package_authoring::open_shadowkeep_package_manager(packages)?;
            crate::emblem::layer_pixels(&manager, TagHash(container), part)
                .map_err(|error| error.to_string())
        }
        Export::Picture(image, (width, height)) => {
            Ok(crate::image_import::cover(image.pixels(), width, height))
        }
    }
}

/// Writes one image to `path` as a PNG, replacing what is there only once it is whole. The page
/// exports through its file task, so only the emblem tests write directly.
#[cfg(test)]
pub(super) fn write_png(
    path: &Path,
    packages: &Path,
    part: NameplatePart,
    source: Export,
) -> Result<(), String> {
    image_files::write_png(path, export_pixels(packages, part, source)?)
}

/// Gives one image a source, or the base's with `None`.
fn set_part(recipe: &mut WeaponRecipe, part: NameplatePart, image: Option<NameplateImage>) {
    let nameplate = recipe
        .overrides
        .nameplate
        .get_or_insert_with(Nameplate::default);
    nameplate.set(part, image);
    if nameplate.is_empty() {
        recipe.overrides.nameplate = None;
    }
}

/// One image as the section shows it.
struct Shown {
    texture: Option<egui::TextureHandle>,
    /// The size it builds at, once its texture or the base's has loaded.
    size: Option<(u32, u32)>,
    /// Where it comes from: the base, another emblem by name, or a picture.
    source: String,
    /// The emblem it comes from, the base included.
    emblem: Option<u32>,
    picture: Option<EmbeddedImage>,
    modified: bool,
}

fn texture_size(texture: &egui::TextureHandle) -> (u32, u32) {
    let [width, height] = texture
        .size()
        .map(|side| u32::try_from(side).unwrap_or(u32::MAX));
    (width, height)
}

/// A size as the section writes it after a name.
fn size_label((width, height): (u32, u32)) -> String {
    format!("({width} × {height} px)")
}

/// One image's tile: its thumbnail, its name and size, and where it comes from. The selected tile
/// is outlined and a changed image's name reads brighter, as the Shader page's surfaces do.
fn draw_tile(
    ui: &mut egui::Ui,
    width: f32,
    part: NameplatePart,
    shown: &Shown,
    selected: bool,
) -> egui::Response {
    let name_font = egui::FontId::proportional(13.0);
    let detail_font = egui::FontId::proportional(11.0);
    let (name_height, detail_height) =
        ui.fonts(|fonts| (fonts.row_height(&name_font), fonts.row_height(&detail_font)));
    let height = 2.0 * TILE_PADDING + THUMBNAIL_HEIGHT + 6.0 + name_height + 2.0 + detail_height;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let stroke = if selected {
            visuals.selection.stroke
        } else if response.hovered() {
            visuals.widgets.hovered.bg_stroke
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            4.0,
            visuals.faint_bg_color,
            stroke,
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(TILE_PADDING);
        let thumbnail =
            egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), THUMBNAIL_HEIGHT));
        draw_contained(ui, thumbnail, shown.texture.as_ref());
        let name = painter.text(
            egui::pos2(inner.left(), thumbnail.bottom() + 6.0),
            egui::Align2::LEFT_TOP,
            part.label(),
            name_font,
            if shown.modified {
                visuals.text_color()
            } else {
                style::secondary(visuals)
            },
        );
        if let Some(size) = shown.size {
            painter.text(
                egui::pos2(name.right() + 6.0, name.bottom()),
                egui::Align2::LEFT_BOTTOM,
                size_label(size),
                detail_font.clone(),
                style::secondary(visuals),
            );
        }
        painter.text(
            egui::pos2(inner.left(), name.bottom() + 2.0),
            egui::Align2::LEFT_TOP,
            &shown.source,
            detail_font,
            style::secondary(visuals),
        );
    }
    named_control(response, part.label())
}

impl PackageAuthoringApp {
    pub(super) fn draw_emblem_trackers(&mut self, ui: &mut egui::Ui) {
        use crate::emblem::StatTrackers;
        ui.heading("Stat Trackers");
        if self.emblem_page.trackers.is_none() {
            self.emblem_page.trackers = Some(
                open_shadowkeep_package_manager(&self.packages).and_then(|manager| {
                    sundial::investment::load_emblem_tracker_categories(&manager)
                }),
            );
        }
        let Some(Ok(options)) = &self.emblem_page.trackers else {
            if let Some(Err(error)) = &self.emblem_page.trackers {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            return;
        };
        let choice = &mut self.recipe.overrides.stat_trackers;
        let mut mode = match choice {
            None => 0,
            Some(StatTrackers::All) => 1,
            Some(StatTrackers::Selected { .. }) => 2,
        };
        let before = mode;
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut mode, 0, "Follow Base Emblem");
            ui.radio_value(&mut mode, 1, "All Trackers");
            ui.radio_value(&mut mode, 2, "Selected Categories");
        });
        if mode != before {
            *choice = match mode {
                0 => None,
                1 => Some(StatTrackers::All),
                _ => Some(StatTrackers::Selected {
                    categories: options.iter().map(|o| o.hash.into()).collect(),
                }),
            };
        }
        ui.weak(
            "Each category allows its available trackers. Choose the displayed tracker in game.",
        );
        if let Some(StatTrackers::Selected { categories }) = choice {
            ui.horizontal(|ui| {
                if ui.small_button("Select All").clicked() {
                    *categories = options.iter().map(|o| o.hash.into()).collect();
                }
                if ui.small_button("Select None").clicked() {
                    categories.clear();
                }
            });
            for option in options {
                let hash = HexHash::from(option.hash);
                let mut selected = categories.contains(&hash);
                if ui
                    .checkbox(&mut selected, &option.name)
                    .on_hover_text(&option.description)
                    .changed()
                {
                    if selected {
                        categories.push(hash);
                    } else {
                        categories.retain(|h| h != &hash);
                    }
                }
            }
        }
    }

    fn emblem_name(&self, hash: u32) -> String {
        self.gear_donors
            .get(&ItemKind::Emblem)
            .and_then(|emblems| emblems.iter().find(|emblem| emblem.hash == hash))
            .map_or_else(|| format!("0x{hash:08X}"), |emblem| emblem.name.clone())
    }

    /// One image as the section shows it, loading its texture as it goes.
    fn shown_nameplate_image(&self, ctx: &egui::Context, part: NameplatePart) -> Shown {
        let base = self.recipe.donor.item_hash.parse_u32().ok();
        let layer = |hash: u32| {
            self.catalog
                .as_ref()
                .and_then(|catalog| catalog.nameplate_texture(ctx, hash, part.layer_offset()))
        };
        let image = self
            .recipe
            .overrides
            .nameplate
            .as_ref()
            .and_then(|nameplate| nameplate.part(part));
        match image {
            None => {
                let texture = base.and_then(layer);
                Shown {
                    size: texture.as_ref().map(texture_size),
                    texture,
                    source: "Base".to_owned(),
                    emblem: base,
                    picture: None,
                    modified: false,
                }
            }
            Some(NameplateImage::Emblem { item_hash }) => {
                let hash = item_hash.parse_u32().ok();
                let texture = hash.and_then(layer);
                Shown {
                    size: texture.as_ref().map(texture_size),
                    texture,
                    source: hash.map_or_else(
                        || "Unknown Emblem".to_owned(),
                        |hash| self.emblem_name(hash),
                    ),
                    emblem: hash,
                    picture: None,
                    modified: true,
                }
            }
            Some(NameplateImage::Image { image }) => {
                // A picture builds at the size of the base's own image.
                let size = base.and_then(layer).as_ref().map(texture_size);
                Shown {
                    texture: Some(image_files::covered_texture(
                        ctx,
                        "nameplate-picture",
                        image,
                        size.unwrap_or_else(|| part.size()),
                    )),
                    size,
                    source: "Picture".to_owned(),
                    emblem: None,
                    picture: Some(image.clone()),
                    modified: true,
                }
            }
        }
    }

    /// An emblem's nameplate: a tile for each image, and an editor for the selected one.
    pub(super) fn draw_emblem_nameplate(&mut self, ui: &mut egui::Ui) {
        self.emblem_page.poll(&mut self.recipe);
        ui.heading("Nameplate");
        let mut colors = self
            .recipe
            .overrides
            .nameplate
            .as_ref()
            .and_then(|n| n.colors)
            .map(|colors| colors.map(|color| color.map(|value| value.get())));
        let before = colors;
        if let Some(values) = &mut colors {
            ui.horizontal(|ui| {
                for (index, value) in values.iter_mut().enumerate() {
                    ui.label(format!("Color {}", index + 1));
                    ui.color_edit_button_rgba_unmultiplied(value);
                }
            });
            if ui.small_button("Use Base Colors").clicked() {
                colors = None;
            }
        } else if ui.small_button("Customize Colors").clicked() {
            colors = Some([[0.0, 0.0, 0.0, 1.0], [0.0; 4]]);
        }
        if colors != before {
            let nameplate = self
                .recipe
                .overrides
                .nameplate
                .get_or_insert_with(Nameplate::default);
            nameplate.colors = colors.map(|colors| {
                colors.map(|color| {
                    color.map(|value| {
                        crate::dye::DyeValue::new(value).expect("Color picker values are finite")
                    })
                })
            });
            if nameplate.is_empty() {
                self.recipe.overrides.nameplate = None;
            }
        }
        ui.add_space(4.0);
        let shown = NameplatePart::ALL.map(|part| self.shown_nameplate_image(ui.ctx(), part));
        let line = ui.available_width();
        let columns: usize = if line >= 3.0 * TILE_MIN_WIDTH + 2.0 * TILE_GAP {
            3
        } else {
            1
        };
        let width = ((line - TILE_GAP * (columns - 1) as f32) / columns as f32).floor();
        let tiles = NameplatePart::ALL
            .into_iter()
            .zip(&shown)
            .collect::<Vec<_>>();
        let mut picked = None;
        for row in tiles.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = TILE_GAP;
                for &(part, image) in row {
                    let selected = self.emblem_page.selected == part;
                    if draw_tile(ui, width, part, image, selected).clicked() {
                        picked = Some(part);
                    }
                }
            });
        }
        if let Some(part) = picked {
            self.emblem_page.selected = part;
        }
        ui.add_space(TILE_GAP);
        let selected = self.emblem_page.selected;
        let index = NameplatePart::ALL
            .iter()
            .position(|part| *part == selected)
            .unwrap_or_default();
        self.draw_nameplate_editor(ui, selected, &shown[index]);
    }

    /// The selected image under its name and size, with Reset once it is changed, and where it
    /// comes from below it.
    fn draw_nameplate_editor(&mut self, ui: &mut egui::Ui, part: NameplatePart, shown: &Shown) {
        style::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(part.label());
                if let Some(size) = shown.size {
                    ui.weak(size_label(size));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if style::reset(ui, shown.modified) {
                        set_part(&mut self.recipe, part, None);
                    }
                });
            });
            let width = ui.available_width();
            let height = shown.size.map_or(PREVIEW_HEIGHT, |(w, h)| {
                (width * h as f32 / w.max(1) as f32).min(PREVIEW_HEIGHT)
            });
            let (frame, _) =
                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            draw_contained(ui, frame, shown.texture.as_ref());
            ui.add_space(4.0);
            self.draw_nameplate_source(ui, part, shown);
            match &self.emblem_page.files.outcome {
                Some(Ok(message)) => {
                    ui.weak(*message);
                }
                Some(Err(error)) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                None => {}
            }
        });
    }

    /// The emblem the image comes from, as a picker, then Import Image… and Export PNG….
    fn draw_nameplate_source(&mut self, ui: &mut egui::Ui, part: NameplatePart, shown: &Shown) {
        let base = self.recipe.donor.item_hash.parse_u32().ok();
        let busy = self.emblem_page.files.busy();
        let (mut import, mut export, mut selection) = (false, false, None);
        let emblems = self
            .gear_donors
            .get(&ItemKind::Emblem)
            .map_or(&[][..], Vec::as_slice);
        // The picker keeps a source's width, with its actions beside it rather than across the card.
        ui.horizontal_wrapped(|ui| {
            let width = ui.available_width().min(SOURCE_WIDTH);
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            selection = ui
                .allocate_ui(size, |ui| {
                    self.catalog.as_ref().and_then(|catalog| {
                        catalog.draw_weapon_donor_dropdown_picker(
                            ui,
                            ("emblem-nameplate", part),
                            &mut self.emblem_page.query,
                            emblems,
                            WeaponDonorPickerOptions {
                                selected_hash: shown.emblem.filter(|_| shown.modified),
                                selected_label: &shown.source,
                                header_label: None,
                                action_label: "",
                                selected_icon_override: None,
                                secondary_action_label: None,
                                row_detail: None,
                                clear: Some(WeaponDonorPickerClearChoice {
                                    label: "Follow Base Emblem",
                                    tooltip: "Use the base emblem's image.",
                                    selected: !shown.modified,
                                }),
                                selected_detail: None,
                            },
                        )
                    })
                })
                .inner;
            import = ui
                .add_enabled(!busy, egui::Button::new("Import Image…"))
                .clicked();
            export = ui
                .add_enabled(
                    !busy && shown.size.is_some(),
                    egui::Button::new("Export PNG…"),
                )
                .clicked();
            if busy {
                ui.spinner();
            }
        });
        match selection {
            Some(WeaponDonorPickerAction::Clear) => set_part(&mut self.recipe, part, None),
            Some(WeaponDonorPickerAction::Select(hash)) => set_part(
                &mut self.recipe,
                part,
                (Some(hash) != base).then(|| NameplateImage::Emblem {
                    item_hash: hash.into(),
                }),
            ),
            Some(WeaponDonorPickerAction::Secondary) | None => {}
        }
        if import {
            self.emblem_page.files.import(
                part,
                format!("Import {}", part.label()),
                "nameplate-file",
                ui.ctx().clone(),
            );
        }
        if export {
            self.export_nameplate_image(ui.ctx(), part, shown);
        }
    }

    /// Exports the image the build carries: a picture at its built size, any other image as its
    /// emblem has it.
    fn export_nameplate_image(&mut self, ctx: &egui::Context, part: NameplatePart, shown: &Shown) {
        let source = match (&shown.picture, shown.size, shown.emblem) {
            (Some(picture), Some(size), _) => Export::Picture(picture.clone(), size),
            (None, _, Some(hash)) => match self
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.nameplate_container(hash))
            {
                Some(container) => Export::Layer(container),
                None => return,
            },
            _ => return,
        };
        let file_name = format!("{}-{}.png", self.recipe.slug(), part.label().to_lowercase());
        let packages = self.packages.clone();
        self.emblem_page.files.export(
            part,
            (format!("Export {}", part.label()), file_name),
            "nameplate-file",
            move || export_pixels(&packages, part, source),
            ctx.clone(),
        );
    }
}
