use std::collections::BTreeMap;

use sundial_account::{
    AccountSettingKey, AccountSettingValue, AccountSettingsCapabilities, AccountSettingsState,
    FiniteF64, KeyBindingSlot,
};

use super::{
    SqliteAccountError,
    contract::{KEY_BINDING_ACTIONS, SETTINGS_PAYLOAD_CAPACITY, SETTINGS_PAYLOAD_VERSION},
};

pub(super) fn payload_version(payload: &[u8]) -> Result<u32, SqliteAccountError> {
    if payload.is_empty() || payload.len() > SETTINGS_PAYLOAD_CAPACITY {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            format!("payload length must be between 1 and {SETTINGS_PAYLOAD_CAPACITY} bytes"),
        ));
    }
    let bytes = payload.get(..4).ok_or_else(|| {
        SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            "payload is shorter than its version field",
        )
    })?;
    let [byte0, byte1, byte2, byte3] = bytes else {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            "payload version field has an invalid width",
        ));
    };
    Ok(u32::from_le_bytes([*byte0, *byte1, *byte2, *byte3]))
}

pub(super) fn decode(
    payload: &[u8],
    account_present: bool,
) -> Result<AccountSettingsState, SqliteAccountError> {
    let mut reader = PayloadReader::new(payload);
    let version = reader.u32()?;
    if version != SETTINGS_PAYLOAD_VERSION {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload.version",
            format!("expected payload version {SETTINGS_PAYLOAD_VERSION}, found {version}"),
        ));
    }

    let controls = Controls {
        button_layout: reader.i8()?,
        movement_mode: reader.i8()?,
        controller_look_sensitivity: reader.i8()?,
        controller_invert_vertical: reader.boolean()?,
        controller_auto_look_centering: reader.boolean()?,
        controller_vibration: reader.boolean()?,
        controller_swap_shoulders: reader.boolean()?,
        controller_invert_horizontal: reader.boolean()?,
        mouse_look_sensitivity: reader.i32()?,
        mouse_invert_vertical: reader.boolean()?,
        mouse_invert_horizontal: reader.boolean()?,
        unidentified_toggle: reader.boolean()?,
        mouse_aim_smoothing: reader.boolean()?,
        ads_sensitivity_modifier: reader.f32()?,
        double_press_delay: reader.i8()?,
    };
    let audio = Audio {
        voice_output_mode: reader.i8()?,
        team_voice_channel: reader.i8()?,
        reserved_mode: reader.i8()?,
        migration_version: reader.i8()?,
        chat_volume: reader.i8()?,
        mute_when_unfocused: reader.boolean()?,
        sound_effects_volume: reader.i8()?,
        dialogue_volume: reader.i8()?,
        music_volume: reader.i8()?,
    };
    let display = Display {
        brightness: reader.i8()?,
        show_fps: reader.boolean()?,
        hdr_mode: reader.i8()?,
        vertical_sync_interval: reader.u8()?,
        field_of_view: reader.i32()?,
        calibration_primary: reader.f32()?,
        calibration_alpha: reader.f32()?,
    };
    let interface = Interface {
        subtitles_mode: reader.i8()?,
        colorblind_mode: reader.i8()?,
        helmet_mode: reader.i8()?,
        hud_opacity: reader.i8()?,
        display_hints: reader.boolean()?,
        background_opacity: reader.i8()?,
        reticle_location: reader.i8()?,
        reticle_color: reader.i8()?,
        text_size: reader.i8()?,
        text_color: reader.i8()?,
        text_background_style: reader.i8()?,
        text_background_opacity: reader.i8()?,
        reserved_text_mode: reader.i8()?,
        subtitle_options_entry: reader.i8()?,
    };
    let social = Social {
        prefer_good_connection: reader.boolean()?,
        text_chat_mode: reader.i8()?,
        show_real_names: reader.boolean()?,
        clan_invite_notifications: reader.boolean()?,
        profanity_filter: reader.boolean()?,
        voice_chat_enabled: reader.boolean()?,
        whisper_chat_mode: reader.i8()?,
        team_chat_join_mode: reader.i8()?,
        local_chat_join_mode: reader.i8()?,
        clan_chat_join_mode: reader.i8()?,
        chat_auto_hide_mode: reader.i8()?,
    };
    let key_binding_source = reader.u8()?;
    let bindings_configured = reader.boolean()?;
    let mut bindings = Vec::with_capacity(KEY_BINDING_ACTIONS.len());
    for action in KEY_BINDING_ACTIONS {
        bindings.push((action, reader.optional_u16()?, reader.optional_u16()?));
    }
    let configured = reader.boolean()?;
    if !reader.complete() {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            "payload contains trailing bytes",
        ));
    }

    if !account_present {
        if configured || bindings_configured {
            return Err(SqliteAccountError::invalid_data(
                "account_state.settings_payload",
                "an empty account cannot have configured settings or bindings",
            ));
        }
        return Ok(AccountSettingsState::default());
    }
    if !configured || !bindings_configured {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            "a nonempty account requires configured settings and key bindings",
        ));
    }
    if audio.migration_version != 8
        || display.calibration_primary != 10_000.0
        || display.calibration_alpha != 0.0
        || interface.reserved_text_mode != 0
        || interface.subtitle_options_entry != 0
    {
        return Err(SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            "fixed Sunrise settings fields do not contain their required values",
        ));
    }

    let mut values = BTreeMap::new();
    insert_unsigned(&mut values, "button_layout", controls.button_layout)?;
    insert_unsigned(&mut values, "movement_mode", controls.movement_mode)?;
    insert_unsigned(
        &mut values,
        "controller_look_sensitivity",
        controls.controller_look_sensitivity,
    )?;
    insert_bool(
        &mut values,
        "controller_invert_vertical",
        controls.controller_invert_vertical,
    )?;
    insert_bool(
        &mut values,
        "controller_auto_look_centering",
        controls.controller_auto_look_centering,
    )?;
    insert_bool(
        &mut values,
        "controller_vibration",
        controls.controller_vibration,
    )?;
    insert_bool(
        &mut values,
        "controller_swap_shoulders",
        controls.controller_swap_shoulders,
    )?;
    insert_bool(
        &mut values,
        "controller_invert_horizontal",
        controls.controller_invert_horizontal,
    )?;
    insert_unsigned_i32(
        &mut values,
        "mouse_look_sensitivity",
        controls.mouse_look_sensitivity,
    )?;
    insert_bool(
        &mut values,
        "mouse_invert_vertical",
        controls.mouse_invert_vertical,
    )?;
    insert_bool(
        &mut values,
        "mouse_invert_horizontal",
        controls.mouse_invert_horizontal,
    )?;
    insert_bool(
        &mut values,
        "unidentified_toggle",
        controls.unidentified_toggle,
    )?;
    insert_bool(
        &mut values,
        "mouse_aim_smoothing",
        controls.mouse_aim_smoothing,
    )?;
    insert(
        &mut values,
        "ads_sensitivity_modifier",
        AccountSettingValue::Decimal(
            FiniteF64::new(f64::from(controls.ads_sensitivity_modifier)).ok_or_else(|| {
                SqliteAccountError::invalid_data(
                    "account_state.settings_payload.ads_sensitivity_modifier",
                    "value must be finite",
                )
            })?,
        ),
    )?;
    insert_unsigned(
        &mut values,
        "double_press_delay",
        controls.double_press_delay,
    )?;

    insert_unsigned(&mut values, "voice_output_mode", audio.voice_output_mode)?;
    insert_unsigned(&mut values, "team_voice_channel", audio.team_voice_channel)?;
    insert_unsigned(&mut values, "reserved_mode", audio.reserved_mode)?;
    insert_unsigned(&mut values, "chat_volume", audio.chat_volume)?;
    insert_bool(
        &mut values,
        "mute_when_unfocused",
        audio.mute_when_unfocused,
    )?;
    insert_unsigned(
        &mut values,
        "sound_effects_volume",
        audio.sound_effects_volume,
    )?;
    insert_unsigned(&mut values, "dialogue_volume", audio.dialogue_volume)?;
    insert_unsigned(&mut values, "music_volume", audio.music_volume)?;

    insert_unsigned(&mut values, "brightness", display.brightness)?;
    insert_bool(&mut values, "show_fps", display.show_fps)?;
    insert_unsigned(&mut values, "hdr_mode", display.hdr_mode)?;
    insert(
        &mut values,
        "vertical_sync_interval",
        AccountSettingValue::Unsigned(u64::from(display.vertical_sync_interval)),
    )?;
    insert_unsigned_i32(&mut values, "field_of_view", display.field_of_view)?;

    insert_unsigned(&mut values, "subtitles_mode", interface.subtitles_mode)?;
    insert_unsigned(&mut values, "colorblind_mode", interface.colorblind_mode)?;
    insert_unsigned(&mut values, "helmet_mode", interface.helmet_mode)?;
    insert_unsigned(&mut values, "hud_opacity", interface.hud_opacity)?;
    insert_bool(&mut values, "display_hints", interface.display_hints)?;
    insert_unsigned(
        &mut values,
        "background_opacity",
        interface.background_opacity,
    )?;
    insert_unsigned(&mut values, "reticle_location", interface.reticle_location)?;
    insert_unsigned(&mut values, "reticle_color", interface.reticle_color)?;
    insert_unsigned(&mut values, "text_size", interface.text_size)?;
    insert_unsigned(&mut values, "text_color", interface.text_color)?;
    insert_unsigned(
        &mut values,
        "text_background_style",
        interface.text_background_style,
    )?;
    insert_unsigned(
        &mut values,
        "text_background_opacity",
        interface.text_background_opacity,
    )?;

    insert_bool(
        &mut values,
        "prefer_good_connection",
        social.prefer_good_connection,
    )?;
    insert_unsigned(&mut values, "text_chat_mode", social.text_chat_mode)?;
    insert_bool(&mut values, "show_real_names", social.show_real_names)?;
    insert_bool(
        &mut values,
        "clan_invite_notifications",
        social.clan_invite_notifications,
    )?;
    insert_bool(&mut values, "profanity_filter", social.profanity_filter)?;
    insert_bool(&mut values, "voice_chat_enabled", social.voice_chat_enabled)?;
    insert_unsigned(&mut values, "whisper_chat_mode", social.whisper_chat_mode)?;
    insert_unsigned(
        &mut values,
        "team_chat_join_mode",
        social.team_chat_join_mode,
    )?;
    insert_unsigned(
        &mut values,
        "local_chat_join_mode",
        social.local_chat_join_mode,
    )?;
    insert_unsigned(
        &mut values,
        "clan_chat_join_mode",
        social.clan_chat_join_mode,
    )?;
    insert_unsigned(
        &mut values,
        "chat_auto_hide_mode",
        social.chat_auto_hide_mode,
    )?;

    insert(
        &mut values,
        "key_binding_source",
        AccountSettingValue::text(match key_binding_source {
            0 => "account",
            1 => "computer",
            value => {
                return Err(SqliteAccountError::invalid_data(
                    "account_state.settings_payload.key_binding_source",
                    format!("expected 0 or 1, found {value}"),
                ));
            }
        }),
    )?;
    for (action, primary, secondary) in bindings {
        values.insert(
            AccountSettingKey::key_binding(action, KeyBindingSlot::Primary),
            primary.map_or(
                AccountSettingValue::Unassigned,
                AccountSettingValue::InputCode,
            ),
        );
        values.insert(
            AccountSettingKey::key_binding(action, KeyBindingSlot::Secondary),
            secondary.map_or(
                AccountSettingValue::Unassigned,
                AccountSettingValue::InputCode,
            ),
        );
    }

    AccountSettingsState::try_new(
        AccountSettingsCapabilities {
            writable: false,
            named_key_bindings_writable: false,
            extended_field_of_view: false,
        },
        values,
    )
    .map_err(Into::into)
}

pub(super) fn encode(state: &AccountSettingsState) -> Result<Vec<u8>, SqliteAccountError> {
    let mut writer = PayloadWriter::default();
    writer.u32(SETTINGS_PAYLOAD_VERSION)?;
    for name in [
        "button_layout",
        "movement_mode",
        "controller_look_sensitivity",
    ] {
        writer.i8(setting_i8(state, name)?)?;
    }
    for name in [
        "controller_invert_vertical",
        "controller_auto_look_centering",
        "controller_vibration",
        "controller_swap_shoulders",
        "controller_invert_horizontal",
    ] {
        writer.boolean(setting_bool(state, name)?)?;
    }
    writer.i32(setting_i32(state, "mouse_look_sensitivity")?)?;
    for name in [
        "mouse_invert_vertical",
        "mouse_invert_horizontal",
        "unidentified_toggle",
        "mouse_aim_smoothing",
    ] {
        writer.boolean(setting_bool(state, name)?)?;
    }
    writer.f32(setting_f32(state, "ads_sensitivity_modifier")?)?;
    writer.i8(setting_i8(state, "double_press_delay")?)?;

    for name in ["voice_output_mode", "team_voice_channel", "reserved_mode"] {
        writer.i8(setting_i8(state, name)?)?;
    }
    writer.i8(8)?;
    writer.i8(setting_i8(state, "chat_volume")?)?;
    writer.boolean(setting_bool(state, "mute_when_unfocused")?)?;
    for name in ["sound_effects_volume", "dialogue_volume", "music_volume"] {
        writer.i8(setting_i8(state, name)?)?;
    }

    writer.i8(setting_i8(state, "brightness")?)?;
    writer.boolean(setting_bool(state, "show_fps")?)?;
    writer.i8(setting_i8(state, "hdr_mode")?)?;
    writer.u8(setting_u8(state, "vertical_sync_interval")?)?;
    writer.i32(setting_i32(state, "field_of_view")?)?;
    writer.f32(10_000.0)?;
    writer.f32(0.0)?;

    for name in [
        "subtitles_mode",
        "colorblind_mode",
        "helmet_mode",
        "hud_opacity",
    ] {
        writer.i8(setting_i8(state, name)?)?;
    }
    writer.boolean(setting_bool(state, "display_hints")?)?;
    for name in [
        "background_opacity",
        "reticle_location",
        "reticle_color",
        "text_size",
        "text_color",
        "text_background_style",
        "text_background_opacity",
    ] {
        writer.i8(setting_i8(state, name)?)?;
    }
    writer.i8(0)?;
    writer.i8(0)?;

    writer.boolean(setting_bool(state, "prefer_good_connection")?)?;
    writer.i8(setting_i8(state, "text_chat_mode")?)?;
    for name in [
        "show_real_names",
        "clan_invite_notifications",
        "profanity_filter",
        "voice_chat_enabled",
    ] {
        writer.boolean(setting_bool(state, name)?)?;
    }
    for name in [
        "whisper_chat_mode",
        "team_chat_join_mode",
        "local_chat_join_mode",
        "clan_chat_join_mode",
        "chat_auto_hide_mode",
    ] {
        writer.i8(setting_i8(state, name)?)?;
    }

    let binding_source = match setting(state, "key_binding_source")? {
        AccountSettingValue::Text(value) if value.as_ref() == "account" => 0,
        AccountSettingValue::Text(value) if value.as_ref() == "computer" => 1,
        _ => return Err(invalid_encoded_setting("key_binding_source")),
    };
    writer.u8(binding_source)?;
    writer.boolean(true)?;
    for action in KEY_BINDING_ACTIONS {
        for slot in [KeyBindingSlot::Primary, KeyBindingSlot::Secondary] {
            let key = AccountSettingKey::key_binding(action, slot);
            let value = state
                .values()
                .get(&key)
                .ok_or_else(|| invalid_encoded_setting(action))?;
            let code = match value {
                AccountSettingValue::InputCode(code) => Some(*code),
                AccountSettingValue::Unassigned => None,
                _ => return Err(invalid_encoded_setting(action)),
            };
            writer.optional_u16(code)?;
        }
    }
    writer.boolean(true)?;
    Ok(writer.bytes)
}

fn setting<'a>(
    state: &'a AccountSettingsState,
    name: &str,
) -> Result<&'a AccountSettingValue, SqliteAccountError> {
    let key =
        AccountSettingKey::known_preference(name).ok_or_else(|| invalid_encoded_setting(name))?;
    state
        .values()
        .get(&key)
        .ok_or_else(|| invalid_encoded_setting(name))
}

fn setting_bool(state: &AccountSettingsState, name: &str) -> Result<bool, SqliteAccountError> {
    match setting(state, name)? {
        AccountSettingValue::Boolean(value) => Ok(*value),
        _ => Err(invalid_encoded_setting(name)),
    }
}

fn setting_u64(state: &AccountSettingsState, name: &str) -> Result<u64, SqliteAccountError> {
    match setting(state, name)? {
        AccountSettingValue::Unsigned(value) => Ok(*value),
        _ => Err(invalid_encoded_setting(name)),
    }
}

fn setting_u8(state: &AccountSettingsState, name: &str) -> Result<u8, SqliteAccountError> {
    u8::try_from(setting_u64(state, name)?).map_err(|_| invalid_encoded_setting(name))
}

fn setting_i8(state: &AccountSettingsState, name: &str) -> Result<i8, SqliteAccountError> {
    i8::try_from(setting_u64(state, name)?).map_err(|_| invalid_encoded_setting(name))
}

fn setting_i32(state: &AccountSettingsState, name: &str) -> Result<i32, SqliteAccountError> {
    i32::try_from(setting_u64(state, name)?).map_err(|_| invalid_encoded_setting(name))
}

fn setting_f32(state: &AccountSettingsState, name: &str) -> Result<f32, SqliteAccountError> {
    match setting(state, name)? {
        AccountSettingValue::Decimal(value) => {
            let value = value.get();
            if value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX) {
                Ok(value as f32)
            } else {
                Err(invalid_encoded_setting(name))
            }
        }
        _ => Err(invalid_encoded_setting(name)),
    }
}

fn invalid_encoded_setting(name: &str) -> SqliteAccountError {
    SqliteAccountError::invalid_data(
        format!("account_state.settings_payload.{name}"),
        "value cannot be represented by the PR-88 settings payload",
    )
}

#[derive(Default)]
struct PayloadWriter {
    bytes: Vec<u8>,
}

impl PayloadWriter {
    fn reserve(&self, width: usize) -> Result<(), SqliteAccountError> {
        if self.bytes.len().saturating_add(width) <= SETTINGS_PAYLOAD_CAPACITY {
            Ok(())
        } else {
            Err(SqliteAccountError::invalid_data(
                "account_state.settings_payload",
                "encoded payload exceeds its fixed capacity",
            ))
        }
    }

    fn u8(&mut self, value: u8) -> Result<(), SqliteAccountError> {
        self.reserve(1)?;
        self.bytes.push(value);
        Ok(())
    }

    fn boolean(&mut self, value: bool) -> Result<(), SqliteAccountError> {
        self.u8(u8::from(value))
    }

    fn u16(&mut self, value: u16) -> Result<(), SqliteAccountError> {
        self.reserve(2)?;
        self.bytes.extend(value.to_le_bytes());
        Ok(())
    }

    fn optional_u16(&mut self, value: Option<u16>) -> Result<(), SqliteAccountError> {
        self.boolean(value.is_some())?;
        self.u16(value.unwrap_or(0))
    }

    fn u32(&mut self, value: u32) -> Result<(), SqliteAccountError> {
        self.reserve(4)?;
        self.bytes.extend(value.to_le_bytes());
        Ok(())
    }

    fn i8(&mut self, value: i8) -> Result<(), SqliteAccountError> {
        self.u8(value.to_le_bytes()[0])
    }

    fn i32(&mut self, value: i32) -> Result<(), SqliteAccountError> {
        self.u32(u32::from_le_bytes(value.to_le_bytes()))
    }

    fn f32(&mut self, value: f32) -> Result<(), SqliteAccountError> {
        self.u32(value.to_bits())
    }
}

fn insert(
    values: &mut BTreeMap<AccountSettingKey, AccountSettingValue>,
    name: &str,
    value: AccountSettingValue,
) -> Result<(), SqliteAccountError> {
    let key = AccountSettingKey::known_preference(name).ok_or_else(|| {
        SqliteAccountError::invalid_data(
            "account_state.settings_payload",
            format!("Sundial does not register the PR-88 setting {name}"),
        )
    })?;
    values.insert(key, value);
    Ok(())
}

fn insert_bool(
    values: &mut BTreeMap<AccountSettingKey, AccountSettingValue>,
    name: &str,
    value: bool,
) -> Result<(), SqliteAccountError> {
    insert(values, name, AccountSettingValue::Boolean(value))
}

fn insert_unsigned(
    values: &mut BTreeMap<AccountSettingKey, AccountSettingValue>,
    name: &str,
    value: i8,
) -> Result<(), SqliteAccountError> {
    let value = u64::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data(
            format!("account_state.settings_payload.{name}"),
            "value must be nonnegative",
        )
    })?;
    insert(values, name, AccountSettingValue::Unsigned(value))
}

fn insert_unsigned_i32(
    values: &mut BTreeMap<AccountSettingKey, AccountSettingValue>,
    name: &str,
    value: i32,
) -> Result<(), SqliteAccountError> {
    let value = u64::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data(
            format!("account_state.settings_payload.{name}"),
            "value must be nonnegative",
        )
    })?;
    insert(values, name, AccountSettingValue::Unsigned(value))
}

struct PayloadReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u8(&mut self) -> Result<u8, SqliteAccountError> {
        let value = self.bytes.get(self.offset).copied().ok_or_else(|| {
            SqliteAccountError::invalid_data(
                "account_state.settings_payload",
                format!("payload ends at byte {}", self.offset),
            )
        })?;
        self.offset += 1;
        Ok(value)
    }

    fn boolean(&mut self) -> Result<bool, SqliteAccountError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(SqliteAccountError::invalid_data(
                "account_state.settings_payload",
                format!("boolean at byte {} is {value}", self.offset - 1),
            )),
        }
    }

    fn u16(&mut self) -> Result<u16, SqliteAccountError> {
        let low = self.u8()?;
        let high = self.u8()?;
        Ok(u16::from_le_bytes([low, high]))
    }

    fn u32(&mut self) -> Result<u32, SqliteAccountError> {
        let byte0 = self.u8()?;
        let byte1 = self.u8()?;
        let byte2 = self.u8()?;
        let byte3 = self.u8()?;
        Ok(u32::from_le_bytes([byte0, byte1, byte2, byte3]))
    }

    fn i8(&mut self) -> Result<i8, SqliteAccountError> {
        Ok(i8::from_le_bytes([self.u8()?]))
    }

    fn i32(&mut self) -> Result<i32, SqliteAccountError> {
        Ok(i32::from_le_bytes(self.u32()?.to_le_bytes()))
    }

    fn f32(&mut self) -> Result<f32, SqliteAccountError> {
        let value = f32::from_bits(self.u32()?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(SqliteAccountError::invalid_data(
                "account_state.settings_payload",
                "floating-point value must be finite",
            ))
        }
    }

    fn optional_u16(&mut self) -> Result<Option<u16>, SqliteAccountError> {
        let present = self.boolean()?;
        let value = self.u16()?;
        Ok(present.then_some(value))
    }

    const fn complete(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

struct Controls {
    button_layout: i8,
    movement_mode: i8,
    controller_look_sensitivity: i8,
    controller_invert_vertical: bool,
    controller_auto_look_centering: bool,
    controller_vibration: bool,
    controller_swap_shoulders: bool,
    controller_invert_horizontal: bool,
    mouse_look_sensitivity: i32,
    mouse_invert_vertical: bool,
    mouse_invert_horizontal: bool,
    unidentified_toggle: bool,
    mouse_aim_smoothing: bool,
    ads_sensitivity_modifier: f32,
    double_press_delay: i8,
}

struct Audio {
    voice_output_mode: i8,
    team_voice_channel: i8,
    reserved_mode: i8,
    migration_version: i8,
    chat_volume: i8,
    mute_when_unfocused: bool,
    sound_effects_volume: i8,
    dialogue_volume: i8,
    music_volume: i8,
}

struct Display {
    brightness: i8,
    show_fps: bool,
    hdr_mode: i8,
    vertical_sync_interval: u8,
    field_of_view: i32,
    calibration_primary: f32,
    calibration_alpha: f32,
}

struct Interface {
    subtitles_mode: i8,
    colorblind_mode: i8,
    helmet_mode: i8,
    hud_opacity: i8,
    display_hints: bool,
    background_opacity: i8,
    reticle_location: i8,
    reticle_color: i8,
    text_size: i8,
    text_color: i8,
    text_background_style: i8,
    text_background_opacity: i8,
    reserved_text_mode: i8,
    subtitle_options_entry: i8,
}

struct Social {
    prefer_good_connection: bool,
    text_chat_mode: i8,
    show_real_names: bool,
    clan_invite_notifications: bool,
    profanity_filter: bool,
    voice_chat_enabled: bool,
    whisper_chat_mode: i8,
    team_chat_join_mode: i8,
    local_chat_join_mode: i8,
    clan_chat_join_mode: i8,
    chat_auto_hide_mode: i8,
}
