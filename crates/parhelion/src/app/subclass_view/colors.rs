//! An ability's Effect Colors: each palette its effects draw with, as a swatch, the stock palette
//! its colors come from, and its hue, saturation and brightness. Palettes load on a worker, once
//! per ability entity, from the entity and the graphs it spawns, as the build finds them. Every
//! stock ability's palettes load once on another, named by the abilities that draw with them.
use super::*;
use crate::subclass::PaletteEdit;
use crate::subclass::palette::{MOST_HUE, MOST_PERCENT};
use std::sync::mpsc::{self, Receiver};
use sundial::package_authoring::ability_palette::{
    PALETTE_HEIGHT, PALETTE_WIDTH, ability_palettes, palette_pixels,
};

/// A swatch's size on the page.
const SWATCH: egui::Vec2 = egui::vec2(128.0, 14.0);

/// One palette an ability draws with: its stock pixels and how many effect uses reach it.
pub(super) struct Loaded {
    header: u32,
    uses: usize,
    pixels: Vec<u8>,
}

type Load = Result<Vec<Loaded>, String>;

/// A stock palette an ability's colors can come from: the abilities that draw with it, and its
/// pixels.
pub(super) struct Stock {
    header: u32,
    label: String,
    pixels: Vec<u8>,
}

type StockLoad = Result<Vec<Stock>, String>;

/// Each entity's palettes once loaded, every stock palette, and the swatches shown for them.
#[derive(Default)]
pub(super) struct Colors {
    entities: BTreeMap<u32, Result<Arc<Vec<Loaded>>, String>>,
    loading: Option<(u32, Receiver<Load>)>,
    stock: Option<Result<Arc<Vec<Stock>>, String>>,
    stock_loading: Option<Receiver<StockLoad>>,
    /// Each palette's swatch for the change it shows, and whether a taken palette's pixels had
    /// loaded for it.
    swatches: BTreeMap<u32, ((PaletteEdit, bool), egui::TextureHandle)>,
    stock_swatches: BTreeMap<u32, egui::TextureHandle>,
    query: String,
}

impl Colors {
    /// Takes a finished load, and starts one for `entity` when it has none. Returns its palettes
    /// once they are loaded.
    fn poll(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        entity: u32,
    ) -> Option<Result<Arc<Vec<Loaded>>, String>> {
        if let Some((loading, receiver)) = &self.loading {
            let loading = *loading;
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The loader stopped.".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(finished) = finished {
                self.entities.insert(loading, finished.map(Arc::new));
                self.loading = None;
            }
        }
        if let Some(loaded) = self.entities.get(&entity) {
            return Some(loaded.clone());
        }
        if self.loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            std::thread::spawn(move || {
                let _ = sender.send(load(&packages, entity));
            });
            self.loading = Some((entity, receiver));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        None
    }

    /// Takes the finished stock load, and starts it with `abilities` when it has not run.
    fn poll_stock(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        abilities: impl FnOnce() -> Vec<(u32, String)>,
    ) {
        if let Some(receiver) = &self.stock_loading {
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The loader stopped.".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(finished) = finished {
                self.stock = Some(finished.map(Arc::new));
                self.stock_loading = None;
            }
        }
        if self.stock.is_none() && self.stock_loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            let abilities = abilities();
            std::thread::spawn(move || {
                let _ = sender.send(load_stock(&packages, &abilities));
            });
            self.stock_loading = Some(receiver);
        }
        if self.stock.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// The pixels of stock palette `header`, once the stock palettes have loaded.
    fn stock_pixels(&self, header: u32) -> Option<&[u8]> {
        self.stock
            .as_ref()?
            .as_ref()
            .ok()?
            .iter()
            .find(|stock| stock.header == header)
            .map(|stock| stock.pixels.as_slice())
    }

    /// The swatch of `palette` as `edit` colors it, made again only when the edit changes or a
    /// taken palette's pixels arrive.
    fn swatch(
        &mut self,
        ctx: &egui::Context,
        palette: &Loaded,
        edit: PaletteEdit,
    ) -> egui::TextureHandle {
        let taken = edit.from.and_then(|from| self.stock_pixels(from));
        let key = (edit, taken.is_some());
        if let Some((shown, texture)) = self.swatches.get(&palette.header)
            && *shown == key
        {
            return texture.clone();
        }
        let mut pixels = taken.unwrap_or(&palette.pixels).to_vec();
        edit.apply(&mut pixels);
        let texture = texture(ctx, palette.header, &pixels);
        self.swatches.insert(palette.header, (key, texture.clone()));
        texture
    }

    fn stock_swatch(&mut self, ctx: &egui::Context, stock: &Stock) -> egui::TextureHandle {
        self.stock_swatches
            .entry(stock.header)
            .or_insert_with(|| texture(ctx, stock.header, &stock.pixels))
            .clone()
    }
}

fn texture(ctx: &egui::Context, header: u32, pixels: &[u8]) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [usize::from(PALETTE_WIDTH), usize::from(PALETTE_HEIGHT)],
        pixels,
    );
    ctx.load_texture(
        format!("ability-palette-{header:08X}"),
        image,
        egui::TextureOptions::LINEAR,
    )
}

/// Draws a palette swatch in `rect`, outlined.
fn paint_swatch(ui: &egui::Ui, rect: egui::Rect, texture: &egui::TextureHandle) {
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    ui.painter().rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
}

fn load(packages: &Path, entity: u32) -> Load {
    let manager = open_shadowkeep_package_manager(packages)?;
    ability_palettes(&manager, entity, tuning::SPAWN_DEPTH)?
        .into_iter()
        .map(|palette| {
            Ok(Loaded {
                header: palette.header,
                uses: palette.uses.len(),
                pixels: palette_pixels(&manager, palette.header)?,
            })
        })
        .collect()
}

/// Every palette the stock abilities' effects draw with, named by the first two abilities that
/// draw with it. An ability whose graphs cannot be read is left out rather than failing the list.
fn load_stock(packages: &Path, abilities: &[(u32, String)]) -> StockLoad {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut found = BTreeMap::<u32, BTreeSet<String>>::new();
    for (entity, name) in abilities {
        let Ok(palettes) = ability_palettes(&manager, *entity, tuning::SPAWN_DEPTH) else {
            continue;
        };
        for palette in palettes {
            found
                .entry(palette.header)
                .or_default()
                .insert(name.clone());
        }
    }
    let mut stock = found
        .into_iter()
        .map(|(header, names)| {
            let names = names.into_iter().collect::<Vec<_>>();
            let label = match names.len() {
                0 => format!("0x{header:08X}"),
                1 | 2 => names.join(", "),
                more => format!("{}, {} +{}", names[0], names[1], more - 2),
            };
            Ok(Stock {
                header,
                label,
                pixels: palette_pixels(&manager, header)?,
            })
        })
        .collect::<StockLoad>()?;
    stock.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(stock)
}

impl PackageAuthoringApp {
    /// Every stock ability entity with the name of an entry that uses it.
    fn stock_abilities(&self) -> Vec<(u32, String)> {
        let mut abilities = BTreeMap::<u32, String>::new();
        for subclass in &self.subclasses {
            for (entry, entity) in &subclass.entry_entities {
                if let Some(name) = subclass.entry_names.get(entry) {
                    abilities.entry(*entity).or_insert_with(|| name.clone());
                }
            }
        }
        abilities.into_iter().collect()
    }

    /// The Effect Colors field of an ability with an entity whose effects draw with palettes.
    /// Returns the entry's edits once they change.
    pub(super) fn draw_effect_colors(
        &self,
        ui: &mut egui::Ui,
        (summary, entry): (Option<&SubclassSummary>, u8),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let entity = *summary?.entry_entities.get(&entry)?;
        let loaded = page.colors.poll(ui.ctx(), &self.packages, entity);
        page.colors
            .poll_stock(ui.ctx(), &self.packages, || self.stock_abilities());
        // An ability whose effects draw with no palette shows no field.
        if loaded
            .as_ref()
            .is_some_and(|loaded| loaded.as_ref().is_ok_and(|palettes| palettes.is_empty()))
            && edits.palettes.is_empty()
        {
            return None;
        }
        let (changed, reset) =
            detail::field(ui, "Effect Colors", !edits.palettes.is_empty(), |ui| {
                let palettes = match loaded {
                    None => {
                        ui.weak("Loading…");
                        return None;
                    }
                    Some(Err(error)) => {
                        ui.colored_label(ui.visuals().warn_fg_color, "Colors unavailable.")
                            .on_hover_text(error);
                        return None;
                    }
                    Some(Ok(palettes)) => palettes,
                };
                let mut changed = None;
                // One palette restores with the field. Several each restore on their own.
                let own_reset = palettes.len() > 1;
                for palette in palettes.iter() {
                    let edit = edits.palette(palette.header);
                    if let Some(edit) = self.draw_palette(ui, palette, (edit, own_reset), page) {
                        let mut edited = edits.clone();
                        edited.set_palette(edit);
                        changed = Some(edited);
                    }
                }
                changed
            });
        changed.or_else(|| {
            reset.then(|| EntryEdits {
                palettes: Vec::new(),
                ..edits.clone()
            })
        })
    }

    /// One palette's swatch and controls, with a reset of its own when `own_reset`. Returns its
    /// change once one of them moves.
    fn draw_palette(
        &self,
        ui: &mut egui::Ui,
        palette: &Loaded,
        (edit, own_reset): (PaletteEdit, bool),
        page: &mut PageState,
    ) -> Option<PaletteEdit> {
        let mut next = edit;
        ui.horizontal_wrapped(|ui| {
            let texture = page.colors.swatch(ui.ctx(), palette, edit);
            let (rect, response) = ui.allocate_exact_size(SWATCH, egui::Sense::hover());
            paint_swatch(ui, rect, &texture);
            let uses = if palette.uses == 1 {
                "1 Effect".to_owned()
            } else {
                format!("{} Effects", palette.uses)
            };
            style::named_control(response, "Palette")
                .on_hover_text(format!("{uses} · 0x{:08X}", palette.header));
            next.from = self.draw_colors_from(ui, palette.header, edit.from, page);
            ui.label(quiet(ui, "Hue"));
            let hue = ui.add(
                egui::DragValue::new(&mut next.hue)
                    .range(-MOST_HUE..=MOST_HUE)
                    .suffix("°"),
            );
            style::named_control(hue, "Hue");
            ui.label(quiet(ui, "Saturation"));
            let saturation = ui.add(
                egui::DragValue::new(&mut next.saturation)
                    .range(0..=MOST_PERCENT)
                    .suffix("%"),
            );
            style::named_control(saturation, "Saturation");
            ui.label(quiet(ui, "Brightness"));
            let brightness = ui.add(
                egui::DragValue::new(&mut next.brightness)
                    .range(0..=MOST_PERCENT)
                    .suffix("%"),
            );
            style::named_control(brightness, "Brightness");
            if own_reset && !edit.is_stock() && detail::reset_icon(ui) {
                next = PaletteEdit::new(palette.header);
            }
        });
        (next != edit).then_some(next)
    }

    /// The stock palette a palette's colors come from: its own, or another ability's. Each choice
    /// shows its swatch and the abilities that draw with it.
    fn draw_colors_from(
        &self,
        ui: &mut egui::Ui,
        own: u32,
        from: Option<u32>,
        page: &mut PageState,
    ) -> Option<u32> {
        const OWN: &str = "Own Colors";
        let stock = match page.colors.stock.clone() {
            Some(Ok(stock)) => stock,
            Some(Err(error)) => {
                ui.weak(OWN).on_hover_text(error);
                return from;
            }
            None => {
                ui.weak(OWN);
                return from;
            }
        };
        let label = |header: u32| {
            stock
                .iter()
                .find(|each| each.header == header)
                .map_or_else(|| format!("0x{header:08X}"), |each| each.label.clone())
        };
        let mut chosen = from;
        let salt = ("subclass-colors-from", own);
        egui::ComboBox::from_id_salt(salt)
            .width(200.0)
            .truncate()
            .selected_text(from.map_or_else(|| OWN.to_owned(), label))
            .show_ui(ui, |ui| {
                let choices = stock.iter().filter(|each| each.header != own).count();
                let query = if crate::app::pickers::wants_filter(choices) {
                    ui.add(
                        egui::TextEdit::singleline(&mut page.colors.query)
                            .hint_text(format!(
                                "{} Filter",
                                egui_phosphor::regular::MAGNIFYING_GLASS
                            ))
                            .desired_width(f32::INFINITY),
                    );
                    page.colors.query.trim().to_lowercase()
                } else {
                    String::new()
                };
                ui.selectable_value(&mut chosen, None, OWN);
                for each in stock.iter().filter(|each| each.header != own) {
                    if !query.is_empty() && !each.label.to_lowercase().contains(&query) {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        let texture = page.colors.stock_swatch(ui.ctx(), each);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(64.0, 12.0), egui::Sense::hover());
                        paint_swatch(ui, rect, &texture);
                        ui.selectable_value(&mut chosen, Some(each.header), &each.label);
                    });
                }
            });
        crate::app::pickers::name_combo(ui, salt, "Colors From");
        chosen
    }
}
