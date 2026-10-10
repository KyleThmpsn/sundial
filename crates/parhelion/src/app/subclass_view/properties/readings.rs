//! How values read and parse as text: numbers, factors, travel distances and lengths.
use super::*;

/// What a timer's length reads with `values`: its seconds, or [`UNLIMITED`] when it never ends.
pub(super) fn length_reading(
    values: &[WeaponRuntimeValueOverride],
    length: &EffectLength,
) -> Option<f32> {
    let seconds = own(values, &length.field)
        .map_or(Some(length.stock()), |value| effect_length::seconds(&value))?;
    let flag = length.unlimited.as_ref().is_some_and(|flag| {
        flag.is_set(&own(values, &flag.field).unwrap_or_else(|| flag.field.value.clone()))
    });
    Some(if effect_length::is_unlimited(seconds, None, flag) {
        UNLIMITED
    } else {
        seconds.max(0.0)
    })
}

pub(super) fn float(value: &WeaponRuntimeValue) -> Option<f32> {
    match value {
        WeaponRuntimeValue::Float32Bits(bits) => Some(f32::from_bits(*bits)),
        _ => None,
    }
}

pub(super) fn integer(value: &WeaponRuntimeValue) -> Option<i64> {
    match value {
        WeaponRuntimeValue::Signed(number) => Some(*number),
        WeaponRuntimeValue::Unsigned(number) => i64::try_from(*number).ok(),
        _ => None,
    }
}

/// A field whose name came from the game or a traced reader, not a placeholder.
pub(super) fn named(field: &WeaponRuntimeField) -> bool {
    !["Unnamed", "Member 0x", "Unreflected", "Value 0x", "M "]
        .iter()
        .any(|prefix| field.name.starts_with(prefix))
}

/// A number with up to three decimals.
pub(super) fn number(value: f64) -> String {
    egui::emath::format_with_decimals_in_range(value, 0..=3)
}

pub(super) fn duration_text(value: f64) -> String {
    if value < 0.0 {
        "Unlimited".to_owned()
    } else if value == 0.0 {
        "Immediately".to_owned()
    } else {
        format!("{} s", number(value))
    }
}

pub(super) fn parse_duration(text: &str) -> Option<f64> {
    match text.trim().to_ascii_lowercase().as_str() {
        "unlimited" => Some(-1.0),
        "immediately" => Some(0.0),
        _ => parse_number(text),
    }
}

pub(super) fn movement_text(unit: Unit, value: f64) -> String {
    match unit {
        Unit::Count => format!("{value:.0}"),
        Unit::Distance => format!("{} Units", number(value)),
        Unit::Factor => format!("×{}", number(value)),
        Unit::Rate => format!("{}%/s", number(value * 100.0)),
        Unit::Number => number(value),
        Unit::Mode => if value == 0.0 { "Direct" } else { "Blend" }.to_owned(),
    }
}

pub(super) fn movement_field<'a>(
    value: &'a mut f64,
    unit: Unit,
    label: &str,
) -> egui::DragValue<'a> {
    match unit {
        Unit::Count => egui::DragValue::new(value)
            .speed(0.1)
            .range(0.0..=100.0)
            .clamp_existing_to_range(false)
            .fixed_decimals(0),
        Unit::Distance => egui::DragValue::new(value)
            .speed(0.05)
            .range(0.0..=500.0)
            .clamp_existing_to_range(false)
            .custom_formatter(move |value, _| movement_text(unit, value))
            .custom_parser(parse_number),
        Unit::Factor => {
            let field = factor_field(value);
            if label == "Height Fade Factor" {
                field.range(0.0..=1.0)
            } else {
                field
            }
        }
        Unit::Rate => egui::DragValue::new(value)
            .speed(0.001)
            .range(0.0..=1.0)
            .clamp_existing_to_range(false)
            .custom_formatter(move |value, _| movement_text(unit, value))
            .custom_parser(|text| {
                text.trim()
                    .trim_end_matches("%/s")
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(|value| value / 100.0)
            }),
        Unit::Number => egui::DragValue::new(value)
            .speed(0.01)
            .range(0.0..=10.0)
            .clamp_existing_to_range(false)
            .max_decimals(3),
        Unit::Mode => egui::DragValue::new(value)
            .range(0.0..=1.0)
            .fixed_decimals(0),
    }
}

pub(super) fn movement_hint(label: &str) -> &'static str {
    match label {
        "Impulse Height Limit" => {
            "Height threshold in native units. Selected bank rows replace the controller's default"
        }
        "Height Fade Factor" => {
            "Impulse retained each update above the height limit. ×1 keeps the impulse"
        }
        "Active Energy Rate" => {
            "Energy used per second while active, before the ability's input multiplier"
        }
        "Velocity Blending" => {
            "Direct applies the prepared velocity. Blend combines it with the current movement response"
        }
        "Turn Rate Factor" => {
            "Scales glide turning while preserving the native angle conversions. This is a plain factor"
        }
        "Positive X Speed Multiplier" | "Negative X Speed Multiplier" | "Y Speed Multiplier" => {
            "Scales input speed and a braking boundary on a native vector coordinate. It is not an absolute speed cap"
        }
        "Acceleration Multiplier" => {
            "Scales the glide profile acceleration. The controller baseline remains in effect"
        }
        "Gravity Blend Factor Multiplier" => "Scales the gravity blend applied each update",
        "Directional Velocity Multiplier 1" | "Directional Velocity Multiplier 2" => {
            "Scales one horizontal velocity projection at activation. The exact direction is unconfirmed. Selected bank and controller factors multiply"
        }
        "Vertical Velocity Multiplier" => {
            "Scales existing vertical velocity at activation. Selected bank and controller factors multiply"
        }
        _ => "",
    }
}

pub(super) fn movement_mode(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    label: &str,
    current: f32,
) -> Option<f32> {
    let mut selected = if current == 0.0 { 0_u8 } else { 1 };
    let before = selected;
    let response = egui::ComboBox::from_id_salt(salt)
        .width(width)
        .selected_text(if selected == 0 { "Direct" } else { "Blend" })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut selected, 0, "Direct");
            ui.selectable_value(&mut selected, 1, "Blend");
        });
    style::named_control(response.response, label);
    (selected != before).then_some(f32::from(selected))
}

/// An amount a modifier adds, with its sign.
pub(super) fn signed(value: f64) -> String {
    if value < 0.0 {
        number(value)
    } else {
        format!("+{}", number(value))
    }
}

/// A travel distance limit: zero sets none.
pub(super) fn travel_text(distance: f64) -> String {
    if distance <= 0.0 {
        "No Limit".to_owned()
    } else {
        format!("{} Units", number(distance))
    }
}

pub(super) fn parse_travel(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.starts_with("no") {
        return Some(0.0);
    }
    text.trim_end_matches("units").trim().parse().ok()
}

/// A number typed with or without its unit: ×, s, °/s or Units.
pub(super) fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .trim_start_matches(['×', 'x'])
        .trim_end_matches("Units/Update")
        .trim_end_matches("Units/s")
        .trim_end_matches("°/s")
        .trim_end_matches(['°', '%'])
        .trim_end_matches("Units")
        .trim_end_matches("units")
        .trim_end_matches('s')
        .trim()
        .parse()
        .ok()
}

/// A launch speed as its field reads it: Hitscan for the marker instant-hit graphs carry, else a
/// factor.
pub(super) fn speed_text(value: f64) -> String {
    if value == f64::from(crate::weapon::behavior::HITSCAN_SPEED) {
        "Hitscan".to_owned()
    } else {
        format!("×{}", number(value))
    }
}

/// A typed launch speed: Hitscan, or a factor with or without its ×.
pub(super) fn parse_speed(text: &str) -> Option<f64> {
    if text.trim().eq_ignore_ascii_case("hitscan") {
        Some(f64::from(crate::weapon::behavior::HITSCAN_SPEED))
    } else {
        parse_amount(text)
    }
}

pub(super) fn parse_amount(text: &str) -> Option<f64> {
    text.trim()
        .trim_start_matches(['×', 'x', '+'])
        .trim()
        .parse()
        .ok()
}
