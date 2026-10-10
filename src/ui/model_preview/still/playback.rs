//! A per-model transport choice over the source window's playback default.
use super::*;

impl Still {
    pub(super) fn playback_enabled(&mut self, ctx: &egui::Context) -> bool {
        let default = options::get(ctx, ctx.viewport_id()).play_animations;
        if self.autoplay != Some(default) {
            self.autoplay = Some(default);
            self.playing = None;
            self.last_tick = None;
        }
        self.playing.unwrap_or(default)
    }
}

pub(in crate::ui::model_preview) fn control(ui: &mut egui::Ui, id: egui::Id, over: egui::Rect) {
    let Some(state) = ui.ctx().data(|data| data.get_temp::<Arc<Mutex<Still>>>(id)) else {
        return;
    };
    let Ok(mut still) = state.lock() else {
        return;
    };
    if !ui.rect_contains_pointer(over)
        || !still.model.as_ref().is_some_and(|model| {
            model.has_animation() || model.has_cloth() || model.has_shader_animation()
        })
    {
        return;
    }
    let playing = still.playback_enabled(ui.ctx());
    let (icon, label) = if playing {
        (egui_phosphor::regular::PAUSE, "Pause")
    } else {
        (egui_phosphor::regular::PLAY, "Play")
    };
    let response = window::corner_button(
        ui,
        window::corner_rect(over, 1),
        id.with("playback"),
        icon,
        label,
        false,
    );
    if response.clicked() {
        still.playing = Some(!playing);
        still.last_tick = None;
        ui.ctx().request_repaint();
    }
}
