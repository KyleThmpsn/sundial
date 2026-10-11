//! Barrel Settings: a polar schematic beside value tiles and the pattern's rings as a table,
//! sharing the compiler's geometry resolution.
use super::*;
use crate::app::style::{self, named_control};
use crate::weapon::barrel::{Edits, MAX_BULLETS_PER_SHOT, Pattern, Ring};

/// The schematic's side.
const PREVIEW: f32 = 150.0;
/// The narrowest the values get with the schematic beside them, which holds the ring table, so
/// the layout does not change when the rings appear.
const VALUES_MIN_WIDTH: f32 = 420.0;
/// Space between the pattern's choices.
const CHOICE_SPACING: f32 = 4.0;

/// Where a pattern's rings come from: the Barrel, a preset, or the author.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Barrel,
    Circle,
    Ring,
    Custom,
}

const SHAPES: [(Shape, &str); 4] = [
    (Shape::Barrel, "Follow Barrel"),
    (Shape::Circle, "Filled Circle"),
    (Shape::Ring, "Ring"),
    (Shape::Custom, "Custom"),
];

/// The ring table's columns: each field's name, and what it sets on the name's hover.
const RING_COLUMNS: [(&str, &str); 5] = [
    ("Pellets", "Pellets in this ring"),
    (
        "Inner Radius",
        "Percent of the spread. Equal radii form a ring",
    ),
    (
        "Outer Radius",
        "Percent of the spread. Equal radii form a ring",
    ),
    ("Rotation", "Turns the ring, in degrees"),
    (
        "Randomness",
        "0% spaces pellets evenly. 100% varies each within its sector",
    ),
];

/// Bullets per Shot as the weapon fires it, and the stat that picks it, by name and in-game
/// reading, while one does.
pub(super) struct Burst {
    pub(super) bullets: u16,
    pub(super) follows: Option<(String, String)>,
}

/// `custom` remembers, for the recipe on screen, that the author picked Custom, since rings equal
/// to a preset would otherwise read as that preset. `bullets` is the weapon's Bullets per Shot,
/// where it offers one.
pub(super) fn draw(
    ui: &mut egui::Ui,
    saved: &mut Option<Edits>,
    defaults: &BarrelDefaults,
    custom: egui::Id,
    bullets: Option<&Burst>,
) {
    let mut edits = saved.clone().unwrap_or_default();
    let before = edits.clone();
    let inherited = defaults.pattern.as_ref();
    let pattern = match edits.resolve(inherited) {
        Ok(pattern) => pattern,
        Err(error) => {
            ui.colored_label(ui.visuals().warn_fg_color, error);
            return;
        }
    };
    // The schematic beside the values when both fit, under them otherwise. Its square is placed
    // first and painted last, so it shows this frame's edits.
    let gap = ui.spacing().item_spacing.x * 2.0;
    let resolved = if ui.available_width() >= PREVIEW + gap + VALUES_MIN_WIDTH {
        ui.horizontal_top(|ui| {
            let square = preview_square(ui);
            ui.add_space(gap - ui.spacing().item_spacing.x);
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    values(
                        ui,
                        &mut edits,
                        (inherited, &pattern),
                        (custom, defaults.random_rotation),
                        bullets,
                    );
                },
            );
            schematic(ui, square, &edits, inherited)
        })
        .inner
    } else {
        values(
            ui,
            &mut edits,
            (inherited, &pattern),
            (custom, defaults.random_rotation),
            bullets,
        );
        let square = preview_square(ui);
        schematic(ui, square, &edits, inherited)
    };
    if resolved && edits != before {
        *saved = (!edits.is_empty()).then_some(edits);
    }
}

/// Pellets per Bullet, Bullets per Shot where the weapon keeps it in one place, Spread and Pattern
/// as tiles, and the rings under them once they are set. `bullets` is the weapon's own.
fn values(
    ui: &mut egui::Ui,
    edits: &mut Edits,
    (inherited, pattern): (Option<&Pattern>, &Pattern),
    (custom, rotation): (egui::Id, bool),
    bullets: Option<&Burst>,
) {
    let inherited_count = inherited.map_or(1, |pattern| total(&pattern.rings));
    // Without a pattern the firing code skips the spread, so its rotation does nothing.
    let patterned = inherited.is_some() || edits.shapes_pattern();
    let choices = choice_width(ui);
    style::tiles(ui, |ui, width| {
        draw_count(ui, width, edits, pattern, inherited_count);
        if let Some(burst) = bullets {
            draw_bullets(ui, width, edits, burst);
        }
        draw_spread(ui, width, edits);
        // A preset takes the count this frame set.
        let current = edits.resolve(inherited).unwrap_or_else(|_| pattern.clone());
        draw_shape(ui, (choices.max(width), custom), edits, &current);
        if patterned {
            draw_rotation(ui, width, edits, rotation);
        }
    });
    let changed = edits.rings.as_mut().and_then(|rings| {
        ui.add_space(4.0);
        draw_ring_table(ui, rings)
    });
    if let Some(total) = changed {
        edits.pellets = Some(total);
    }
}

fn total(rings: &[Ring]) -> u16 {
    rings.iter().map(|ring| ring.pellets).sum()
}

fn draw_count(
    ui: &mut egui::Ui,
    width: f32,
    edits: &mut Edits,
    pattern: &Pattern,
    inherited_count: u16,
) {
    let mut count = total(&pattern.rings);
    let original = edits.pellets.map(|_| inherited_count.to_string());
    let (changed, reset) = style::stock_tile(
        ui,
        (width, "barrel-pellets"),
        (
            "Pellets per Bullet",
            "Pellets each bullet fires, shared across the rings",
        ),
        original.as_deref(),
        |ui| {
            let field = egui::DragValue::new(&mut count).range(1..=32767).speed(1.0);
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            named_control(ui.add_sized(size, field), "Pellets per Bullet").changed()
        },
    );
    if changed {
        let _ = edits.set_pellets(count);
    } else if reset {
        let _ = edits.set_pellets(inherited_count);
        edits.pellets = None;
    }
}

/// Bullets per Shot: how many bullets one pull fires, each with every pellet, from `burst`, the
/// weapon's own. While a stat picks them, the field says which value of it does, as in 3 at 450
/// RPM, and a set value holds at every value of that stat.
fn draw_bullets(ui: &mut egui::Ui, width: f32, edits: &mut Edits, burst: &Burst) {
    let stock = burst.bullets;
    let mut bullets = edits.bullets_per_shot.unwrap_or(stock);
    let original = edits.bullets_per_shot.map(|_| stock.to_string());
    let hint = match &burst.follows {
        Some((stat, _)) => {
            format!("Bullets one pull fires, each with every pellet. {stat} sets it until changed")
        }
        None => "Bullets one pull fires, each with every pellet".to_owned(),
    };
    let reading = burst
        .follows
        .as_ref()
        .filter(|_| edits.bullets_per_shot.is_none())
        .map(|(_, reading)| format!(" at {reading}"));
    let (changed, reset) = style::stock_tile(
        ui,
        (width, "barrel-bullets"),
        ("Bullets per Shot", hint.as_str()),
        original.as_deref(),
        |ui| {
            let mut field = egui::DragValue::new(&mut bullets)
                .range(1..=MAX_BULLETS_PER_SHOT)
                .speed(0.1);
            if let Some(reading) = reading {
                field = field.suffix(reading);
            }
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            named_control(ui.add_sized(size, field), "Bullets per Shot").changed()
        },
    );
    if changed {
        edits.bullets_per_shot = (bullets != stock).then_some(bullets);
    } else if reset {
        edits.bullets_per_shot = None;
    }
}

fn draw_spread(ui: &mut egui::Ui, width: f32, edits: &mut Edits) {
    let mut percent = f64::from(f32::from_bits(
        edits.spread_scale_bits.unwrap_or(1.0_f32.to_bits()),
    )) * 100.0;
    let original = edits.spread_scale_bits.map(|_| "100%");
    let (changed, reset) = style::stock_tile(
        ui,
        (width, "barrel-spread"),
        (
            "Spread",
            "Percent of the Barrel's spread. 0% centers every pellet",
        ),
        original,
        |ui| number(ui, "Spread", &mut percent, (0.0..=10000.0, "%"), width),
    );
    if changed {
        edits.spread_scale_bits = (percent != 100.0).then(|| ((percent / 100.0) as f32).to_bits());
    } else if reset {
        edits.spread_scale_bits = None;
    }
}

/// A preset's one ring: every pellet between `inner` and the whole spread.
fn preset(count: u16, inner: f32) -> Vec<Ring> {
    let mut rings = Pattern::circular(count).rings;
    rings[0].inner_radius_bits = inner.to_bits();
    rings[0].outer_radius_bits = 1.0_f32.to_bits();
    rings
}

fn shape_of(rings: Option<&[Ring]>) -> Shape {
    let Some(rings) = rings else {
        return Shape::Barrel;
    };
    let count = total(rings);
    if rings == preset(count, 0.0).as_slice() {
        Shape::Circle
    } else if rings == preset(count, 1.0).as_slice() {
        Shape::Ring
    } else {
        Shape::Custom
    }
}

/// The width the pattern's choices take side by side, with a little to spare, since the tile clips
/// what runs past it.
fn choice_width(ui: &egui::Ui) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let padding = ui.spacing().button_padding.x * 2.0 + 2.0;
    let labels = SHAPES.map(|(_, label)| label);
    let text = ui.fonts_mut(|fonts| {
        labels
            .iter()
            .map(|label| {
                fonts
                    .layout_no_wrap(
                        (*label).to_owned(),
                        font.clone(),
                        egui::Color32::PLACEHOLDER,
                    )
                    .size()
                    .x
            })
            .sum::<f32>()
    });
    (text + padding * labels.len() as f32 + CHOICE_SPACING * (labels.len() - 1) as f32).ceil()
}

/// Pattern: the Barrel's own rings, a preset, or rings of the author's, side by side. Custom
/// starts from the rings the weapon has now and stays picked, though they match a preset, until
/// another is. Editing a preset's ring makes it Custom.
fn draw_shape(
    ui: &mut egui::Ui,
    (width, custom): (f32, egui::Id),
    edits: &mut Edits,
    pattern: &Pattern,
) {
    let picked_custom = ui
        .data(|data| data.get_temp::<bool>(custom))
        .unwrap_or(false);
    let current = match edits.rings.as_deref() {
        Some(_) if picked_custom => Shape::Custom,
        rings => shape_of(rings),
    };
    let (picked, reset) = style::tile(
        ui,
        width,
        "barrel-pattern",
        "Pattern",
        "Pellet rings around the center",
        current != Shape::Barrel,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CHOICE_SPACING;
                let mut picked = None;
                for (shape, label) in SHAPES {
                    let selected = shape == current;
                    if ui
                        .add(egui::Button::new(label).selected(selected))
                        .clicked()
                        && !selected
                    {
                        picked = Some(shape);
                    }
                }
                picked
            })
            .inner
        },
    );
    let Some(picked) = (if reset { Some(Shape::Barrel) } else { picked }) else {
        return;
    };
    let count = total(&pattern.rings);
    edits.rings = match picked {
        Shape::Barrel => None,
        Shape::Circle => Some(preset(count, 0.0)),
        Shape::Ring => Some(preset(count, 1.0)),
        Shape::Custom => Some(edits.rings.clone().unwrap_or_else(|| pattern.rings.clone())),
    };
    ui.data_mut(|data| data.insert_temp(custom, picked == Shape::Custom));
}

/// Random Rotation: whether each bullet's pattern takes a new angle, the selected Barrel's own
/// `stock` setting until changed. Off keeps a pattern such as a level row level.
fn draw_rotation(ui: &mut egui::Ui, width: f32, edits: &mut Edits, stock: bool) {
    let current = edits.random_rotation.unwrap_or(stock);
    let (picked, reset) = style::tile(
        ui,
        width,
        "barrel-rotation",
        "Random Rotation",
        "A new pattern angle for each bullet",
        edits.random_rotation.is_some(),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CHOICE_SPACING;
                let mut picked = None;
                for (value, label) in [(true, "On"), (false, "Off")] {
                    let selected = value == current;
                    if ui
                        .add(egui::Button::new(label).selected(selected))
                        .clicked()
                        && !selected
                    {
                        picked = Some(value);
                    }
                }
                picked
            })
            .inner
        },
    );
    if reset {
        edits.random_rotation = None;
    } else if let Some(value) = picked {
        edits.random_rotation = (value != stock).then_some(value);
    }
}

/// A percentage or angle field, `width` wide. Returns whether it changed.
fn number(
    ui: &mut egui::Ui,
    name: &str,
    value: &mut f64,
    (range, suffix): (std::ops::RangeInclusive<f64>, &str),
    width: f32,
) -> bool {
    ui.push_id(name, |ui| {
        let field = egui::DragValue::new(value)
            .range(range)
            .clamp_existing_to_range(false)
            .speed(1.0)
            .max_decimals(2)
            .suffix(suffix);
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        named_control(ui.add_sized(size, field), name).changed()
    })
    .inner
}

/// The width of a ring's fields: the line shared among the five, after the ring's number and its
/// remove button, within bounds.
fn ring_field_width(ui: &egui::Ui) -> f32 {
    let spacing = ui.spacing().item_spacing.x * (RING_COLUMNS.len() + 1) as f32;
    let fixed = 24.0 + ui.spacing().interact_size.y + spacing;
    ((ui.available_width() - fixed) / RING_COLUMNS.len() as f32).clamp(48.0, 72.0)
}

/// A column's name over the ring table, small and grey as a tile's name is.
fn column_name(ui: &mut egui::Ui, name: &str, hint: &str) {
    let text = egui::RichText::new(name)
        .size(12.0)
        .color(style::secondary(ui.visuals()));
    let response = ui.label(text);
    if !hint.is_empty() {
        response.on_hover_text(hint);
    }
}

/// The rings as a table, a row each, with Add Ring under it. Returns the new total once the
/// rings changed, and puts them back when the total leaves the range a Barrel can hold.
fn draw_ring_table(ui: &mut egui::Ui, rings: &mut Vec<Ring>) -> Option<u16> {
    let before = rings.clone();
    let width = ring_field_width(ui);
    let removable = rings.len() > 1;
    let mut remove = None;
    egui::Grid::new("barrel-rings")
        .num_columns(RING_COLUMNS.len() + 2)
        .spacing([ui.spacing().item_spacing.x, 4.0])
        .show(ui, |ui| {
            column_name(ui, "Ring", "");
            for (name, hint) in RING_COLUMNS {
                column_name(ui, name, hint);
            }
            ui.end_row();
            for (index, ring) in rings.iter_mut().enumerate() {
                let ordinal = egui::RichText::new((index + 1).to_string())
                    .color(style::secondary(ui.visuals()));
                ui.label(ordinal);
                draw_ring(ui, index, ring, width);
                if removable && remove_ring(ui, index) {
                    remove = Some(index);
                }
                ui.end_row();
            }
        });
    if let Some(index) = remove {
        rings.remove(index);
    }
    if rings.len() < 32767 && ui.small_button("Add Ring").clicked() {
        rings.push(Ring {
            pellets: 1,
            inner_radius_bits: 1.0_f32.to_bits(),
            outer_radius_bits: 1.0_f32.to_bits(),
            rotation_bits: 0.0_f32.to_bits(),
            randomness_bits: 0.0_f32.to_bits(),
        });
    }
    let total = rings
        .iter()
        .map(|ring| u32::from(ring.pellets))
        .sum::<u32>();
    if !(1..=32767).contains(&total) {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Rings need 1 to 32767 pellets in total.",
        );
        *rings = before;
        return None;
    }
    (*rings != before).then_some(total as u16)
}

/// One ring's fields, each named by its ring for a screen reader.
fn draw_ring(ui: &mut egui::Ui, index: usize, ring: &mut Ring, width: f32) {
    let name = |column: &str| format!("Ring {} {column}", index + 1);
    let pellets = name("Pellets");
    ui.push_id(&pellets, |ui| {
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        let field = egui::DragValue::new(&mut ring.pellets).range(0..=32767);
        named_control(ui.add_sized(size, field), &pellets);
    });
    let mut inner = f64::from(f32::from_bits(ring.inner_radius_bits)) * 100.0;
    let mut outer = f64::from(f32::from_bits(ring.outer_radius_bits)) * 100.0;
    let mut rotation = f64::from(f32::from_bits(ring.rotation_bits)).to_degrees();
    let mut randomness = f64::from(f32::from_bits(ring.randomness_bits)) * 100.0;
    let radius = (0.0..=10000.0, "%");
    let mut radii = None;
    if number(ui, &name("Inner Radius"), &mut inner, radius.clone(), width) {
        radii = Some((inner, outer.max(inner)));
    }
    if number(ui, &name("Outer Radius"), &mut outer, radius, width) {
        radii = Some((inner.min(outer), outer));
    }
    if let Some((inner, outer)) = radii {
        ring.inner_radius_bits = ((inner / 100.0) as f32).to_bits();
        ring.outer_radius_bits = ((outer / 100.0) as f32).to_bits();
    }
    let angle = (-360.0..=360.0, "°");
    if number(ui, &name("Rotation"), &mut rotation, angle, width) {
        ring.rotation_bits = (rotation.to_radians() as f32).to_bits();
    }
    let share = (0.0..=100.0, "%");
    if number(ui, &name("Randomness"), &mut randomness, share, width) {
        ring.randomness_bits = ((randomness / 100.0) as f32).to_bits();
    }
}

/// The quiet X that removes a ring.
fn remove_ring(ui: &mut egui::Ui, index: usize) -> bool {
    ui.scope(|ui| {
        style::quiet(ui);
        let icon = style::light_icon(ui, egui_phosphor::regular::X);
        let response = ui.add(egui::Button::new(icon)).on_hover_text("Remove Ring");
        named_control(response, format!("Remove Ring {}", index + 1)).clicked()
    })
    .inner
}

/// The schematic's square, named for a screen reader, with what it shows on its hover.
fn preview_square(ui: &mut egui::Ui) -> egui::Rect {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(PREVIEW), egui::Sense::hover());
    named_control(response, "Pattern Preview").on_hover_text(
        "Reference circle is 100% spread. Wider patterns scale to fit. Exact placement varies",
    );
    rect
}

/// The edited pattern drawn into `rect`, or why it does not resolve, wrapped to the square.
/// Returns whether it resolved.
fn schematic(ui: &egui::Ui, rect: egui::Rect, edits: &Edits, inherited: Option<&Pattern>) -> bool {
    match edits.resolve(inherited) {
        Ok(pattern) => {
            let spread = f32::from_bits(edits.spread_scale_bits.unwrap_or(1.0_f32.to_bits()));
            paint_pattern(ui, rect, &pattern, spread);
            true
        }
        Err(error) => {
            let color = ui.visuals().warn_fg_color;
            let font = egui::TextStyle::Body.resolve(ui.style());
            let galley = ui.painter().layout(error, font, color, rect.width());
            ui.painter().galley(rect.min, galley, color);
            false
        }
    }
}

fn paint_pattern(ui: &egui::Ui, rect: egui::Rect, pattern: &Pattern, factor: f32) {
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let factor = if f32::from_bits(pattern.scale_bits) == 0.0 {
        0.0
    } else {
        f64::from(factor)
    };
    let extent = (f64::from(
        pattern
            .rings
            .iter()
            .map(|ring| f32::from_bits(ring.outer_radius_bits))
            .fold(0.0_f32, f32::max),
    ) * factor)
        .max(1.0);
    let scale = 64.0 * factor / extent;
    let line = egui::Stroke::new(1.0, ui.visuals().weak_text_color());
    painter.circle_stroke(center, (64.0 / extent) as f32, line);
    painter.line_segment(
        [center - egui::vec2(4.0, 0.0), center + egui::vec2(4.0, 0.0)],
        line,
    );
    painter.line_segment(
        [center - egui::vec2(0.0, 4.0), center + egui::vec2(0.0, 4.0)],
        line,
    );
    for ring in &pattern.rings {
        if ring.pellets == 0 {
            continue;
        }
        let inner = (f64::from(f32::from_bits(ring.inner_radius_bits)) * scale) as f32;
        let outer = (f64::from(f32::from_bits(ring.outer_radius_bits)) * scale) as f32;
        painter.circle_stroke(center, outer, line);
        if inner > 0.0 && inner != outer {
            painter.circle_stroke(center, inner, line);
        }
        // Mid-sector angular positions and midpoint area radii. The game supplies random
        // samples and an additional phase, so the schematic cannot promise exact hit positions.
        let count = usize::from(ring.pellets).min(256);
        let radius = ((inner * inner + outer * outer) * 0.5).sqrt();
        for i in 0..count {
            let fraction = (i as f32 + 0.5) / count as f32;
            let angle = std::f32::consts::TAU * fraction - std::f32::consts::PI
                + f32::from_bits(ring.rotation_bits);
            let point = center + egui::vec2(angle.cos(), angle.sin()) * radius;
            painter.circle_filled(point, 2.0, ui.visuals().text_color());
        }
    }
}
