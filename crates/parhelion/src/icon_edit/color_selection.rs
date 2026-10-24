//! Independent color replacements, shared by previews and every authored texture lane.
use super::preview::DecodedIconImage;
use serde::{Deserialize, Serialize};

pub(super) const MAX_REPLACEMENTS: usize = 16;

pub(super) fn is_identity(rules: &[IconColorReplacement]) -> bool {
    rules.iter().all(IconColorReplacement::is_identity)
}
pub(super) fn serialize_changes<S: serde::Serializer>(
    rules: &[IconColorReplacement],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    rules
        .iter()
        .filter(|rule| !rule.is_identity())
        .collect::<Vec<_>>()
        .serialize(serializer)
}

/// A source-color replacement blended after global color edits, matching the original artwork.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct IconColorReplacement {
    pub source: [u8; 3],
    pub replacement: [u8; 3],
    /// Maximum channel distance as a percentage of the full RGB range, with feathered edges.
    #[serde(skip_serializing_if = "is_default_range")]
    pub range_percent: u8,
    /// Match every shade that shares the source hue within this many degrees instead.
    ///
    /// Artwork shades one color across a wide brightness range, which puts its darkest and
    /// brightest pixels far apart in RGB. Matching on hue takes the whole shaded surface in one
    /// rule, and the replacement keeps carrying the source pixel's own brightness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hue_range_degrees: Option<u8>,
}

/// Chroma a pixel needs before a hue match will claim it, so greys and whites are left alone.
const MINIMUM_HUE_MATCH_CHROMA: i32 = 12;
pub(super) const MAX_HUE_RANGE_DEGREES: u8 = 180;
/// Wide enough to hold a shaded surface together, narrow enough to leave adjacent hues alone.
const DEFAULT_HUE_RANGE_DEGREES: u8 = 20;

fn is_default_range(value: &u8) -> bool {
    *value == 20
}

impl Default for IconColorReplacement {
    fn default() -> Self {
        Self {
            source: [255, 255, 255],
            replacement: [255, 255, 255],
            range_percent: 20,
            hue_range_degrees: None,
        }
    }
}

/// Returns a color's hue in degrees and its chroma, or `None` for a color with no hue at all.
fn hue_and_chroma(rgb: [u8; 3]) -> Option<(f32, i32)> {
    let max = *rgb.iter().max()?;
    let min = *rgb.iter().min()?;
    if max == min {
        return None;
    }
    let chroma = f32::from(max) - f32::from(min);
    let (r, g, b) = (f32::from(rgb[0]), f32::from(rgb[1]), f32::from(rgb[2]));
    let hue = if max == rgb[0] {
        60.0 * (((g - b) / chroma) % 6.0)
    } else if max == rgb[1] {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    Some((((hue % 360.0) + 360.0) % 360.0, chroma as i32))
}

impl IconColorReplacement {
    pub(super) fn is_identity(&self) -> bool {
        self.source == self.replacement
    }

    pub(super) fn weight(&self, rgb: [u8; 3]) -> i32 {
        if self.is_identity() {
            return 0;
        }
        let (distance, radius) = match self.hue_range_degrees {
            Some(degrees) => {
                let (Some((hue, chroma)), Some((source_hue, _))) =
                    (hue_and_chroma(rgb), hue_and_chroma(self.source))
                else {
                    return 0;
                };
                if chroma < MINIMUM_HUE_MATCH_CHROMA {
                    return 0;
                }
                let delta = (hue - source_hue).abs();
                (delta.min(360.0 - delta) as i32, i32::from(degrees))
            }
            None => {
                let distance = rgb
                    .iter()
                    .zip(self.source)
                    .map(|(a, b)| i32::from(a.abs_diff(b)))
                    .max()
                    .unwrap_or(0);
                (distance, i32::from(self.range_percent) * 255 / 100)
            }
        };
        if distance > radius {
            return 0;
        }
        let weight = if radius == 0 {
            255
        } else {
            (radius - distance) * 255 / radius
        };
        weight * weight * (765 - 2 * weight) / (255 * 255)
    }

    fn apply(&self, source_rgb: [u8; 3], adjusted_rgb: [u8; 3], weight: i32) -> [u8; 3] {
        let source_value = i32::from(*self.source.iter().max().unwrap());
        let value = i32::from(*source_rgb.iter().max().unwrap());
        std::array::from_fn(|i| {
            let target = i32::from(self.replacement[i]);
            let shaded = if source_value == 0 {
                target + value
            } else {
                (target * value + source_value / 2) / source_value
            }
            .clamp(0, 255);
            ((i32::from(adjusted_rgb[i]) * (255 - weight) + shaded * weight + 127) / 255) as u8
        })
    }
}

/// Evaluate every rule against the original pixel. Strongest match wins; earlier rules break ties.
/// Replacing blue with red therefore cannot accidentally trigger a separate red-to-orange rule.
/// Blend the source-shaded target over the adjusted pixel so its chosen color is not transformed.
pub(super) fn apply(
    replacements: &[IconColorReplacement],
    source_rgb: [u8; 3],
    adjusted_rgb: [u8; 3],
) -> [u8; 3] {
    let mut best = None;
    let mut best_weight = 0;
    for replacement in replacements {
        let weight = replacement.weight(source_rgb);
        if weight > best_weight {
            best = Some(replacement);
            best_weight = weight;
        }
    }
    best.map_or(adjusted_rgb, |replacement| {
        replacement.apply(source_rgb, adjusted_rgb, best_weight)
    })
}

pub(super) fn draw_controls(
    ui: &mut egui::Ui,
    replacements: &mut Vec<IconColorReplacement>,
    page: &mut usize,
    per_page: usize,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            super::secondary_text_color(ui),
            format!("{} / {MAX_REPLACEMENTS} replacements", replacements.len()),
        );
        if ui
            .add_enabled(
                replacements.len() < MAX_REPLACEMENTS,
                egui::Button::new("Add Replacement"),
            )
            .clicked()
        {
            replacements.push(IconColorReplacement::default());
            *page = (replacements.len() - 1) / per_page;
            changed = true;
        }
    });
    ui.colored_label(
        super::secondary_text_color(ui),
        "Pick source colors. Replacements keep their chosen color and source shading.",
    );
    let pages = replacements.len().div_ceil(per_page).max(1);
    *page = (*page).min(pages - 1);
    let mut remove = None;
    for (index, replacement) in replacements
        .iter_mut()
        .enumerate()
        .skip(*page * per_page)
        .take(per_page)
    {
        ui.push_id(("icon-color-replacement", index), |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    changed |= color_control(ui, "From", &mut replacement.source, index);
                    // A text label rather than an arrow glyph: the loaded fonts have no ⇄.
                    if ui
                        .small_button("Swap")
                        .on_hover_text("Exchange the source and replacement colors")
                        .clicked()
                    {
                        std::mem::swap(&mut replacement.source, &mut replacement.replacement);
                        changed = true;
                    }
                    changed |= color_control(ui, "To", &mut replacement.replacement, index);
                    if ui.small_button("Remove").clicked() { remove = Some(index); }
                });
                let mut by_hue = replacement.hue_range_degrees.is_some();
                if ui
                    .checkbox(&mut by_hue, "Match Every Shade")
                    .on_hover_text("Recolors every shade of this hue at once. Each pixel keeps its brightness.")
                    .changed()
                {
                    replacement.hue_range_degrees = by_hue.then_some(DEFAULT_HUE_RANGE_DEGREES);
                    changed = true;
                }
                changed |= match &mut replacement.hue_range_degrees {
                    Some(degrees) => ui
                        .add(egui::Slider::new(degrees, 0..=MAX_HUE_RANGE_DEGREES).text("Hue range").suffix("°"))
                        .on_hover_text("0° matches only the source hue. Widen to take neighbouring hues with a soft transition. Greys and near-greys are never matched by hue.")
                        .changed(),
                    None => ui
                        .add(egui::Slider::new(&mut replacement.range_percent, 0..=100).text("Color range").suffix("%"))
                        .on_hover_text("0% matches the exact source color before adjustments. Increase to include nearby shades with a soft transition. Replacement colors are applied last. The strongest match wins. Replacements never chain.")
                        .changed(),
                };
            });
        });
    }
    if let Some(index) = remove {
        replacements.remove(index);
        changed = true;
    }
    if pages > 1 {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(*page > 0, egui::Button::new("Previous"))
                .clicked()
            {
                *page -= 1;
            }
            ui.label(format!("{} / {pages}", *page + 1));
            if ui
                .add_enabled(*page + 1 < pages, egui::Button::new("Next"))
                .clicked()
            {
                *page += 1;
            }
        });
    }
    changed
}

fn color_control(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3], index: usize) -> bool {
    ui.label(label);
    let response = ui.color_edit_button_srgb(color);
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_label(format!("Color replacement {} {label}", index + 1));
    });
    response
        .on_hover_text(format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]))
        .changed()
}

/// Coordinates refer to untransformed source artwork, never background or badge pixels.
pub(super) fn sample(primary: &DecodedIconImage, uv: egui::Vec2) -> Option<[u8; 3]> {
    if primary.size.contains(&0)
        || !uv.is_finite()
        || uv.x < 0.0
        || uv.y < 0.0
        || uv.x > 1.0
        || uv.y > 1.0
    {
        return None;
    }
    let x = ((uv.x * primary.size[0] as f32) as usize).min(primary.size[0] - 1);
    let y = ((uv.y * primary.size[1] as f32) as usize).min(primary.size[1] - 1);
    let pixel = primary
        .rgba
        .get((y * primary.size[0] + x) * 4..)?
        .get(..4)?;
    (pixel[3] != 0).then(|| [pixel[0], pixel[1], pixel[2]])
}
