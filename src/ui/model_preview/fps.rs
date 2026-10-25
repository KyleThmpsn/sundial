//! Completed model frames, sampled without asking an idle preview to repaint.
use super::*;

#[derive(Default)]
pub(super) struct Counter {
    sample: Option<(f64, u64)>,
    value: f64,
}

impl Counter {
    pub(super) fn draw(&mut self, ui: &egui::Ui, rect: egui::Rect, enabled: bool, frames: u64) {
        if !enabled {
            *self = Self::default();
            return;
        }
        let now = ui.input(|input| input.time);
        if let Some((started, before)) = self.sample {
            let elapsed = now - started;
            if elapsed >= 0.5 {
                self.value = frames.saturating_sub(before) as f64 / elapsed;
                self.sample = Some((now, frames));
            }
        } else {
            self.sample = Some((now, frames));
        }
        let painter = ui.painter_at(rect);
        let at = rect.min + egui::vec2(8.0, 7.0);
        let font = egui::FontId::proportional(10.5);
        let text = format!("{:.0} FPS", self.value);
        painter.text(
            at,
            egui::Align2::LEFT_TOP,
            text,
            font,
            egui::Color32::from_gray(210),
        );
    }
}
