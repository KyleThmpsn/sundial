//! Or, And and Not on a condition's line.
use super::*;

/// The width Or… and And… take at the end of a condition's line, with the space before them.
pub(super) fn logic_width(ui: &egui::Ui, and: bool) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let spacing = ui.spacing();
    let command = |label: &str| {
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        }) + spacing.button_padding.x * 2.0
            + spacing.item_spacing.x
    };
    8.0 + command("Or…") + if and { command("And…") } else { 0.0 }
}

/// Or adds an alternative to the list. And requires another condition beside the whole list.
/// They read quietly beside the condition, which is what the line is about.
pub(super) fn logic_commands(
    ui: &mut egui::Ui,
    pick: &mut NativePicker<'_>,
    list: &List,
    and: bool,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    ui.scope(|ui| {
        crate::app::style::quiet(ui);
        // An alternative is one condition, so it comes from the condition picker. A trigger
        // preset there added only its first condition and dropped the ending it promised.
        let added = ui
            .push_id((&list.owner, list.field, "add-condition"), |ui| {
                pick_condition(pick, ui, "Or…")
            })
            .inner;
        if let Some(node) = added {
            *pending = Some((list.clone(), Edit::Add(node)));
        }
        if !and {
            return Ok(());
        }
        let required = ui
            .push_id((&list.owner, list.field, "require-condition"), |ui| {
                pick_condition(pick, ui, "And…")
            })
            .inner;
        if let Some(node) = required {
            *pending = Some((list.clone(), Edit::Require(node)));
        }
        Ok(())
    })
    .inner
}

/// A condition's title beside its Not box. The box shows the inversion, so the title reads
/// the test itself. An inverted empty check read "Not" checked beside "Never", and an
/// inverted state "Not" beside "While Not in" the state.
pub(super) fn uninverted_title(condition: &DecodedCondition) -> String {
    if matches!(condition.class, 0x80803DCE | 0x80803DCC)
        && condition.native.get(0xF8).is_some_and(|flag| *flag != 0)
    {
        let mut plain = condition.clone();
        plain.native[0xF8] = 0;
        return super::super::decoded_condition_title(&plain);
    }
    super::super::decoded_condition_title(condition)
}

/// The general predicates carry a flag at +F8 that inverts their result.
pub(in crate::app::custom_perks::workbench::program) fn negation(
    ui: &mut egui::Ui,
    block: &mut native::Block,
) {
    if !matches!(block.class, 0x80803DCE | 0x80803DCC) {
        return;
    }
    let Some(flag) = block.bytes.get_mut(0xF8) else {
        return;
    };
    let mut inverted = *flag != 0;
    if ui
        .checkbox(&mut inverted, "Not")
        .on_hover_text("Passes when this condition fails")
        .changed()
    {
        *flag = u8::from(inverted);
    }
}
