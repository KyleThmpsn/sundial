use std::sync::OnceLock;

use eframe::egui;
use serde::Deserialize;

const CHEVRON_ASSETS: &str = include_str!("../../assets/glyphs/chevrons.json");
const ACTION_ASSETS: &str = include_str!("../../assets/glyphs/actions.json");
const GLYPH_VIEWBOX: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    ChevronUp,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Trash,
    Lock,
    Unlock,
}

#[derive(Deserialize)]
struct ChevronAssets {
    up: [[f32; 2]; 3],
    down: [[f32; 2]; 3],
    left: [[f32; 2]; 3],
    right: [[f32; 2]; 3],
}

impl ChevronAssets {
    fn points(&self, glyph: Glyph) -> &[[f32; 2]; 3] {
        match glyph {
            Glyph::ChevronUp => &self.up,
            Glyph::ChevronDown => &self.down,
            Glyph::ChevronLeft => &self.left,
            Glyph::ChevronRight => &self.right,
            Glyph::Trash | Glyph::Lock | Glyph::Unlock => {
                unreachable!("action glyph requested from chevron assets")
            }
        }
    }
}

#[derive(Deserialize)]
struct ActionAssets {
    view_box: f32,
    trash: VectorGlyph,
    lock: VectorGlyph,
    unlock: VectorGlyph,
}

#[derive(Deserialize)]
struct VectorGlyph {
    #[serde(default)]
    minimum_stroke: f32,
    paths: Vec<Vec<[f32; 2]>>,
    #[serde(default)]
    segments: Vec<[[f32; 2]; 2]>,
    #[serde(default)]
    rounded_rects: Vec<VectorRoundedRect>,
    #[serde(default)]
    filled_rounded_rects: Vec<VectorRoundedRect>,
}

#[derive(Deserialize)]
struct VectorRoundedRect {
    min: [f32; 2],
    max: [f32; 2],
    corner_radius: f32,
}

impl ActionAssets {
    fn glyph(&self, glyph: Glyph) -> &VectorGlyph {
        match glyph {
            Glyph::Trash => &self.trash,
            Glyph::Lock => &self.lock,
            Glyph::Unlock => &self.unlock,
            Glyph::ChevronUp | Glyph::ChevronDown | Glyph::ChevronLeft | Glyph::ChevronRight => {
                unreachable!("chevron requested from action glyph assets")
            }
        }
    }
}

fn chevrons() -> &'static ChevronAssets {
    static ASSETS: OnceLock<ChevronAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        serde_json::from_str(CHEVRON_ASSETS).expect("bundled chevron glyphs must be valid")
    })
}

fn actions() -> &'static ActionAssets {
    static ASSETS: OnceLock<ActionAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        serde_json::from_str(ACTION_ASSETS).expect("bundled action glyphs must be valid")
    })
}

pub(super) fn paint(ui: &egui::Ui, rect: egui::Rect, glyph: Glyph) {
    paint_with_stroke(
        ui,
        rect,
        glyph,
        egui::Stroke::new(1.4, ui.visuals().text_color()),
    );
}

pub(super) fn paint_with_stroke(
    ui: &egui::Ui,
    rect: egui::Rect,
    glyph: Glyph,
    stroke: egui::Stroke,
) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let rect = pixel_fitted_square(rect, pixels_per_point);
    if matches!(
        glyph,
        Glyph::ChevronUp | Glyph::ChevronDown | Glyph::ChevronLeft | Glyph::ChevronRight
    ) {
        let stroke = pixel_fitted_stroke(stroke, pixels_per_point);
        let points = chevrons().points(glyph).map(|[x, y]| {
            pixel_snap_stroke_point(
                egui::pos2(
                    rect.left() + x / GLYPH_VIEWBOX * rect.width(),
                    rect.top() + y / GLYPH_VIEWBOX * rect.height(),
                ),
                stroke,
                pixels_per_point,
            )
        });
        ui.painter().add(egui::Shape::line(points.to_vec(), stroke));
        return;
    }

    let assets = actions();
    let geometry = assets.glyph(glyph);
    let stroke = pixel_fitted_stroke(
        egui::Stroke::new(stroke.width.max(geometry.minimum_stroke), stroke.color),
        pixels_per_point,
    );
    let point = |[x, y]: [f32; 2]| {
        pixel_snap_stroke_point(
            egui::pos2(
                rect.left() + x / assets.view_box * rect.width(),
                rect.top() + y / assets.view_box * rect.height(),
            ),
            stroke,
            pixels_per_point,
        )
    };
    let filled_point = |[x, y]: [f32; 2]| {
        pixel_snap_fill_point(
            egui::pos2(
                rect.left() + x / assets.view_box * rect.width(),
                rect.top() + y / assets.view_box * rect.height(),
            ),
            pixels_per_point,
        )
    };

    for path in &geometry.paths {
        ui.painter().add(egui::Shape::line(
            path.iter().copied().map(point).collect(),
            stroke,
        ));
    }
    for segment in &geometry.segments {
        ui.painter().line_segment(segment.map(point), stroke);
    }
    let corner_radius = |rounded_rect: &VectorRoundedRect| {
        egui::CornerRadius::same(
            (rounded_rect.corner_radius * rect.width() / assets.view_box).round() as u8,
        )
    };
    for rounded_rect in &geometry.rounded_rects {
        ui.painter().rect_stroke(
            egui::Rect::from_min_max(point(rounded_rect.min), point(rounded_rect.max)),
            corner_radius(rounded_rect),
            stroke,
            egui::StrokeKind::Middle,
        );
    }
    for rounded_rect in &geometry.filled_rounded_rects {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                filled_point(rounded_rect.min),
                filled_point(rounded_rect.max),
            ),
            corner_radius(rounded_rect),
            stroke.color,
        );
    }
}

fn pixel_fitted_square(rect: egui::Rect, pixels_per_point: f32) -> egui::Rect {
    let side_pixels = (rect.width().min(rect.height()) * pixels_per_point)
        .round()
        .max(1.0);
    let center_pixels = rect.center().to_vec2() * pixels_per_point;
    let min_pixels = (center_pixels - egui::vec2(side_pixels, side_pixels) * 0.5).round();
    egui::Rect::from_min_size(
        egui::pos2(
            min_pixels.x / pixels_per_point,
            min_pixels.y / pixels_per_point,
        ),
        egui::vec2(side_pixels, side_pixels) / pixels_per_point,
    )
}

fn pixel_fitted_stroke(stroke: egui::Stroke, pixels_per_point: f32) -> egui::Stroke {
    let width_pixels = (stroke.width * pixels_per_point).round().max(1.0);
    egui::Stroke::new(width_pixels / pixels_per_point, stroke.color)
}

fn pixel_snap_stroke_point(
    point: egui::Pos2,
    stroke: egui::Stroke,
    pixels_per_point: f32,
) -> egui::Pos2 {
    let width_pixels = (stroke.width * pixels_per_point).round() as i32;
    let offset = if width_pixels % 2 == 0 { 0.0 } else { 0.5 };
    let snap =
        |value: f32| ((value * pixels_per_point - offset).round() + offset) / pixels_per_point;
    egui::pos2(snap(point.x), snap(point.y))
}

fn pixel_snap_fill_point(point: egui::Pos2, pixels_per_point: f32) -> egui::Pos2 {
    egui::pos2(
        (point.x * pixels_per_point).round() / pixels_per_point,
        (point.y * pixels_per_point).round() / pixels_per_point,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_chevrons_are_bounded_and_point_in_the_named_direction() {
        let assets = chevrons();
        for points in [&assets.up, &assets.down, &assets.left, &assets.right] {
            assert!(
                points
                    .iter()
                    .flatten()
                    .all(|value| (0.0..=24.0).contains(value))
            );
        }
        assert!(assets.up[1][1] < assets.up[0][1]);
        assert!(assets.down[1][1] > assets.down[0][1]);
        assert!(assets.left[1][0] < assets.left[0][0]);
        assert!(assets.right[1][0] > assets.right[0][0]);
    }

    #[test]
    fn bundled_action_variants_have_valid_bounded_geometry() {
        let assets = actions();
        assert_eq!(assets.view_box, GLYPH_VIEWBOX);
        for glyph in [Glyph::Trash, Glyph::Lock, Glyph::Unlock] {
            let geometry = assets.glyph(glyph);
            assert!(geometry.minimum_stroke.is_finite() && geometry.minimum_stroke >= 0.0);
            assert!(
                !geometry.paths.is_empty()
                    || !geometry.segments.is_empty()
                    || !geometry.rounded_rects.is_empty(),
                "{glyph:?} has no stroked geometry"
            );
            assert!(geometry.paths.iter().all(|path| path.len() >= 2));
            assert!(
                geometry
                    .paths
                    .iter()
                    .flatten()
                    .chain(geometry.segments.iter().flatten())
                    .flatten()
                    .all(|value| { value.is_finite() && (0.0..=assets.view_box).contains(value) })
            );
            assert!(
                geometry
                    .rounded_rects
                    .iter()
                    .chain(&geometry.filled_rounded_rects)
                    .all(|rounded_rect| {
                        rounded_rect.corner_radius >= 0.0
                            && rounded_rect.min[0] <= rounded_rect.max[0]
                            && rounded_rect.min[1] <= rounded_rect.max[1]
                            && rounded_rect
                                .min
                                .iter()
                                .chain(&rounded_rect.max)
                                .all(|value| (0.0..=assets.view_box).contains(value))
                    })
            );
        }
    }

    #[test]
    fn trash_glyph_is_simple_open_and_bilaterally_symmetric() {
        let trash = &actions().trash;
        assert!(
            trash.segments.is_empty(),
            "tiny trash glyph must not contain uneven interior strokes"
        );
        assert_eq!(trash.paths.len(), 2, "trash needs only a lid and body");
        assert_eq!(trash.paths[0].len(), 2, "lid must be a single line");
        assert_eq!(trash.paths[1].len(), 4, "body must remain open at the top");
        assert_ne!(trash.paths[1][0], trash.paths[1][3]);
        assert!(trash.rounded_rects.is_empty());
        assert_eq!(trash.filled_rounded_rects.len(), 1);

        let mirrored = |left: [f32; 2], right: [f32; 2]| {
            assert_eq!(left[0] + right[0], GLYPH_VIEWBOX);
            assert_eq!(left[1], right[1]);
        };
        mirrored(trash.paths[0][0], trash.paths[0][1]);
        mirrored(trash.paths[1][0], trash.paths[1][3]);
        mirrored(trash.paths[1][1], trash.paths[1][2]);
        let handle = &trash.filled_rounded_rects[0];
        assert_eq!(handle.min[0] + handle.max[0], GLYPH_VIEWBOX);
        assert_eq!(handle.corner_radius * 2.0, handle.max[1] - handle.min[1]);
    }

    #[test]
    fn sundial_lock_states_are_solid_and_visibly_distinct_at_ten_pixels() {
        let assets = actions();
        let lock = assets.glyph(Glyph::Lock);
        let unlock = assets.glyph(Glyph::Unlock);

        for geometry in [lock, unlock] {
            assert!(geometry.minimum_stroke >= 1.25);
            assert_eq!(geometry.filled_rounded_rects.len(), 1);
            let body = &geometry.filled_rounded_rects[0];
            let rendered_width = (body.max[0] - body.min[0]) / assets.view_box * 10.0;
            let rendered_height = (body.max[1] - body.min[1]) / assets.view_box * 10.0;
            assert!((5.0..=7.0).contains(&rendered_width));
            assert!((4.0..=5.0).contains(&rendered_height));
            assert_eq!(body.min[0] + body.max[0], assets.view_box);
        }

        assert_ne!(lock.paths, unlock.paths);
        assert_eq!(lock.rounded_rects.len(), 1);
        assert!(unlock.rounded_rects.is_empty());
        let shackle = &lock.rounded_rects[0];
        assert_eq!(shackle.min[0] + shackle.max[0], assets.view_box);
        assert_eq!(shackle.max[0] - shackle.min[0], 8.0);
        assert_eq!(shackle.corner_radius, 4.0);
        let open_end = unlock.paths[0].last().expect("unlock has a shackle");
        let rendered_gap = (10.8 - open_end[1]) / assets.view_box * 10.0;
        assert!(rendered_gap >= 1.0, "unlock needs a visible gap");
    }
}
