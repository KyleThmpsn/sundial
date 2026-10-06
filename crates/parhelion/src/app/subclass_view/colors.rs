//! An ability's Effect Colors: a row that grades the final color of every particle effect, a row
//! that turns every palette and tint at once, each palette its effects draw with, as a swatch with
//! the stock palette its colors come from, and each tint, a color its materials hold, as a chip. Each has its hue, saturation and brightness. Palettes and tints load
//! on a worker, once per ability entity, from the entity and the graphs it spawns, as the build
//! finds them. Every stock ability's palettes load once on another, named by the abilities that
//! draw with them.
use super::*;
use crate::subclass::palette::{MOST_HUE, MOST_PERCENT};
use crate::subclass::{EffectGrade, PaletteEdit, TintEdit};
use std::sync::mpsc::{self, Receiver};
use sundial::package_authoring::ability_palette::{
    PALETTE_HEIGHT, PALETTE_WIDTH, ability_graphs, ability_palettes, palette_pixels, particle_sites,
};
use sundial::package_authoring::ability_tint::ability_tints;

/// A swatch's size on the page.
const SWATCH: egui::Vec2 = egui::vec2(128.0, 14.0);

/// One palette an ability draws with: its stock pixels and how many effect uses reach it.
pub(super) struct Loaded {
    header: u32,
    uses: usize,
    pixels: Vec<u8>,
}

/// One color an ability's materials hold, and how many effect uses reach it.
pub(super) struct LoadedTint {
    rgb: [f32; 3],
    uses: usize,
}

/// An ability's palettes and tints, and how many particle systems its effects draw.
pub(super) struct Found {
    palettes: Vec<Loaded>,
    tints: Vec<LoadedTint>,
    systems: usize,
}

impl Found {
    fn is_empty(&self) -> bool {
        self.palettes.is_empty() && self.tints.is_empty() && self.systems == 0
    }
}

type Load = Result<Found, String>;

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
    entities: BTreeMap<u32, Result<Arc<Found>, String>>,
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
    ) -> Option<Result<Arc<Found>, String>> {
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
    let palettes = ability_palettes(&manager, entity, tuning::SPAWN_DEPTH)?
        .into_iter()
        .map(|palette| {
            Ok(Loaded {
                header: palette.header,
                uses: palette.uses.len(),
                pixels: palette_pixels(&manager, palette.header)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let tints = ability_tints(&manager, entity, tuning::SPAWN_DEPTH)?
        .into_iter()
        .map(|tint| LoadedTint {
            rgb: tint.rgb,
            uses: tint.uses.len(),
        })
        .collect();
    let mut systems = BTreeSet::new();
    for (_, payload) in ability_graphs(&manager, entity, tuning::SPAWN_DEPTH)? {
        systems.extend(
            particle_sites(&manager, &payload)?
                .into_iter()
                .map(|site| site.system),
        );
    }
    Ok(Found {
        palettes,
        tints,
        systems: systems.len(),
    })
}

/// "1 Effect" or "N Effects".
fn effects(count: usize) -> String {
    if count == 1 {
        "1 Effect".to_owned()
    } else {
        format!("{count} Effects")
    }
}

/// The values a row's controls set: hue, saturation, brightness and whether it colorizes.
type Adjustment = (i16, u16, u16, bool);

/// Colorize, Hue, Saturation and Brightness controls over one set of values. A colorized hue is
/// the one every color takes, 0 to 359 degrees, where a turn runs both ways.
fn adjust_controls(
    ui: &mut egui::Ui,
    (hue, saturation, brightness, colorize): (&mut i16, &mut u16, &mut u16, &mut bool),
) {
    let response = ui
        .checkbox(colorize, "Colorize")
        .on_hover_text("Every color takes this hue, grays included");
    style::named_control(response, "Colorize");
    ui.label(quiet(ui, "Hue"));
    let response = if *colorize {
        let mut degrees = hue.rem_euclid(360);
        let response = ui.add(
            egui::DragValue::new(&mut degrees)
                .range(0..=359)
                .suffix("°"),
        );
        if response.changed() {
            *hue = if degrees > MOST_HUE {
                degrees - 360
            } else {
                degrees
            };
        }
        response
    } else {
        ui.add(
            egui::DragValue::new(hue)
                .range(-MOST_HUE..=MOST_HUE)
                .suffix("°"),
        )
    };
    style::named_control(response, "Hue");
    ui.label(quiet(ui, "Saturation"));
    let response = ui.add(
        egui::DragValue::new(saturation)
            .range(0..=MOST_PERCENT)
            .suffix("%"),
    );
    style::named_control(response, "Saturation");
    ui.label(quiet(ui, "Brightness"));
    let response = ui.add(
        egui::DragValue::new(brightness)
            .range(0..=MOST_PERCENT)
            .suffix("%"),
    );
    style::named_control(response, "Brightness");
}

/// A shader color as a chip shows it: scaled into range when it is brighter than 1.
fn chip_color(rgb: [f32; 3]) -> egui::Color32 {
    let max = rgb[0].max(rgb[1]).max(rgb[2]).max(1.0);
    let [r, g, b] = rgb.map(|channel| ((channel / max).clamp(0.0, 1.0) * 255.0).round() as u8);
    egui::Color32::from_rgb(r, g, b)
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
    pub(super) fn stock_abilities(&self) -> Vec<(u32, String)> {
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
        // An ability whose effects draw with no color the build can change shows no field.
        if loaded
            .as_ref()
            .is_some_and(|loaded| loaded.as_ref().is_ok_and(|found| found.is_empty()))
            && !edits.recolors()
        {
            ui.weak("No Colors");
            return None;
        }
        let (changed, reset) = detail::field(ui, "Effect Colors", edits.recolors(), |ui| {
            let found = match loaded {
                None => {
                    ui.weak("Loading…");
                    return None;
                }
                Some(Err(error)) => {
                    ui.colored_label(ui.visuals().warn_fg_color, "Colors unavailable.")
                        .on_hover_text(error);
                    return None;
                }
                Some(Ok(found)) => found,
            };
            let mut changed = None;
            let rows = found.palettes.len() + found.tints.len();
            if found.systems > 0 {
                if let Some(grade) = Self::draw_grade(ui, found.systems, edits.grade) {
                    let mut edited = edits.clone();
                    edited.set_grade(grade);
                    changed = Some(edited);
                }
                // Overall lies over everything, and the rows below set the colors under it.
                if rows > 0 {
                    ui.separator();
                }
            }
            if rows > 1
                && let Some(edited) =
                    Self::draw_all_colors(ui, &found, changed.as_ref().unwrap_or(edits))
            {
                changed = Some(edited);
            }
            // One color restores with the field. Several each restore on their own.
            let own_reset = rows > 1;
            for palette in &found.palettes {
                let edit = edits.palette(palette.header);
                if let Some(edit) = self.draw_palette(ui, palette, (edit, own_reset), page) {
                    let mut edited = changed.clone().unwrap_or_else(|| edits.clone());
                    edited.set_palette(edit);
                    changed = Some(edited);
                }
            }
            for tint in &found.tints {
                let Some(edit) = edits.tint(tint.rgb) else {
                    continue;
                };
                if let Some(edit) = Self::draw_tint(ui, tint, (edit, own_reset)) {
                    let mut edited = changed.clone().unwrap_or_else(|| edits.clone());
                    edited.set_tint(edit);
                    changed = Some(edited);
                }
            }
            changed
        });
        changed.or_else(|| {
            reset.then(|| EntryEdits {
                palettes: Vec::new(),
                tints: Vec::new(),
                grade: None,
                ..edits.clone()
            })
        })
    }

    /// Overall: the row that grades the final color of every particle effect, after any palette
    /// and tint changes. Returns the grade once one of its controls moves.
    fn draw_grade(
        ui: &mut egui::Ui,
        systems: usize,
        grade: Option<EffectGrade>,
    ) -> Option<EffectGrade> {
        let shown = grade.unwrap_or(EffectGrade::STOCK);
        let mut next = shown;
        ui.horizontal_wrapped(|ui| {
            let (_, response) = ui.allocate_exact_size(SWATCH, egui::Sense::hover());
            ui.painter().text(
                response.rect.left_center(),
                egui::Align2::LEFT_CENTER,
                "Overall",
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
            style::named_control(response, "Overall").on_hover_text(format!(
                "Recolors everything it draws, on top of the colors below\n{}",
                effects(systems)
            ));
            adjust_controls(
                ui,
                (
                    &mut next.hue,
                    &mut next.saturation,
                    &mut next.brightness,
                    &mut next.colorize,
                ),
            );
            if !shown.is_stock() && detail::reset_icon(ui) {
                next = EffectGrade::STOCK;
            }
        });
        (next != shown).then_some(next)
    }

    /// Set All: one row of controls over every palette and tint. It shows the first color's
    /// values, and a change gives every color those values, each palette keeping where its
    /// colors come from.
    fn draw_all_colors(ui: &mut egui::Ui, found: &Found, edits: &EntryEdits) -> Option<EntryEdits> {
        let palettes = found
            .palettes
            .iter()
            .map(|palette| edits.palette(palette.header))
            .collect::<Vec<_>>();
        let tints = found
            .tints
            .iter()
            .filter_map(|tint| edits.tint(tint.rgb))
            .collect::<Vec<_>>();
        let shown: Adjustment = palettes
            .first()
            .map(|edit| (edit.hue, edit.saturation, edit.brightness, edit.colorize))
            .or_else(|| {
                tints
                    .first()
                    .map(|edit| (edit.hue, edit.saturation, edit.brightness, edit.colorize))
            })?;
        let (mut hue, mut saturation, mut brightness, mut colorize) = shown;
        ui.horizontal_wrapped(|ui| {
            let (_, response) = ui.allocate_exact_size(SWATCH, egui::Sense::hover());
            ui.painter().text(
                response.rect.left_center(),
                egui::Align2::LEFT_CENTER,
                "Set All",
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
            style::named_control(response, "Set All")
                .on_hover_text("Gives every color below these values");
            adjust_controls(
                ui,
                (&mut hue, &mut saturation, &mut brightness, &mut colorize),
            );
        });
        if (hue, saturation, brightness, colorize) == shown {
            return None;
        }
        let mut edited = edits.clone();
        for edit in palettes {
            edited.set_palette(PaletteEdit {
                hue,
                saturation,
                brightness,
                colorize,
                ..edit
            });
        }
        for edit in tints {
            edited.set_tint(TintEdit {
                hue,
                saturation,
                brightness,
                colorize,
                ..edit
            });
        }
        Some(edited)
    }

    /// One tint's chip, its stock color then as changed, and its controls. Returns its change
    /// once one of them moves.
    fn draw_tint(
        ui: &mut egui::Ui,
        tint: &LoadedTint,
        (edit, own_reset): (TintEdit, bool),
    ) -> Option<TintEdit> {
        let mut next = edit;
        ui.horizontal_wrapped(|ui| {
            let (rect, response) = ui.allocate_exact_size(SWATCH, egui::Sense::hover());
            let (stock, changed) = rect.split_left_right_at_fraction(0.5);
            ui.painter().rect_filled(stock, 2.0, chip_color(tint.rgb));
            ui.painter()
                .rect_filled(changed, 2.0, chip_color(edit.apply(tint.rgb)));
            ui.painter().rect_stroke(
                rect,
                2.0,
                ui.visuals().widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let [r, g, b] = tint.rgb;
            style::named_control(response, "Tint")
                .on_hover_text(format!("{} · {r:.2}, {g:.2}, {b:.2}", effects(tint.uses)));
            adjust_controls(
                ui,
                (
                    &mut next.hue,
                    &mut next.saturation,
                    &mut next.brightness,
                    &mut next.colorize,
                ),
            );
            if own_reset && !edit.is_stock() && detail::reset_icon(ui) {
                next = TintEdit {
                    hue: 0,
                    saturation: 100,
                    brightness: 100,
                    colorize: false,
                    ..edit
                };
            }
        });
        (next != edit).then_some(next)
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
            style::named_control(response, "Palette").on_hover_text(format!(
                "{} · 0x{:08X}",
                effects(palette.uses),
                palette.header
            ));
            // The picker follows the shared controls, so they line up with the other rows'.
            adjust_controls(
                ui,
                (
                    &mut next.hue,
                    &mut next.saturation,
                    &mut next.brightness,
                    &mut next.colorize,
                ),
            );
            next.from = self.draw_colors_from(ui, palette.header, edit.from, page);
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
