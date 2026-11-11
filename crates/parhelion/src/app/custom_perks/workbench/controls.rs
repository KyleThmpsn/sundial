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
    let width = width.min(ui.available_width()).max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        content,
    )
    .inner
}

/// One filter of a picker toolbar: a combo held to `width` that elides its selection, keeps its
/// full reading on hover and carries `name` as its accessible name. `shown` is the selection as
/// the combo draws it and `hover` its full reading. Each choice is its value, label and hint,
/// where an empty hint draws none. Returns whether the choice changed.
pub(super) fn filter<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    (salt, name): (&'static str, &str),
    width: f32,
    (shown, hover): (&str, &str),
    value: &mut T,
    choices: impl IntoIterator<Item = (T, &'static str, &'static str)>,
) -> bool {
    let before = *value;
    sized(ui, width, |ui| {
        egui::ComboBox::from_id_salt(salt)
            .width(width)
            .truncate()
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (choice, label, hint) in choices {
                    let response = ui.selectable_value(value, choice, label);
                    if !hint.is_empty() {
                        response.on_hover_text(hint);
                    }
                }
            })
            .response
            .on_hover_text(hover);
        // The same salt names the combo, so the name reaches the node the combo draws.
        crate::app::pickers::name_combo(ui, salt, name);
    });
    *value != before
}

/// A picker's search box on a line of its own, as wide as the line less the room its clear
/// button takes. `search` draws the box at the width it is given and returns whether the query
/// changed, which this returns.
pub(super) fn search_line(
    ui: &mut egui::Ui,
    search: impl FnOnce(&mut egui::Ui, f32) -> bool,
) -> bool {
    ui.horizontal(|ui| {
        let width = ui.available_width() - crate::app::pickers::CLEAR_WIDTH;
        search(ui, width)
    })
    .inner
}

/// A picker's list under its toolbar, as tall as the window leaves it. `keys` hold each row's
/// identity, so a filter change keeps the selected row.
pub(super) fn browser_list<'a>(
    ui: &egui::Ui,
    keys: &'a [u64],
    reset: bool,
    select: Option<u64>,
) -> crate::app::pickers::BrowserList<'a> {
    crate::app::pickers::BrowserList {
        keys,
        height: (ui.available_height() - 4.0).max(110.0),
        reset,
        row_height: sundial::investment::authoring_choice_row_height(ui),
        select,
    }
}

/// A picker detail pane's primary command. A double-click on the row, `activated`, counts as a
/// click where the command is enabled. `reason`, when given, is the hover text of the disabled
/// command. Returns whether the command was used.
pub(super) fn command(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    reason: Option<&str>,
    activated: bool,
) -> bool {
    let mut response = ui.add_enabled(enabled, crate::app::style::primary(ui, label));
    if let Some(reason) = reason {
        response = response.on_disabled_hover_text(reason);
    }
    response.clicked() || (activated && enabled)
}

/// Width of the name beside a cell. It matches the ceiling the proportional label rows
/// settle at in a wide pane, so a cell and a row in the same block share one column edge
/// and their controls start at the same place.
pub(super) const CELL_LABEL_WIDTH: f32 = 220.0;

/// The room one cell asks for, so a caller can ask whether its pane affords two of them.
/// A cell holds itself to its line rather than overflowing it, so a pane narrower than this
/// gets a cell with its control squeezed instead of one that runs off the edge.
pub(super) fn cell_width(ui: &egui::Ui) -> f32 {
    CELL_LABEL_WIDTH + ui.spacing().item_spacing.x + COLUMN_WIDTH
}

/// One named control as a fixed width cell.
///
/// A row that measures its own label column against `ui.available_width()` cannot be used
/// inside a wrapping layout, because available width there reports the whole line, so every
/// cell claims the full row and nothing wraps. A cell of known width lets a wrapping row
/// place as many side by side as the pane affords and move the rest to the next line.
///
/// `salt` scopes the cell's widgets. It is pushed inside the allocation, since a scope
/// around the cell would place it at the cursor without wrapping.
pub(super) fn cell<R>(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    hint: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let width = cell_width(ui);
    if ui.available_width() < width {
        // A row takes a line of its own. In a wrapping line it starts one, or it would sit
        // after a row that already fills the line and push the pane wider.
        if ui.cursor().left() > ui.max_rect().left() + 0.5 {
            ui.end_row();
        }
        return ui
            .push_id(salt, |ui| {
                super::properties::field(ui, label, hint, content)
            })
            .inner;
    }
    sized(ui, width, |ui| {
        ui.push_id(salt, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(CELL_LABEL_WIDTH, ui.spacing().interact_size.y),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_min_width(CELL_LABEL_WIDTH);
                    ui.add(crate::app::style::cut_label(ui, label).halign(egui::Align::Max))
                        .on_hover_text(if hint.is_empty() {
                            label.to_owned()
                        } else {
                            format!("{label}\n{hint}")
                        });
                },
            );
            content(ui)
        })
        .inner
    })
}

/// Holds a control to the shared value column.
pub(super) fn column<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    sized(ui, COLUMN_WIDTH, content)
}

/// A float stored as its bit pattern, so a recipe round-trips the exact native value.
/// Returns the control's response so the caller can give it an accessible name.
pub(super) fn float_field(ui: &mut egui::Ui, bits: &mut u32) -> egui::Response {
    float_field_with(ui, bits, "")
}

/// A float with its unit inside the field, as the Timing tiles read seconds, so the unit never
/// takes room beside the control or sits apart from the number it measures.
pub(super) fn float_field_with(ui: &mut egui::Ui, bits: &mut u32, unit: &str) -> egui::Response {
    let mut value = f32::from_bits(*bits);
    if !value.is_finite() {
        return ui.monospace(value.to_string());
    }
    let mut drag = egui::DragValue::new(&mut value)
        .speed(0.01)
        .clamp_existing_to_range(false)
        .custom_formatter(|value, _| format!("{:?}", value as f32));
    if !unit.trim().is_empty() {
        drag = drag.suffix(format!(" {}", unit.trim()));
    }
    let response = ui.add(drag);
    if response.changed() && value.is_finite() {
        *bits = value.to_bits();
    }
    response
}

/// A hash key edited as `0x` hexadecimal text. The stored value only changes on valid input.
/// Returns the text field's response so the caller can give it an accessible name.
pub(super) fn hex_key(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    key: &mut u32,
) -> egui::Response {
    let id = ui.make_persistent_id(("hex-key", salt));
    // Zero and the hash of an empty name both mean no key, so neither reads as digits.
    let empty = matches!(*key, 0 | 0x811C_9DC5);
    let mut text = ui
        .data_mut(|state| state.get_temp::<String>(id))
        .unwrap_or_else(|| {
            if empty {
                String::new()
            } else {
                format!("0x{key:08X}")
            }
        });
    // In a tile the control is as wide as the tile. A row keeps its own width.
    let width = ui
        .spacing()
        .interact_size
        .x
        .max(110.0)
        .min(ui.available_width());
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(width)
            .hint_text("None"),
    );
    let trimmed = text.trim();
    let digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    let parsed = if trimmed.is_empty() {
        Some(0)
    } else {
        u32::from_str_radix(digits, 16).ok()
    };
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
        ui.colored_label(ui.visuals().warn_fg_color, "Use up to 8 hex digits.");
    }
    response
}
