//! JSON-preserving settings widgets and display refresh-rate choices.

use std::{ops::BitOrAssign, sync::OnceLock};

use eframe::egui;
use serde_json::{Map, Value};
use sundial_account::{AccountSettingKey, AccountSettingValue, AccountSettingsCommand, FiniteF64};

#[derive(Default)]
pub(super) struct CommandBatch(Vec<AccountSettingsCommand>);

impl CommandBatch {
    pub(super) fn into_vec(self) -> Vec<AccountSettingsCommand> {
        self.0
    }
}

impl BitOrAssign<Option<AccountSettingsCommand>> for CommandBatch {
    fn bitor_assign(&mut self, command: Option<AccountSettingsCommand>) {
        self.0.extend(command);
    }
}

fn command(key: &str, value: AccountSettingValue) -> AccountSettingsCommand {
    AccountSettingsCommand::Set {
        key: AccountSettingKey::known_preference(key)
            .expect("guided account settings use registered logical keys"),
        value,
    }
}

pub(super) fn boolean(
    ui: &mut egui::Ui,
    values: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Option<AccountSettingsCommand> {
    ui.label(label);
    let mut replacement = None;
    if let Some(value) = values.get(key) {
        if let Some(mut checked) = value.as_bool() {
            if ui.checkbox(&mut checked, "").changed() {
                replacement = Some(command(key, AccountSettingValue::Boolean(checked)));
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    replacement
}

pub(super) fn choice<T: AsRef<str>>(
    ui: &mut egui::Ui,
    values: &Map<String, Value>,
    key: &str,
    label: &str,
    choices: &[(u64, T)],
) -> Option<AccountSettingsCommand> {
    ui.label(label);
    let mut changed = false;
    let mut replacement = None;
    if let Some(value) = values.get(key) {
        if let Some(mut current) = value.as_u64() {
            let selected = choices
                .iter()
                .find(|(candidate, _)| *candidate == current)
                .map_or("Invalid value", |(_, name)| name.as_ref());
            egui::ComboBox::from_id_salt(("game_setting", key))
                .selected_text(selected)
                .width(210.0)
                .show_ui(ui, |ui| {
                    for (candidate, name) in choices {
                        if ui
                            .selectable_value(&mut current, *candidate, name.as_ref())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
            if changed {
                replacement = Some(command(key, AccountSettingValue::Unsigned(current)));
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    replacement
}

pub(super) fn vertical_sync_intervals(refresh_rate_hz: Option<u32>) -> Vec<(u64, String)> {
    std::iter::once((0, "Off (Default)".to_owned()))
        .chain((1..=4).map(|interval| {
            let refreshes = if interval == 1 {
                "Every refresh".to_owned()
            } else {
                format!("Every {interval} refreshes")
            };
            let label = refresh_rate_hz.map_or(refreshes.clone(), |refresh_rate_hz| {
                let frame_rate = f64::from(refresh_rate_hz) / interval as f64;
                let frame_rate = if frame_rate.fract() == 0.0 {
                    format!("{frame_rate:.0}")
                } else {
                    format!("{frame_rate:.1}")
                };
                format!("{refreshes} ({frame_rate} FPS)")
            });
            (interval, label)
        }))
        .collect()
}

pub(super) fn display_refresh_rate_hz() -> Option<u32> {
    static REFRESH_RATE_HZ: OnceLock<Option<u32>> = OnceLock::new();
    *REFRESH_RATE_HZ.get_or_init(query_display_refresh_rate_hz)
}

#[cfg(any(windows, test))]
pub(super) fn nominal_refresh_rate_hz(reported: u32) -> u32 {
    const COMMON_REFRESH_RATES: &[u32] = &[
        24, 25, 30, 50, 60, 72, 75, 90, 100, 120, 144, 165, 170, 175, 180, 200, 240, 360, 480, 500,
    ];
    COMMON_REFRESH_RATES
        .iter()
        .copied()
        .find(|candidate| candidate.abs_diff(reported) <= 1)
        .unwrap_or(reported)
}

#[cfg(windows)]
pub(super) fn query_display_refresh_rate_hz() -> Option<u32> {
    use windows_sys::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW,
    };

    // SAFETY: DEVMODEW is a Windows C data structure for which an all-zero initial state is valid.
    // The required dmSize field is initialized before it is passed to Windows.
    let mut mode = unsafe { std::mem::zeroed::<DEVMODEW>() };
    mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    // SAFETY: A null device name selects the current display, and mode points to initialized,
    // writable DEVMODEW storage that remains live for the synchronous call.
    let found =
        unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut mode) } != 0;
    (found && (24..=1_000).contains(&mode.dmDisplayFrequency))
        .then(|| nominal_refresh_rate_hz(mode.dmDisplayFrequency))
}

#[cfg(not(windows))]
pub(super) fn query_display_refresh_rate_hz() -> Option<u32> {
    None
}

pub(super) fn account_string_choice(
    ui: &mut egui::Ui,
    values: &Map<String, Value>,
    key: &str,
    label: &str,
    choices: &[(&str, &str)],
) -> Option<AccountSettingsCommand> {
    ui.label(label);
    let mut changed = false;
    let mut replacement = None;
    if let Some(value) = values.get(key) {
        if let Some(current) = value.as_str() {
            let mut current = current.to_owned();
            let selected = choices
                .iter()
                .find(|(candidate, _)| *candidate == current)
                .map_or("Invalid value", |(_, name)| *name);
            egui::ComboBox::from_id_salt(("game_setting", key))
                .selected_text(selected)
                .width(210.0)
                .show_ui(ui, |ui| {
                    for &(candidate, name) in choices {
                        if ui
                            .selectable_value(&mut current, candidate.to_owned(), name)
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
            if changed {
                replacement = Some(command(key, AccountSettingValue::text(current)));
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    replacement
}

/// JSON-owned player fields do not belong to the account backend.
pub(super) fn json_string_choice(
    ui: &mut egui::Ui,
    values: &mut Map<String, Value>,
    key: &str,
    label: &str,
    choices: &[(&str, &str)],
) -> bool {
    ui.label(label);
    let mut changed = false;
    if let Some(value) = values.get_mut(key) {
        if let Some(current) = value.as_str() {
            let mut current = current.to_owned();
            let selected = choices
                .iter()
                .find(|(candidate, _)| *candidate == current)
                .map_or("Invalid value", |(_, name)| *name);
            egui::ComboBox::from_id_salt(("game_setting", key))
                .selected_text(selected)
                .width(210.0)
                .show_ui(ui, |ui| {
                    for &(candidate, name) in choices {
                        changed |= ui
                            .selectable_value(&mut current, candidate.to_owned(), name)
                            .changed();
                    }
                });
            if changed {
                *value = Value::String(current);
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    changed
}

pub(super) fn integer_slider(
    ui: &mut egui::Ui,
    values: &Map<String, Value>,
    key: &str,
    label: &str,
    minimum: u64,
    maximum: u64,
) -> Option<AccountSettingsCommand> {
    ui.label(label);
    let mut replacement = None;
    if let Some(value) = values.get(key) {
        if let Some(mut current) = value.as_u64() {
            if ui
                .add(egui::Slider::new(&mut current, minimum..=maximum))
                .changed()
            {
                replacement = Some(command(key, AccountSettingValue::Unsigned(current)));
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    replacement
}

pub(super) fn offset_slider(
    ui: &mut egui::Ui,
    values: &Map<String, Value>,
    key: &str,
    label: &str,
    minimum: u64,
    maximum: u64,
    display_offset: u64,
) -> Option<AccountSettingsCommand> {
    ui.label(label);
    let mut replacement = None;
    if let Some(value) = values.get(key) {
        if let Some(current) = value.as_u64() {
            let mut displayed = current.saturating_add(display_offset);
            if ui
                .add(egui::Slider::new(
                    &mut displayed,
                    minimum + display_offset..=maximum + display_offset,
                ))
                .changed()
            {
                replacement = Some(command(
                    key,
                    AccountSettingValue::Unsigned(displayed - display_offset),
                ));
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    replacement
}

pub(super) fn float_slider(
    ui: &mut egui::Ui,
    values: &Map<String, Value>,
    key: &str,
    label: &str,
    minimum: f64,
    maximum: f64,
    step: f64,
) -> Option<AccountSettingsCommand> {
    ui.label(label);
    let mut replacement = None;
    if let Some(value) = values.get(key) {
        if let Some(mut current) = value.as_f64() {
            if ui
                .add(
                    egui::Slider::new(&mut current, minimum..=maximum)
                        .step_by(step)
                        .fixed_decimals(1),
                )
                .changed()
            {
                if let Some(current) = FiniteF64::new(current) {
                    replacement = Some(command(key, AccountSettingValue::Decimal(current)));
                }
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid value");
        }
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
    replacement
}

pub(super) fn fixed(ui: &mut egui::Ui, values: &Map<String, Value>, key: &str, label: &str) {
    ui.label(label);
    if let Some(value) = values.get(key) {
        ui.add_enabled(false, egui::Label::new(value.to_string()))
            .on_hover_text("Project Sunrise requires this exact value.");
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing");
    }
    ui.end_row();
}
