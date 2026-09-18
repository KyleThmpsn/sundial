//! Small controls the program editor shares: timing fields, hex keys and float bits.

/// Width shared by the numeric controls so the timing rows line up.
pub(super) const CONTROL_WIDTH: f32 = 84.0;

/// Width of one value control that states a choice, so a pane of them reads as one column
/// beside their names rather than rows of differing length.
pub(super) const COLUMN_WIDTH: f32 = 240.0;

/// A control whose whole vocabulary is a few short words needs less room, and a row of them
/// stays readable when it does not claim the width of a sentence.
pub(super) const NARROW_COLUMN: f32 = 160.0;

/// Holds a control to a fixed width.
///
/// A combo box takes the width of its selected text: `ComboBox::width` sets a floor, not a
/// ceiling, and a vertical parent lets that text run to the edge of the pane. A fixed
/// allocation bounds the width the text may ask for, and `truncate` on the combo inside
/// makes it elide to that width rather than wrap. Neither one holds the column alone, so a
/// combo placed here should carry `.truncate()` and keep its full reading on hover.
pub(super) fn sized<R>(
    ui: &mut egui::Ui,
    width: f32,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // A narrow pane can leave a row less room than the column asks for. A control wider than
    // the line it sits on cannot wrap out of the way, so it widens its whole block instead,
    // and every labelled row inside that block then measures its label column against the
    // wider block and marches off the pane edge. Holding the control to its line keeps the
    // eliding inside the control, where `truncate` can do it.
    let width = width.min(ui.max_rect().width());
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        content,
    )
    .inner
}

/// Width of the name beside a cell. It matches the ceiling the proportional label rows
/// settle at in a wide pane, so a cell and a row in the same block share one column edge
/// and their controls start at the same place.
pub(super) const CELL_LABEL_WIDTH: f32 = 220.0;

/// The room one cell claims, so a caller can ask whether its pane affords two of them. A
/// cell cannot shrink, so a pane narrower than this is one a cell would overflow.
pub(super) fn cell_width(ui: &egui::Ui) -> f32 {
    CELL_LABEL_WIDTH + ui.spacing().item_spacing.x + COLUMN_WIDTH
}

/// One named control as a fixed width cell.
///
/// A row that measures its own label column against `ui.available_width()` cannot be used
/// inside a wrapping layout, because available width there reports the whole line, so every
/// cell claims the full row and nothing wraps. A cell of known width lets a wrapping row
/// place as many side by side as the pane affords and move the rest to the next line.
pub(super) fn cell<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let width = cell_width(ui);
    sized(ui, width, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(CELL_LABEL_WIDTH, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(CELL_LABEL_WIDTH);
                ui.add(egui::Label::new(label).halign(egui::Align::Max).truncate())
                    .on_hover_text(if hint.is_empty() {
                        label.to_owned()
                    } else {
                        format!("{label}\n{hint}")
                    });
            },
        );
        content(ui)
    })
}

/// Holds a control to the shared value column.
pub(super) fn column<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    sized(ui, COLUMN_WIDTH, content)
}

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
