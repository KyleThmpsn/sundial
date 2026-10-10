//! Camera interaction shared by inline item and shader previews.
use super::*;

const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 8.0;
const ZOOM_STEP: f32 = 1.25;

impl Still {
    pub(super) fn interact(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if response.double_clicked() {
            self.camera = Camera::default();
        } else if response.dragged() {
            let delta = response.drag_delta();
            let panning = ui.input(|input| {
                input.modifiers.shift
                    || input.pointer.button_down(egui::PointerButton::Secondary)
                    || input.pointer.button_down(egui::PointerButton::Middle)
            });
            if panning {
                self.camera.pan[0] += delta.x / response.rect.width().max(1.0);
                self.camera.pan[1] += delta.y / response.rect.height().max(1.0);
            } else {
                self.camera.yaw += delta.x * 0.01;
                self.camera.pitch = (self.camera.pitch + delta.y * 0.01)
                    .clamp(-render::MAX_PITCH, render::MAX_PITCH);
            }
        }
        if response.hovered()
            && let Some(at) = response.hover_pos()
        {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                // The surrounding page must not scroll with the model.
                ui.ctx()
                    .input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
                let anchor = [
                    (at.x - response.rect.left()) / response.rect.width().max(1.0),
                    (at.y - response.rect.top()) / response.rect.height().max(1.0),
                ];
                self.zoom((scroll * 0.002).exp(), anchor);
            }
        }
    }

    /// Keep the model point at this normalized screen position under the zoom anchor.
    fn zoom(&mut self, factor: f32, anchor: [f32; 2]) {
        let zoom = (self.camera.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = zoom / self.camera.zoom;
        for (pan, cursor) in self.camera.pan.iter_mut().zip(anchor) {
            *pan += (cursor - 0.5 - *pan) * (1.0 - applied);
        }
        self.camera.zoom = zoom;
    }
}

/// Optional visible controls for an inline preview. The percentage is relative to its fitted
/// starting view. These controls only change camera state, never the item being authored.
pub fn zoom_controls(ui: &mut egui::Ui, id: egui::Id) {
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Still>>>(id)
            .clone()
    });
    let Ok(mut still) = state.lock() else { return };
    ui.add_enabled_ui(still.model.is_some(), |ui| {
        ui.horizontal(|ui| {
            ui.label("Zoom");
            if zoom_button(ui, "−", "Zoom Out", still.camera.zoom > MIN_ZOOM) {
                still.zoom(ZOOM_STEP.recip(), [0.5; 2]);
            }
            ui.label(format!("{:.0}%", still.camera.zoom * 100.0));
            if zoom_button(ui, "+", "Zoom In", still.camera.zoom < MAX_ZOOM) {
                still.zoom(ZOOM_STEP, [0.5; 2]);
            }
            if ui
                .add_enabled(still.camera != Camera::default(), egui::Button::new("Fit"))
                .on_hover_text("Restore the starting view.")
                .clicked()
            {
                still.camera = Camera::default();
            }
        });
    });
}

fn zoom_button(ui: &mut egui::Ui, glyph: &str, label: &str, enabled: bool) -> bool {
    let response = ui
        .add_enabled(enabled, egui::Button::new(glyph))
        .on_hover_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response.clicked()
}
