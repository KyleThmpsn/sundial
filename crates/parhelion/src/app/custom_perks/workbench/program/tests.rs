use super::*;

#[test]
fn spawn_and_retained_actions_describe_draw_activation_differently() {
    assert_eq!(trigger_label(Trigger::Drawn, false), "On Draw");
    assert_eq!(trigger_label(Trigger::Drawn, true), "While Drawn");
    assert_eq!(trigger_label(Trigger::WeaponKill, false), "On Weapon Kill");
    let mut workbench = Workbench::default();
    let ctx = egui::Context::default();
    for retained in [false, true] {
        let mut program = Program {
            trigger: Trigger::Drawn,
            actions: vec![if retained {
                Action::Pattern {
                    asset: Asset {
                        graph: 1,
                        path: String::new(),
                        values: vec![],
                    },
                }
            } else {
                Action::Spawn {
                    asset: Asset {
                        graph: 1,
                        path: String::new(),
                        values: vec![],
                    },
                    position: Position::Owner,
                }
            }],
            ..Default::default()
        };
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| workbench.draw_removal_block(ui, &mut program));
        });
        let has_end = output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "End Condition")
        });
        assert_eq!(has_end, retained);
    }
}

#[test]
fn key_names_refresh_with_the_catalog_without_rebuilding_on_each_frame() {
    use sundial::investment::{IngredientCatalog, PerkSource};
    let index: KeyIndex = serde_json::from_value(serde_json::json!({
        "perks": [[1, 100]],
        "actions": {"100": {"properties": [{"key": 42, "value": 1065353216,
            "target": 2, "operation": 1, "removal": 0}], "removals": []}},
        "issues": []
    }))
    .unwrap();
    let index = Arc::new(index);
    let sources = |name: &str| {
        Arc::new(IngredientCatalog {
            choices: Vec::new(),
            references: [(
                1,
                PerkSource {
                    hash: 1,
                    name: name.into(),
                    type_name: "Test".into(),
                },
            )]
            .into_iter()
            .collect(),
            sources: BTreeMap::new(),
            context: BTreeMap::new(),
        })
    };
    let original = sources("Test Perk");
    let mut keys = Keys::default();
    keys.sync(Some(&index), Some(&original));
    assert_eq!(keys.catalog.property_key(42).unwrap().perks, ["Test Perk"]);
    let address = keys.catalog.property_keys().as_ptr();
    keys.sync(Some(&index), Some(&original));
    assert_eq!(address, keys.catalog.property_keys().as_ptr());
    keys.sync(Some(&index), Some(&sources("Renamed Test Perk")));
    assert_eq!(
        keys.catalog.property_key(42).unwrap().perks,
        ["Renamed Test Perk"]
    );
    keys.sync(None, None);
    assert!(keys.catalog.property_keys().is_empty());
}

#[test]
fn trigger_transitions_clear_hidden_endings_and_invalid_spawn_positions() {
    for trigger in Trigger::ALL {
        let mut program = Program {
            trigger: Trigger::Always,
            removal_key: Some(0xA628_8DD1),
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        };
        change_trigger(&mut program, trigger);
        assert_eq!(program.removal_key.is_some(), trigger == Trigger::Always);
        assert!(program.validate_structure().is_ok(), "{trigger:?}");
        program.removal_key = None;
        program.native_removal = NativeNode::condition(29);
        change_trigger(&mut program, trigger);
        assert_eq!(
            program.native_removal.is_some(),
            matches!(trigger, Trigger::Always | Trigger::Native)
        );
        assert!(program.validate_structure().is_ok(), "{trigger:?}");
    }
    let mut program = Program {
        trigger: Trigger::WeaponKill,
        actions: vec![Action::Spawn {
            asset: Asset::default(),
            position: Position::Event,
        }],
        ..Program::default()
    };
    change_trigger(&mut program, Trigger::Drawn);
    assert!(matches!(
        program.actions[0],
        Action::Spawn {
            position: Position::Owner,
            ..
        }
    ));
}

#[test]
fn selecting_an_ending_key_replaces_the_native_ending() {
    let mut program = Program {
        trigger: Trigger::Always,
        native_removal: NativeNode::condition(29),
        actions: vec![Action::add_rounds(1)],
        ..Program::default()
    };
    select_ending_key(&mut program, Some(0xA628_8DD1));
    assert!(program.native_removal.is_none());
    assert!(program.validate_structure().is_ok());
    select_ending_key(&mut program, None);
    assert!(program.removal_key.is_none());
}

#[test]
fn native_actions_preserve_editable_retained_state() {
    use sundial::package_authoring::sandbox_perk::action::native::{Graph, fields};
    for layout in layout::EFFECT_LAYOUTS {
        let node = NativeNode::effect(layout.kind).unwrap();
        let entry = nodes::effect(node.kind).unwrap();
        let mut graph = Graph::read(&node.bytes, 0, entry.class).unwrap();
        let fields = fields::describe(entry.class).unwrap();
        let retained = fields.iter().find(|field| field.offset == 1).unwrap();
        assert!(retained.editable);
        retained
            .write(&mut graph.blocks[0], 0, &[1 - node.bytes[1]])
            .unwrap();
        let program = Program {
            actions: vec![Action::Native {
                node: NativeNode {
                    kind: node.kind,
                    bytes: graph.emit().unwrap(),
                },
            }],
            ..Program::default()
        };
        assert!(program.validate_structure().is_ok(), "kind {}", node.kind);
    }
}

#[test]
fn changing_orb_trigger_preserves_other_native_fields() {
    let Action::Native { mut node } = Action::generate_orb(Position::Event) else {
        panic!("orb node")
    };
    node.bytes[8..12].copy_from_slice(&0.25f32.to_le_bytes());
    let before = node.clone();
    let mut program = Program {
        trigger: Trigger::WeaponKill,
        actions: vec![Action::Native { node }],
        ..Program::default()
    };
    assert!(guidance::summary(&program, None).contains("1 Orb of Light at the defeated enemy"));
    change_trigger(&mut program, Trigger::Drawn);
    let Action::Native { node } = &program.actions[0] else {
        panic!("orb node")
    };
    assert_eq!(node.bytes[2], 0);
    assert_eq!(&node.bytes[3..], &before.bytes[3..]);
}
