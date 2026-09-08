//! Independent color replacements, shared by previews and every authored texture lane.
#[cfg(test)]
use super::WeaponIconEdit;
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
}

fn is_default_range(value: &u8) -> bool {
    *value == 20
}

impl Default for IconColorReplacement {
    fn default() -> Self {
        Self {
            source: [255, 255, 255],
            replacement: [255, 255, 255],
            range_percent: 20,
        }
    }
}

impl IconColorReplacement {
    pub(super) fn is_identity(&self) -> bool {
        self.source == self.replacement
    }

    pub(super) fn weight(&self, rgb: [u8; 3]) -> i32 {
        if self.is_identity() {
            return 0;
        }
        let distance = rgb
            .iter()
            .zip(self.source)
            .map(|(a, b)| i32::from(a.abs_diff(b)))
            .max()
            .unwrap_or(0);
        let radius = i32::from(self.range_percent) * 255 / 100;
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
                    ui.label("→");
                    changed |= color_control(ui, "To", &mut replacement.replacement, index);
                    if ui.small_button("Remove").clicked() { remove = Some(index); }
                });
                changed |= ui.add(egui::Slider::new(&mut replacement.range_percent, 0..=100).text("Color range").suffix("%"))
                    .on_hover_text("0% matches the exact source color before adjustments. Increase to include nearby shades with a soft transition. Replacement colors are applied last. The strongest match wins. Replacements never chain.")
                    .changed();
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
        node.set_label(format!("Color replacement {} {label}", index + 1))
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

#[cfg(test)]
mod tests {
    use super::*;
    fn rule(source: [u8; 3], replacement: [u8; 3]) -> IconColorReplacement {
        IconColorReplacement {
            source,
            replacement,
            range_percent: 0,
        }
    }

    #[test]
    fn multiple_replacements_are_independent_and_preserve_alpha() {
        let edit = WeaponIconEdit {
            color_replacements: vec![
                rule([0, 0, 255], [255, 0, 0]),
                rule([255; 3], [255, 128, 0]),
                rule([255, 0, 0], [0, 255, 0]),
            ],
            ..Default::default()
        };
        let mut pixels = [
            0, 0, 255, 128, 255, 255, 255, 255, 255, 0, 0, 255, 0, 0, 255, 0,
        ];
        edit.apply_to_rgba8(&mut pixels).unwrap();
        assert_eq!(
            pixels,
            [
                255, 0, 0, 128, 255, 128, 0, 255, 0, 255, 0, 255, 0, 0, 255, 0
            ]
        );
    }

    #[test]
    fn matching_feathers_preserves_shading_and_ignores_unrelated_colors() {
        let replacement = IconColorReplacement {
            range_percent: 100,
            ..rule([0, 0, 255], [255, 0, 0])
        };
        let shaded = replacement.apply([0, 0, 100], [0, 0, 100], 255);
        assert_eq!(shaded, [100, 0, 0]);
        assert!(replacement.weight([0, 0, 200]) > replacement.weight([0, 0, 100]));
        assert_eq!(replacement.weight([255, 255, 0]), 0);
        let overlap = [replacement, rule([0, 0, 100], [255, 128, 0])];
        assert_eq!(apply(&overlap, [0, 0, 100], [0, 0, 100]), [255, 128, 0]);
        assert_eq!(
            rule([0; 3], [255, 128, 0]).apply([0; 3], [0; 3], 255),
            [255, 128, 0]
        );
    }

    #[test]
    fn chosen_replacement_color_is_not_transformed_by_global_adjustments() {
        for mut edit in [
            WeaponIconEdit {
                hue_shift_degrees: 120,
                ..Default::default()
            },
            WeaponIconEdit {
                saturation: -100,
                ..Default::default()
            },
            WeaponIconEdit {
                brightness: 50,
                ..Default::default()
            },
            WeaponIconEdit {
                contrast: -100,
                ..Default::default()
            },
            WeaponIconEdit {
                red_balance: -100,
                ..Default::default()
            },
            WeaponIconEdit {
                green_balance: 100,
                ..Default::default()
            },
            WeaponIconEdit {
                blue_balance: 100,
                ..Default::default()
            },
            WeaponIconEdit {
                invert: true,
                ..Default::default()
            },
        ] {
            let original = [46, 40, 40, 128, 10, 100, 200, 255, 46, 40, 40, 0];
            let mut adjusted = original;
            edit.apply_to_rgba8(&mut adjusted).unwrap();
            edit.color_replacements = vec![rule([46, 40, 40], [150, 43, 43])];
            let mut replaced = original;
            edit.apply_to_rgba8(&mut replaced).unwrap();
            assert_eq!(&replaced[..4], &[150, 43, 43, 128], "{edit:?}");
            assert_eq!(&replaced[4..], &adjusted[4..], "{edit:?}");
        }
    }

    #[test]
    fn replacement_edges_blend_adjusted_pixels_with_original_shading() {
        let mut pixels = [0, 0, 150, 200];
        let edit = WeaponIconEdit {
            color_replacements: vec![IconColorReplacement {
                range_percent: 100,
                ..rule([0, 0, 200], [150, 43, 43])
            }],
            hue_shift_degrees: 120,
            opacity_percent: 50,
            ..Default::default()
        };
        edit.apply_to_rgba8(&mut pixels).unwrap();
        // The original shade produces [113, 32, 32], blended at 229/255 over
        // the hue-adjusted base [150, 0, 0]. Opacity still applies afterward.
        assert_eq!(pixels, [117, 29, 29, 100]);
    }

    #[test]
    fn adjusted_pixels_and_replacement_targets_cannot_trigger_other_rules() {
        let mut pixels = [0, 0, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255];
        WeaponIconEdit {
            color_replacements: vec![
                rule([0, 0, 255], [255, 0, 0]),
                rule([255, 0, 0], [0, 255, 0]),
            ],
            hue_shift_degrees: 120,
            ..Default::default()
        }
        .apply_to_rgba8(&mut pixels)
        .unwrap();
        assert_eq!(pixels, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn replacements_round_trip_and_old_recipes_remain_unchanged() {
        let old: WeaponIconEdit = serde_json::from_str("{}").unwrap();
        assert!(old.is_identity());
        assert!(
            !serde_json::to_string(&old)
                .unwrap()
                .contains("color_replacements")
        );
        let mut edit = old;
        edit.color_replacements = vec![
            rule([0, 0, 255], [255, 0, 0]),
            rule([255; 3], [255, 128, 0]),
        ];
        assert!(!edit.is_identity());
        assert_eq!(
            serde_json::from_str::<WeaponIconEdit>(&serde_json::to_string(&edit).unwrap()).unwrap(),
            edit
        );
        edit.color_replacements[0].range_percent = 101;
        assert!(edit.validate().is_err());
        edit.color_replacements = vec![IconColorReplacement::default(); MAX_REPLACEMENTS + 1];
        assert!(edit.validate().is_err());
    }

    #[test]
    fn eyedropper_ignores_transparent_pixels_and_outside_clicks() {
        let primary = DecodedIconImage {
            size: [2, 1],
            rgba: vec![123, 45, 67, 255, 90, 80, 70, 0],
        };
        assert_eq!(sample(&primary, egui::vec2(0.1, 0.5)), Some([123, 45, 67]));
        assert_eq!(sample(&primary, egui::vec2(0.9, 0.5)), None);
        assert_eq!(sample(&primary, egui::vec2(1.1, 0.5)), None);
    }
}
