//! Small controls the program editor shares: timing fields, hex keys and float bits.

/// Width shared by the numeric controls so the timing rows line up.
pub(super) const CONTROL_WIDTH: f32 = 84.0;

/// A millisecond field shown and edited in seconds.
pub(super) fn seconds(ui: &mut egui::Ui, label: &str, hint: &str, millis: &mut u32, minimum: u32) {
    if !label.is_empty() {
        ui.label(label).on_hover_text(hint);
    }
    let mut value = *millis as f32 / 1000.0;
    let control = ui
        .add_sized(
            [CONTROL_WIDTH, ui.spacing().interact_size.y],
            egui::DragValue::new(&mut value)
                .speed(0.05)
                .range((minimum as f32 / 1000.0)..=3600.0)
                .suffix(" s"),
        )
        .on_hover_text(hint);
    // The visible label sits beside the control without being linked to it, so the control
    // takes the label as its accessible name.
    if !label.is_empty() {
        super::pickers::name_response(ui, &control, label);
    }
    if control.changed() {
        *millis = (value * 1000.0).round() as u32;
    }
}

/// A float stored as its bit pattern, so a recipe round-trips the exact native value.
/// Returns the control's response so the caller can give it an accessible name.
pub(super) fn float_field(ui: &mut egui::Ui, bits: &mut u32) -> egui::Response {
    let mut value = f32::from_bits(*bits);
    if !value.is_finite() {
        return ui.monospace(value.to_string());
    }
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .speed(0.01)
            .clamp_existing_to_range(false)
            .custom_formatter(|value, _| format!("{:?}", value as f32)),
    );
    if response.changed() && value.is_finite() {
        *bits = value.to_bits();
    }
    response
}

/// A hash key edited as `0x` hexadecimal text. The stored value only changes on valid input.
/// Returns the text field's response so the caller can give it an accessible name.
pub(super) fn hex_key(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    key: &mut u32,
) -> egui::Response {
    let id = ui.make_persistent_id(("hex-key", salt));
    let mut text = ui
        .data_mut(|state| state.get_temp::<String>(id))
        .unwrap_or_else(|| format!("0x{key:08X}"));
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0));
    let parsed = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))
        .and_then(|digits| u32::from_str_radix(digits, 16).ok());
    if response.changed() {
        if let Some(parsed) = parsed {
            *key = parsed;
        }
        ui.data_mut(|state| state.insert_temp(id, text.clone()));
    }
    if response.lost_focus() || !response.has_focus() {
        ui.data_mut(|state| state.remove_temp::<String>(id));
    }
    if parsed.is_none() {
        ui.colored_label(ui.visuals().warn_fg_color, "Use 0x and 8 hex digits.");
    }
    response
}

/// Comma separated numbers for a stock evidence line.
pub(super) fn numbers(values: &[f32]) -> String {
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Alternatives for a stock evidence line, joined with "or".
pub(super) fn bytes(values: &[u8]) -> String {
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(" or ")
}
