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

pub(super) fn inline_right_arrow(ui: &mut egui::Ui, color: egui::Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(13.0, ui.spacing().interact_size.y),
        egui::Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        let pixels_per_point = ui.ctx().pixels_per_point();
        let stroke = pixel_fitted_stroke(egui::Stroke::new(1.5, color), pixels_per_point);
        let point = |point| pixel_snap_stroke_point(point, stroke, pixels_per_point);
        let tip = point(egui::pos2(rect.right() - 1.5, rect.center().y));
        let tail = point(egui::pos2(rect.left() + 1.5, rect.center().y));
        ui.painter().line_segment([tail, tip], stroke);
        ui.painter()
            .line_segment([point(egui::pos2(tip.x - 4.0, tip.y - 3.0)), tip], stroke);
        ui.painter()
            .line_segment([point(egui::pos2(tip.x - 4.0, tip.y + 3.0)), tip], stroke);
    }
    response
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
