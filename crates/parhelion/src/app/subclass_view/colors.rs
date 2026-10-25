//! An ability's Effect Colors: Overall, a row that grades the final color of every particle effect,
//! beside its palettes and tints as graded, then each palette its effects draw with, as a swatch
//! with the stock palette its colors come from, and each tint, a color its materials hold, as a
//! chip. Each has its hue, saturation and brightness. Overall applies on top of every row, so each
//! swatch and chip shows its color with Overall's grade, as the game draws it. With several
//! colors, each row's menu gives its values to every color once, so no second row of controls
//! stacks with Overall. Palettes and tints load on a worker, once per ability entity, from the
//! entity and the graphs it spawns, as the build finds them. Every stock ability's palettes load
//! once on another, named by the abilities that draw with them.
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
/// Colors From's width, which a tint row keeps empty so every row's menu and reset line up.
const COLORS_FROM_WIDTH: f32 = 200.0;

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

/// What the Overall preview shows: the entity, its grade, each palette's change and whether a
/// taken palette's pixels had loaded for it, and each tint's change.
type OverallKey = (
    u32,
    EffectGrade,
    Vec<(PaletteEdit, bool)>,
    Vec<Option<TintEdit>>,
);

/// One color of the Overall preview: a palette's pixels as its change colors them, or a tint's
/// color as changed.
enum Shown {
    Palette(Vec<u8>),
    Tint([f32; 3]),
}

/// Each entity's palettes once loaded, every stock palette, and the swatches shown for them.
#[derive(Default)]
pub(super) struct Colors {
    entities: BTreeMap<u32, Result<Arc<Found>, String>>,
    loading: Option<(u32, Receiver<Load>)>,
    stock: Option<Result<Arc<Vec<Stock>>, String>>,
    stock_loading: Option<Receiver<StockLoad>>,
    /// Each palette's swatch for the change and grade it shows, and whether a taken palette's
    /// pixels had loaded for it.
    swatches: BTreeMap<u32, ((PaletteEdit, EffectGrade, bool), egui::TextureHandle)>,
    stock_swatches: BTreeMap<u32, egui::TextureHandle>,
    overall: Option<(OverallKey, egui::TextureHandle)>,
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

    /// The pixels of the stock palette `edit` takes its colors from, once they have loaded.
    fn taken(&self, edit: PaletteEdit) -> Option<&[u8]> {
        edit.from.and_then(|from| self.stock_pixels(from))
    }

    /// The swatch of `palette` as `edit` colors it and `grade` grades it, made again only when
    /// either changes or a taken palette's pixels arrive.
    fn swatch(
        &mut self,
        ctx: &egui::Context,
        palette: &Loaded,
        edit: PaletteEdit,
        grade: EffectGrade,
    ) -> egui::TextureHandle {
        let taken = self.taken(edit);
        let key = (edit, grade, taken.is_some());
        if let Some((shown, texture)) = self.swatches.get(&palette.header)
            && *shown == key
        {
            return texture.clone();
        }
        let mut pixels = taken.unwrap_or(&palette.pixels).to_vec();
        edit.apply(&mut pixels);
        grade_pixels(&mut pixels, grade);
        let texture = texture(
            ctx,
            format!("ability-palette-{:08X}", palette.header),
            &pixels,
        );
        self.swatches.insert(palette.header, (key, texture.clone()));
        texture
    }

    fn stock_swatch(&mut self, ctx: &egui::Context, stock: &Stock) -> egui::TextureHandle {
        self.stock_swatches
            .entry(stock.header)
            .or_insert_with(|| {
                texture(
                    ctx,
                    format!("ability-palette-{:08X}", stock.header),
                    &stock.pixels,
                )
            })
            .clone()
    }

    /// The Overall preview of `entity`'s palettes and tints as `edits` change and grade them, made
    /// again only when they change or a taken palette's pixels arrive.
    fn overall(
        &mut self,
        ctx: &egui::Context,
        (entity, found): (u32, &Found),
        edits: &EntryEdits,
    ) -> egui::TextureHandle {
        let grade = edits.grade.unwrap_or(EffectGrade::STOCK);
        let palettes = found
            .palettes
            .iter()
            .map(|palette| {
                let edit = edits.palette(palette.header);
                (edit, self.taken(edit).is_some())
            })
            .collect();
        let tints = found
            .tints
            .iter()
            .map(|tint| edits.tint(tint.rgb))
            .collect();
        let key: OverallKey = (entity, grade, palettes, tints);
        if let Some((shown, texture)) = &self.overall
            && *shown == key
        {
            return texture.clone();
        }
        let mut sources = found
            .palettes
            .iter()
            .map(|palette| {
                let edit = edits.palette(palette.header);
                let mut pixels = self.taken(edit).unwrap_or(&palette.pixels).to_vec();
                edit.apply(&mut pixels);
                grade_pixels(&mut pixels, grade);
                (palette.uses, Shown::Palette(pixels))
            })
            .chain(found.tints.iter().map(|tint| {
                let rgb = final_tint(tint.rgb, edits.tint(tint.rgb), grade);
                (tint.uses, Shown::Tint(rgb))
            }))
            .collect::<Vec<_>>();
        // Effects whose colors no palette or tint holds show the grade over every hue.
        if sources.is_empty() {
            let mut pixels = spectrum();
            grade_pixels(&mut pixels, grade);
            sources.push((1, Shown::Palette(pixels)));
        }
        let pixels = overall_pixels(&sources);
        let texture = texture(ctx, format!("ability-colors-{entity:08X}"), &pixels);
        self.overall = Some((key, texture.clone()));
        texture
    }
}

fn texture(ctx: &egui::Context, name: String, pixels: &[u8]) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [usize::from(PALETTE_WIDTH), usize::from(PALETTE_HEIGHT)],
        pixels,
    );
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

/// The Overall preview's pixels, a palette's size: each final color across a share of the columns
/// as wide as its share of the effect uses. A tint shows as its chip does.
fn overall_pixels(shown: &[(usize, Shown)]) -> Vec<u8> {
    let (width, height) = (usize::from(PALETTE_WIDTH), usize::from(PALETTE_HEIGHT));
    let total = shown.iter().map(|(uses, _)| (*uses).max(1)).sum::<usize>();
    let mut pixels = vec![0; width * height * 4];
    let mut reached = 0;
    for (uses, color) in shown {
        let start = reached * width / total;
        reached += (*uses).max(1);
        let end = reached * width / total;
        for column in start..end {
            // The palette column this one shows, its share stretched over the whole palette.
            let source = (column - start) * width / (end - start);
            for row in 0..height {
                let pixel: [u8; 4] = match color {
                    Shown::Palette(palette) => {
                        let at = (row * width + source) * 4;
                        palette[at..at + 4].try_into().unwrap_or_default()
                    }
                    Shown::Tint(rgb) => chip_color(*rgb).to_array(),
                };
                let at = (row * width + column) * 4;
                pixels[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }
    pixels
}

/// Grades sRGB palette `pixels` as Overall grades the color every effect draws. The grade works in
/// linear light, as a pixel program writes it, so each pixel is graded linear.
fn grade_pixels(pixels: &mut [u8], grade: EffectGrade) {
    use egui::ecolor::{gamma_u8_from_linear_f32, linear_f32_from_gamma_u8};
    if grade.is_stock() {
        return;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        let linear = [0, 1, 2].map(|channel| linear_f32_from_gamma_u8(pixel[channel]));
        let graded = grade.apply(linear).map(gamma_u8_from_linear_f32);
        pixel[..3].copy_from_slice(&graded);
    }
}

/// A tint as the game draws it: its own change, then Overall's grade. A tint is linear already.
fn final_tint(rgb: [f32; 3], edit: Option<TintEdit>, grade: EffectGrade) -> [f32; 3] {
    let rgb = edit.map_or(rgb, |edit| edit.apply(rgb));
    if grade.is_stock() {
        rgb
    } else {
        grade.apply(rgb)
    }
}

/// Every hue at full saturation and brightness, as sRGB pixels of a palette's size.
fn spectrum() -> Vec<u8> {
    let width = usize::from(PALETTE_WIDTH);
    let row = (0..width)
        .flat_map(|column| {
            let hue = column as f32 / width as f32;
            let [r, g, b] = egui::ecolor::Hsva::new(hue, 1.0, 1.0, 1.0).to_srgb();
            [r, g, b, u8::MAX]
        })
        .collect::<Vec<_>>();
    row.repeat(usize::from(PALETTE_HEIGHT))
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

/// The values a color row's controls set: hue, saturation, brightness and whether it colorizes.
type Adjustment = (i16, u16, u16, bool);

/// Colors From's place while the stock palettes load or fail to: `text`, as wide as the picker
/// so the controls after it keep their column.
fn colors_from_placeholder(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let size = egui::vec2(COLORS_FROM_WIDTH, ui.spacing().interact_size.y);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_width(COLORS_FROM_WIDTH);
        ui.weak(text)
    })
    .inner
}

/// Gives every palette and tint of `found` one row's values, each palette keeping where its colors
/// come from.
fn give_every_color(
    edits: &mut EntryEdits,
    found: &Found,
    (hue, saturation, brightness, colorize): Adjustment,
) {
    for palette in &found.palettes {
        let edit = edits.palette(palette.header);
        edits.set_palette(PaletteEdit {
            hue,
            saturation,
            brightness,
            colorize,
            ..edit
        });
    }
    for tint in &found.tints {
        if let Some(edit) = edits.tint(tint.rgb) {
            edits.set_tint(TintEdit {
                hue,
                saturation,
                brightness,
                colorize,
                ..edit
            });
        }
    }
}

/// A color row's menu, named for what the row holds. Returns whether Apply to Every Color was
/// chosen.
fn row_menu(ui: &mut egui::Ui, subject: &str) -> bool {
    let mut chosen = false;
    style::more_menu(ui, subject, |ui| {
        if ui.button("Apply to Every Color").clicked() {
            chosen = true;
            ui.close();
        }
    });
    chosen
}

/// Colorize, Hue, Saturation and Brightness controls over one set of values. A colorized hue is
/// the one every color takes, 0 to 359 degrees, where a turn runs both ways. A row under
/// Overall's Colorize has its Colorize and Hue held, since Overall sets every hue (`hue_held`).
fn adjust_controls(
    ui: &mut egui::Ui,
    (hue, saturation, brightness, colorize): (&mut i16, &mut u16, &mut u16, &mut bool),
    hue_held: bool,
) {
    // Each control is held on its own, as a scope would be placed whole and stop the line wrapping.
    const HELD: &str = "Overall's Colorize sets every hue";
    let response = ui
        .add_enabled(!hue_held, egui::Checkbox::new(colorize, "Colorize"))
        .on_hover_text("Every color takes this hue, grays included")
        .on_disabled_hover_text(HELD);
    style::named_control(response, "Colorize");
    ui.add_enabled(!hue_held, egui::Label::new(quiet(ui, "Hue")));
    let response = if *colorize {
        let mut degrees = hue.rem_euclid(360);
        let response = ui.add_enabled(
            !hue_held,
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
        ui.add_enabled(
            !hue_held,
            egui::DragValue::new(hue)
                .range(-MOST_HUE..=MOST_HUE)
                .suffix("°"),
        )
    };
    style::named_control(response, "Hue").on_disabled_hover_text(HELD);
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

/// A shader color as a chip shows it: scaled into range when it is brighter than 1, and encoded
/// for the screen from the linear light the shader writes.
fn chip_color(rgb: [f32; 3]) -> egui::Color32 {
    let max = rgb[0].max(rgb[1]).max(rgb[2]).max(1.0);
    let [r, g, b] = rgb.map(|channel| egui::ecolor::gamma_u8_from_linear_f32(channel / max));
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
                if let Some(grade) = Self::draw_grade(ui, (entity, &found), edits, &mut page.colors)
                {
                    let mut edited = edits.clone();
                    edited.set_grade(grade);
                    changed = Some(edited);
                }
                // Overall lies over everything, and the rows below set the colors under it.
                if rows > 0 {
                    ui.separator();
                }
            }
            if let Some(edited) =
                self.draw_color_rows(ui, &found, changed.as_ref().unwrap_or(edits), page)
            {
                changed = Some(edited);
            }
            // A change to a color these effects don't draw, kept from another source or an older
            // version, stops the build. It shows here so it can go.
            let drawn_palette = |edit: &PaletteEdit| {
                found
                    .palettes
                    .iter()
                    .any(|each| each.header == edit.palette)
            };
            let drawn_tint =
                |edit: &TintEdit| found.tints.iter().any(|each| edit.starts_from(each.rgb));
            let gone = edits
                .palettes
                .iter()
                .filter(|edit| !drawn_palette(edit))
                .map(|edit| format!("Palette 0x{:08X}", edit.palette))
                .chain(
                    edits
                        .tints
                        .iter()
                        .filter(|edit| !drawn_tint(edit))
                        .map(|_| "Color constant".to_owned()),
                )
                .collect::<Vec<_>>();
            if !gone.is_empty() {
                let label = if gone.len() == 1 {
                    "1 color change its effects don't draw".to_owned()
                } else {
                    format!("{} color changes its effects don't draw", gone.len())
                };
                if style::missing(ui, &label, &gone.join("\n")) {
                    let mut edited = changed.clone().unwrap_or_else(|| edits.clone());
                    edited.palettes.retain(drawn_palette);
                    edited.tints.retain(drawn_tint);
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
    /// and tint changes, beside the palettes and tints as graded. Returns the grade once one of
    /// its controls moves.
    fn draw_grade(
        ui: &mut egui::Ui,
        (entity, found): (u32, &Found),
        edits: &EntryEdits,
        colors: &mut Colors,
    ) -> Option<EffectGrade> {
        let shown = edits.grade.unwrap_or(EffectGrade::STOCK);
        let mut next = shown;
        ui.horizontal_wrapped(|ui| {
            let (rect, response) = ui.allocate_exact_size(SWATCH, egui::Sense::hover());
            let name = ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                "Overall",
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
            let preview = rect.with_min_x(name.right() + ui.spacing().item_spacing.x);
            paint_swatch(
                ui,
                preview,
                &colors.overall(ui.ctx(), (entity, found), edits),
            );
            style::named_control(response, "Overall").on_hover_text(format!(
                "Recolors everything it draws, on top of every color below\n{}",
                effects(found.systems)
            ));
            adjust_controls(
                ui,
                (
                    &mut next.hue,
                    &mut next.saturation,
                    &mut next.brightness,
                    &mut next.colorize,
                ),
                false,
            );
            if !shown.is_stock() && detail::reset_icon(ui) {
                next = EffectGrade::STOCK;
            }
        });
        (next != shown).then_some(next)
    }

    /// A row for each palette and tint, its color as the game draws it, with the grade of `edits`
    /// on top. Returns the entry's edits once a row changes, or once one gives every color its
    /// values.
    fn draw_color_rows(
        &self,
        ui: &mut egui::Ui,
        found: &Found,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let grade = edits.grade.unwrap_or(EffectGrade::STOCK);
        // One color restores with the field. Several each restore on their own, and each can give
        // its values to every color.
        let several = found.palettes.len() + found.tints.len() > 1;
        let mut changed: Option<EntryEdits> = None;
        let mut every = None;
        for palette in &found.palettes {
            let edit = edits.palette(palette.header);
            let (next, apply) = self.draw_palette(ui, palette, (edit, several), grade, page);
            if let Some(next) = next {
                changed
                    .get_or_insert_with(|| edits.clone())
                    .set_palette(next);
            }
            if apply {
                every = Some((edit.hue, edit.saturation, edit.brightness, edit.colorize));
            }
        }
        for tint in &found.tints {
            let Some(edit) = edits.tint(tint.rgb) else {
                continue;
            };
            let column = !found.palettes.is_empty();
            let (next, apply) = Self::draw_tint(ui, tint, (edit, several, column), grade);
            if let Some(next) = next {
                changed.get_or_insert_with(|| edits.clone()).set_tint(next);
            }
            if apply {
                every = Some((edit.hue, edit.saturation, edit.brightness, edit.colorize));
            }
        }
        if let Some(values) = every {
            give_every_color(changed.get_or_insert_with(|| edits.clone()), found, values);
        }
        changed
    }

    /// One tint's chip, its stock color then as the game draws it, with its own change and
    /// `grade`, and its controls. With `several` colors it has a reset of its own and a menu
    /// holding Apply to Every Color, after the palettes' Colors From `column` when there is one.
    /// Returns its change once one of them moves, and whether Apply to Every Color was chosen.
    fn draw_tint(
        ui: &mut egui::Ui,
        tint: &LoadedTint,
        (edit, several, column): (TintEdit, bool, bool),
        grade: EffectGrade,
    ) -> (Option<TintEdit>, bool) {
        let mut next = edit;
        let mut every = false;
        ui.horizontal_wrapped(|ui| {
            let (rect, response) = ui.allocate_exact_size(SWATCH, egui::Sense::hover());
            let (stock, changed) = rect.split_left_right_at_fraction(0.5);
            ui.painter().rect_filled(stock, 2.0, chip_color(tint.rgb));
            ui.painter().rect_filled(
                changed,
                2.0,
                chip_color(final_tint(tint.rgb, Some(edit), grade)),
            );
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
                grade.colorize,
            );
            if several {
                if column {
                    ui.add_space(COLORS_FROM_WIDTH + ui.spacing().item_spacing.x);
                }
                every = row_menu(ui, "Tint");
                if !edit.is_stock() && detail::reset_icon(ui) {
                    next = TintEdit {
                        hue: 0,
                        saturation: 100,
                        brightness: 100,
                        colorize: false,
                        ..edit
                    };
                }
            }
        });
        ((next != edit).then_some(next), every)
    }

    /// One palette's swatch, as the game draws it with its own change and `grade`, and its
    /// controls. With `several` colors it has a reset of its own and a menu holding Apply to
    /// Every Color. Returns its change once one of them moves, and whether Apply to Every Color
    /// was chosen.
    fn draw_palette(
        &self,
        ui: &mut egui::Ui,
        palette: &Loaded,
        (edit, several): (PaletteEdit, bool),
        grade: EffectGrade,
        page: &mut PageState,
    ) -> (Option<PaletteEdit>, bool) {
        let mut next = edit;
        let mut every = false;
        ui.horizontal_wrapped(|ui| {
            let texture = page.colors.swatch(ui.ctx(), palette, edit, grade);
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
                grade.colorize,
            );
            next.from = self.draw_colors_from(ui, palette.header, edit.from, page);
            if several {
                every = row_menu(ui, "Palette");
                if !edit.is_stock() && detail::reset_icon(ui) {
                    next = PaletteEdit::new(palette.header);
                }
            }
        });
        ((next != edit).then_some(next), every)
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
                colors_from_placeholder(ui, OWN).on_hover_text(error);
                return from;
            }
            None => {
                colors_from_placeholder(ui, OWN);
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
            .width(COLORS_FROM_WIDTH)
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
