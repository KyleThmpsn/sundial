use crate::app::tests::driver::*;
use crate::app::*;
use crate::catalog::{self, AbilityChoice, ItemDef};

fn frame(app: &mut SundialApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.draw_character_fields(ui, 0, true);
            });
        },
    )
}

fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            egui::Shape::Vec(shapes) => shapes.iter().rev().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing {label}"))
}

fn click(app: &mut SundialApp, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(app, ctx, crate::test_support::primary_press(pos, pressed));
    }
}

fn set_document(app: &mut SundialApp, value: Value) {
    app.document = WorkspaceDocument::json_only(value);
    app.persisted_document = app.document.clone();
    app.dirty = false;
}

#[test]
fn choosing_character_identity_still_materializes_the_selected_defaults() {
    for version in [8, 16] {
        let directory = TestDirectory::new("character-fields-select");
        let mut app = app(directory.0.clone());
        set_document(
            &mut app,
            serde_json::json!({
                "version": version,
                "state": {"characters": [{"soid": 1, "future_character": {"keep": true}}]}
            }),
        );
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, text_position(&output, "Human"));
        let output = frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, text_position(&output, "Awoken"));

        let character = app.document.pointer("/state/characters/0").unwrap();
        assert_eq!(character["race"], 1);
        assert_eq!(character["gender"], 0);
        assert_eq!(character["class"], 0);
        assert_eq!(character["future_character"]["keep"], true);
        assert_eq!(character.get("movement_ability").is_some(), version == 8);
        assert!(app.dirty);
    }
}

#[test]
fn legacy_attunement_repairs_require_an_explicit_selection() {
    let directory = TestDirectory::new("character-fields-attunement");
    let mut app = app(directory.0.clone());
    let choice = |entry, name: &str| AbilityChoice {
        entry,
        name: name.into(),
    };
    app.manifest = Manifest::for_test(
        vec![ItemDef {
            hash: 0xB920_CE9A,
            name: "Sunbreaker".into(),
            type_name: "Solar Subclass".into(),
            bucket_hash: 3_284_755_031,
            class_type: 0,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: catalog::AbilityOptions {
                attunements: vec![
                    catalog::AttunementChoice {
                        name: "Top".into(),
                        super_abilities: vec![choice(10, "Base Super")],
                        melee: choice(11, "Top Melee"),
                        perks: Vec::new(),
                    },
                    catalog::AttunementChoice {
                        name: "Middle".into(),
                        super_abilities: vec![choice(20, "Middle Super")],
                        melee: choice(21, "Middle Melee"),
                        perks: Vec::new(),
                    },
                ],
                ..Default::default()
            },
        }],
        HashMap::new(),
    );
    set_document(
        &mut app,
        serde_json::json!({
            "version": 8,
            "state": {"characters": [{
                "soid": 1,
                "race": 0,
                "gender": 0,
                "class": 0,
                "movement_ability": 4,
                "grenade_ability": 7,
                "super_ability": 20,
                "melee_ability": 15,
                "class_ability": 2,
                "equipment": {"subclass": {
                    "instance_soid": 2,
                    "definition_hash": 0xB920_CE9A_u32,
                    "level": 0,
                    "quantity": 1,
                    "plugs": null
                }},
                "future_character": {"keep": true}
            }]}
        }),
    );
    let before = app.document.clone();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert_eq!(app.document, before);
    assert!(!app.dirty);

    click(&mut app, &ctx, text_position(&output, "Middle"));
    let output = frame(&mut app, &ctx, vec![]);
    click(&mut app, &ctx, text_position(&output, "Middle"));
    let character = app.document.pointer("/state/characters/0").unwrap();
    assert_eq!(character["super_ability"], 20);
    assert_eq!(character["melee_ability"], 21);
    assert_eq!(character["future_character"]["keep"], true);
    assert!(app.dirty);
}

use serde_json::{Value, json};
use sundial_account::{CharacterAbilities, CharacterMetadataUpdate};

use super::{apply_updates, legacy};

#[test]
fn production_updates_match_the_frozen_json_behavior() {
    let cases = [
        CharacterMetadataUpdate::SetAppearanceAndClass {
            race: 2,
            gender: 1,
            class_type: 2,
        },
        CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
            movement: 6,
            grenade: 9,
            super_ability: 20,
            melee: 21,
            class_ability: 3,
        }),
        CharacterMetadataUpdate::SetSuperAndMelee {
            super_ability: 10,
            melee: 15,
        },
    ];
    for update in cases {
        let mut production = json!({
            "version": 8,
            "state": {"characters": [{
                "race": "malformed",
                "future_character": {"keep": true}
            }]},
            "future_root": {"keep": true}
        });
        let mut expected = production.clone();

        legacy::apply_updates(&mut expected, 0, vec![update]).unwrap();
        apply_updates(&mut production, 0, vec![update]).unwrap();

        assert_eq!(production, expected);
    }
}
