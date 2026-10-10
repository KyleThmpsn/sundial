//! The main page for shaders. A shader carries one dye per channel (armor, cloth and suit) for
//! each gear type, and each dye paints a primary and a secondary surface and binds the detail
//! textures both share. This page keeps its base shader's dyes, takes a channel from another stock
//! shader, and gives any surface any material value of its own and any dye other textures, which
//! the build writes into custom dyes. All Gear edits every gear type at once, and choosing one
//! gear type shows and edits its own dyes.
use super::*;
use crate::app::style;
use crate::dye::{
    DyeChannel, DyeEdit, DyeSurface, DyeTextureEdit, DyeValue, GearType, NO_IRIDESCENCE,
    linear_to_srgb, surface_edit, texture_edit,
};
use gear_view::draw_gear_rarity;
use materials::{TextureValues, Values};
use sundial::package_authoring::{IridescenceRow, load_iridescence_rows};
use sundial::ui::model_preview::{DyeTextureOverride, SurfaceOverride, still};

mod icon;
mod materials;
pub(super) use materials::DyeMaterials;

/// The gear types a shader paints besides weapons, whose items the preview can show it on.
const PREVIEW_KINDS: [(ItemKind, GearType); 4] = [
    (ItemKind::Armor, GearType::Armor),
    (ItemKind::Ship, GearType::Ship),
    (ItemKind::Sparrow, GearType::Sparrow),
    (ItemKind::GhostShell, GearType::GhostShell),
];

/// The six surfaces, in the order the page lays them out.
const SLOTS: [(DyeChannel, DyeSurface); 6] = [
    (DyeChannel::Armor, DyeSurface::Primary),
    (DyeChannel::Armor, DyeSurface::Secondary),
    (DyeChannel::Cloth, DyeSurface::Primary),
    (DyeChannel::Cloth, DyeSurface::Secondary),
    (DyeChannel::Suit, DyeSurface::Primary),
    (DyeChannel::Suit, DyeSurface::Secondary),
];

/// The game's iridescence lookup, read once in the background.
#[derive(Default)]
pub(super) struct Iridescence {
    rows: Option<Result<Vec<IridescenceRow>, String>>,
    job: Option<thread::JoinHandle<Result<Vec<IridescenceRow>, String>>>,
}

impl Drop for Iridescence {
    fn drop(&mut self) {
        // No package handles may outlive the catalog, as with the dye colors.
        if let Some(job) = self.job.take() {
            let _ = job.join();
        }
    }
}

impl Iridescence {
    fn update(&mut self, ctx: &egui::Context, packages: &Path) {
        if let Some(job) = self.job.take_if(|job| job.is_finished()) {
            self.rows = Some(
                job.join()
                    .unwrap_or_else(|_| Err("Iridescence loading stopped".into())),
            );
        }
        if self.rows.is_none() && self.job.is_none() && !packages.as_os_str().is_empty() {
            let packages = packages.to_owned();
            let ctx = ctx.clone();
            self.job = Some(thread::spawn(move || {
                let rows = load_iridescence_rows(&packages);
                ctx.request_repaint();
                rows
            }));
        }
    }

    fn row(&self, id: i16) -> Option<&IridescenceRow> {
        match &self.rows {
            Some(Ok(rows)) => rows.iter().find(|row| row.id == id),
            _ => None,
        }
    }

    fn rows(&self) -> &[IridescenceRow] {
        match &self.rows {
            Some(Ok(rows)) => rows,
            _ => &[],
        }
    }
}

/// What a tile shows while its dye is read, or when it cannot be.
const LOADING: &str = "Loading…";
const UNAVAILABLE: &str = "Unavailable";

/// How large a surface's ball shows, and how narrow its tile gets before each channel takes a line
/// of its own.
const SWATCH_SIZE: f32 = 44.0;
const SURFACE_TILE_MIN_WIDTH: f32 = 150.0;
/// How large a dye's textures show in their tile, and in the menu that changes them.
const THUMBNAIL_SIZE: f32 = 40.0;
const BROWSER_THUMBNAIL_SIZE: f32 = 72.0;

/// One value the inspector changes.
#[derive(Clone, Copy)]
enum Change {
    Color([u8; 3]),
    Iridescence(i16),
    Metalness(f32),
    Smoothness([f32; 2]),
    Detail(f32),
    Bumps(f32),
    DetailSmoothness(f32),
    Glow([u8; 3]),
    WornColor([u8; 3]),
    WornMetalness(f32),
    WornSmoothness([f32; 2]),
    Wear([f32; 2]),
}

impl Change {
    /// Every value of a surface, as copying the surface sets them.
    const fn all(values: Values) -> [Self; 12] {
        [
            Self::Color(values.color),
            Self::Iridescence(values.iridescence),
            Self::Metalness(values.metalness),
            Self::Smoothness(values.smoothness),
            Self::Detail(values.detail),
            Self::Bumps(values.bumps),
            Self::DetailSmoothness(values.detail_smoothness),
            Self::Glow(values.glow),
            Self::WornColor(values.worn_color),
            Self::WornMetalness(values.worn_metalness),
            Self::WornSmoothness(values.worn_smoothness),
            Self::Wear(values.wear),
        ]
    }

    /// The value's name within its group, so Worn and Glow do not repeat themselves.
    const fn label(self) -> &'static str {
        match self {
            Self::Color(_) | Self::WornColor(_) | Self::Glow(_) => "Color",
            Self::Iridescence(_) => "Iridescence",
            Self::Metalness(_) | Self::WornMetalness(_) => "Metalness",
            Self::Smoothness(_) | Self::WornSmoothness(_) => "Smoothness",
            Self::Detail(_) => "Detail Strength",
            Self::Bumps(_) => "Bump Strength",
            Self::DetailSmoothness(_) => "Detail Smoothness",
            Self::Wear(_) => "Wear",
        }
    }

    /// Clears the value from an edit.
    fn clear(self, edit: &mut DyeEdit) {
        match self {
            Self::Color(_) => edit.color = None,
            Self::Iridescence(_) => edit.iridescence = None,
            Self::Metalness(_) => edit.metalness = None,
            Self::Smoothness(_) => edit.smoothness = None,
            Self::Detail(_) => edit.detail = None,
            Self::Bumps(_) => edit.bumps = None,
            Self::DetailSmoothness(_) => edit.detail_smoothness = None,
            Self::Glow(_) => edit.glow = None,
            Self::WornColor(_) => edit.worn_color = None,
            Self::WornMetalness(_) => edit.worn_metalness = None,
            Self::WornSmoothness(_) => edit.worn_smoothness = None,
            Self::Wear(_) => edit.wear = None,
        }
    }

    /// Whether an edit sets the value, which clearing it would change.
    fn is_set(self, edit: &DyeEdit) -> bool {
        let mut cleared = *edit;
        self.clear(&mut cleared);
        cleared != *edit
    }

    /// Sets the value in an edit, kept inside what a recipe accepts.
    fn apply(self, edit: &mut DyeEdit) {
        let amount = |value: f32, most: f32| DyeValue::new(value.clamp(0.0, most));
        let range = |[least, most]: [f32; 2]| Some([amount(least, 1.0)?, amount(most, 1.0)?]);
        match self {
            Self::Color(color) => edit.color = Some(color),
            Self::Iridescence(id) => edit.iridescence = Some(id),
            Self::Metalness(value) => edit.metalness = amount(value, 1.0),
            Self::Smoothness(bounds) => edit.smoothness = range(bounds),
            Self::Detail(value) => edit.detail = amount(value, 1.0),
            Self::Bumps(value) => edit.bumps = amount(value, 4.0),
            Self::DetailSmoothness(value) => edit.detail_smoothness = amount(value, 1.0),
            Self::Glow(color) => edit.glow = Some(color),
            Self::WornColor(color) => edit.worn_color = Some(color),
            Self::WornMetalness(value) => edit.worn_metalness = amount(value, 1.0),
            Self::WornSmoothness(bounds) => edit.worn_smoothness = range(bounds),
            Self::Wear([offset, scale]) => {
                edit.wear = DyeValue::new(offset)
                    .zip(DyeValue::new(scale))
                    .map(|(offset, scale)| [offset, scale]);
            }
        }
    }
}

/// A dye's detail or normal texture tiling.
#[derive(Clone, Copy)]
enum Tiling {
    Detail,
    Normal,
}

impl Tiling {
    const fn label(self) -> &'static str {
        match self {
            Self::Detail => "Detail Tiling",
            Self::Normal => "Normal Tiling",
        }
    }

    fn set(self, edit: &mut DyeTextureEdit, tiling: Option<[DyeValue; 4]>) {
        match self {
            Self::Detail => edit.detail_tiling = tiling,
            Self::Normal => edit.normal_tiling = tiling,
        }
    }
}

/// What the inspector asks the page to change.
enum SlotAction {
    Set(DyeChannel, DyeSurface, Change),
    /// Clears one value, back to the dye's own or the one for every gear type.
    Clear(DyeChannel, DyeSurface, Change),
    Reset(DyeChannel, DyeSurface),
    /// Copy every value of one surface from another shader.
    Copy(DyeChannel, DyeSurface, u32),
    /// Take a channel's whole dye from another shader.
    UseDye(DyeChannel, u32),
    /// Take a dye's textures and tiling from another shader.
    UseTextures(DyeChannel, u32),
    Tiling(DyeChannel, Tiling, [f32; 4]),
    ClearTiling(DyeChannel, Tiling),
    ClearTextures(DyeChannel),
}

/// A copy waiting for its source shader's dye to load: the gear type it edits (or every gear
/// type), the channel, and the source shader.
#[derive(Clone, Copy)]
pub(super) enum DyeCopy {
    /// Every value of one surface.
    Surface(Option<GearType>, DyeChannel, DyeSurface, u32),
    /// A dye's textures and tiling.
    Textures(Option<GearType>, DyeChannel, u32),
}

impl DyeCopy {
    const fn source(self) -> (Option<GearType>, DyeChannel, u32) {
        match self {
            Self::Surface(gear, channel, _, shader) | Self::Textures(gear, channel, shader) => {
                (gear, channel, shader)
            }
        }
    }
}

/// A dye's tiling as a recipe keeps it.
fn tiling(values: [f32; 4]) -> Option<[DyeValue; 4]> {
    let [scale_x, scale_y, offset_x, offset_y] = values.map(DyeValue::new);
    Some([scale_x?, scale_y?, offset_x?, offset_y?])
}

/// Sets part of one surface's edit for one gear type or every gear type, adding the edit or
/// dropping it once it sets nothing.
pub(super) fn set_dye_edit(
    edits: &mut Vec<DyeEdit>,
    gear: Option<GearType>,
    channel: DyeChannel,
    surface: DyeSurface,
    change: impl FnOnce(&mut DyeEdit),
) {
    let position = edits
        .iter()
        .position(|edit| (edit.gear, edit.channel, edit.surface) == (gear, channel, surface));
    let mut edit = position.map_or(DyeEdit::new(gear, channel, surface), |position| {
        edits[position]
    });
    change(&mut edit);
    if let Some(position) = position {
        edits.remove(position);
    }
    if !edit.is_empty() {
        edits.push(edit);
        edits.sort_by_key(|edit| (edit.gear, edit.channel, edit.surface));
    }
}

/// Sets part of one dye's texture edit for one gear type or every gear type, adding the edit or
/// dropping it once it sets nothing.
pub(super) fn set_texture_edit(
    edits: &mut Vec<DyeTextureEdit>,
    gear: Option<GearType>,
    channel: DyeChannel,
    change: impl FnOnce(&mut DyeTextureEdit),
) {
    let position = edits
        .iter()
        .position(|edit| (edit.gear, edit.channel) == (gear, channel));
    let mut edit = position.map_or(DyeTextureEdit::new(gear, channel), |position| {
        edits[position]
    });
    change(&mut edit);
    if let Some(position) = position {
        edits.remove(position);
    }
    if !edit.is_empty() {
        edits.push(edit);
        edits.sort_by_key(|edit| (edit.gear, edit.channel));
    }
}

/// The dye row of one channel on one gear type.
fn gear_row(
    rows: &DyeRows,
    gear: GearType,
    channel: DyeChannel,
) -> Option<WeaponDyeReferenceRecipe> {
    rows.iter()
        .flatten()
        .find(|row| row.channel_index == gear.key(channel))
        .copied()
}

/// The page's unbuilt values for one gear type, as the preview draws them on an item of that type:
/// what a build writes into each dye's vectors. A dye's tiling rides with its primary surface.
fn surface_overrides(
    edits: &[DyeEdit],
    textures: &[DyeTextureEdit],
    gear: GearType,
) -> Vec<SurfaceOverride> {
    SLOTS
        .into_iter()
        .filter_map(|(channel, surface)| {
            let mut writes = surface_edit(edits, gear, channel, surface)
                .map(|edit| edit.writes())
                .unwrap_or_default();
            if surface == DyeSurface::Primary
                && let Some(textures) = texture_edit(textures, gear, channel)
            {
                writes.extend(textures.writes());
            }
            (!writes.is_empty()).then(|| SurfaceOverride {
                slot: channel.index() * 2 + surface.index(),
                writes,
            })
        })
        .collect()
}

/// The page's unbuilt textures for one gear type, which the preview loads for its item.
fn texture_overrides(textures: &[DyeTextureEdit], gear: GearType) -> Vec<DyeTextureOverride> {
    DyeChannel::ALL
        .into_iter()
        .filter_map(|channel| {
            let edit = texture_edit(textures, gear, channel)?;
            (edit.detail.is_some() || edit.normal.is_some()).then_some(DyeTextureOverride {
                channel: channel.index(),
                detail: edit.detail,
                normal: edit.normal,
            })
        })
        .collect()
}

/// A color swatch that opens a picker, with its hex. Returns the new color when one is picked.
fn color_field(ui: &mut egui::Ui, color: [u8; 3]) -> Option<[u8; 3]> {
    let mut edited = color;
    let changed = ui
        .horizontal(|ui| {
            let changed = egui::color_picker::color_edit_button_srgb(ui, &mut edited).changed();
            ui.monospace(hex(edited));
            changed
        })
        .inner;
    changed.then_some(edited)
}

/// A color as `#RRGGBB`.
fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

/// A slider from 0 to `most` as wide as its tile, that never rewrites a stock value by drawing it.
fn amount_field(ui: &mut egui::Ui, width: f32, value: f32, most: f32) -> Option<f32> {
    let mut edited = value;
    ui.spacing_mut().slider_width = (width - 56.0).max(60.0);
    ui.add(
        egui::Slider::new(&mut edited, 0.0..=most)
            .clamping(egui::SliderClamping::Edits)
            .max_decimals(2),
    )
    .changed()
    .then_some(edited)
}

/// Named numbers side by side, each held to `range` when edited.
fn named_numbers<const N: usize>(
    ui: &mut egui::Ui,
    values: [f32; N],
    names: [&str; N],
    (range, speed): (std::ops::RangeInclusive<f32>, f32),
) -> Option<[f32; N]> {
    let mut edited = values;
    let changed = ui
        .horizontal(|ui| {
            let mut changed = false;
            for (value, name) in edited.iter_mut().zip(names) {
                ui.weak(name);
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .range(range.clone())
                            .clamp_existing_to_range(false)
                            .speed(speed)
                            .max_decimals(2),
                    )
                    .changed();
            }
            changed
        })
        .inner;
    changed.then_some(edited)
}

/// A smoothness range, its least and most from 0 to 1.
fn range_field(ui: &mut egui::Ui, values: [f32; 2]) -> Option<[f32; 2]> {
    named_numbers(ui, values, ["Min", "Max"], (0.0..=1.0, 0.01))
}

/// How often a texture repeats across the gear, keeping its offset.
fn tiling_field(ui: &mut egui::Ui, values: [f32; 4]) -> Option<[f32; 4]> {
    let [x, y, offset_x, offset_y] = values;
    named_numbers(ui, [x, y], ["X", "Y"], (f32::MIN..=f32::MAX, 0.05))
        .map(|[x, y]| [x, y, offset_x, offset_y])
}

/// Wear's offset and scale.
fn wear_field(ui: &mut egui::Ui, values: [f32; 2]) -> Option<[f32; 2]> {
    named_numbers(ui, values, ["Offset", "Scale"], (f32::MIN..=f32::MAX, 0.05))
}

/// A strip of an iridescence row's colors. The lookup holds linear values, shown here in sRGB.
fn draw_gradient(ui: &mut egui::Ui, colors: &[[u8; 3]], size: egui::Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let step = rect.width() / colors.len().max(1) as f32;
    for (index, color) in colors.iter().enumerate() {
        let left = rect.left() + step * index as f32;
        let [red, green, blue] = color.map(|channel| linear_to_srgb(f32::from(channel) / 255.0));
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2((left + step + 0.5).min(rect.right()), rect.bottom()),
            ),
            0.0,
            egui::Color32::from_rgb(red, green, blue),
        );
    }
    ui.painter().rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    response
}

/// Channel names and their dye-row key within each gear type.
const CHANNELS: [(&str, i8); 3] = [("Armor", 0), ("Cloth", 1), ("Suit", 2)];

pub(super) type DyeRows = [Vec<WeaponDyeReferenceRecipe>; 3];

/// A stock shader's dye rows, as a recipe carries them.
pub(super) fn stock_rows(catalog: &InvestmentCatalog, shader: u32) -> DyeRows {
    catalog.item_render_dye_rows(shader).map(|rows| {
        rows.into_iter()
            .map(|row| WeaponDyeReferenceRecipe {
                channel_index: row.channel_index,
                dye_reference_index: row.dye_reference_index,
            })
            .collect()
    })
}

/// The dye a set of rows gives one key.
fn dye_for(rows: &DyeRows, key: i8) -> Option<u16> {
    rows.iter()
        .flatten()
        .find(|row| row.channel_index == key)
        .map(|row| row.dye_reference_index)
}

/// Takes one channel's dyes from `source` on one gear type or every gear type. A recipe whose dyes
/// match its base carries no rows of its own.
pub(super) fn set_shader_channel(
    recipe: &mut WeaponRecipe,
    base: &DyeRows,
    source: &DyeRows,
    channel: i8,
    gear: Option<GearType>,
) {
    let mut rows = recipe
        .overrides
        .render_dye_rows
        .clone()
        .unwrap_or_else(|| base.clone());
    for first in GearType::ALL
        .into_iter()
        .filter(|each| gear.is_none_or(|only| only == *each))
        .map(GearType::first_key)
    {
        let key = first + channel;
        let Some(dye) = dye_for(source, key) else {
            continue;
        };
        for row in rows
            .iter_mut()
            .flatten()
            .filter(|row| row.channel_index == key)
        {
            row.dye_reference_index = dye;
        }
    }
    recipe.overrides.render_dye_rows = (rows != *base).then_some(rows);
}

/// The items the preview can show the shader on, each with its gear type. Weapons come first.
fn preview_items<'a>(
    weapons: &'a [WeaponDonorSummary],
    gear: &'a BTreeMap<ItemKind, Vec<WeaponDonorSummary>>,
) -> Vec<(&'a WeaponDonorSummary, GearType)> {
    weapons
        .iter()
        .map(|item| (item, GearType::Weapon))
        .chain(PREVIEW_KINDS.iter().flat_map(|&(kind, each)| {
            gear.get(&kind)
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(move |item| (item, each))
        }))
        .collect()
}

/// The item the preview shows once a gear type is chosen: the one it shows when that is of the
/// gear type, otherwise the gear type's first.
fn preview_item_for(
    items: &[(&WeaponDonorSummary, GearType)],
    shown: Option<u32>,
    gear: GearType,
) -> Option<u32> {
    let current = shown
        .and_then(|hash| items.iter().find(|(item, _)| item.hash == hash))
        .or(items.first());
    if current.is_some_and(|(_, current)| *current == gear) {
        return shown;
    }
    items
        .iter()
        .find(|(_, each)| *each == gear)
        .map(|(item, _)| item.hash)
        .or(shown)
}

/// A searchable list of stock shaders. Returns the one clicked.
fn shader_list(
    ui: &mut egui::Ui,
    shaders: &[WeaponDonorSummary],
    query: &mut String,
    selected: Option<u32>,
) -> Option<u32> {
    browse_shaders(ui, shaders, query, selected).0
}

/// A searchable list of stock shaders. Returns the one clicked and the one under the pointer.
fn browse_shaders(
    ui: &mut egui::Ui,
    shaders: &[WeaponDonorSummary],
    query: &mut String,
    selected: Option<u32>,
) -> (Option<u32>, Option<u32>) {
    workbench_style(ui);
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text("Search Shaders…")
            .desired_width(f32::INFINITY),
    );
    let needle = query.trim().to_lowercase();
    let (mut clicked, mut hovered) = (None, None);
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            for shader in shaders
                .iter()
                .filter(|shader| needle.is_empty() || shader.name.to_lowercase().contains(&needle))
            {
                let response = ui.selectable_label(selected == Some(shader.hash), &shader.name);
                if response.hovered() {
                    hovered = Some(shader.hash);
                }
                if response.clicked() {
                    clicked = Some(shader.hash);
                }
            }
        });
    (clicked, hovered)
}

impl PackageAuthoringApp {
    pub(super) fn draw_shader_editor(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        #[cfg(feature = "d2-model-importer")]
        self.dye_materials.update_source(ui.ctx(), &self.recipe);
        let inherited = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .and_then(|hash| {
                self.gear_donors_for(ItemKind::Shader)
                    .iter()
                    .find(|donor| donor.hash == hash)
            })
            .map_or(WeaponRarity::Unknown, |donor| donor.rarity);
        if let Some(base_width) = workbench_left_column_width(ui.available_width()) {
            let definition_width = ui.available_width() - base_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(base_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(base_width);
                        self.draw_shader_source(ui);
                        ui.add_space(8.0);
                        self.draw_shader_icon(ui);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(definition_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(definition_width);
                        self.draw_shader_definition(ui, inherited);
                    },
                );
            });
        } else {
            self.draw_shader_source(ui);
            ui.add_space(8.0);
            self.draw_shader_icon(ui);
            ui.separator();
            self.draw_shader_definition(ui, inherited);
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        if let Some(preview_width) = workbench_left_column_width(ui.available_width()) {
            let dyes_width = ui.available_width() - preview_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(preview_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(preview_width);
                        self.draw_shader_preview(ui);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(dyes_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(dyes_width);
                        self.draw_shader_dyes(ui);
                    },
                );
            });
        } else {
            self.draw_shader_preview(ui);
            ui.separator();
            self.draw_shader_dyes(ui);
        }
    }

    fn source_shader(&self) -> bool {
        #[cfg(feature = "d2-model-importer")]
        {
            self.recipe.overrides.imported_graph.is_some()
        }
        #[cfg(not(feature = "d2-model-importer"))]
        {
            false
        }
    }

    fn draw_shader_source(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "d2-model-importer")]
        if let Some(graph) = &self.recipe.overrides.imported_graph {
            draw_donor_section_label(
                ui,
                "Source Shader",
                Some("Materials, textures and animation come from the imported source."),
            );
            let icon = self.dye_materials.source_icon(ui.ctx());
            let source = self.dye_materials.source.as_ref();
            let action = sundial::investment::draw_authoring_item_header(
                ui,
                sundial::investment::AuthoringItemHeader {
                    name: source
                        .and_then(|s| s.name.as_deref())
                        .unwrap_or(&self.recipe.name),
                    type_name: "Shader",
                    hash: source.and_then(|s| s.hash),
                    icon: icon.as_ref(),
                },
                "Open Imported Assets",
            );
            if action.clicked()
                && let Err(error) = sundial::package_authoring::open_directory(&graph.directory)
            {
                self.log.push(LogEntry::error(error));
            }
            return;
        }
        self.draw_gear_base(ui);
    }

    fn shader_base_rows(&self) -> DyeRows {
        #[cfg(feature = "d2-model-importer")]
        if self.source_shader() {
            return crate::shader::rows(self.dye_materials.sources.keys().copied());
        }
        self.catalog
            .as_ref()
            .and_then(|catalog| {
                self.recipe
                    .donor
                    .item_hash
                    .parse_u32()
                    .ok()
                    .map(|base| stock_rows(catalog, base))
            })
            .unwrap_or_default()
    }

    /// The shader on an item of any gear type, with the page's unbuilt colors for that type.
    fn draw_shader_preview(&mut self, ui: &mut egui::Ui) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        ui.heading("Preview");
        let items = preview_items(&self.donor_summaries, &self.gear_donors);
        let item = self
            .shader_preview_item
            .and_then(|hash| items.iter().find(|(item, _)| item.hash == hash))
            .or(items.first())
            .copied();
        let label = item.map_or("Choose Item", |(item, _)| item.name.as_str());
        let picked = catalog.draw_weapon_donor_dropdown_picker(
            ui,
            "shader-preview-item",
            &mut self.shader_preview_query,
            items.iter().map(|(item, _)| *item),
            WeaponDonorPickerOptions {
                selected_hash: item.map(|(item, _)| item.hash),
                selected_label: label,
                header_label: None,
                action_label: "Preview on Item",
                selected_icon_override: None,
                secondary_action_label: None,
                clear: None,
                row_detail: None,
                selected_detail: None,
            },
        );
        let shown = match picked {
            Some(WeaponDonorPickerAction::Select(hash)) => {
                self.shader_preview_item = Some(hash);
                items.iter().find(|(item, _)| item.hash == hash).copied()
            }
            _ => item,
        };
        let gear = shown.map_or(GearType::Weapon, |(_, gear)| gear);
        let shown = shown.map(|(item, _)| item.hash);
        let rows = self
            .recipe
            .overrides
            .render_dye_rows
            .clone()
            .unwrap_or_else(|| self.shader_base_rows())
            .map(|rows| {
                rows.iter()
                    .map(|row| (row.channel_index, row.dye_reference_index))
                    .collect::<Vec<_>>()
            });
        #[cfg(feature = "d2-model-importer")]
        let sources = if self.source_shader() {
            let mut sources = BTreeMap::new();
            for channel in DyeChannel::ALL {
                if let Some(index) = rows
                    .iter()
                    .flatten()
                    .find(|(key, _)| *key == gear.key(channel))
                    .map(|(_, index)| *index)
                    && let Some(source_channel) = crate::shader::source_channel(index)
                    && let Some(source) = self.dye_materials.sources.get(&source_channel)
                {
                    let mut source = source.clone();
                    if let Some(edit) =
                        texture_edit(&self.recipe.overrides.dye_texture_edits, gear, channel)
                    {
                        for (own, tag) in [
                            (&mut source.detail, edit.detail),
                            (&mut source.normal, edit.normal),
                        ] {
                            if let Some(tag) = tag {
                                *own = self
                                    .dye_materials
                                    .sources
                                    .values()
                                    .flat_map(|s| [&s.detail, &s.normal])
                                    .flatten()
                                    .find(|t| t.tag() == tag)
                                    .cloned()
                                    .or(Some(crate::shader::DyeTextureSource::Native(tag)));
                            }
                        }
                    }
                    sources.insert(channel.index(), source);
                }
            }
            Some(std::sync::Arc::new(sources))
        } else {
            None
        };
        let textures = &self.recipe.overrides.dye_texture_edits;
        // Local source rows never go through the installed dye-table lookup.
        let native_rows = rows.map(|rows| {
            rows.into_iter()
                .filter(|(_, _index)| {
                    #[cfg(feature = "d2-model-importer")]
                    if self.source_shader() && crate::shader::source_channel(*_index).is_some() {
                        return false;
                    }
                    true
                })
                .collect::<Vec<_>>()
        });
        let appearance = shown
            .and_then(|hash| catalog.shader_preview_appearance(hash, &native_rows))
            .map(|mut appearance| {
                #[cfg(feature = "d2-model-importer")]
                if let Some(sources) = &sources {
                    appearance.dyes.retain(|(key, _)| {
                        crate::dye::slot_of_key(*key)
                            .is_none_or(|(_, channel)| !sources.contains_key(&channel.index()))
                    });
                }
                appearance.dye_textures = texture_overrides(textures, gear)
                    .into_iter()
                    .filter(|_edit| {
                        #[cfg(feature = "d2-model-importer")]
                        if sources
                            .as_ref()
                            .is_some_and(|s| s.contains_key(&_edit.channel))
                        {
                            return false;
                        }
                        true
                    })
                    .collect();
                appearance
            });
        #[cfg(feature = "d2-model-importer")]
        let appearance = if self.source_shader() && self.dye_materials.sources.is_empty() {
            None
        } else {
            appearance
        };
        let width = ui.available_width();
        let preview = egui::Id::new("shader-preview");
        still::show_sources(
            ui,
            preview,
            &self.packages,
            appearance,
            &surface_overrides(&self.recipe.overrides.dye_edits, textures, gear),
            egui::vec2(width, (width * 0.75).clamp(220.0, 340.0)),
            {
                #[cfg(feature = "d2-model-importer")]
                {
                    sources
                }
                #[cfg(not(feature = "d2-model-importer"))]
                {
                    None
                }
            },
        );
        still::zoom_controls(ui, preview);
    }

    /// The inventory icon, and whether it is drawn from the dyes.
    fn draw_shader_icon(&mut self, ui: &mut egui::Ui) {
        self.draw_icon_donor_picker(ui);
        let checkbox = ui
            .checkbox(&mut self.recipe.overrides.icon_from_dyes, "Icon From Dyes")
            .on_hover_text("Draw the icon from the dyes.");
        if checkbox.changed() && !self.recipe.overrides.icon_from_dyes {
            #[cfg(feature = "d2-model-importer")]
            if self.source_shader() {
                self.recipe.overrides.icon_edit.imported_image =
                    crate::shader::source_icon(&self.recipe).ok();
                return;
            }
            self.recipe.overrides.icon_edit.imported_image = None;
        }
    }

    fn draw_shader_definition(&mut self, ui: &mut egui::Ui, inherited: WeaponRarity) {
        self.draw_item_text(ui, Some("Shader"));
        ui.add_space(4.0);
        let branding = self.presentation_editor.branding();
        style::tiles(ui, |ui, width| {
            style::tile_column(ui, (width, "shader-rarity"), |ui| {
                draw_gear_rarity(
                    ui,
                    &mut self.recipe.overrides,
                    inherited,
                    ItemKind::Shader,
                    branding,
                );
            });
        });
    }

    fn draw_shader_dyes(&mut self, ui: &mut egui::Ui) {
        let shaders = self.gear_donors_for(ItemKind::Shader).to_vec();
        // A pick names its source shader and one channel, or every channel.
        let mut take: Option<(u32, Option<i8>)> = None;
        self.draw_dyes_header(ui, &shaders, &mut take);
        let gear = self.draw_gear_tabs(ui);
        ui.add_space(4.0);
        let base_rows = self.shader_base_rows();
        #[cfg(feature = "d2-model-importer")]
        if let Some(error) = &self.dye_materials.source_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let rows = self
            .recipe
            .overrides
            .render_dye_rows
            .clone()
            .unwrap_or_else(|| base_rows.clone());
        // Armor's dyes stand for every gear type.
        let shown_dyes = DyeChannel::ALL.map(|channel| {
            gear_row(&rows, gear.unwrap_or(GearType::Armor), channel)
                .map(|row| row.dye_reference_index)
        });
        let stock_dye = |shader: u32, gear: Option<GearType>, channel: DyeChannel| {
            gear_row(
                &self
                    .catalog
                    .as_ref()
                    .map(|catalog| stock_rows(catalog, shader))
                    .unwrap_or_default(),
                gear.unwrap_or(GearType::Armor),
                channel,
            )
            .map(|row| row.dye_reference_index)
        };
        let copy_source = self.pending_dye_copy.and_then(|copy| {
            let (gear, channel, shader) = copy.source();
            stock_dye(shader, gear, channel)
        });
        let browsed = self
            .shader_texture_browse
            .and_then(|shader| stock_dye(shader, gear, self.shader_surface.0));
        // The dyes the tiles, the inspector, the icon, a pending copy and the texture menu read,
        // and the textures edits swap in.
        let mut dyes = shown_dyes
            .into_iter()
            .chain([copy_source, browsed])
            .flatten()
            .collect::<BTreeSet<_>>();
        if self.recipe.overrides.icon_from_dyes
            && let Some(icon) = icon::icon_dyes(&rows)
        {
            dyes.extend(icon.map(|(dye, _)| dye));
        }
        let tags = self
            .recipe
            .overrides
            .dye_texture_edits
            .iter()
            .flat_map(|edit| [edit.detail, edit.normal])
            .flatten()
            .collect::<BTreeSet<_>>();
        self.dye_materials
            .update(ui.ctx(), &self.packages, &dyes, &tags);
        self.iridescence.update(ui.ctx(), &self.packages);
        match (self.pending_dye_copy, copy_source) {
            (Some(copy), Some(source)) => self.finish_dye_copy(copy, source),
            // The source shader has no dye for this channel on the gear type.
            (Some(_), None) => self.pending_dye_copy = None,
            (None, _) => {}
        }
        if self.recipe.overrides.icon_from_dyes {
            self.draw_icon_from_dyes(ui.ctx(), &rows);
        }
        self.draw_surface_tiles(ui, gear, shown_dyes);
        ui.add_space(8.0);
        let slot = self.shader_surface;
        let mut actions = Vec::new();
        self.draw_surface_inspector(
            ui,
            (gear, shown_dyes[slot.0.index()]),
            slot,
            (browsed, &shaders),
            &mut actions,
        );
        self.apply_dye_actions(gear, actions, &mut take);
        // Drawing needed the whole page, so the catalog is borrowed again.
        if let Some((hash, only)) = take
            && let Some(catalog) = self.catalog.as_ref()
        {
            let source = stock_rows(catalog, hash);
            for (_, channel) in CHANNELS
                .into_iter()
                .filter(|(_, channel)| only.is_none_or(|only| only == *channel))
            {
                set_shader_channel(&mut self.recipe, &base_rows, &source, channel, gear);
            }
        }
    }

    /// The Dyes heading and its commands for the whole shader. `take` is set to a shader picked
    /// to copy every channel from.
    fn draw_dyes_header(
        &mut self,
        ui: &mut egui::Ui,
        shaders: &[WeaponDonorSummary],
        take: &mut Option<(u32, Option<i8>)>,
    ) {
        let overrides = &self.recipe.overrides;
        let customized = overrides.render_dye_rows.is_some()
            || !overrides.dye_edits.is_empty()
            || !overrides.dye_texture_edits.is_empty();
        ui.horizontal_wrapped(|ui| {
            style::heading(ui, "Dyes", customized)
                .on_hover_text("Six surfaces for each gear type. All Gear sets them all");
            ui.menu_button("Copy from Shader…", |ui| {
                if let Some(hash) = shader_list(ui, shaders, &mut self.gear_plug_query, None) {
                    *take = Some((hash, None));
                    ui.close();
                }
            });
            if ui
                .add_enabled(
                    customized,
                    egui::Button::new(if self.source_shader() {
                        "Restore Source Dyes"
                    } else {
                        "Restore Base Dyes"
                    }),
                )
                .clicked()
            {
                let overrides = &mut self.recipe.overrides;
                overrides.render_dye_rows = None;
                overrides.dye_edits.clear();
                overrides.dye_texture_edits.clear();
            }
        });
    }

    /// The gear types as tabs, All Gear first, each marked once it has edits of its own. Choosing
    /// one moves the preview to an item of that type. Returns the chosen one.
    fn draw_gear_tabs(&mut self, ui: &mut egui::Ui) -> Option<GearType> {
        let overrides = &self.recipe.overrides;
        let edited = |gear: Option<GearType>| {
            overrides.dye_edits.iter().any(|edit| edit.gear == gear)
                || overrides
                    .dye_texture_edits
                    .iter()
                    .any(|edit| edit.gear == gear)
        };
        let mut chosen = self.shader_dye_gear;
        ui.horizontal_wrapped(|ui| {
            for gear in std::iter::once(None).chain(GearType::ALL.map(Some)) {
                let label = gear.map_or("All Gear", GearType::label);
                let label = if edited(gear) {
                    format!("{label} •")
                } else {
                    label.to_owned()
                };
                ui.selectable_value(&mut chosen, gear, label);
            }
        });
        if chosen != self.shader_dye_gear {
            self.shader_dye_gear = chosen;
            if let Some(gear) = chosen {
                let items = preview_items(&self.donor_summaries, &self.gear_donors);
                self.shader_preview_item = preview_item_for(&items, self.shader_preview_item, gear);
            }
        }
        chosen
    }

    /// The six surfaces as tiles, channels side by side with primary over secondary, or a line to
    /// each channel in a narrow pane. Clicking one shows it in the inspector.
    fn draw_surface_tiles(
        &mut self,
        ui: &mut egui::Ui,
        gear: Option<GearType>,
        dyes: [Option<u16>; 3],
    ) {
        let gap = 8.0;
        let line = ui.available_width();
        let lines = if line >= 3.0 * SURFACE_TILE_MIN_WIDTH + 2.0 * gap {
            DyeSurface::ALL
                .map(|surface| DyeChannel::ALL.map(|channel| (channel, surface)).to_vec())
                .to_vec()
        } else {
            DyeChannel::ALL
                .map(|channel| DyeSurface::ALL.map(|surface| (channel, surface)).to_vec())
                .to_vec()
        };
        let columns = lines[0].len() as f32;
        let width = ((line - gap * (columns - 1.0)) / columns).floor();
        let mut picked = None;
        for slots in lines {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for slot in slots {
                    let dye = dyes[slot.0.index()];
                    if self
                        .draw_surface_tile(ui, width, (gear, dye), slot)
                        .clicked()
                    {
                        picked = Some(slot);
                    }
                }
            });
        }
        if let Some(slot) = picked {
            self.shader_surface = slot;
            self.shader_texture_browse = None;
        }
    }

    /// One surface's tile: a ball of its material, its name and its color.
    fn draw_surface_tile(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        (gear, dye): (Option<GearType>, Option<u16>),
        (channel, surface): (DyeChannel, DyeSurface),
    ) -> egui::Response {
        let overrides = &self.recipe.overrides;
        let edit = view_edit(&overrides.dye_edits, gear, channel, surface);
        let textures = view_texture_edit(&overrides.dye_texture_edits, gear, channel);
        let edited = own_edit(&overrides.dye_edits, gear, channel, surface).is_some();
        let color = self
            .surface_values(dye, gear, (channel, surface))
            .map_or_else(str::to_owned, |values| hex(values.color));
        let pixels = (SWATCH_SIZE * ui.ctx().pixels_per_point()).round() as u32;
        let swatch = dye.and_then(|dye| {
            self.dye_materials.swatch(
                ui.ctx(),
                (dye, edit, textures),
                (channel, surface),
                (&self.iridescence, pixels),
            )
        });
        SurfaceTile {
            swatch: swatch.as_ref(),
            name: &format!("{} {}", channel.name_on(gear), surface.label()),
            color: &color,
            selected: self.shader_surface == (channel, surface),
            edited,
        }
        .show(ui, width)
    }

    /// Applies a pending copy once its source dye has loaded, or drops it when the dye cannot be
    /// read.
    fn finish_dye_copy(&mut self, copy: DyeCopy, source: u16) {
        let material = match self.dye_materials.material(source) {
            None => return,
            Some(Err(_)) => {
                self.pending_dye_copy = None;
                return;
            }
            Some(Ok(material)) => material,
        };
        match copy {
            DyeCopy::Surface(gear, channel, surface, _) => {
                let values = Values::read(&material.vectors, &[], surface);
                set_dye_edit(
                    &mut self.recipe.overrides.dye_edits,
                    gear,
                    channel,
                    surface,
                    |edit| {
                        for change in Change::all(values) {
                            change.apply(edit);
                        }
                    },
                );
            }
            DyeCopy::Textures(gear, channel, _) => {
                let (detail, normal) = (material.detail, material.normal);
                let (detail_tiling, normal_tiling) = (
                    tiling(material.detail_tiling),
                    tiling(material.normal_tiling),
                );
                set_texture_edit(
                    &mut self.recipe.overrides.dye_texture_edits,
                    gear,
                    channel,
                    |edit| {
                        edit.detail = detail;
                        edit.normal = normal;
                        edit.detail_tiling = detail_tiling;
                        edit.normal_tiling = normal_tiling;
                    },
                );
            }
        }
        self.pending_dye_copy = None;
    }

    /// One surface's values as the page shows them: the shown dye's with the view's edit written
    /// over them, or what shows while the dye is read.
    fn surface_values(
        &self,
        dye: Option<u16>,
        gear: Option<GearType>,
        (channel, surface): (DyeChannel, DyeSurface),
    ) -> Result<Values, &'static str> {
        let material = match dye.map(|dye| self.dye_materials.material(dye)) {
            Some(Some(Ok(material))) => material,
            Some(None) => return Err(LOADING),
            Some(Some(Err(_))) | None => return Err(UNAVAILABLE),
        };
        let writes = view_edit(&self.recipe.overrides.dye_edits, gear, channel, surface)
            .map(|edit| edit.writes())
            .unwrap_or_default();
        Ok(Values::read(&material.vectors, &writes, surface))
    }

    /// One dye's textures and tiling as the page shows them, or what shows while the dye is read.
    fn texture_values(
        &self,
        dye: Option<u16>,
        (channel, gear): (DyeChannel, Option<GearType>),
    ) -> Result<TextureValues, &'static str> {
        let material = match dye.map(|dye| self.dye_materials.material(dye)) {
            Some(Some(Ok(material))) => material,
            Some(None) => return Err(LOADING),
            Some(Some(Err(_))) | None => return Err(UNAVAILABLE),
        };
        let edit = view_texture_edit(&self.recipe.overrides.dye_texture_edits, gear, channel);
        Ok(TextureValues::read(material, edit))
    }

    /// The chosen surface's values, grouped into Paint, Detail, Worn and Glow, with a menu to copy
    /// or reset it.
    fn draw_surface_inspector(
        &mut self,
        ui: &mut egui::Ui,
        (gear, dye): (Option<GearType>, Option<u16>),
        slot: (DyeChannel, DyeSurface),
        (browsed, shaders): (Option<u16>, &[WeaponDonorSummary]),
        actions: &mut Vec<SlotAction>,
    ) {
        let (channel, surface) = slot;
        let values = self.surface_values(dye, gear, slot);
        let textures = self.texture_values(dye, (channel, gear));
        let overrides = &self.recipe.overrides;
        let own = own_edit(&overrides.dye_edits, gear, channel, surface)
            .unwrap_or(DyeEdit::new(gear, channel, surface));
        let own_textures = own_texture_edit(&overrides.dye_texture_edits, gear, channel)
            .unwrap_or(DyeTextureEdit::new(gear, channel));
        let name = format!("{} {}", channel.name_on(gear), surface.label());
        style::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(&name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.draw_surface_menu(
                        ui,
                        (&name, channel.name_on(gear)),
                        (slot, !own.is_empty()),
                        shaders,
                        actions,
                    );
                });
            });
            let values = match values {
                Ok(values) => values,
                Err(missing) => {
                    ui.weak(missing);
                    return;
                }
            };
            section(ui, "Paint");
            style::tiles(ui, |ui, width| {
                let paint = [
                    Change::Color(values.color),
                    Change::Iridescence(values.iridescence),
                    Change::Metalness(values.metalness),
                    Change::Smoothness(values.smoothness),
                ];
                self.draw_value_tiles(ui, width, (slot, own), &paint, actions);
            });
            section(ui, "Detail");
            style::tiles(ui, |ui, width| {
                self.draw_texture_tiles(
                    ui,
                    width,
                    (channel, browsed),
                    (textures, own_textures),
                    shaders,
                    actions,
                );
                let detail = [
                    Change::Detail(values.detail),
                    Change::Bumps(values.bumps),
                    Change::DetailSmoothness(values.detail_smoothness),
                ];
                self.draw_value_tiles(ui, width, (slot, own), &detail, actions);
            });
            section(ui, "Worn");
            style::tiles(ui, |ui, width| {
                let worn = [
                    Change::WornColor(values.worn_color),
                    Change::WornMetalness(values.worn_metalness),
                    Change::WornSmoothness(values.worn_smoothness),
                    Change::Wear(values.wear),
                ];
                self.draw_value_tiles(ui, width, (slot, own), &worn, actions);
            });
            section(ui, "Glow");
            style::tiles(ui, |ui, width| {
                let glow = [Change::Glow(values.glow)];
                self.draw_value_tiles(ui, width, (slot, own), &glow, actions);
            });
        });
    }

    /// A surface's menu: copy it or its channel's dye from a stock shader, or reset it.
    fn draw_surface_menu(
        &mut self,
        ui: &mut egui::Ui,
        (name, channel_name): (&str, &str),
        ((channel, surface), resettable): ((DyeChannel, DyeSurface), bool),
        shaders: &[WeaponDonorSummary],
        actions: &mut Vec<SlotAction>,
    ) {
        style::more_menu(ui, name, |ui| {
            ui.menu_button("Copy from Shader…", |ui| {
                if let Some(hash) = shader_list(ui, shaders, &mut self.gear_plug_query, None) {
                    actions.push(SlotAction::Copy(channel, surface, hash));
                    ui.close();
                }
            });
            ui.menu_button(format!("Use {channel_name} Dye from Shader…"), |ui| {
                if let Some(hash) = shader_list(ui, shaders, &mut self.gear_plug_query, None) {
                    actions.push(SlotAction::UseDye(channel, hash));
                    ui.close();
                }
            });
            if ui
                .add_enabled(resettable, egui::Button::new("Reset Surface"))
                .clicked()
            {
                actions.push(SlotAction::Reset(channel, surface));
                ui.close();
            }
        });
    }

    /// A tile for each of a surface's values. A value the view sets itself reads brighter and
    /// offers Reset.
    fn draw_value_tiles(
        &self,
        ui: &mut egui::Ui,
        width: f32,
        (slot, own): ((DyeChannel, DyeSurface), DyeEdit),
        values: &[Change],
        actions: &mut Vec<SlotAction>,
    ) {
        let (channel, surface) = slot;
        for &current in values {
            let label = current.label();
            let salt = std::mem::discriminant(&current);
            let (edited, reset) =
                style::tile(ui, width, salt, label, "", current.is_set(&own), |ui| {
                    self.draw_value(ui, width, slot, current)
                });
            if let Some(change) = edited {
                actions.push(SlotAction::Set(channel, surface, change));
            } else if reset {
                actions.push(SlotAction::Clear(channel, surface, current));
            }
        }
    }

    /// A value's control. Returns the value edited to.
    fn draw_value(
        &self,
        ui: &mut egui::Ui,
        width: f32,
        slot: (DyeChannel, DyeSurface),
        current: Change,
    ) -> Option<Change> {
        match current {
            Change::Color(color) => color_field(ui, color).map(Change::Color),
            Change::Glow(color) => color_field(ui, color).map(Change::Glow),
            Change::WornColor(color) => color_field(ui, color).map(Change::WornColor),
            Change::Iridescence(id) => self
                .draw_iridescence_picker(ui, slot, id)
                .map(Change::Iridescence),
            Change::Metalness(value) => amount_field(ui, width, value, 1.0).map(Change::Metalness),
            Change::Detail(value) => amount_field(ui, width, value, 1.0).map(Change::Detail),
            Change::Bumps(value) => amount_field(ui, width, value, 4.0).map(Change::Bumps),
            Change::DetailSmoothness(value) => {
                amount_field(ui, width, value, 1.0).map(Change::DetailSmoothness)
            }
            Change::WornMetalness(value) => {
                amount_field(ui, width, value, 1.0).map(Change::WornMetalness)
            }
            Change::Smoothness(range) => range_field(ui, range).map(Change::Smoothness),
            Change::WornSmoothness(range) => range_field(ui, range).map(Change::WornSmoothness),
            Change::Wear(wear) => wear_field(ui, wear).map(Change::Wear),
        }
    }

    /// A dye's textures and their tiling, which both its surfaces share.
    fn draw_texture_tiles(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        (channel, browsed): (DyeChannel, Option<u16>),
        (textures, own): (Result<TextureValues, &'static str>, DyeTextureEdit),
        shaders: &[WeaponDonorSummary],
        actions: &mut Vec<SlotAction>,
    ) {
        let textures = match textures {
            Ok(textures) => textures,
            Err(missing) => {
                style::tile(ui, width, "textures", "Textures", "", false, |ui| {
                    ui.weak(missing)
                });
                return;
            }
        };
        let modified = own.detail.is_some() || own.normal.is_some();
        let (_, reset) = style::tile(
            ui,
            width,
            "textures",
            "Textures",
            "Shared by both surfaces",
            modified,
            |ui| self.draw_texture_choice(ui, (channel, textures), browsed, shaders, actions),
        );
        if reset {
            actions.push(SlotAction::ClearTextures(channel));
        }
        for (which, values, modified) in [
            (
                Tiling::Detail,
                textures.detail_tiling,
                own.detail_tiling.is_some(),
            ),
            (
                Tiling::Normal,
                textures.normal_tiling,
                own.normal_tiling.is_some(),
            ),
        ] {
            let label = which.label();
            let (edited, reset) = style::tile(ui, width, label, label, "", modified, |ui| {
                tiling_field(ui, values)
            });
            if let Some(values) = edited {
                actions.push(SlotAction::Tiling(channel, which, values));
            } else if reset {
                actions.push(SlotAction::ClearTiling(channel, which));
            }
        }
    }

    /// A dye's detail and normal textures, and a menu of stock shaders to take them from that
    /// shows the textures of the one under the pointer.
    fn draw_texture_choice(
        &mut self,
        ui: &mut egui::Ui,
        (channel, textures): (DyeChannel, TextureValues),
        browsed: Option<u16>,
        shaders: &[WeaponDonorSummary],
        actions: &mut Vec<SlotAction>,
    ) {
        ui.horizontal(|ui| {
            for texture in [("Detail", textures.detail), ("Normal", textures.normal)] {
                self.draw_texture_thumbnail(ui, texture, THUMBNAIL_SIZE);
            }
            let menu = ui.menu_button("Change…", |ui| {
                workbench_style(ui);
                self.draw_browsed_textures(ui, browsed);
                let (clicked, hovered) =
                    browse_shaders(ui, shaders, &mut self.gear_plug_query, None);
                if hovered.is_some() {
                    self.shader_texture_browse = hovered;
                }
                if let Some(hash) = clicked {
                    actions.push(SlotAction::UseTextures(channel, hash));
                    ui.close();
                }
            });
            if menu.inner.is_none() {
                self.shader_texture_browse = None;
            }
        });
    }

    /// The textures of the shader under the pointer in the texture menu, in empty frames until
    /// one has loaded.
    fn draw_browsed_textures(&mut self, ui: &mut egui::Ui, browsed: Option<u16>) {
        let tags = match browsed.map(|dye| self.dye_materials.material(dye)) {
            Some(Some(Ok(material))) => [material.detail, material.normal],
            Some(Some(Err(_))) => {
                ui.weak(UNAVAILABLE);
                return;
            }
            Some(None) | None => [None, None],
        };
        ui.horizontal(|ui| {
            for texture in ["Detail", "Normal"].into_iter().zip(tags) {
                self.draw_texture_thumbnail(ui, texture, BROWSER_THUMBNAIL_SIZE);
            }
        });
    }

    /// A texture shown small, or an empty frame while it loads or when there is none.
    fn draw_texture_thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        (name, tag): (&str, Option<u32>),
        size: f32,
    ) {
        let handle = tag.and_then(|tag| self.dye_materials.thumbnail(ui.ctx(), tag));
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
        match handle {
            Some(handle) => {
                ui.painter().image(
                    handle.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    ui.visuals().widgets.noninteractive.bg_stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let Some(tag) = tag {
            response.on_hover_text(format!("{name} 0x{tag:08X}"));
        }
    }

    /// A surface's iridescence row, with a strip of its colors. Returns the row picked.
    fn draw_iridescence_picker(
        &self,
        ui: &mut egui::Ui,
        (channel, surface): (DyeChannel, DyeSurface),
        current: i16,
    ) -> Option<i16> {
        let swatch = egui::vec2(48.0, 12.0);
        ui.horizontal(|ui| {
            let mut picked = None;
            egui::ComboBox::from_id_salt(("shader-iridescence", channel, surface))
                .width(80.0)
                .height(360.0)
                .selected_text(match current {
                    NO_IRIDESCENCE => "None".to_owned(),
                    id => id.to_string(),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current == NO_IRIDESCENCE, "None")
                        .clicked()
                    {
                        picked = Some(NO_IRIDESCENCE);
                    }
                    for row in self.iridescence.rows() {
                        ui.horizontal(|ui| {
                            draw_gradient(ui, &row.colors, swatch);
                            if ui
                                .selectable_label(current == row.id, row.id.to_string())
                                .clicked()
                            {
                                picked = Some(row.id);
                            }
                        });
                    }
                });
            if let Some(row) = self.iridescence.row(current) {
                draw_gradient(ui, &row.colors, swatch);
            }
            picked
        })
        .inner
    }

    /// Applies what the inspector asked for to the view's edits. Taking a whole dye needs the
    /// catalog, so it goes through `take`.
    fn apply_dye_actions(
        &mut self,
        gear: Option<GearType>,
        actions: Vec<SlotAction>,
        take: &mut Option<(u32, Option<i8>)>,
    ) {
        let overrides = &mut self.recipe.overrides;
        let (edits, textures) = (&mut overrides.dye_edits, &mut overrides.dye_texture_edits);
        for action in actions {
            match action {
                SlotAction::Set(channel, surface, change) => {
                    set_dye_edit(edits, gear, channel, surface, |edit| change.apply(edit));
                }
                SlotAction::Clear(channel, surface, change) => {
                    set_dye_edit(edits, gear, channel, surface, |edit| change.clear(edit));
                }
                SlotAction::Reset(channel, surface) => {
                    set_dye_edit(edits, gear, channel, surface, |edit| {
                        *edit = DyeEdit::new(gear, channel, surface);
                    });
                }
                SlotAction::Copy(channel, surface, shader) => {
                    self.pending_dye_copy = Some(DyeCopy::Surface(gear, channel, surface, shader));
                }
                SlotAction::UseDye(channel, shader) => {
                    *take = Some((shader, Some(channel.offset())));
                }
                SlotAction::UseTextures(channel, shader) => {
                    self.pending_dye_copy = Some(DyeCopy::Textures(gear, channel, shader));
                    self.shader_texture_browse = None;
                }
                SlotAction::Tiling(channel, which, values) => {
                    set_texture_edit(textures, gear, channel, |edit| {
                        which.set(edit, tiling(values));
                    });
                }
                SlotAction::ClearTiling(channel, which) => {
                    set_texture_edit(textures, gear, channel, |edit| which.set(edit, None));
                }
                SlotAction::ClearTextures(channel) => {
                    set_texture_edit(textures, gear, channel, |edit| {
                        edit.detail = None;
                        edit.normal = None;
                    });
                }
            }
        }
    }
}

/// A group's name over its tiles.
fn section(ui: &mut egui::Ui, name: &str) {
    ui.add_space(6.0);
    ui.strong(name);
}

/// The edit a view shows for one surface: a gear type's own over the one for every gear type, or
/// for All Gear the one for every gear type alone.
fn view_edit(
    edits: &[DyeEdit],
    gear: Option<GearType>,
    channel: DyeChannel,
    surface: DyeSurface,
) -> Option<DyeEdit> {
    match gear {
        Some(gear) => surface_edit(edits, gear, channel, surface),
        None => own_edit(edits, None, channel, surface),
    }
}

/// The edit a view makes for one surface, which its resets clear.
fn own_edit(
    edits: &[DyeEdit],
    gear: Option<GearType>,
    channel: DyeChannel,
    surface: DyeSurface,
) -> Option<DyeEdit> {
    edits
        .iter()
        .find(|edit| (edit.gear, edit.channel, edit.surface) == (gear, channel, surface))
        .copied()
}

/// The texture edit a view shows for one dye, as `view_edit` is for a surface.
fn view_texture_edit(
    edits: &[DyeTextureEdit],
    gear: Option<GearType>,
    channel: DyeChannel,
) -> Option<DyeTextureEdit> {
    match gear {
        Some(gear) => texture_edit(edits, gear, channel),
        None => own_texture_edit(edits, None, channel),
    }
}

/// The texture edit a view makes for one dye.
fn own_texture_edit(
    edits: &[DyeTextureEdit],
    gear: Option<GearType>,
    channel: DyeChannel,
) -> Option<DyeTextureEdit> {
    edits
        .iter()
        .find(|edit| (edit.gear, edit.channel) == (gear, channel))
        .copied()
}

/// A surface as a tile: a ball of its material, its name and its color. The name reads brighter
/// once the view edits the surface, as a value tile's name does.
struct SurfaceTile<'a> {
    swatch: Option<&'a egui::TextureHandle>,
    name: &'a str,
    color: &'a str,
    selected: bool,
    edited: bool,
}

impl SurfaceTile<'_> {
    fn show(self, ui: &mut egui::Ui, width: f32) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, SWATCH_SIZE + 12.0), egui::Sense::click());
        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals();
            let stroke = if self.selected {
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
            let ball = egui::Rect::from_min_size(
                rect.min + egui::Vec2::splat(6.0),
                egui::Vec2::splat(SWATCH_SIZE),
            );
            match self.swatch {
                Some(texture) => {
                    painter.image(
                        texture.id(),
                        ball,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                None => {
                    painter.circle_stroke(
                        ball.center(),
                        SWATCH_SIZE / 2.0 - 1.0,
                        visuals.widgets.noninteractive.bg_stroke,
                    );
                }
            }
            let left = ball.right() + 8.0;
            let name = if self.edited {
                visuals.text_color()
            } else {
                style::secondary(visuals)
            };
            painter.text(
                egui::pos2(left, rect.center().y - 8.0),
                egui::Align2::LEFT_CENTER,
                self.name,
                egui::FontId::proportional(13.0),
                name,
            );
            painter.text(
                egui::pos2(left, rect.center().y + 9.0),
                egui::Align2::LEFT_CENTER,
                self.color,
                egui::FontId::monospace(11.0),
                style::secondary(visuals),
            );
        }
        style::named_control(response, self.name)
    }
}
