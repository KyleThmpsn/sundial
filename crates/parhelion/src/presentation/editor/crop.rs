use super::super::composition::UNITS;

#[derive(Default)]
pub(super) struct Selection {
    anchor: Option<egui::Pos2>,
}

impl Selection {
    pub(super) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        texture: &egui::TextureHandle,
        crop: &mut [u16; 4],
    ) {
        let source = texture.size_vec2();
        let scale = (ui.available_width() / source.x).min(235.0 / source.y);
        let (rect, response) = ui.allocate_exact_size(source * scale, egui::Sense::drag());
        ui.painter()
            .rect_filled(rect, 0, egui::Color32::from_gray(42));
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        if response.drag_started() {
            self.anchor = response.interact_pointer_pos().map(|p| rect.clamp(p));
        }
        if response.dragged() {
            if let (Some(anchor), Some(pointer)) = (self.anchor, response.interact_pointer_pos()) {
                *crop = selection(rect, anchor, rect.clamp(pointer));
            }
        }
        if response.drag_stopped() {
            self.anchor = None;
        }
        let [x, y, w, h] = *crop;
        let point = |x: u16, y: u16| {
            rect.min + rect.size() * egui::vec2(f32::from(x), f32::from(y)) / f32::from(UNITS)
        };
        let selected = egui::Rect::from_min_max(point(x, y), point(x + w, y + h));
        let shade = egui::Color32::from_black_alpha(150);
        for outside in [
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), selected.top())),
            egui::Rect::from_min_max(egui::pos2(rect.left(), selected.bottom()), rect.max),
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), selected.top()),
                egui::pos2(selected.left(), selected.bottom()),
            ),
            egui::Rect::from_min_max(
                egui::pos2(selected.right(), selected.top()),
                egui::pos2(rect.right(), selected.bottom()),
            ),
        ] {
            ui.painter().rect_filled(outside, 0, shade);
        }
        ui.painter().rect_stroke(
            selected,
            0,
            egui::Stroke::new(2.0, egui::Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
}

fn selection(rect: egui::Rect, a: egui::Pos2, b: egui::Pos2) -> [u16; 4] {
    let from = ((a.min(b) - rect.min) / rect.size() * f32::from(UNITS)).round();
    let to = ((a.max(b) - rect.min) / rect.size() * f32::from(UNITS)).round();
    let x = (from.x as u16).min(UNITS - 1);
    let y = (from.y as u16).min(UNITS - 1);
    [
        x,
        y,
        (to.x as u16).saturating_sub(x).clamp(1, UNITS - x),
        (to.y as u16).saturating_sub(y).clamp(1, UNITS - y),
    ]
}

pub(super) fn controls(ui: &mut egui::Ui, crop: &mut [u16; 4], source: &image::RgbaImage) {
    ui.strong("Crop Selection");
    ui.weak("Select an area on the source image, or adjust its bounds here.");
    for (i, label) in ["Left", "Top", "Width", "Height"].into_iter().enumerate() {
        let max = match i {
            0 => UNITS - crop[2],
            1 => UNITS - crop[3],
            2 => UNITS - crop[0],
            _ => UNITS - crop[1],
        };
        let min = if i < 2 { 0.0 } else { 0.01 };
        let mut value = f32::from(crop[i]) / 100.0;
        ui.horizontal(|ui| {
            ui.label(label);
            if ui
                .add(
                    egui::DragValue::new(&mut value)
                        .range(min..=f32::from(max) / 100.0)
                        .speed(0.1)
                        .suffix("%"),
                )
                .changed()
            {
                crop[i] = (value * 100.0).round() as u16;
            }
        });
    }
    ui.add_space(8.0);
    if ui.button("Reset Crop").clicked() {
        *crop = [0, 0, UNITS, UNITS];
    }
    if ui.button("Trim Transparent Padding").clicked() {
        if let Some(bounds) = alpha_bounds(source) {
            *crop = bounds;
        }
    }
}

fn alpha_bounds(source: &image::RgbaImage) -> Option<[u16; 4]> {
    let (mut left, mut top, mut right, mut bottom) = (source.width(), source.height(), 0, 0);
    for (x, y, pixel) in source.enumerate_pixels() {
        if pixel[3] != 0 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    if left >= right || top >= bottom {
        return None;
    }
    let lower = |n: u32, size: u32| (n * u32::from(UNITS) / size) as u16;
    let upper = |n: u32, size: u32| (n * u32::from(UNITS)).div_ceil(size) as u16;
    let x = lower(left, source.width());
    let y = lower(top, source.height());
    Some([
        x,
        y,
        upper(right, source.width()) - x,
        upper(bottom, source.height()) - y,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversed_crop_drags_and_transparent_padding_preserve_source_bounds() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 100.0));
        let a = egui::pos2(60.0, 45.0);
        let b = egui::pos2(160.0, 95.0);
        assert_eq!(selection(rect, a, b), [2500, 2500, 5000, 5000]);
        assert_eq!(selection(rect, b, a), [2500, 2500, 5000, 5000]);
        let source = image::RgbaImage::from_fn(80, 40, |x, y| {
            image::Rgba([
                0,
                0,
                0,
                if (20..60).contains(&x) && (10..30).contains(&y) {
                    255
                } else {
                    0
                },
            ])
        });
        assert_eq!(alpha_bounds(&source), Some([2500, 2500, 5000, 5000]));
        assert_eq!(alpha_bounds(&image::RgbaImage::new(20, 20)), None);
    }
}
