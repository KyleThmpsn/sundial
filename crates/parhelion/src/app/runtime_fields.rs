//! Focused runtime fields controls; recipe mutation occurs on user actions.
use super::*;
use sundial::package_authoring::weapon_runtime::{WeaponRuntimeOwner, WeaponRuntimeRoot};

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
                "This saved field cannot be written.",
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
    let current_value = override_index.map_or_else(
        || field.value.clone(),
        |index| overrides[index].value.clone(),
    );
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
                if can_reset && ui.small_button("Reset to Donor").clicked() {
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
        ui.colored_label(ui.visuals().error_fg_color, "Saved value is invalid.");
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

pub(super) use sundial::package_authoring::weapon_runtime::presentation::kind_label as runtime_value_kind_label;

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
    let next = draw_runtime_value_editor_contents(ui, locator, kind, current, text_state, None);
    ui.data_mut(|data| {
        data.insert_temp(value_id, next.as_ref().unwrap_or(current).clone());
    });
    next
}

/// `draw_runtime_value_editor` with names for the value that its record decides, such as a
/// property named by the component it changes.
pub(super) fn draw_runtime_value_editor_with(
    ui: &mut egui::Ui,
    locator: &WeaponRuntimeFieldLocator,
    kind: &WeaponRuntimeValueKind,
    current: &WeaponRuntimeValue,
    text_state: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    choices: Option<&'static [(i64, &'static str)]>,
) -> Option<WeaponRuntimeValue> {
    if choices.is_none() {
        return draw_runtime_value_editor(ui, locator, kind, current, text_state);
    }
    draw_runtime_value_editor_contents(ui, locator, kind, current, text_state, choices)
}

fn draw_runtime_value_editor_contents(
    ui: &mut egui::Ui,
    locator: &WeaponRuntimeFieldLocator,
    kind: &WeaponRuntimeValueKind,
    current: &WeaponRuntimeValue,
    text_state: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    choices: Option<&'static [(i64, &'static str)]>,
) -> Option<WeaponRuntimeValue> {
    let meaning = sundial::package_authoring::weapon_runtime::modifiers::field_meaning(
        locator.type_handle,
        locator.value_offset,
    );
    let help = meaning.as_ref().map_or("", |meaning| meaning.help);
    // Choices a caller supplies name some values of a numeric input, such as the properties a
    // component's stock perks establish. An unnamed one reads as its number, as it does in rows
    // whose component has no names at all.
    let numbered = choices.is_some();
    let choices = choices.or_else(|| {
        meaning
            .as_ref()
            .map(|meaning| meaning.choices)
            .filter(|choices| !choices.is_empty())
    });
    if let Some(choices) = choices {
        let number = match current {
            WeaponRuntimeValue::Signed(value) => Some(*value),
            WeaponRuntimeValue::Unsigned(value) => i64::try_from(*value).ok(),
            _ => None,
        };
        if let Some(number) = number {
            let mut selected = number;
            let label = choices.iter().find(|(v, _)| *v == number).map_or_else(
                || {
                    if numbered {
                        number.to_string()
                    } else {
                        format!("Option {number}")
                    }
                },
                |(_, name)| (*name).into(),
            );
            let unnamed = !choices.iter().any(|(value, _)| *value == number);
            egui::ComboBox::from_id_salt(("component-modifier-choice", locator))
                .width(ui.spacing().combo_width)
                .truncate()
                .selected_text(label.clone())
                .show_ui(ui, |ui| {
                    for &(value, name) in choices {
                        ui.selectable_value(&mut selected, value, name);
                    }
                    // The stock value stays reachable after choosing a named one.
                    if unnamed {
                        ui.selectable_value(&mut selected, number, label);
                    }
                })
                .response
                .on_hover_text(help);
            return (selected != number).then_some(match current {
                WeaponRuntimeValue::Signed(_) => WeaponRuntimeValue::Signed(selected),
                _ => WeaponRuntimeValue::Unsigned(selected as u64),
            });
        }
    }
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
                    .desired_width(184.0_f32.min(ui.available_width())),
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
                    .desired_width(184.0_f32.min(ui.available_width())),
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
                    "Invalid identifier. Not applied.",
                );
            }
            response
                .changed()
                .then_some(parsed)
                .flatten()
                .map(WeaponRuntimeValue::Unsigned)
        }
        (WeaponRuntimeValueKind::Float64, WeaponRuntimeValue::Float64Bits(current)) => {
            draw_runtime_double(ui, locator, *current, text_state)
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
            response.clone().on_hover_text("Exact IEEE-754 bits.");
            let parsed = parse_runtime_hex_u64(text).and_then(|bits| u32::try_from(bits).ok());
            if parsed.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Invalid 32-bit value. Not applied.",
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
                    "Invalid vector bits. Invalid components were not applied.",
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
                    format!("Expected exactly {size} bytes. Not applied."),
                );
            }
            response
                .changed()
                .then_some(parsed)
                .flatten()
                .map(WeaponRuntimeValue::Bytes)
        }
        _ => {
            ui.colored_label(ui.visuals().error_fg_color, "Type mismatch.");
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

fn draw_runtime_double(
    ui: &mut egui::Ui,
    locator: &WeaponRuntimeFieldLocator,
    current: u64,
    text_state: &mut BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
) -> Option<WeaponRuntimeValue> {
    let mut value = f64::from_bits(current);
    let edited = if value.is_finite() {
        ui.add(
            egui::DragValue::new(&mut value)
                .speed(0.01)
                .clamp_existing_to_range(false)
                .custom_formatter(|value, _| format!("{value:?}")),
        )
        .changed()
    } else {
        ui.monospace(value.to_string())
            .on_hover_text("Non-finite donor value. Edit the exact IEEE-754 bits to change it.");
        false
    };
    let text = text_state
        .entry((locator.clone(), 0))
        .or_insert_with(|| format!("0x{current:016X}"));
    if edited && value.is_finite() {
        *text = format!("0x{:016X}", value.to_bits());
    }
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .font(egui::TextStyle::Monospace)
            .desired_width(154.0),
    );
    let parsed = parse_runtime_hex_u64(text);
    if parsed.is_none() {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Invalid 64-bit float. Not applied.",
        );
    }
    if response.changed() {
        parsed.map(WeaponRuntimeValue::Float64Bits)
    } else if edited && value.is_finite() {
        Some(WeaponRuntimeValue::Float64Bits(value.to_bits()))
    } else {
        None
    }
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

use sundial::package_authoring::weapon_runtime::presentation::field_tooltip as runtime_field_tooltip;

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

/// What the Runtime Values list reads about every field of one graph, worked out once per graph
/// rather than once per frame, and the list it last filtered.
pub(super) struct RuntimeValuesCache {
    graph: std::sync::Weak<WeaponRuntimeGraph>,
    /// Per field, in `WeaponRuntimeGraph::fields` order.
    editable: Vec<bool>,
    /// Per field, the lowercase texts a query is matched against, joined by NUL.
    search: Vec<String>,
    pub(super) resolved: usize,
    pub(super) technical: usize,
    view: Option<(RuntimeValuesKey, RuntimeValuesView)>,
}

/// What the list is filtered by. `customized` is the recipe's runtime value locators in order.
#[derive(PartialEq)]
pub(super) struct RuntimeValuesKey {
    pub(super) query: String,
    pub(super) show_experimental_options: bool,
    pub(super) show_all_native_values: bool,
    pub(super) customized: Vec<WeaponRuntimeFieldLocator>,
}

/// The fields a filter leaves visible, as positions in the graph.
#[derive(Default)]
pub(super) struct RuntimeValuesView {
    /// Positions in `customized` whose locator the graph no longer has.
    pub(super) stale: Vec<usize>,
    /// One group per resource, in graph order.
    pub(super) resources: Vec<RuntimeValuesGroup>,
    /// One group per owner, in graph order.
    pub(super) owners: Vec<RuntimeValuesGroup>,
    pub(super) visible: usize,
}

/// The visible fields of one resource or owner.
#[derive(Default)]
pub(super) struct RuntimeValuesGroup {
    /// Each root with a visible field: its position in the group, and the positions of its
    /// visible fields.
    pub(super) roots: Vec<(usize, Vec<usize>)>,
    pub(super) count: usize,
}

impl RuntimeValuesCache {
    pub(super) fn new(graph: &Arc<WeaponRuntimeGraph>) -> Self {
        let mut editable = Vec::new();
        let mut search = Vec::new();
        let mut resolved = 0;
        let mut technical = 0;
        for field in graph.fields() {
            let is_editable = runtime_field_is_editable(field);
            if is_editable {
                if field.source == WeaponRuntimeFieldSource::OpaqueNativeType {
                    technical += 1;
                } else {
                    resolved += 1;
                }
            }
            editable.push(is_editable);
            search.push(runtime_field_search_text(field));
        }
        Self {
            graph: Arc::downgrade(graph),
            editable,
            search,
            resolved,
            technical,
            view: None,
        }
    }

    /// The held graph keeps its allocation, so another graph cannot take its address.
    pub(super) fn is_for(&self, graph: &Arc<WeaponRuntimeGraph>) -> bool {
        std::ptr::eq(self.graph.as_ptr(), Arc::as_ptr(graph))
    }

    /// The filtered list for `key`, rebuilt only when the key changes.
    pub(super) fn view(
        &mut self,
        graph: &WeaponRuntimeGraph,
        key: RuntimeValuesKey,
    ) -> &RuntimeValuesView {
        if self.view.as_ref().is_none_or(|(cached, _)| *cached != key) {
            let view = self.filter(graph, &key);
            self.view = Some((key, view));
        }
        &self.view.as_ref().expect("the view was just built").1
    }

    fn filter(&self, graph: &WeaponRuntimeGraph, key: &RuntimeValuesKey) -> RuntimeValuesView {
        let live = graph
            .fields()
            .map(|field| &field.locator)
            .collect::<BTreeSet<_>>();
        let stale = key
            .customized
            .iter()
            .enumerate()
            .filter(|(_, locator)| !live.contains(locator))
            .map(|(index, _)| index)
            .collect();
        let filter = RuntimeValuesFilter {
            cache: self,
            key,
            customized: key.customized.iter().collect(),
        };
        let query = key.query.as_str();
        let mut view = RuntimeValuesView {
            stale,
            ..RuntimeValuesView::default()
        };
        let mut position = 0;
        for resource in &graph.resources {
            let matches = query.is_empty()
                || resource.binding_label.to_ascii_lowercase().contains(query)
                || format!("0x{:08x}", resource.binding_hash).contains(query)
                || format!("0x{:08x}", resource.owner_tag).contains(query)
                || format!("0x{:08x}", resource.concrete_class).contains(query)
                || resource.definition.as_ref().is_some_and(|definition| {
                    format!("0x{:08x}", definition.schema).contains(query)
                });
            let group = filter.group(
                std::iter::once(&resource.instance).chain(resource.definition.iter()),
                &mut position,
                matches,
            );
            view.visible += group.count;
            view.resources.push(group);
        }
        for owner in &graph.owners {
            let matches = query.is_empty()
                || runtime_owner_label(graph, owner)
                    .to_ascii_lowercase()
                    .contains(query)
                || format!("0x{:08x}", owner.owner_tag).contains(query)
                || format!("0x{:08x}", owner.anchor_binding_hash).contains(query);
            let group = filter.group(owner.roots.iter(), &mut position, matches);
            view.visible += group.count;
            view.owners.push(group);
        }
        view
    }
}

/// Decides which fields the value list shows.
struct RuntimeValuesFilter<'a> {
    cache: &'a RuntimeValuesCache,
    key: &'a RuntimeValuesKey,
    customized: BTreeSet<&'a WeaponRuntimeFieldLocator>,
}

impl RuntimeValuesFilter<'_> {
    /// `position` is the first field's position in `WeaponRuntimeGraph::fields`, and advances
    /// past every field of `roots`.
    fn group<'r>(
        &self,
        roots: impl Iterator<Item = &'r WeaponRuntimeRoot>,
        position: &mut usize,
        group_matches: bool,
    ) -> RuntimeValuesGroup {
        let mut group = RuntimeValuesGroup::default();
        for (root_position, root) in roots.enumerate() {
            let mut fields = Vec::new();
            for (index, field) in root.fields.iter().enumerate() {
                if self.visible(*position + index, field, group_matches) {
                    fields.push(index);
                }
            }
            *position += root.fields.len();
            if !fields.is_empty() {
                group.count += fields.len();
                group.roots.push((root_position, fields));
            }
        }
        group
    }

    fn visible(&self, position: usize, field: &WeaponRuntimeField, group_matches: bool) -> bool {
        let in_scope = self.customized.contains(&field.locator)
            || runtime_field_is_in_editor_scope(
                field.source,
                self.cache.editable[position],
                false,
                self.key.show_experimental_options,
                self.key.show_all_native_values,
            );
        let query = self.key.query.as_str();
        // A query holding NUL could match across two joined texts.
        let text_matches = || {
            if query.contains('\0') {
                runtime_field_matches_query(field, query)
            } else {
                self.cache.search[position].contains(query)
            }
        };
        in_scope && (query.is_empty() || group_matches || text_matches())
    }
}

/// The label an owner's values are listed under.
pub(super) fn runtime_owner_label(
    graph: &WeaponRuntimeGraph,
    owner: &WeaponRuntimeOwner,
) -> String {
    graph
        .bindings
        .iter()
        .find(|binding| binding.binding_hash == owner.anchor_binding_hash)
        .map_or_else(
            || format!("Binding 0x{:08X}", owner.anchor_binding_hash),
            |binding| binding.binding_label.clone(),
        )
}

/// Every text `runtime_field_matches_query` searches, so one `contains` stands in for it.
fn runtime_field_search_text(field: &WeaponRuntimeField) -> String {
    let mut parts = vec![
        field.name.to_ascii_lowercase(),
        field.path_label.to_ascii_lowercase(),
        runtime_value_kind_label(&field.kind).to_ascii_lowercase(),
        format!("0x{:08x}", field.locator.root_schema),
        format!("0x{:08x}", field.locator.type_handle),
        format!("0x{:x}", field.owner_offset),
        format!("0x{:x}", field.locator.value_offset),
    ];
    for element in &field.locator.path {
        parts.push(format!("0x{:08x}", element.name_hash));
        parts.push(format!("0x{:08x}", element.type_handle));
    }
    parts.join("\0")
}

fn runtime_field_matches_query(field: &WeaponRuntimeField, query: &str) -> bool {
    field.name.to_ascii_lowercase().contains(query)
        || field.path_label.to_ascii_lowercase().contains(query)
        || runtime_value_kind_label(&field.kind)
            .to_ascii_lowercase()
            .contains(query)
        || format!("0x{:08x}", field.locator.root_schema).contains(query)
        || format!("0x{:08x}", field.locator.type_handle).contains(query)
        || format!("0x{:x}", field.owner_offset).contains(query)
        || format!("0x{:x}", field.locator.value_offset).contains(query)
        || field.locator.path.iter().any(|element| {
            format!("0x{:08x}", element.name_hash).contains(query)
                || format!("0x{:08x}", element.type_handle).contains(query)
        })
}
