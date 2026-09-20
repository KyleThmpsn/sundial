use super::*;
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
fn all_runtime_controls_enforce_the_schema_boundary_and_preserve_unrelated_data() {
    for field in FIELDS.iter().chain(services::FIELDS) {
        let value = match field.kind {
            fields::Kind::Bool(default) => Value::Bool(!default),
            fields::Kind::Choice(values) => Value::from(values[1]),
            fields::Kind::UInt(default, _, _) => Value::from(default + 2),
            fields::Kind::Ipv4(_) => Value::from("192.168.1.2"),
            fields::Kind::Text(default, _, _) => Value::from(default.to_uppercase()),
        };
        for version in [6, 8, 15, 16] {
            let mut document = json!({"version":version,"unknown":{"keep":[1,2]}});
            let original = document.clone();
            let field = field.for_document(&document);
            let result = set_field(&mut document, field.path, value.clone(), true);
            if version < 16 {
                assert!(result.is_err(), "{}", field.path);
                assert_eq!(document, original);
            } else {
                assert_eq!(result, Ok(true), "{}", field.path);
                assert_eq!(document.pointer(field.path), Some(&value));
                assert_eq!(document["unknown"], original["unknown"]);
                assert_eq!(document["version"], version);
            }
        }
    }
}

#[test]
fn sqlite_does_not_expose_or_validate_the_inactive_json_account() {
    let mut document = json!({"version":16,"state":{"account":"opaque"}});
    let original = document.clone();
    assert!(
        set_field(
            &mut document,
            "/state/account/profile_setup_completed",
            true.into(),
            false
        )
        .is_err()
    );
    assert_eq!(document, original);
    assert_eq!(validate(&document, false), Ok(()));
}

#[test]
fn malformed_values_and_parents_are_not_replaced() {
    for mut document in [
        json!({"version":16,"client":[]}),
        json!({"version":16,"client":null}),
    ] {
        let original = document.clone();
        assert!(
            set_field(
                &mut document,
                "/client/reveal_lore_books",
                true.into(),
                true
            )
            .is_err()
        );
        assert_eq!(document, original);
    }
    let mut document = json!({"version":16});
    assert!(set_field(&mut document, "/client/reveal_lore_books", 1.into(), true).is_err());
    assert_eq!(document, json!({"version":16}));
}

#[test]
fn retired_settings_are_opaque_and_cannot_be_authored() {
    let mut document = fixture();
    let retired = [
        "/complete_exotic_catalysts",
        "/client/skip_profile_setup",
        "/client/fade_release",
        "/client/suppress_peer_relay",
        "/client/hold_spawn",
        "/client/spawn_hold_ms",
        "/client/dump_game_image",
        "/client/stock_entity_pool",
        "/client/restock_drained_entity_pool",
        "/server/gameplay/client_lease_high_water",
        "/server/gameplay/ignore_client_slot_release",
        "/state/activity/author_director_bodies",
        "/state/activity/author_wide_record_bodies",
        "/core/activity_sdk_generation/enabled",
        "/state/account/record_rewards",
    ];
    for path in retired {
        crate::persistence::json_fields::write_value(&mut document, path, json!({"preserve":true}))
            .unwrap();
    }
    let before = document.clone();
    assert_eq!(validate(&document, true), Ok(()));
    for path in retired {
        assert!(set_field(&mut document, path, false.into(), true).is_err());
    }
    assert_eq!(document, before);
    assert_eq!(
        set_field(
            &mut document,
            "/state/investment/complete_exotic_catalysts",
            false.into(),
            true
        ),
        Ok(true)
    );
    for path in retired {
        assert_eq!(document.pointer(path), before.pointer(path));
    }
    assert_eq!(document["state"]["unlocks"], before["state"]["unlocks"]);
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
fn advanced_rows_validate_ownership_and_character_types() {
    for rows in [
        json!([{"name":"same"},{"name":"same"}]),
        json!([{"name":"named","owned":"application"}]),
        json!([{"name":"4294967296","owned":"application"}]),
    ] {
        assert!(validate(&json!({"version":16,"server":{"entitlements":rows}}), true).is_err());
    }
    let mut doc = json!({"version":16,"state":{"characters":[{"last_orbited_destination":"0xFFFFFFFF","appearance_value":1.0,"preview_available":true,"content_bypass":false,"unknown":[1]}]}});
    assert_eq!(validate(&doc, true), Ok(()));
    doc["state"]["characters"][0]["last_orbited_destination"] = "0x100000000".into();
    assert!(validate(&doc, true).is_err());
    assert_eq!(validate(&doc, false), Ok(()));
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
fn opening_sunrise_and_display_preserves_omissions_and_extended_fov() {
    let ctx = eframe::egui::Context::default();
    let mut doc = fixture();
    doc["state"]["account"]["settings"]["display"]["field_of_view"] = 155.into();
    doc.as_object_mut().unwrap().remove("core");
    let original = doc.clone();
    let _ = ctx.run(eframe::egui::RawInput::default(), |ctx| {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            assert!(!page::draw(ui, &mut doc, true, Capabilities::default()));
            let settings = doc
                .pointer("/state/account/settings")
                .unwrap()
                .as_object()
                .unwrap();
            let edits = super::super::preferences::draw_display(ui, settings, false, 4);
            assert!(edits.into_vec().is_empty());
        });
    });
    assert_eq!(doc, original);
}

#[test]
fn text_edits_are_visible_before_focus_changes_and_can_be_repaired() {
    use eframe::egui;
    let context = egui::Context::default();
    let field = *services::FIELDS
        .iter()
        .find(|field| field.path == "/client/external_server/host")
        .unwrap();
    let mut document = json!({"version":16});
    let mut editor_id = egui::Id::NULL;
    let _ = context.run(egui::RawInput::default(), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            editor_id = ui.make_persistent_id(("runtime-field", field.path));
            assert!(!page::draw_field(ui, &mut document, field, true));
        });
    });
    for replacement in ["192.", "192.168.1.2"] {
        context.memory_mut(|memory| memory.request_focus(editor_id));
        let mut state = egui::text_edit::TextEditState::load(&context, editor_id).unwrap();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(100),
            )));
        state.store(&context, editor_id);
        let input = egui::RawInput {
            events: vec![egui::Event::Text(replacement.into())],
            ..Default::default()
        };
        let _ = context.run(input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                assert!(page::draw_field(ui, &mut document, field, true));
            });
        });
        assert_eq!(
            document.pointer(field.path),
            Some(&Value::from(replacement))
        );
        assert!(context.memory(|memory| memory.has_focus(editor_id)));
        assert_eq!(
            validate(&document, true).is_ok(),
            replacement == "192.168.1.2"
        );
    }
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
