CREATE TABLE account_preferences (id INTEGER PRIMARY KEY CHECK(id=1), key_binding_source INTEGER NOT NULL CHECK(key_binding_source IN(0,1))) STRICT;

CREATE TABLE account_controls (
    id INTEGER PRIMARY KEY CHECK(id=1),
    button_layout INTEGER NOT NULL,
    movement_mode INTEGER NOT NULL,
    controller_look_sensitivity INTEGER NOT NULL,
    controller_invert_vertical INTEGER NOT NULL CHECK(controller_invert_vertical IN(0,1)),
    controller_auto_look_centering INTEGER NOT NULL CHECK(controller_auto_look_centering IN(0,1)),
    controller_vibration INTEGER NOT NULL CHECK(controller_vibration IN(0,1)),
    controller_swap_shoulders INTEGER NOT NULL CHECK(controller_swap_shoulders IN(0,1)),
    controller_invert_horizontal INTEGER NOT NULL CHECK(controller_invert_horizontal IN(0,1)),
    mouse_look_sensitivity INTEGER NOT NULL,
    mouse_invert_vertical INTEGER NOT NULL CHECK(mouse_invert_vertical IN(0,1)),
    mouse_invert_horizontal INTEGER NOT NULL CHECK(mouse_invert_horizontal IN(0,1)),
    unidentified_toggle INTEGER NOT NULL CHECK(unidentified_toggle IN(0,1)),
    mouse_aim_smoothing INTEGER NOT NULL CHECK(mouse_aim_smoothing IN(0,1)),
    ads_sensitivity_modifier REAL NOT NULL,
    double_press_delay INTEGER NOT NULL
) STRICT;

CREATE TABLE account_audio (
    id INTEGER PRIMARY KEY CHECK(id=1),
    voice_output_mode INTEGER NOT NULL,
    team_voice_channel INTEGER NOT NULL,
    reserved_mode INTEGER NOT NULL,
    migration_version INTEGER NOT NULL,
    chat_volume INTEGER NOT NULL,
    mute_when_unfocused INTEGER NOT NULL CHECK(mute_when_unfocused IN(0,1)),
    sound_effects_volume INTEGER NOT NULL,
    dialogue_volume INTEGER NOT NULL,
    music_volume INTEGER NOT NULL
) STRICT;

CREATE TABLE account_display (
    id INTEGER PRIMARY KEY CHECK(id=1),
    brightness INTEGER NOT NULL,
    show_fps INTEGER NOT NULL CHECK(show_fps IN(0,1)),
    hdr_mode INTEGER NOT NULL,
    vertical_sync_interval INTEGER NOT NULL,
    field_of_view INTEGER NOT NULL,
    calibration_primary REAL NOT NULL,
    calibration_alpha REAL NOT NULL
) STRICT;

CREATE TABLE account_interface (
    id INTEGER PRIMARY KEY CHECK(id=1),
    subtitles_mode INTEGER NOT NULL,
    colorblind_mode INTEGER NOT NULL,
    helmet_mode INTEGER NOT NULL,
    hud_opacity INTEGER NOT NULL,
    display_hints INTEGER NOT NULL CHECK(display_hints IN(0,1)),
    background_opacity INTEGER NOT NULL,
    reticle_location INTEGER NOT NULL,
    reticle_color INTEGER NOT NULL,
    text_size INTEGER NOT NULL,
    text_color INTEGER NOT NULL,
    text_background_style INTEGER NOT NULL,
    text_background_opacity INTEGER NOT NULL,
    reserved_text_mode INTEGER NOT NULL,
    subtitle_options_entry INTEGER NOT NULL
) STRICT;

CREATE TABLE account_social (
    id INTEGER PRIMARY KEY CHECK(id=1),
    prefer_good_connection INTEGER NOT NULL CHECK(prefer_good_connection IN(0,1)),
    text_chat_mode INTEGER NOT NULL,
    show_real_names INTEGER NOT NULL CHECK(show_real_names IN(0,1)),
    clan_invite_notifications INTEGER NOT NULL CHECK(clan_invite_notifications IN(0,1)),
    profanity_filter INTEGER NOT NULL CHECK(profanity_filter IN(0,1)),
    voice_chat_enabled INTEGER NOT NULL CHECK(voice_chat_enabled IN(0,1)),
    whisper_chat_mode INTEGER NOT NULL,
    team_chat_join_mode INTEGER NOT NULL,
    local_chat_join_mode INTEGER NOT NULL,
    clan_chat_join_mode INTEGER NOT NULL,
    chat_auto_hide_mode INTEGER NOT NULL
) STRICT;

CREATE TABLE account_key_bindings (action INTEGER PRIMARY KEY CHECK(action BETWEEN 0 AND 59), primary_code INTEGER NOT NULL CHECK(primary_code BETWEEN -1 AND 65535), secondary_code INTEGER NOT NULL CHECK(secondary_code BETWEEN -1 AND 65535)) STRICT;
