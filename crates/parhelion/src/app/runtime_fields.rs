//! Focused runtime fields controls; recipe mutation occurs on user actions.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn draw_runtime_value_override_field(
    ui: &mut egui::Ui,
    field: &WeaponRuntimeField,
    overrides: &mut Vec<WeaponRuntimeValueOverride>,
    text_state: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) {
    let override_index = overrides
        .iter()
        .position(|value| value.locator == field.locator);
    if !runtime_field_is_editable(field) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::Label::new(&field.path_label)
                    .selectable(true)
                    .truncate(),
            )
            .on_hover_text(runtime_field_tooltip(field));
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Saved field is not supported by the runtime encoder",
            );
            if let Some(index) = override_index
                && ui.small_button("Remove").clicked()
            {
                overrides.remove(index);
                text_state.retain(|(locator, _), _| locator != &field.locator);
            }
        });
        return;
    }
    let current_value = override_index
        .map(|index| overrides[index].value.clone())
        .unwrap_or_else(|| field.value.clone());
    let compatible = encode_weapon_runtime_value(&field.kind, &current_value).is_ok();
    let shown_value = if compatible {
        current_value
    } else {
        field.value.clone()
    };
    let mut next_value = None;
    let mut reset = false;
    let can_reset = override_index.is_some()
        || text_state
            .keys()
            .any(|(locator, _)| locator == &field.locator);
    let complex = matches!(
        field.kind,
        WeaponRuntimeValueKind::Vector4Float32 | WeaponRuntimeValueKind::FixedBytes { .. }
    );
    match RuntimeEditorLayout::choose(ui.available_width(), complex) {
        RuntimeEditorLayout::Inline => {
            ui.horizontal(|ui| {
                let label_width = (ui.available_width() * 0.42).clamp(220.0, 360.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(label_width, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(label_width);
                        ui.add(
                            egui::Label::new(&field.path_label)
                                .selectable(true)
                                .truncate(),
                        )
                        .on_hover_text(runtime_field_tooltip(field));
                    },
                );
                next_value = draw_runtime_value_editor(
                    ui,
                    &field.locator,
                    &field.kind,
                    &shown_value,
                    text_state,
                );
                if can_reset && ui.small_button("Reset").clicked() {
                    reset = true;
                }
            });
        }
        RuntimeEditorLayout::Stacked => {
            ui.add(
                egui::Label::new(&field.path_label)
                    .selectable(true)
                    .truncate(),
            )
            .on_hover_text(runtime_field_tooltip(field));
            ui.horizontal_wrapped(|ui| {
                next_value = draw_runtime_value_editor(
                    ui,
                    &field.locator,
                    &field.kind,
                    &shown_value,
                    text_state,
                );
            });
            if can_reset && ui.small_button("Reset to Donor").clicked() {
                reset = true;
            }
        }
    }
    if !compatible {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "The saved value is invalid for this field's type, size, or range. Edit it or reset to the donor value.",
        );
    }
    if reset {
        if let Some(index) = override_index {
            overrides.remove(index);
        }
        text_state.retain(|(locator, _), _| locator != &field.locator);
    } else if let Some(value) = next_value {
        if let Some(index) = override_index {
            overrides[index].value = value;
        } else {
            overrides.push(WeaponRuntimeValueOverride {
                locator: field.locator.clone(),
                value,
            });
        }
    }
}

pub(super) fn runtime_value_kind_label(kind: &WeaponRuntimeValueKind) -> String {
    match kind {
        WeaponRuntimeValueKind::Boolean => "Boolean".to_owned(),
        WeaponRuntimeValueKind::SignedInteger { bits } => format!("Signed {bits}-bit integer"),
        WeaponRuntimeValueKind::UnsignedInteger { bits } => {
            format!("Unsigned {bits}-bit integer")
        }
        WeaponRuntimeValueKind::Enum { bits } => format!("{bits}-bit enum"),
        WeaponRuntimeValueKind::BitFlags { bits } => format!("{bits}-bit flags"),
        WeaponRuntimeValueKind::HexIdentifier { bits } => format!("{bits}-bit identifier"),
        WeaponRuntimeValueKind::Float32 => "32-bit float".to_owned(),
        WeaponRuntimeValueKind::Vector4Float32 => "Four 32-bit floats".to_owned(),
        WeaponRuntimeValueKind::FixedBytes { size } => format!("{size} exact bytes"),
    }
}

pub(super) fn draw_runtime_value_editor(
    ui: &mut egui::Ui,
    locator: &WeaponRuntimeFieldLocator,
    kind: &WeaponRuntimeValueKind,
    current: &WeaponRuntimeValue,
    text_state: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) -> Option<WeaponRuntimeValue> {
    let value_id = ui.id().with(("runtime-editor-current-value", locator));
    let previous = ui.data(|data| data.get_temp::<WeaponRuntimeValue>(value_id));
    if previous.as_ref().is_some_and(|value| value != current) {
        // An external edit must replace old display text. An unfinished draft is
        // preserved while its underlying value remains unchanged.
        text_state.retain(|(candidate, _), _| candidate != locator);
    }
    let next = draw_runtime_value_editor_contents(ui, locator, kind, current, text_state);
    ui.data_mut(|data| {
        data.insert_temp(value_id, next.as_ref().unwrap_or(current).clone());
    });
    next
}

fn draw_runtime_value_editor_contents(
    ui: &mut egui::Ui,
    locator: &WeaponRuntimeFieldLocator,
    kind: &WeaponRuntimeValueKind,
    current: &WeaponRuntimeValue,
    text_state: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) -> Option<WeaponRuntimeValue> {
    match (kind, current) {
        (WeaponRuntimeValueKind::Boolean, WeaponRuntimeValue::Boolean(current)) => {
            let mut value = *current;
            ui.checkbox(&mut value, "")
                .changed()
                .then_some(WeaponRuntimeValue::Boolean(value))
        }
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 64 },
            WeaponRuntimeValue::Signed(current),
        ) => {
            let text = text_state
                .entry((locator.clone(), 0))
                .or_insert_with(|| current.to_string());
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(184.0),
            );
            let parsed = text.trim().parse::<i64>().ok();
            if parsed.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Invalid signed 64-bit integer. Not applied.",
                );
            }
            response
                .changed()
                .then_some(parsed)
                .flatten()
                .map(WeaponRuntimeValue::Signed)
        }
        (WeaponRuntimeValueKind::SignedInteger { .. }, WeaponRuntimeValue::Signed(current)) => {
            let mut value = *current;
            let (minimum, maximum) = kind.signed_range().unwrap_or((i64::MIN, i64::MAX));
            ui.add(
                egui::DragValue::new(&mut value)
                    .range(minimum..=maximum)
                    .speed(1),
            )
            .changed()
            .then_some(WeaponRuntimeValue::Signed(value))
        }
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits: 64 }
            | WeaponRuntimeValueKind::Enum { bits: 64 }
            | WeaponRuntimeValueKind::BitFlags { bits: 64 },
            WeaponRuntimeValue::Unsigned(current),
        ) => {
            let text = text_state
                .entry((locator.clone(), 0))
                .or_insert_with(|| current.to_string());
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(184.0),
            );
            let parsed = text.trim().parse::<u64>().ok();
            ui.monospace(format!("0x{:X}", parsed.unwrap_or(*current)));
            if parsed.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Invalid unsigned 64-bit integer. Not applied.",
                );
            }
            response
                .changed()
                .then_some(parsed)
                .flatten()
                .map(WeaponRuntimeValue::Unsigned)
        }
        (
            WeaponRuntimeValueKind::UnsignedInteger { .. }
            | WeaponRuntimeValueKind::Enum { .. }
            | WeaponRuntimeValueKind::BitFlags { .. },
            WeaponRuntimeValue::Unsigned(current),
        ) => {
            let mut value = *current;
            let maximum = kind.unsigned_maximum().unwrap_or(u64::MAX);
            let changed = ui
                .add(egui::DragValue::new(&mut value).range(0..=maximum).speed(1))
                .changed();
            ui.monospace(format!("0x{value:X}"));
            changed.then_some(WeaponRuntimeValue::Unsigned(value))
        }
        (WeaponRuntimeValueKind::HexIdentifier { bits }, WeaponRuntimeValue::Unsigned(current)) => {
            let width = usize::from(*bits / 4);
            let text = text_state
                .entry((locator.clone(), 0))
                .or_insert_with(|| format!("0x{current:0width$X}"));
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(150.0),
            );
            let parsed = parse_runtime_hex_u64(text)
                .filter(|value| *value <= kind.unsigned_maximum().unwrap_or(u64::MAX));
            if parsed.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Invalid identifier; not applied",
                );
            }
            response
                .changed()
                .then_some(parsed)
                .flatten()
                .map(WeaponRuntimeValue::Unsigned)
        }
        (WeaponRuntimeValueKind::Float32, WeaponRuntimeValue::Float32Bits(current)) => {
            let edited_bits = draw_runtime_float_decimal(ui, *current);
            let text = text_state
                .entry((locator.clone(), 0))
                .or_insert_with(|| format!("0x{current:08X}"));
            if let Some(bits) = edited_bits {
                *text = format!("0x{bits:08X}");
            }
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(98.0),
            );
            response
                .clone()
                .on_hover_text("Exact IEEE-754 bits written to the package");
            let parsed = parse_runtime_hex_u64(text).and_then(|bits| u32::try_from(bits).ok());
            if parsed.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Invalid 32-bit value; not applied",
                );
            }
            let raw_bits = response.changed().then_some(parsed).flatten();
            raw_bits
                .or(edited_bits)
                .map(WeaponRuntimeValue::Float32Bits)
        }
        (
            WeaponRuntimeValueKind::Vector4Float32,
            WeaponRuntimeValue::Vector4Float32Bits(current),
        ) => {
            let mut bits = *current;
            let mut changed = false;
            let mut invalid_bits = false;
            egui::Grid::new(("runtime-vector-editor", locator))
                .num_columns(3)
                .spacing([8.0, 3.0])
                .show(ui, |ui| {
                    for (index, current_bits) in bits.iter_mut().enumerate() {
                        ui.monospace(["X", "Y", "Z", "W"][index]);
                        let edited_bits = draw_runtime_float_decimal(ui, *current_bits);
                        let text = text_state
                            .entry((locator.clone(), u8::try_from(index).unwrap_or(0)))
                            .or_insert_with(|| format!("0x{:08X}", *current_bits));
                        if let Some(edited_bits) = edited_bits {
                            *current_bits = edited_bits;
                            *text = format!("0x{:08X}", *current_bits);
                            changed = true;
                        }
                        let response = ui.add(
                            egui::TextEdit::singleline(text)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(98.0),
                        );
                        response
                            .clone()
                            .on_hover_text("Exact IEEE-754 bits written to the package");
                        let parsed =
                            parse_runtime_hex_u64(text).and_then(|bits| u32::try_from(bits).ok());
                        invalid_bits |= parsed.is_none();
                        if response.changed() {
                            if let Some(parsed) = parsed {
                                *current_bits = parsed;
                                changed = true;
                            }
                        }
                        ui.end_row();
                    }
                });
            if invalid_bits {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Invalid vector bits; invalid components were not applied",
                );
            }
            changed.then_some(WeaponRuntimeValue::Vector4Float32Bits(bits))
        }
        (WeaponRuntimeValueKind::FixedBytes { size }, WeaponRuntimeValue::Bytes(current)) => {
            let text = text_state
                .entry((locator.clone(), 0))
                .or_insert_with(|| format_runtime_bytes(current));
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(ui.available_width().clamp(0.0, 720.0)),
            );
            let parsed =
                parse_runtime_hex_bytes(text, usize::try_from(*size).unwrap_or(usize::MAX));
            if parsed.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Expected exactly {size} bytes; not applied"),
                );
            }
            response
                .changed()
                .then_some(parsed)
                .flatten()
                .map(WeaponRuntimeValue::Bytes)
        }
        _ => {
            ui.colored_label(ui.visuals().error_fg_color, "Type mismatch");
            None
        }
    }
}

fn draw_runtime_float_decimal(ui: &mut egui::Ui, bits: u32) -> Option<u32> {
    let mut value = f32::from_bits(bits);
    if !value.is_finite() {
        // DragValue compares and clamps through f64. A NaN can become infinity
        // merely by drawing it, or lose its payload during conversion.
        ui.monospace(value.to_string())
            .on_hover_text("Non-finite value. Edit the exact IEEE-754 bits to change it.");
        return None;
    }
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .speed(0.01)
            .clamp_existing_to_range(false)
            .custom_formatter(|value, _| format!("{:?}", value as f32)),
    );
    (response.changed() && value.to_bits() != bits).then_some(value.to_bits())
}

pub(super) fn runtime_field_is_editable(field: &WeaponRuntimeField) -> bool {
    field.locator.is_buildable()
        && encode_weapon_runtime_value(&field.kind, &field.value)
            .is_ok_and(|bytes| bytes.len() == field.locator.byte_size as usize)
}

pub(super) fn runtime_field_is_in_editor_scope(
    source: WeaponRuntimeFieldSource,
    buildable: bool,
    customized: bool,
    show_experimental_options: bool,
    show_all_native_values: bool,
) -> bool {
    if customized {
        return true;
    }
    if !buildable {
        return false;
    }
    source != WeaponRuntimeFieldSource::OpaqueNativeType
        || (show_experimental_options && show_all_native_values)
}

pub(super) fn private_perk_runtime_field_is_visible(
    field: &WeaponRuntimeField,
    query: &str,
    values: &[WeaponRuntimeValueOverride],
    show_all_native_values: bool,
    occurrences: usize,
) -> bool {
    let customized = values.iter().any(|value| value.locator == field.locator);
    if occurrences != 1 && !customized {
        return false;
    }
    if !runtime_field_is_in_editor_scope(
        field.source,
        runtime_field_is_editable(field),
        customized,
        true,
        show_all_native_values,
    ) {
        return false;
    }
    query.is_empty()
        || field.name.to_ascii_lowercase().contains(query)
        || field.path_label.to_ascii_lowercase().contains(query)
        || runtime_value_kind_label(&field.kind)
            .to_ascii_lowercase()
            .contains(query)
        || format!("0x{:08x}", field.locator.binding_hash).contains(query)
        || format!("0x{:08x}", field.locator.root_schema).contains(query)
        || format!("0x{:08x}", field.locator.type_handle).contains(query)
        || format!("0x{:x}", field.owner_offset).contains(query)
        || format!("0x{:x}", field.locator.value_offset).contains(query)
        || field.locator.path.iter().any(|element| {
            format!("0x{:08x}", element.name_hash).contains(query)
                || format!("0x{:08x}", element.type_handle).contains(query)
        })
}

fn runtime_field_tooltip(field: &WeaponRuntimeField) -> String {
    let source = match field.source {
        WeaponRuntimeFieldSource::GeneratedSchema => "generated package schema",
        WeaponRuntimeFieldSource::NativeMember => "named native member",
        WeaponRuntimeFieldSource::OpaqueNativeType => "unnamed native fixed-size type",
    };
    let path = field
        .locator
        .path
        .iter()
        .map(|element| {
            format!(
                "0x{:08X}:0x{:08X}@+0x{:X}",
                element.name_hash, element.type_handle, element.byte_offset
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    let generated_kind = field
        .generated_kind
        .map_or_else(|| "N/A".to_owned(), |kind| format!("0x{kind:02X}"));
    format!(
        "{}\nSource: {source}\nValue Type: {}\nOriginal: {}\nBinding: 0x{:08X}, resource index {} (zero-based)\nRoot: {} · schema 0x{:08X}\nType: 0x{:08X} · generated kind {generated_kind}\nRoot offset: 0x{:X} · resolved owner offset: 0x{:X} · {} bytes\nReflected path: {path}",
        field.name,
        runtime_value_kind_label(&field.kind),
        original_value(&field.value),
        field.locator.binding_hash,
        field.locator.resource_index,
        field.locator.root.label(),
        field.locator.root_schema,
        field.locator.type_handle,
        field.locator.value_offset,
        field.owner_offset,
        field.locator.byte_size,
    )
}

fn original_value(value: &WeaponRuntimeValue) -> String {
    match value {
        WeaponRuntimeValue::Boolean(value) => value.to_string(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => value.to_string(),
        WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits).to_string(),
        WeaponRuntimeValue::Vector4Float32Bits(bits) => format!("{:?}", bits.map(f32::from_bits)),
        WeaponRuntimeValue::Bytes(bytes) => {
            format!("{} bytes (Reset restores the package value)", bytes.len())
        }
    }
}

pub(super) fn parse_runtime_hex_u64(value: &str) -> Option<u64> {
    let digits = value
        .trim()
        .strip_prefix("0x")
        .or_else(|| value.trim().strip_prefix("0X"))
        .unwrap_or(value.trim())
        .replace('_', "");
    (!digits.is_empty())
        .then(|| u64::from_str_radix(&digits, 16).ok())
        .flatten()
}

pub(super) fn format_runtime_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn parse_runtime_hex_bytes(value: &str, expected_size: usize) -> Option<Vec<u8>> {
    let digits = normalized_hex_bytes(value);
    if digits.len() != expected_size.checked_mul(2)?
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    (0..expected_size)
        .map(|index| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).ok())
        .collect()
}

pub(super) fn valid_hex_patch_text(value: &str) -> bool {
    let digits = normalized_hex_bytes(value);
    !digits.is_empty()
        && digits.len() % 2 == 0
        && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalized_hex_bytes(value: &str) -> String {
    let mut digits = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '_')
        .collect::<String>();
    if digits.starts_with("0x") || digits.starts_with("0X") {
        digits.drain(..2);
    }
    digits
}
