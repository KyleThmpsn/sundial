//! Account preference groups and their Sunrise-compatible choices.

use eframe::egui;
use serde_json::{Map, Value};

use super::{
    page::{group, missing_group},
    schema::{
        FIELD_OF_VIEW_KEY, FIELD_OF_VIEW_MAXIMUM, FIELD_OF_VIEW_MINIMUM,
        VERTICAL_SYNC_INTERVAL_KEY, show_presence_gated_preference,
    },
    widgets::{
        CommandBatch, boolean, choice, display_refresh_rate_hz, fixed, float_slider,
        integer_slider, offset_slider, vertical_sync_intervals,
    },
};

pub(super) const BUTTON_LAYOUTS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Green Thumb"),
    (2, "Puppeteer"),
    (3, "Mirror"),
    (5, "Jumper"),
    (6, "Cold Shoulder"),
    (9, "Custom"),
];

pub(super) const KEY_BINDING_SOURCES: &[(&str, &str)] =
    &[("account", "Account"), ("computer", "Computer (Default)")];

pub(super) const STICK_LAYOUTS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Southpaw"),
    (2, "Legacy"),
    (3, "Legacy Southpaw"),
];
pub(super) const DOUBLE_PRESS_DELAYS: &[(u64, &str)] = &[
    (0, "1 - 167 ms (Default)"),
    (1, "2 - 212 ms"),
    (2, "3 - 302 ms"),
    (3, "4 - 347 ms"),
    (4, "5 - 392 ms"),
];
pub(super) const VOICE_OUTPUT_MODES: &[(u64, &str)] = &[
    (0, "Blended"),
    (1, "Headset Only (Default)"),
    (2, "Speakers Only"),
];
pub(super) const TEAM_VOICE_MODES: &[(u64, &str)] = &[
    (0, "Manually Opt-in (Default)"),
    (1, "Automatic Opt-in When Solo"),
];
pub(super) const RESERVED_AUDIO_STATES: &[(u64, &str)] =
    &[(0, "State 0 (Default)"), (1, "State 1")];
pub(super) const HDR_MODES: &[(u64, &str)] = &[(0, "Off (Default)"), (1, "On")];
pub(super) const SUBTITLE_MODES: &[(u64, &str)] =
    &[(0, "Language-Based (Default)"), (1, "On"), (2, "Off")];
pub(super) const COLORBLIND_MODES: &[(u64, &str)] = &[
    (0, "Off (Default)"),
    (1, "Deuteranopia (Red-Green)"),
    (2, "Protanopia (Red-Green)"),
    (3, "Tritanopia (Yellow-Blue)"),
];
pub(super) const HELMET_MODES: &[(u64, &str)] = &[(0, "Off in Non-Combat Zones"), (1, "Always On")];
pub(super) const HUD_OPACITY: &[(u64, &str)] =
    &[(0, "Off"), (1, "Low"), (2, "High"), (3, "Full (Default)")];
pub(super) const BACKGROUND_OPACITY: &[(u64, &str)] = &[
    (0, "Lowest"),
    (1, "Low"),
    (2, "Medium (Default)"),
    (3, "High"),
    (4, "Highest"),
];
pub(super) const RETICLE_LOCATIONS: &[(u64, &str)] = &[(0, "PC Default"), (1, "Console Default")];
pub(super) const RETICLE_COLORS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Red"),
    (2, "Green"),
    (3, "Yellow"),
    (4, "Blue"),
    (5, "Purple"),
    (6, "Cyan"),
];
pub(super) const GAME_LANGUAGES: &[(&str, &str)] = &[
    ("english", "English"),
    ("french", "French"),
    ("german", "German"),
    ("italian", "Italian"),
    ("japanese", "Japanese"),
    ("brazilian", "Portuguese (Brazil)"),
    ("spanish", "Spanish (Spain)"),
    ("russian", "Russian"),
    ("polish", "Polish"),
    ("schinese", "Chinese (Simplified)"),
    ("tchinese", "Chinese (Traditional)"),
    ("latam", "Spanish (Latin America)"),
    ("koreana", "Korean"),
];
pub(super) const TEXT_CHAT_MODES: &[(u64, &str)] = &[
    (0, "Off"),
    (1, "On (No Notifications)"),
    (2, "On (No Audio)"),
    (3, "On (Default)"),
];
pub(super) const WHISPER_CHAT_MODES: &[(u64, &str)] = &[(0, "On (Default)"), (1, "Off")];
pub(super) const MANUAL_AUTOMATIC: &[(u64, &str)] = &[(0, "Manual"), (1, "Automatic")];
pub(super) const AUTO_HIDE_MODES: &[(u64, &str)] = &[(0, "Off"), (1, "On")];

pub(super) fn draw_controls(ui: &mut egui::Ui, settings: &Map<String, Value>) -> CommandBatch {
    let Some(values) = group(settings, "controls") else {
        missing_group(ui, "controls");
        return CommandBatch::default();
    };
    ui.heading("Controls");
    ui.label("Controller and mouse behavior.");
    ui.add_space(8.0);
    egui::Grid::new("game_controls_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = CommandBatch::default();
            changed |= choice(ui, values, "button_layout", "Button Layout", BUTTON_LAYOUTS);
            changed |= choice(ui, values, "movement_mode", "Stick Layout", STICK_LAYOUTS);
            changed |= offset_slider(
                ui,
                values,
                "controller_look_sensitivity",
                "Controller Look Sensitivity",
                0,
                9,
                1,
            );
            changed |= boolean(
                ui,
                values,
                "controller_invert_vertical",
                "Invert Controller Vertical Look",
            );
            changed |= boolean(
                ui,
                values,
                "controller_auto_look_centering",
                "Controller Auto-Look Centering",
            );
            changed |= boolean(ui, values, "controller_vibration", "Controller Vibration");
            changed |= boolean(
                ui,
                values,
                "controller_swap_shoulders",
                "Swap Controller Shoulder Buttons",
            );
            changed |= boolean(
                ui,
                values,
                "controller_invert_horizontal",
                "Invert Controller Horizontal Look",
            );
            changed |= integer_slider(
                ui,
                values,
                "mouse_look_sensitivity",
                "Mouse Look Sensitivity",
                1,
                100,
            );
            changed |= boolean(
                ui,
                values,
                "mouse_invert_vertical",
                "Invert Mouse Vertical Look",
            );
            changed |= boolean(
                ui,
                values,
                "mouse_invert_horizontal",
                "Invert Mouse Horizontal Look",
            );
            changed |= boolean(
                ui,
                values,
                "unidentified_toggle",
                "Unidentified Control Toggle",
            );
            changed |= boolean(ui, values, "mouse_aim_smoothing", "Mouse Aim Smoothing");
            changed |= float_slider(
                ui,
                values,
                "ads_sensitivity_modifier",
                "ADS sensitivity modifier",
                0.5,
                1.5,
                0.1,
            );
            changed |= choice(
                ui,
                values,
                "double_press_delay",
                "Double-Press Delay",
                DOUBLE_PRESS_DELAYS,
            );
            changed
        })
        .inner
}

pub(super) fn draw_audio(ui: &mut egui::Ui, settings: &Map<String, Value>) -> CommandBatch {
    let Some(values) = group(settings, "audio") else {
        missing_group(ui, "audio");
        return CommandBatch::default();
    };
    ui.heading("Audio");
    ui.label("Voice, volume, and focus behavior.");
    ui.add_space(8.0);
    egui::Grid::new("game_audio_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = CommandBatch::default();
            changed |= choice(
                ui,
                values,
                "voice_output_mode",
                "Voice Output Mode",
                VOICE_OUTPUT_MODES,
            );
            changed |= choice(
                ui,
                values,
                "team_voice_channel",
                "Team Voice Channel",
                TEAM_VOICE_MODES,
            );
            changed |= choice(
                ui,
                values,
                "reserved_mode",
                "Unidentified Audio State",
                RESERVED_AUDIO_STATES,
            );
            fixed(ui, values, "migration_version", "Game Volume State");
            changed |= integer_slider(ui, values, "chat_volume", "Voice Chat Volume", 0, 8);
            changed |= boolean(ui, values, "mute_when_unfocused", "Mute when Unfocused");
            changed |= integer_slider(
                ui,
                values,
                "sound_effects_volume",
                "Sound Effects Volume",
                0,
                10,
            );
            changed |= integer_slider(ui, values, "dialogue_volume", "Dialogue Volume", 0, 10);
            changed |= integer_slider(ui, values, "music_volume", "Music Volume", 0, 10);
            changed
        })
        .inner
}

pub(super) fn draw_display(
    ui: &mut egui::Ui,
    settings: &Map<String, Value>,
    extended_fov: bool,
) -> CommandBatch {
    let Some(values) = group(settings, "display") else {
        missing_group(ui, "display");
        return CommandBatch::default();
    };
    ui.heading("Display");
    ui.label("Brightness and display overlays.");
    ui.add_space(8.0);
    egui::Grid::new("game_display_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = CommandBatch::default();
            changed |= integer_slider(ui, values, "brightness", "Brightness", 0, 6);
            changed |= boolean(ui, values, "show_fps", "Show FPS");
            changed |= choice(ui, values, "hdr_mode", "HDR Mode", HDR_MODES);
            if show_presence_gated_preference(values, VERTICAL_SYNC_INTERVAL_KEY) {
                let refresh_rate_hz = display_refresh_rate_hz();
                let intervals = vertical_sync_intervals(refresh_rate_hz);
                changed |= choice(
                    ui,
                    values,
                    VERTICAL_SYNC_INTERVAL_KEY,
                    &refresh_rate_hz.map_or_else(
                        || "Vertical Sync".to_owned(),
                        |refresh_rate_hz| {
                            format!("Vertical Sync ({refresh_rate_hz} Hz Primary Display)")
                        },
                    ),
                    &intervals,
                );
            }
            if show_presence_gated_preference(values, FIELD_OF_VIEW_KEY) {
                changed |= integer_slider(
                    ui,
                    values,
                    FIELD_OF_VIEW_KEY,
                    "Field of View",
                    FIELD_OF_VIEW_MINIMUM,
                    if extended_fov {
                        FIELD_OF_VIEW_MAXIMUM
                    } else {
                        105
                    },
                );
            }
            fixed(ui, values, "calibration_primary", "Renderer Calibration");
            fixed(
                ui,
                values,
                "calibration_alpha",
                "Renderer Calibration Alpha",
            );
            changed
        })
        .inner
}

pub(super) fn draw_interface(ui: &mut egui::Ui, settings: &Map<String, Value>) -> CommandBatch {
    let Some(values) = group(settings, "interface") else {
        missing_group(ui, "interface");
        return CommandBatch::default();
    };
    ui.heading("Interface");
    ui.label("HUD, subtitle, reticle, and text presentation.");
    ui.add_space(8.0);
    egui::Grid::new("game_interface_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = CommandBatch::default();
            changed |= choice(
                ui,
                values,
                "subtitles_mode",
                "Subtitles Mode",
                SUBTITLE_MODES,
            );
            changed |= choice(
                ui,
                values,
                "colorblind_mode",
                "Colorblind Mode",
                COLORBLIND_MODES,
            );
            changed |= choice(ui, values, "helmet_mode", "Helmet Mode", HELMET_MODES);
            changed |= choice(ui, values, "hud_opacity", "HUD Opacity", HUD_OPACITY);
            changed |= boolean(ui, values, "display_hints", "Display Hints");
            changed |= choice(
                ui,
                values,
                "background_opacity",
                "Background Opacity",
                BACKGROUND_OPACITY,
            );
            changed |= choice(
                ui,
                values,
                "reticle_location",
                "Reticle Location",
                RETICLE_LOCATIONS,
            );
            changed |= choice(ui, values, "reticle_color", "Reticle Color", RETICLE_COLORS);
            changed |= integer_slider(ui, values, "text_size", "Text Size", 0, 4);
            changed |= integer_slider(ui, values, "text_color", "Text Color", 0, 3);
            changed |= integer_slider(
                ui,
                values,
                "text_background_style",
                "Text Background Style",
                0,
                3,
            );
            changed |= integer_slider(
                ui,
                values,
                "text_background_opacity",
                "Text Background Opacity",
                0,
                4,
            );
            fixed(ui, values, "reserved_text_mode", "Reserved Text Mode");
            fixed(
                ui,
                values,
                "subtitle_options_entry",
                "Subtitle Options Entry",
            );
            changed
        })
        .inner
}

pub(super) fn draw_social(ui: &mut egui::Ui, settings: &Map<String, Value>) -> CommandBatch {
    let Some(values) = group(settings, "social") else {
        missing_group(ui, "social");
        return CommandBatch::default();
    };
    ui.heading("Social");
    ui.label("Chat, voice, names, and notifications.");
    ui.add_space(8.0);
    egui::Grid::new("game_social_grid")
        .num_columns(2)
        .spacing([18.0, 9.0])
        .striped(true)
        .show(ui, |ui| {
            let mut changed = CommandBatch::default();
            changed |= boolean(ui, values, "prefer_good_connection", "Matchmaking Search");
            changed |= choice(
                ui,
                values,
                "text_chat_mode",
                "Text Chat Mode",
                TEXT_CHAT_MODES,
            );
            changed |= boolean(ui, values, "show_real_names", "Show Real Names");
            changed |= boolean(
                ui,
                values,
                "clan_invite_notifications",
                "Clan Invite Notifications",
            );
            changed |= boolean(ui, values, "profanity_filter", "Profanity Filter");
            changed |= boolean(ui, values, "voice_chat_enabled", "Voice Chat Enabled");
            changed |= choice(
                ui,
                values,
                "whisper_chat_mode",
                "Whisper Chat Mode",
                WHISPER_CHAT_MODES,
            );
            changed |= choice(
                ui,
                values,
                "team_chat_join_mode",
                "Team Chat Join Mode",
                MANUAL_AUTOMATIC,
            );
            changed |= choice(
                ui,
                values,
                "local_chat_join_mode",
                "Local Chat Join Mode",
                MANUAL_AUTOMATIC,
            );
            changed |= choice(
                ui,
                values,
                "clan_chat_join_mode",
                "Clan Chat Join Mode",
                MANUAL_AUTOMATIC,
            );
            changed |= choice(
                ui,
                values,
                "chat_auto_hide_mode",
                "Chat Auto-Hide Mode",
                AUTO_HIDE_MODES,
            );
            changed
        })
        .inner
}
