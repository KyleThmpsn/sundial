//! Preview preferences follow the window that owns the preview.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Display and playback defaults supplied by the window hosting a preview.
pub struct Options {
    pub show_fps: bool,
    /// Whether inline previews start playing. Full viewers retain their own transport.
    pub play_animations: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            show_fps: false,
            play_animations: true,
        }
    }
}

fn key(viewport: egui::ViewportId) -> egui::Id {
    egui::Id::new(("model-preview-options", viewport))
}

/// Applies this source window's preview choices, including viewers opened from its child windows.
pub fn set_options(ctx: &egui::Context, options: Options) {
    let key = key(ctx.viewport_id());
    ctx.data_mut(|data| data.insert_temp(key, options));
}

pub(super) fn get(ctx: &egui::Context, viewport: egui::ViewportId) -> Options {
    // Inspectors can have child windows of their own. Follow their source window's preference,
    // while the workbench can keep a different choice from Sundial's main window.
    let mut viewport = Some(viewport);
    for _ in 0..16 {
        let Some(current) = viewport else { break };
        if let Some(options) = ctx.data(|data| data.get_temp(key(current))) {
            return options;
        }
        viewport = ctx.input(|input| {
            input
                .raw
                .viewports
                .get(&current)
                .and_then(|info| info.parent)
        });
    }
    Options::default()
}
