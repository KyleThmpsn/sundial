//! Official Sunrise contract at 169fd296f27e30d8edb29ed2d866aa5b3db1dd81.

pub(super) const SCHEMA_VERSION: i64 = 2;
pub(super) const CHARACTER_CAPACITY: usize = crate::account_contract::CHARACTER_CAPACITY;
pub(super) const PROFILE_ITEM_CAPACITY: usize = crate::account_contract::PROFILE_ITEM_CAPACITY;
pub(super) const DISMANTLE_REWARD_CAPACITY: usize =
    crate::account_contract::FILTERED_DISMANTLE_REWARD_CAPACITY;
pub(super) const CHARACTER_ITEM_CAPACITY: usize =
    crate::account_contract::CHARACTER_INVENTORY_CAPACITY;
pub(super) const PLUG_CAPACITY: usize = crate::account_contract::MAX_ITEM_PLUGS;

pub(super) const EQUIPMENT_LOCATION: i64 = 0;
pub(super) const INVENTORY_LOCATION: i64 = 1;

pub(super) const EQUIPMENT_SLOTS: [&str; 17] = [
    "kinetic",
    "energy",
    "heavy",
    "helmet",
    "gauntlets",
    "chest",
    "legs",
    "class_item",
    "ghost",
    "vehicle",
    "ship",
    "subclass",
    "clan_banner",
    "emblem",
    "emote",
    "finisher",
    "artifact",
];

pub(super) const KEY_BINDING_ACTIONS: [&str; 60] = [
    "fire",
    "toggle_zoom",
    "hold_zoom",
    "melee",
    "grenade",
    "super",
    "reload",
    "light_attack",
    "heavy_attack",
    "block",
    "switch_weapons",
    "next_weapon",
    "previous_weapon",
    "primary_weapon",
    "special_weapon",
    "heavy_weapon",
    "move_forward",
    "move_backward",
    "move_left",
    "move_right",
    "jump",
    "toggle_crouch",
    "hold_crouch",
    "toggle_sprint",
    "hold_sprint",
    "vehicle_boost",
    "vehicle_brake",
    "vehicle_zoom",
    "vehicle_fire_primary",
    "vehicle_fire_secondary",
    "vehicle_exit",
    "interact",
    "highlight_player",
    "emote_1",
    "emote_2",
    "emote_3",
    "emote_4",
    "air_move",
    "class_ability",
    "death_cam_zoom_in",
    "death_cam_zoom_out",
    "push_to_talk",
    "ui_gamepad_button_back",
    "ui_open_director",
    "ui_open_director_store_tab",
    "ui_open_director_pursuits_tab",
    "ui_open_director_map_tab",
    "ui_open_director_destinations_tab",
    "ui_open_director_roster_tab",
    "ui_open_director_seasons_tab",
    "ui_open_start_menu_alternative",
    "ui_open_start_menu_records_tab",
    "ui_open_start_menu_collections_tab",
    "ui_open_start_menu_clan_tab",
    "ui_open_start_menu_inventory_tab",
    "ui_open_start_menu_settings_tab",
    "ui_open_exit_dialog_confirm",
    "ui_abort_activity",
    "ui_text_chat_toggle_state",
    "screenshot",
];

pub(super) const APPLICATION_ID: i64 = 1397902921;
pub(super) const SCHEMA: &str = include_str!("fixtures/investment_schema.sql");
pub(super) const SETTINGS_SCHEMA: &str = include_str!("fixtures/account_settings_schema.sql");
