//! The pieces every vehicle card is made of: its header, value tiles and trait toggles.
use crate::app::style;
use sundial::investment::draw_authoring_info_icon;

/// What a card's header asked for.
pub(super) enum Action {
    Reset,
    /// The preset at this index of the card's presets.
    Preset(usize),
}

/// A card's title, with its reset once the card holds a change and its presets in the card's
/// overflow menu.
pub(super) fn header(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    changed: bool,
    presets: &[(&str, &str)],
) -> Option<Action> {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong())
            .on_hover_text(hint);
        let mut action =
            (changed && style::reset_icon(ui, &format!("Reset {title}"))).then_some(Action::Reset);
        if !presets.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                style::more_menu(ui, title, |ui| {
                    style::workbench_style(ui);
                    for (index, (label, hint)) in presets.iter().enumerate() {
                        if ui.button(*label).on_hover_text(*hint).clicked() {
                            action = Some(Action::Preset(index));
                            ui.close();
                        }
                    }
                });
            });
        }
        action
    })
    .inner
}

pub(super) fn info(ui: &mut egui::Ui, label: &str, help: &str) {
    style::named_control(draw_authoring_info_icon(ui, help), format!("{label} Info"));
}

/// One percentage of the vehicle's own value as a tile, 100% being its own.
pub(super) fn percent(
    ui: &mut egui::Ui,
    width: f32,
    (label, hint): (&str, &str),
    (value, minimum): (&mut u16, u16),
) {
    let current = *value;
    let original = (current != 100).then_some("100%");
    let (edited, reset) = style::stock_tile(ui, (width, label), (label, hint), original, |ui| {
        let mut next = current;
        let response = ui.add_sized(
            [width, ui.spacing().interact_size.y],
            egui::DragValue::new(&mut next)
                .range(minimum..=1000)
                .speed(5.0)
                .suffix("%"),
        );
        style::named_control(response, label);
        (next != current).then_some(next)
    });
    if let Some(next) = edited {
        *value = next;
    } else if reset {
        *value = 100;
    }
}

/// A stock trait the vehicle can add beside the item's perks. `enabled` is false for a trait
/// only a Sparrow takes while another vehicle is summoned.
pub(super) fn trait_toggle(
    ui: &mut egui::Ui,
    value: &mut bool,
    (label, hint): (&str, &str),
    enabled: bool,
) {
    let response = ui
        .add_enabled(enabled, egui::Checkbox::new(value, label))
        .on_hover_text(hint)
        .on_disabled_hover_text("Sparrows only");
    style::named_control(response, label);
}

/// A card's one line when the summoned vehicle has nothing there to change.
pub(super) fn unavailable(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(style::secondary(ui.visuals())));
}
