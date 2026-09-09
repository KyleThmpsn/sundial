use crate::app::equipment::displayed_plugs;
use crate::app::equipment::equip_definition;
use crate::app::equipment::materialize_authored_plugs;
use crate::app::equipment::native_plug_default;
use crate::app::equipment::selected_attunement_index;
use crate::app::equipment::set_weapon_slot_empty;
use crate::app::item_editor::NativePlugDefault;
use crate::app::settings::validate_characters;
use crate::app::*;
use crate::catalog;
use crate::catalog::AbilityChoice;

#[test]
fn sunrise_native_plugs_are_displayed_and_materialized_on_edit() {
    let defaults = vec![Some("0x0000002A".into()), None, Some("0x0000002B".into())];
    let mut plugs = Value::Null;

    let (displayed, native_defaults) = displayed_plugs(Some(&plugs), &defaults);
    assert!(native_defaults);
    assert_eq!(
        displayed,
        serde_json::json!(["0x0000002A", null, "0x0000002B"])
            .as_array()
            .unwrap()
            .clone()
    );

    let authored_defaults = Value::Array(displayed.clone());
    let (_, native_defaults) = displayed_plugs(Some(&authored_defaults), &defaults);
    assert!(native_defaults);

    let authored_override = serde_json::json!(["0x0000002A", "0x0000002C", "0x0000002B"]);
    let (_, native_defaults) = displayed_plugs(Some(&authored_override), &defaults);
    assert!(!native_defaults);

    let authored = materialize_authored_plugs(&mut plugs, &defaults).unwrap();
    authored[1] = Value::String("0x0000002C".into());
    assert_eq!(
        plugs,
        serde_json::json!(["0x0000002A", "0x0000002C", "0x0000002B"])
    );
}

#[test]
fn native_socket_defaults_distinguish_explicit_empty_from_unusable_values() {
    let defaults = vec![Some("0x0000002A".into()), None, Some("invalid".into())];

    assert_eq!(
        native_plug_default(&defaults, 0),
        Some(NativePlugDefault::Plug(42))
    );
    assert_eq!(
        native_plug_default(&defaults, 1),
        Some(NativePlugDefault::Empty)
    );
    assert_eq!(native_plug_default(&defaults, 2), None);
    assert_eq!(native_plug_default(&defaults, 3), None);
}

#[test]
fn weapon_slots_can_be_emptied_and_equipped_again() {
    let mut document = serde_json::json!({
        "state": {
            "characters": [{
                "soid": 1,
                "equipment": {
                    "kinetic": {
                        "instance_soid": "0x4000000000000001",
                        "definition_hash": "0x0000002A",
                        "level": 67,
                        "quantity": 1,
                        "plugs": [],
                        "preserved_until_emptied": true
                    },
                    "energy": {
                        "instance_soid": "0x4000000000000002",
                        "definition_hash": "0x0000002B",
                        "level": 67,
                        "quantity": 1,
                        "plugs": []
                    }
                },
                "untouched": "kept"
            }]
        }
    });

    set_weapon_slot_empty(&mut document, 0, "kinetic").unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&Value::Null)
    );
    assert_eq!(
        document.pointer("/state/characters/0/untouched"),
        Some(&Value::String("kept".into()))
    );

    let defaults = vec![Some("0x00000030".into()), None];
    equip_definition(&mut document, 0, "kinetic", 0x2C, &defaults).unwrap();
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&serde_json::json!({
            "instance_soid": "0x4000000000000001",
            "definition_hash": "0x0000002C",
            "level": 67,
            "quantity": 1,
            "plugs": ["0x00000030", null]
        }))
    );
    assert_eq!(validate_characters(&document), Ok(()));
}

#[test]
fn empty_weapon_action_never_overwrites_unexpected_slot_data() {
    let mut document = serde_json::json!({
        "state": { "characters": [{ "equipment": { "kinetic": "unexpected" } }] }
    });

    assert!(set_weapon_slot_empty(&mut document, 0, "kinetic").is_err());
    assert_eq!(
        document.pointer("/state/characters/0/equipment/kinetic"),
        Some(&Value::String("unexpected".into()))
    );
    assert!(set_weapon_slot_empty(&mut document, 0, "helmet").is_err());
}

#[test]
fn distinctive_super_selection_wins_when_old_attunements_are_mixed() {
    let choice = |entry, name: &str| AbilityChoice {
        entry,
        name: name.into(),
    };
    let abilities = catalog::AbilityOptions {
        attunements: vec![
            catalog::AttunementChoice {
                name: "Top".into(),
                super_abilities: vec![choice(10, "Base super")],
                melee: choice(11, "Top melee"),
                perks: vec![choice(13, "Former top selector")],
            },
            catalog::AttunementChoice {
                name: "Bottom".into(),
                super_abilities: vec![choice(10, "Base super")],
                melee: choice(15, "Bottom melee"),
                perks: vec![choice(18, "Former bottom selector")],
            },
            catalog::AttunementChoice {
                name: "Middle".into(),
                super_abilities: vec![choice(20, "Middle super")],
                melee: choice(21, "Middle melee"),
                perks: Vec::new(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(selected_attunement_index(&abilities, 10, 15), 1);
    assert_eq!(selected_attunement_index(&abilities, 20, 15), 2);
    assert_eq!(selected_attunement_index(&abilities, 18, 21), 1);
}
