//! Viewer-only orbit, separate from material and skeleton playback.
use super::*;

impl Preview {
    pub(super) fn draw_rotation_option(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(&mut self.auto_rotate, "Auto Rotate")
            .on_hover_text("Orbit the camera around the model. Drag to pause rotation.")
            .changed()
        {
            self.rotation_tick = None;
        }
        ui.separator();
    }

    pub(super) fn advance_rotation(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
    ) -> bool {
        let now = ctx.input(|input| input.time);
        if !self.auto_rotate
            || response.is_pointer_button_down_on()
            || response.dragged()
            || !animating(ctx)
            || !now.is_finite()
        {
            self.rotation_tick = None;
            return false;
        }
        if let Some(last) = self.rotation_tick.replace(now) {
            let elapsed = (now - last).clamp(0.0, 0.1) as f32;
            if elapsed > 0.0 {
                let yaw = self.camera.yaw + elapsed * 15.0_f32.to_radians();
                self.camera.yaw = (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        true
    }
}

/// Whether the viewer keeps moving: while its window has focus or the pointer, and is not
/// minimized. Every window draws on one thread, so a viewer that kept repainting while another
/// window was in use took that window's frames. In the background it holds still and resumes
/// where it stood.
pub(super) fn animating(ctx: &egui::Context) -> bool {
    ctx.input(|input| {
        let viewport = input.viewport();
        !viewport.minimized.unwrap_or(false)
            && (viewport.focused.unwrap_or(true) || input.pointer.has_pointer())
    })
}
