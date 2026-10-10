//! Shared presentation for reading Sundial content. Editing workflows remain in their tools.
use std::sync::{Arc, OnceLock};

pub mod catalog;
pub(crate) mod help;
pub mod model_preview;

/// The icon data for Sundial's native windows, including hosted tools.
///
/// On Windows, empty data leaves the window's icon unset so it uses the class icons installed
/// from the embedded ICO during startup. Those have separate small and large sizes. Supplying
/// a PNG overrides the small class icon, which Windows then scales down for the title bar.
/// Other platforms share the 64 pixel PNG.
pub fn window_icon() -> Arc<eframe::egui::IconData> {
    static ICON: OnceLock<Arc<eframe::egui::IconData>> = OnceLock::new();
    ICON.get_or_init(|| {
        #[cfg(windows)]
        let icon = eframe::egui::IconData::default();
        #[cfg(not(windows))]
        let icon = eframe::icon_data::from_png_bytes(include_bytes!(
            "../assets/linux/io.github.kylethmpsn.Sundial-window.png"
        ))
        .expect("embedded Sundial window icon must be a valid PNG");
        Arc::new(icon)
    })
    .clone()
}

/// Phosphor alone, for an icon whose codepoint a game symbol also uses.
pub(crate) const ICON_FONT_FAMILY: &str = "Sundial Icons";

/// A Phosphor icon's font at `size`. The game's symbol fonts lead the proportional family and
/// share Phosphor's codepoints, so this goes through the Phosphor-only family when it is set up.
pub(crate) fn icon_font(ui: &eframe::egui::Ui, size: f32) -> eframe::egui::FontId {
    let family = eframe::egui::FontFamily::Name(ICON_FONT_FAMILY.into());
    if ui.fonts_mut(|fonts| fonts.families().contains(&family)) {
        eframe::egui::FontId::new(size, family)
    } else {
        eframe::egui::FontId::proportional(size)
    }
}

/// An amber notice with an information icon, a `title` and a wrapped `message`, across the width
/// it is given, as the JSON Editor's account notice and the ability editor's use it.
pub fn notice(ui: &mut eframe::egui::Ui, title: &str, message: &str) {
    use eframe::egui;
    let (background, border, foreground) = if ui.visuals().dark_mode {
        (
            egui::Color32::from_rgb(55, 40, 26),
            egui::Color32::from_rgb(102, 72, 39),
            egui::Color32::from_rgb(245, 215, 177),
        )
    } else {
        (
            egui::Color32::from_rgb(255, 240, 221),
            egui::Color32::from_rgb(220, 181, 131),
            egui::Color32::from_rgb(104, 60, 16),
        )
    };
    egui::Frame::NONE
        .fill(background)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.visuals_mut().override_text_color = Some(foreground);
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.horizontal_top(|ui| {
                ui.label(
                    egui::RichText::new(egui_phosphor::regular::INFO).font(icon_font(ui, 18.0)),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.strong(title);
                    ui.add(egui::Label::new(message).wrap());
                });
            });
        });
    ui.add_space(8.0);
}

/// A label for `text` cut to the room left on `ui`, which shows no tooltip of its own. egui's label
/// adds its full text as a tooltip once it cuts the text, so a cut label given a hover of ours
/// showed two. The galley is cut here and handed over marked whole, so the hover the caller adds,
/// which should hold the full text, is the only one. Add it with `ui.add`.
pub fn cut_label(
    ui: &eframe::egui::Ui,
    text: impl Into<eframe::egui::WidgetText>,
) -> eframe::egui::Label {
    cut_label_within(ui, text, ui.available_width())
}

/// [`cut_label`] cut to `width`, for a label added in a cell of that width.
pub fn cut_label_within(
    ui: &eframe::egui::Ui,
    text: impl Into<eframe::egui::WidgetText>,
    width: f32,
) -> eframe::egui::Label {
    use eframe::egui;
    let galley = text.into().into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        width,
        egui::TextStyle::Body,
    );
    let mut galley = (*galley).clone();
    galley.elided = false;
    egui::Label::new(Arc::new(galley))
}

/// Text for a native window title, which the desktop draws without the game's symbol fonts:
/// the private-use symbols are dropped and the spacing around them closed up.
/// A dropdown under `anchor` whose open state lives in egui's memory, so a click on the anchor
/// toggles it with `egui::Popup::toggle_id` and a chosen row closes it with `close_all`.
pub fn dropdown(
    anchor: &eframe::egui::Response,
    id: eframe::egui::Id,
) -> eframe::egui::Popup<'static> {
    eframe::egui::Popup::from_response(anchor)
        .id(id)
        .open_memory(None)
}

/// A menu that stays open while its own controls are used and closes on a click elsewhere.
/// egui's menus close on any click since 0.32, which suits lists of actions but not a menu
/// holding a search field, a value or a checkbox.
pub(crate) fn sticky_menu_button<'a, R>(
    ui: &mut eframe::egui::Ui,
    title: impl eframe::egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut eframe::egui::Ui) -> R,
) -> eframe::egui::InnerResponse<Option<R>> {
    use eframe::egui::containers::menu::{MenuButton, MenuConfig};
    let (response, inner) = MenuButton::new(title)
        .config(
            MenuConfig::new().close_behavior(eframe::egui::PopupCloseBehavior::CloseOnClickOutside),
        )
        .ui(ui, add_contents);
    eframe::egui::InnerResponse::new(inner.map(|inner| inner.inner), response)
}

pub(crate) fn native_title(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFD}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
