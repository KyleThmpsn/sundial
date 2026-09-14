//! Behavioral coverage for the inventory document contract and mutation facade.

use super::document::{LEGACY_DISMANTLE_REWARD_CAPACITY, NO_DEFINITION_HASH};
use super::*;
use serde_json::json;

mod dismantle_validation;
mod identity;
mod movement_equipment;
mod schema_profile;

fn item(soid: u64, hash: u32) -> Value {
    json!({
        "instance_soid": format_instance_soid(soid),
        "definition_hash": format_definition_hash_hex(hash),
        "level": 106,
        "quantity": 1,
        "plugs": null
    })
}

fn document(version: u64) -> Value {
    json!({
        "version": version,
        "state": {
            "account": {
                "primary_soid": "0x9EAA300100100100",
                "profile_items": []
            },
            "characters": [{
                "soid": "0x9EAA300200100100",
                "class": 0,
                "equipment": {},
                "inventory": []
            }]
        }
    })
}

fn add_character(document: &mut Value, soid: u64, class_type: u64) {
    document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .expect("test document has a characters array")
        .push(json!({
            "soid": format_instance_soid(soid),
            "class": class_type,
            "equipment": {},
            "inventory": []
        }));
}
