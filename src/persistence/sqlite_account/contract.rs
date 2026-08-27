//! Provisional SQLite contract copied from Sunrise PR 88 at one pinned commit.

#[cfg(test)]
pub(crate) const PR_88_COMMIT: &str = "5a5583ab0cc4244bca11974a928bdc1a0b49f4b7";
pub(super) const SCHEMA_VERSION: i64 = 1;
pub(super) const ACCOUNT_FORMAT_VERSION: i64 = 1;
pub(super) const SETTINGS_PAYLOAD_VERSION: u32 = 1;
pub(super) const SETTINGS_PAYLOAD_CAPACITY: usize = 1024;
pub(super) const CHARACTER_CAPACITY: usize = 3;
pub(super) const PROFILE_ITEM_CAPACITY: usize = 701;
pub(super) const DISMANTLE_REWARD_CAPACITY: usize = 32;
pub(super) const CHARACTER_ITEM_CAPACITY: usize = 135;
pub(super) const PLUG_CAPACITY: usize = 12;

pub(super) const EQUIPMENT_LOCATION: i64 = 0;
pub(super) const INVENTORY_LOCATION: i64 = 1;

pub(super) const EQUIPMENT_SLOTS: [&str; 16] = [
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

#[derive(Clone, Copy)]
pub(super) struct ColumnContract {
    pub name: &'static str,
    pub declared_type: &'static str,
    pub not_null: bool,
    pub primary_key_position: i64,
}

macro_rules! column {
    ($name:literal, $type:literal, $not_null:literal, $primary_key:literal) => {
        ColumnContract {
            name: $name,
            declared_type: $type,
            not_null: $not_null,
            primary_key_position: $primary_key,
        }
    };
}

pub(super) const ACCOUNT_STATE_COLUMNS: &[ColumnContract] = &[
    column!("singleton", "INTEGER", false, 1),
    column!("format_version", "INTEGER", true, 0),
    column!("primary_soid", "INTEGER", true, 0),
    column!("dismantle_reward_count", "INTEGER", true, 0),
    column!("profile_item_count", "INTEGER", true, 0),
    column!("character_count", "INTEGER", true, 0),
    column!("settings_payload", "BLOB", true, 0),
    column!("updated_unix_seconds", "INTEGER", true, 0),
];

pub(super) const DISMANTLE_REWARD_COLUMNS: &[ColumnContract] = &[
    column!("account_id", "INTEGER", true, 1),
    column!("position", "INTEGER", true, 2),
    column!("definition_hash", "INTEGER", true, 0),
    column!("quantity", "INTEGER", true, 0),
    column!("tier_mask", "INTEGER", true, 0),
    column!("class_mask", "INTEGER", true, 0),
    column!("masterwork", "INTEGER", true, 0),
];

pub(super) const PROFILE_ITEM_COLUMNS: &[ColumnContract] = &[
    column!("account_id", "INTEGER", true, 1),
    column!("position", "INTEGER", true, 2),
    column!("instance_soid", "INTEGER", true, 0),
    column!("definition_hash", "INTEGER", true, 0),
    column!("quantity", "INTEGER", true, 0),
    column!("mutation_serial", "INTEGER", true, 0),
];

pub(super) const CHARACTER_COLUMNS: &[ColumnContract] = &[
    column!("account_id", "INTEGER", true, 1),
    column!("position", "INTEGER", true, 2),
    column!("soid", "INTEGER", true, 0),
    column!("selected", "INTEGER", true, 0),
    column!("race", "INTEGER", true, 0),
    column!("gender", "INTEGER", true, 0),
    column!("character_class", "INTEGER", true, 0),
    column!("level", "INTEGER", true, 0),
    column!("accepted", "INTEGER", true, 0),
    column!("preview_available", "INTEGER", true, 0),
    column!("appearance_value", "REAL", true, 0),
    column!("last_orbited_destination", "INTEGER", true, 0),
    column!("content_bypass", "INTEGER", true, 0),
    column!("acquired_subclass_ability_mask", "INTEGER", true, 0),
    column!("inventory_count", "INTEGER", true, 0),
    column!("next_inventory_serial", "INTEGER", true, 0),
];

pub(super) const CHARACTER_ITEM_COLUMNS: &[ColumnContract] = &[
    column!("account_id", "INTEGER", true, 1),
    column!("character_position", "INTEGER", true, 2),
    column!("location", "INTEGER", true, 3),
    column!("position", "INTEGER", true, 4),
    column!("instance_soid", "INTEGER", true, 0),
    column!("definition_hash", "INTEGER", true, 0),
    column!("item_level", "INTEGER", true, 0),
    column!("quantity", "INTEGER", true, 0),
    column!("mutation_serial", "INTEGER", true, 0),
    column!("flags", "INTEGER", true, 0),
    column!("socket_policy", "INTEGER", true, 0),
    column!("plug_count", "INTEGER", true, 0),
    column!("movement_ability_entry", "INTEGER", true, 0),
    column!("grenade_ability_entry", "INTEGER", true, 0),
    column!("super_ability_entry", "INTEGER", true, 0),
    column!("melee_ability_entry", "INTEGER", true, 0),
    column!("class_ability_entry", "INTEGER", true, 0),
];

pub(super) const ITEM_PLUG_COLUMNS: &[ColumnContract] = &[
    column!("account_id", "INTEGER", true, 1),
    column!("character_position", "INTEGER", true, 2),
    column!("location", "INTEGER", true, 3),
    column!("item_position", "INTEGER", true, 4),
    column!("plug_position", "INTEGER", true, 5),
    column!("definition_hash", "INTEGER", false, 0),
];

pub(super) const TABLES: [(&str, &[ColumnContract]); 6] = [
    ("account_state", ACCOUNT_STATE_COLUMNS),
    ("dismantle_rewards", DISMANTLE_REWARD_COLUMNS),
    ("profile_items", PROFILE_ITEM_COLUMNS),
    ("characters", CHARACTER_COLUMNS),
    ("character_items", CHARACTER_ITEM_COLUMNS),
    ("item_plugs", ITEM_PLUG_COLUMNS),
];
