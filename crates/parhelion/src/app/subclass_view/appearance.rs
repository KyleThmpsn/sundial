//! A subclass's Appearance page: its Screen Art, the full-screen character picture its subclass
//! screen shows for each attunement, top, bottom and middle. Each is the base's, another
//! subclass's or a picture of the recipe's own, and exports as a PNG.
use super::*;
use crate::app::image_files::{self, ImageFiles, draw_contained};
use crate::image_import::EmbeddedImage;
use crate::subclass::{ArtImage, ArtPart, ScreenArt};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// The thumbnail inside a tile, and the tallest the editor draws a picture.
const THUMBNAIL: f32 = 110.0;
const PREVIEW: f32 = 320.0;
const TILE_PADDING: f32 = 8.0;
const TILE_GAP: f32 = 8.0;
/// The narrowest a tile gets before the tiles stack.
const TILE_MIN_WIDTH: f32 = 180.0;

/// A stock picture: the subclass's art container and which of its pictures.
type Key = (u32, ArtPart);

/// The selected picture, the picker's search, stock pictures loaded or loading, and a running
/// import or export with how the last one went.
#[derive(Default)]
pub(super) struct ArtPage {
    selected: ArtPart,
    query: String,
    pictures: BTreeMap<Key, Result<egui::TextureHandle, String>>,
    loading: Option<(Key, Receiver<Result<image::RgbaImage, String>>)>,
    files: ImageFiles<ArtPart>,
}

impl ArtPage {
    /// A stock picture once loaded, loading it on a worker, one at a time, when it is not.
    fn picture(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        key: Key,
    ) -> Option<Result<egui::TextureHandle, String>> {
        if let Some((loading, receiver)) = &self.loading {
            let loading = *loading;
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err("The loader stopped.".to_owned())),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(finished) = finished {
                let texture = finished.map(|pixels| {
                    ctx.load_texture(
                        format!("screen-art-{:08X}-{:?}", loading.0, loading.1),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [pixels.width() as usize, pixels.height() as usize],
                            pixels.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                });
                self.pictures.insert(loading, texture);
                self.loading = None;
            }
        }
        if let Some(picture) = self.pictures.get(&key) {
            return Some(picture.clone());
        }
        if self.loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            let context = ctx.clone();
            std::thread::spawn(move || {
                let _ = sender.send(load(&packages, key));
                context.request_repaint();
            });
            self.loading = Some((key, receiver));
        }
        None
    }

    /// Takes a finished import into the recipe, for the picture it was started for.
    fn poll(&mut self, recipe: &mut WeaponRecipe) {
        if let Some((part, image)) = self.files.poll() {
            set_part(recipe, part, Some(ArtImage::Image { image }));
        }
    }
}

fn load(packages: &Path, (container, part): Key) -> Result<image::RgbaImage, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    crate::subclass::art::picture_pixels(&manager, TagHash(container), part)
        .map_err(|error| error.to_string())
}

/// Gives one picture a source, or the base's with `None`.
fn set_part(recipe: &mut WeaponRecipe, part: ArtPart, image: Option<ArtImage>) {
    let art = recipe
        .overrides
        .screen_art
        .get_or_insert_with(ScreenArt::default);
    art.set(part, image);
    if art.is_empty() {
        recipe.overrides.screen_art = None;
    }
}

/// One picture as the page shows it.
struct Shown {
    texture: Option<egui::TextureHandle>,
    /// Where it comes from: the base, another subclass's picture, or a picture of its own.
    source: String,
    /// The stock picture it is, the base's included.
    stock: Option<(u32, ArtPart)>,
    picture: Option<EmbeddedImage>,
    modified: bool,
}

/// One picture's tile: its thumbnail, the attunements it shows for, and where it comes from.
fn draw_tile(
    ui: &mut egui::Ui,
    width: f32,
    part: ArtPart,
    shown: &Shown,
    selected: bool,
) -> egui::Response {
    let name_font = egui::FontId::proportional(13.0);
    let detail_font = egui::FontId::proportional(11.0);
    let (name_height, detail_height) =
        ui.fonts(|fonts| (fonts.row_height(&name_font), fonts.row_height(&detail_font)));
    let height = 2.0 * TILE_PADDING + THUMBNAIL + 6.0 + name_height + 2.0 + detail_height;
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
        let thumbnail = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), THUMBNAIL));
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
        painter.text(
            egui::pos2(inner.left(), name.bottom() + 2.0),
            egui::Align2::LEFT_TOP,
            &shown.source,
            detail_font,
            style::secondary(visuals),
        );
    }
    style::named_control(response, part.label())
}

impl PackageAuthoringApp {
    /// Whether the Appearance tab's stock pictures, the base's and the taken one, have loaded.
    #[cfg(test)]
    pub(in crate::app) fn subclass_page_art_loaded(&self) -> bool {
        let art = &self.subclass_page.art;
        art.loading.is_none() && art.pictures.len() >= 2
    }

    /// A subclass's own name, by item hash.
    fn subclass_name(&self, hash: u32) -> String {
        self.subclasses
            .iter()
            .find(|subclass| subclass.hash == hash)
            .map_or_else(|| format!("0x{hash:08X}"), |subclass| subclass.name.clone())
    }

    /// A subclass's art container, from the catalog.
    fn art_container(&self, hash: u32) -> Option<u32> {
        self.catalog.as_ref()?.nameplate_container(hash)
    }

    /// One picture as the page shows it, loading its texture as it goes.
    fn shown_art(&self, ctx: &egui::Context, part: ArtPart, page: &mut ArtPage) -> Shown {
        let base = self.recipe.donor.item_hash.parse_u32().ok();
        let mut stock = |hash: u32, taken: ArtPart| {
            let container = self.art_container(hash)?;
            page.picture(ctx, &self.packages, (container, taken))?.ok()
        };
        match self
            .recipe
            .overrides
            .screen_art
            .as_ref()
            .and_then(|art| art.part(part))
        {
            None => Shown {
                texture: base.and_then(|base| stock(base, part)),
                source: "Base".to_owned(),
                stock: base.map(|base| (base, part)),
                picture: None,
                modified: false,
            },
            Some(ArtImage::Subclass {
                item_hash,
                part: taken,
            }) => {
                let hash = item_hash.parse_u32().ok();
                Shown {
                    texture: hash.and_then(|hash| stock(hash, *taken)),
                    source: hash.map_or_else(
                        || "Unknown Subclass".to_owned(),
                        |hash| format!("{} · {}", self.subclass_name(hash), taken.label()),
                    ),
                    stock: hash.map(|hash| (hash, *taken)),
                    picture: None,
                    modified: true,
                }
            }
            Some(ArtImage::Image { image }) => Shown {
                texture: Some(image_files::covered_texture(
                    ctx,
                    "screen-art-picture",
                    image,
                    ArtPart::SIZE,
                )),
                source: "Picture".to_owned(),
                stock: None,
                picture: Some(image.clone()),
                modified: true,
            },
        }
    }

    /// The subclass's Appearance page: a tile for each screen picture, and an editor for the
    /// selected one.
    pub(in crate::app) fn draw_subclass_appearance(&mut self, ui: &mut egui::Ui) {
        let mut page = std::mem::take(&mut self.subclass_page.art);
        page.poll(&mut self.recipe);
        ui.heading("Screen Art");
        ui.add_space(4.0);
        let shown = ArtPart::ALL.map(|part| self.shown_art(ui.ctx(), part, &mut page));
        let line = ui.available_width();
        let count = ArtPart::ALL.len() as f32;
        let side_by_side = line >= count * TILE_MIN_WIDTH + (count - 1.0) * TILE_GAP;
        let width = if side_by_side {
            ((line - (count - 1.0) * TILE_GAP) / count)
                .floor()
                .min(280.0)
        } else {
            line
        };
        let mut picked = None;
        let rows: Vec<&[ArtPart]> = if side_by_side {
            vec![&ArtPart::ALL]
        } else {
            ArtPart::ALL.chunks(1).collect()
        };
        for row in rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = TILE_GAP;
                for part in row {
                    let selected = page.selected == *part;
                    if draw_tile(ui, width, *part, &shown[part.index()], selected).clicked() {
                        picked = Some(*part);
                    }
                }
            });
        }
        if let Some(part) = picked {
            page.selected = part;
        }
        ui.add_space(TILE_GAP);
        let selected = page.selected;
        self.draw_art_editor(ui, selected, &shown[selected.index()], &mut page);
        if page.loading.is_some() || page.files.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.subclass_page.art = page;
    }

    /// The selected picture under its name, with Reset once it is changed, and where it comes
    /// from below it.
    fn draw_art_editor(
        &mut self,
        ui: &mut egui::Ui,
        part: ArtPart,
        shown: &Shown,
        page: &mut ArtPage,
    ) {
        style::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(part.label());
                let (width, height) = ArtPart::SIZE;
                ui.weak(format!("({width} × {height} px)"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if style::reset(ui, shown.modified) {
                        set_part(&mut self.recipe, part, None);
                    }
                });
            });
            let side = ui.available_width().min(PREVIEW);
            let (frame, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            draw_contained(ui, frame, shown.texture.as_ref());
            ui.add_space(4.0);
            self.draw_art_source(ui, part, shown, page);
            match &page.files.outcome {
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

    /// The subclass picture it comes from, as a picker, then Import Image… and Export PNG….
    fn draw_art_source(
        &mut self,
        ui: &mut egui::Ui,
        part: ArtPart,
        shown: &Shown,
        page: &mut ArtPage,
    ) {
        let base = self.recipe.donor.item_hash.parse_u32().ok();
        let busy = page.files.busy();
        let mut choice = None;
        let (mut import, mut export) = (false, false);
        let subclasses = self
            .subclasses
            .iter()
            .map(|subclass| (subclass.hash, subclass.name.clone()))
            .collect::<Vec<_>>();
        ui.horizontal_wrapped(|ui| {
            const BASE: &str = "Follow Base Subclass";
            let salt = ("subclass-screen-art", part);
            let current = if shown.modified {
                shown.source.clone()
            } else {
                BASE.to_owned()
            };
            egui::ComboBox::from_id_salt(salt)
                .width(260.0)
                .truncate()
                .selected_text(current)
                .show_ui(ui, |ui| {
                    let choices = subclasses.len() * ArtPart::ALL.len();
                    let query = if crate::app::pickers::wants_filter(choices) {
                        ui.add(
                            egui::TextEdit::singleline(&mut page.query)
                                .hint_text(format!(
                                    "{} Filter",
                                    egui_phosphor::regular::MAGNIFYING_GLASS
                                ))
                                .desired_width(f32::INFINITY),
                        );
                        page.query.trim().to_lowercase()
                    } else {
                        String::new()
                    };
                    if ui.selectable_label(!shown.modified, BASE).clicked() {
                        choice = Some(None);
                    }
                    for (hash, name) in &subclasses {
                        for taken in ArtPart::ALL {
                            let label = format!("{name} · {}", taken.label());
                            if !query.is_empty() && !label.to_lowercase().contains(&query) {
                                continue;
                            }
                            let selected = shown.modified && shown.stock == Some((*hash, taken));
                            if ui.selectable_label(selected, label).clicked() {
                                choice = Some((Some(*hash) != base || taken != part).then(|| {
                                    ArtImage::Subclass {
                                        item_hash: (*hash).into(),
                                        part: taken,
                                    }
                                }));
                            }
                        }
                    }
                });
            crate::app::pickers::name_combo(ui, salt, "Screen Art Source");
            import = ui
                .add_enabled(!busy, egui::Button::new("Import Image…"))
                .clicked();
            export = ui
                .add_enabled(!busy, egui::Button::new("Export PNG…"))
                .clicked();
            if busy {
                ui.spinner();
            }
        });
        if let Some(choice) = choice {
            set_part(&mut self.recipe, part, choice);
        }
        if import {
            page.files.import(
                part,
                format!("Import {} Screen Art", part.label()),
                "screen-art-file",
                ui.ctx().clone(),
            );
        }
        if export {
            self.export_art(ui.ctx(), part, shown, page);
        }
    }

    /// Exports the picture the build carries: a picture of its own at its built size, any other
    /// as its subclass has it.
    fn export_art(&self, ctx: &egui::Context, part: ArtPart, shown: &Shown, page: &mut ArtPage) {
        let names = (
            format!("Export {} Screen Art", part.label()),
            format!(
                "{}-{}.png",
                self.recipe.slug(),
                part.label().to_lowercase().replace(' ', "-")
            ),
        );
        if let Some(picture) = shown.picture.clone() {
            let (width, height) = ArtPart::SIZE;
            page.files.export(
                part,
                names,
                "screen-art-file",
                move || Ok(crate::image_import::cover(picture.pixels(), width, height)),
                ctx.clone(),
            );
        } else if let Some((hash, taken)) = shown.stock
            && let Some(container) = self.art_container(hash)
        {
            let packages: PathBuf = self.packages.clone();
            page.files.export(
                part,
                names,
                "screen-art-file",
                move || load(&packages, (container, taken)),
                ctx.clone(),
            );
        }
    }
}
