//! Shared presentation for reading Sundial content. Editing workflows remain in their tools.
use std::sync::{Arc, OnceLock};

pub mod catalog;
pub mod model_preview;

/// The icon every Sundial window carries, at the size a title bar draws it. The desktop scales
/// a large icon down itself and poorly, so the windows share the 64 pixel icon; on Windows the
/// taskbar and Alt-Tab take their larger sizes from the embedded ICO once the main window
/// exists.
pub(crate) fn window_icon() -> Arc<eframe::egui::IconData> {
    static ICON: OnceLock<Arc<eframe::egui::IconData>> = OnceLock::new();
    ICON.get_or_init(|| {
        let bytes = include_bytes!("../assets/linux/io.github.kylethmpsn.Sundial-window.png");
        Arc::new(
            eframe::icon_data::from_png_bytes(bytes)
                .expect("embedded Sundial window icon must be a valid PNG"),
        )
    })
    .clone()
}

/// Phosphor alone, for an icon whose codepoint a game symbol also uses.
pub(crate) const ICON_FONT_FAMILY: &str = "Sundial Icons";

/// A Phosphor icon's font at `size`. The game's symbol fonts lead the proportional family and
/// share Phosphor's codepoints, so this goes through the Phosphor-only family when it is set up.
pub(crate) fn icon_font(ui: &eframe::egui::Ui, size: f32) -> eframe::egui::FontId {
    let family = eframe::egui::FontFamily::Name(ICON_FONT_FAMILY.into());
    if ui.fonts(|fonts| fonts.families().contains(&family)) {
        eframe::egui::FontId::new(size, family)
    } else {
        eframe::egui::FontId::proportional(size)
    }
}

/// Text for a native window title, which the desktop draws without the game's symbol fonts:
/// the private-use symbols are dropped and the spacing around them closed up.
pub(crate) fn native_title(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFD}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
