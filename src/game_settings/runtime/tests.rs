use super::*;
use eframe::egui;
use serde_json::json;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
    ))
    .unwrap()
}

#[test]
fn sunrise_0_5_retires_four_controls_without_touching_other_runtimes_or_settings() {
    const RETIRED: [&str; 4] = [
        "/client/region_private",
        "/client/pin_replicated_record",
        "/client/skip_orbit_cinematic_wait",
        "/state/activity/arrival_overrides",
    ];

    // The schema version is 18 on both sides of the removal, so only the module version separates
    // them. Every other case keeps the controls visible rather than guessing.
    let retiring = Capabilities::detect(Some("0.5"), false);
    for path in RETIRED {
        assert!(!retiring.supports(path), "{path}");
    }
    assert!(retiring.supports("/client/socket_menu_routing"));
    assert!(retiring.supports(activity::DESTINATION));

    for (version, dawn) in [
        (Some("0.3.2"), false),
        (Some("0.4.9"), false),
        (Some("Not detected"), false),
        (None, false),
        // Dawn versions independently, so a Dawn 0.5 is not a Sunrise 0.5.
        (Some("0.5"), true),
        (Some("1.0"), true),
    ] {
        let capabilities = Capabilities::detect(version, dawn);
        for path in RETIRED {
            assert!(
                capabilities.supports(path),
                "{version:?} dawn={dawn} {path}"
            );
        }
    }

    for version in ["0.5.1", "0.6", "1.0", "0.5.0.0"] {
        let capabilities = Capabilities::detect(Some(version), false);
        assert!(!capabilities.supports(RETIRED[0]), "{version}");
    }
}

/// One frame of the Sunrise page, reporting whether a control changed the document.
fn draw_page(
    context: &egui::Context,
    events: Vec<egui::Event>,
    document: &mut Value,
    capabilities: Capabilities,
) -> (egui::FullOutput, bool) {
    let mut changed = false;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                changed |= draw(ui, document, false, capabilities);
            });
        },
    );
    (output, changed)
}

/// Clicks the painted text, pressing and releasing in separate frames, then draws the result.
fn click(
    context: &egui::Context,
    document: &mut Value,
    capabilities: Capabilities,
    text: &str,
) -> (egui::FullOutput, bool) {
    let (output, _) = draw_page(context, vec![], document, capabilities);
    let position = crate::test_support::text_position(&output, text)
        .unwrap_or_else(|| panic!("Missing rendered label: {text}"));
    let mut changed = false;
    for pressed in [true, false] {
        let events = crate::test_support::primary_press(position, pressed);
        changed |= draw_page(context, events, document, capabilities).1;
    }
    (
        draw_page(context, vec![], document, capabilities).0,
        changed,
    )
}

/// Sunrise 0.6's three new controls and the keys they save.
const SUNRISE_0_6_CONTROLS: [(&str, &str); 3] = [
    ("Unlock Cheat Tools", "/client/cheats"),
    ("Allow Studio Attach", "/core/studio_attach"),
    ("Export Package Keys", "/client/dump_package_keys"),
];

/// Sunrise 0.6's own `default_settings.json`, plus a field no Sunrise version defines.
fn sunrise_0_6_defaults() -> Value {
    let mut defaults: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v18-9260d10-defaults.json"
    ))
    .unwrap();
    defaults["unknown"] = json!({"keep": [1, 2]});
    defaults
}

/// Opens the Advanced tab and returns its frame, or `None` when the page hides that tab.
fn open_advanced(
    context: &egui::Context,
    document: &mut Value,
    capabilities: Capabilities,
) -> Option<egui::FullOutput> {
    // The first frame of a fresh context only settles the layout.
    draw_page(context, vec![], document, capabilities);
    let (output, _) = draw_page(context, vec![], document, capabilities);
    crate::test_support::text_position(&output, "Advanced")?;
    let (output, changed) = click(context, document, capabilities, "Advanced");
    assert!(!changed, "Opening a tab changed the settings");
    Some(output)
}

#[test]
fn sunrise_0_6_controls_save_their_own_keys() {
    let capabilities = Capabilities::detect(Some("0.6"), false);
    let context = egui::Context::default();
    let mut document = sunrise_0_6_defaults();
    open_advanced(&context, &mut document, capabilities).expect("Sunrise 0.6 shows Advanced");
    // Each control saves its own key as a boolean and leaves the rest of the file alone.
    for (label, path) in SUNRISE_0_6_CONTROLS {
        let mut expected = document.clone();
        *expected.pointer_mut(path).unwrap() = Value::Bool(true);
        assert!(
            click(&context, &mut document, capabilities, label).1,
            "{label}"
        );
        assert_eq!(document, expected, "{label}");
    }
    // Sunrise 0.6's own defaults, with every new key changed, still pass both validators.
    assert_eq!(validate(&document, false), Ok(()));
    assert_eq!(
        crate::game_settings::validate_non_account(&document),
        Ok(())
    );
}

#[test]
fn sunrise_0_5_defaults_load_and_validate_without_the_retired_settings() {
    let encoded = include_str!("../../../tests/fixtures/sunrise-v18-5e4cbc7-defaults.json");
    let document: Value = serde_json::from_str(encoded).unwrap();
    assert_eq!(document["version"], 18);
    for path in [
        "/client/region_private",
        "/client/pin_replicated_record",
        "/client/skip_orbit_cinematic_wait",
        "/state/activity/arrival_overrides",
    ] {
        assert_eq!(document.pointer(path), None, "{path}");
    }
    // The validator only checks fields it finds, so the shorter document still passes.
    assert_eq!(
        crate::game_settings::validate_non_account(&document),
        Ok(())
    );
    assert!(available(&document));
    assert_eq!(validate(&document, false), Ok(()));
    assert_eq!(validate_activity(&document), Ok(()));
}

#[test]
fn destinations_are_atomic_and_preserve_unknown_fields() {
    let mut document = fixture();
    let original = document.clone();
    let mut row = document.pointer(activity::DESTINATION).unwrap().clone();
    row["future"] = json!({"keep":true});
    row["source_activity_index"] = 99.into();
    assert_eq!(
        activity::set_destination(&mut document, row.clone()),
        Ok(true)
    );
    assert_eq!(document.pointer(activity::DESTINATION), Some(&row));
    let edited = document.clone();
    row["stateful_bubble_mask"] = 0.into();
    assert!(activity::set_destination(&mut document, row).is_err());
    assert_eq!(document, edited);
    assert_eq!(
        document["state"]["characters"],
        original["state"]["characters"]
    );
    document["version"] = 8.into();
    assert!(
        activity::set_destination(
            &mut document,
            original.pointer(activity::DESTINATION).unwrap().clone()
        )
        .is_err()
    );
}

#[test]
fn arrival_slice_sets_and_launch_policy_roundtrip() {
    for version in [6, 8, 15, 16] {
        let mut document = json!({"version":version});
        let row = json!({"package_name":"raid_gluttony_0","bubble":2,"slice_set":17,"spawn_set_hash":"0x8029E4B4","current_activity_from_launch":true,"unknown":{"keep":1}});
        assert_eq!(
            activity::set_arrival(&mut document, 0, Some(row.clone())).is_ok(),
            version >= 16
        );
        if version >= 16 {
            assert_eq!(document.pointer(activity::ARRIVALS).unwrap()[0], row);
            assert_eq!(activity::set_arrival(&mut document, 0, None), Ok(true));
        } else {
            assert_eq!(document, json!({"version":version}));
        }
    }
    for row in [
        json!({"package_name":"x","slice_set":512}),
        json!({"package_name":"x","bubble":64}),
        json!({"package_name":"x"}),
        json!({"package_name":"x","current_activity_from_launch":false}),
    ] {
        assert!(activity::validate_arrival(&row).is_err());
    }
    assert!(
        activity::validate_arrival(
            &json!({"package_name":"x","current_activity_from_launch":true})
        )
        .is_ok()
    );
}

#[test]
fn destination_wire_boundaries_match_upstream() {
    let base = fixture().pointer(activity::DESTINATION).unwrap().clone();
    for (key, invalid) in [
        ("reason", json!(15)),
        ("source_activity_index", json!(4095)),
        ("activity_index", json!(-1)),
        ("bubble_count", json!(65)),
        ("initial_slice_set", json!(512)),
        ("spawn_set_hash", json!("0x100000000")),
    ] {
        let mut row = base.clone();
        row[key] = invalid;
        assert!(activity::validate_destination(&row).is_err(), "{key}");
    }
    let mut row = base;
    row["bubble_count"] = 64.into();
    row["stateful_bubble_mask"] = json!("0xFFFFFFFFFFFFFFFF");
    row["initial_slice_set"] = 511.into();
    assert_eq!(activity::validate_destination(&row), Ok(()));
}

#[test]
fn network_validation_matches_sunrise_port_pool_and_slot_limits() {
    for port in [0, 1, 3044, 3074, 65506] {
        let doc = json!({"version":16,"server":{"gameplay":{"port":port}}});
        assert!(validate(&doc, true).is_err(), "{port}");
    }
    for port in [2, 3042, 3076, 65504] {
        let doc = json!({"version":16,"server":{"gameplay":{"port":port}}});
        assert_eq!(validate(&doc, true), Ok(()), "{port}");
    }
    for (key, value) in [
        ("server_reserve_count", 7),
        ("server_reserve_count", 4097),
        ("client_join_grant_count", 399),
        ("client_join_grant_count", 8193),
    ] {
        let mut doc = json!({"version":16,"server":{"gameplay":{}}});
        doc["server"]["gameplay"][key] = value.into();
        assert!(validate(&doc, true).is_err());
        doc["server"]["gameplay"]["topology"] = "disabled".into();
        assert_eq!(validate(&doc, true), Ok(()));
    }
}

#[test]
fn experimental_fov_validates_current_bounds_and_preserves_legacy_limit() {
    let mut doc = fixture();
    for value in [55, 105, 155] {
        doc["state"]["account"]["settings"]["display"]["field_of_view"] = value.into();
        assert_eq!(super::super::validate(&doc), Ok(()));
    }
    doc["state"]["account"]["settings"]["display"]["field_of_view"] = 156.into();
    assert!(super::super::validate(&doc).is_err());
    doc["version"] = 8.into();
    doc["state"]["account"]["settings"]["display"]["field_of_view"] = 155.into();
    assert!(super::super::validate(&doc).is_err());
}

#[test]
fn character_preferences_use_the_same_hash_contract_as_account_validation() {
    for value in [json!("123"), json!("0x100000000"), json!(-1)] {
        let doc = json!({"version":16,"state":{"characters":[{"last_orbited_destination":value}]}});
        assert!(validate(&doc, true).is_err());
    }
    for value in [json!(123), json!("0xFFFFFFFF")] {
        let doc = json!({"version":16,"state":{"characters":[{"last_orbited_destination":value}]}});
        assert_eq!(validate(&doc, true), Ok(()));
    }
}
