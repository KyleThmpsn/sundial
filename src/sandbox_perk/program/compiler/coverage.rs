use super::*;
use crate::sandbox_perk::{
    action::{self, FactValue, layout},
    program::decompile,
};

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn native_pickups_and_world_objects_spawn_without_becoming_weapon_patterns() {
    let packages = std::path::PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("clean packages"),
    );
    let manager =
        crate::package_runtime::open_shadowkeep_packages(packages.parent().unwrap()).unwrap();
    for graph in [
        0x80C10B00, 0x81A69920, 0x80C1D182, 0x80BB3594, 0x80C00E02, 0x80BB757C,
    ] {
        let asset = Asset {
            graph,
            path: String::new(),
            values: Vec::new(),
        };
        let mut program = Program {
            trigger: Trigger::WeaponKill,
            actions: vec![Action::Spawn {
                asset: asset.clone(),
                position: Position::Event,
            }],
            ..Program::default()
        };
        let compiled = compile(&manager, &program).unwrap();
        let decoded = action::decode(&compiled.payload).unwrap();
        assert_eq!(
            decompile::decompile(&decoded, &program.name, |tag| tag).unwrap(),
            program
        );
        program.actions = vec![Action::Pattern { asset }];
        assert!(
            compile(&manager, &program)
                .err()
                .expect("world object cannot replace a projectile")
                .contains("requires a projectile")
        );
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES with clean client resources"]
fn every_native_kind_compiles_against_clean_client_resources() {
    use crate::sandbox_perk::nodes;
    let packages = std::path::PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("clean packages"),
    );
    let manager =
        crate::package_runtime::open_shadowkeep_packages(packages.parent().unwrap()).unwrap();
    let templates: Vec<(bool, u8, u32, usize, String)> =
        serde_json::from_str(include_str!("../../action/native/templates.json")).unwrap();
    for (condition, kind, tag, offset, _) in templates {
        let entry = if condition {
            nodes::condition(kind)
        } else {
            nodes::effect(kind)
        }
        .unwrap();
        let source = manager
            .read_tag(TagHash(tag))
            .unwrap_or_else(|error| panic!("{} template source 0x{tag:08X}: {error}", entry.name));
        let bytes = crate::sandbox_perk::action::native::template(condition, kind).unwrap();
        let template =
            crate::sandbox_perk::action::native::Graph::read(&bytes, 0, entry.class).unwrap();
        let clean =
            crate::sandbox_perk::action::native::Graph::read(&source, offset, entry.class).unwrap();
        assert_eq!(
            template, clean,
            "{} template differs from the clean client",
            entry.name
        );
    }
    let mut count = 0;
    for (condition, entries) in [
        (true, nodes::CONDITIONS.as_slice()),
        (false, nodes::EFFECTS.as_slice()),
    ] {
        for entry in entries.iter().filter(|entry| entry.observed()) {
            let program = if condition {
                Program {
                    trigger: Trigger::Native,
                    native_trigger: NativeNode::condition(entry.kind),
                    duration_ms: 1000,
                    actions: vec![Action::native(43).unwrap()],
                    ..Program::default()
                }
            } else {
                Program {
                    trigger: Trigger::Always,
                    actions: vec![Action::native(entry.kind).unwrap()],
                    ..Program::default()
                }
            };
            let compiled = compile(&manager, &program)
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            let decoded = action::decode(&compiled.payload).unwrap();
            if condition {
                assert_eq!(decoded.groups[0].activation[0].kind, entry.kind);
            } else {
                assert_eq!(decoded.groups[0].effects[0].kind, entry.kind);
            }
            count += 1;
        }
    }
    assert_eq!(count, 82);
    for empty in [0_u32, u32::MAX] {
        let mut node = NativeNode::effect(13).unwrap();
        node.bytes[16..20].copy_from_slice(&empty.to_le_bytes());
        let program = Program {
            trigger: Trigger::Always,
            actions: vec![Action::Native { node }],
            ..Program::default()
        };
        compile(&manager, &program).expect("weighted spawning has an optional attachment");
    }
}

#[test]
fn every_authorable_catalog_kind_has_a_checked_compiler_path() {
    use crate::sandbox_perk::nodes::{self, Support};
    use std::collections::BTreeSet;
    let asset = Asset {
        graph: 0x80BC_5810,
        path: String::new(),
        values: Vec::new(),
    };
    let mut conditions = BTreeSet::new();
    let mut effects = BTreeSet::new();
    let mut programs = Vec::new();
    for trigger in Trigger::ALL
        .into_iter()
        .filter(|trigger| *trigger != Trigger::Native)
    {
        let mut program = Program {
            trigger,
            actions: vec![
                Action::attach(asset.clone()),
                Action::Spawn {
                    asset: asset.clone(),
                    position: Position::Owner,
                },
                Action::Pattern {
                    asset: asset.clone(),
                },
                Action::property(0x5EE2_66FC),
                Action::add_rounds(2),
                Action::add_fraction(0.5),
            ],
            ..Program::default()
        };
        if trigger.is_event() {
            program.actions.push(Action::ExtendTimers {
                extend_ms: 1000,
                cap_ms: 5000,
            });
        }
        programs.push(program);
    }
    for entry in nodes::EFFECTS.iter().filter(|entry| entry.observed()) {
        programs.push(Program {
            trigger: Trigger::Always,
            actions: vec![Action::Native {
                node: NativeNode::effect(entry.kind).unwrap(),
            }],
            ..Program::default()
        });
    }
    for entry in nodes::CONDITIONS.iter().filter(|entry| entry.observed()) {
        programs.push(Program {
            trigger: Trigger::Native,
            native_trigger: NativeNode::condition(entry.kind),
            actions: vec![Action::Native {
                node: NativeNode::effect(43).unwrap(),
            }],
            ..Program::default()
        });
    }
    for program in programs {
        let labels = program.trigger.is_event().then_some((&[][..], [0; 40]));
        let compiled = assemble(&program, labels).unwrap();
        let decoded = action::decode(&compiled.payload).unwrap();
        conditions.extend(decoded.conditions().iter().map(|node| node.kind));
        effects.extend(decoded.effects().map(|node| node.kind));
    }
    assert_eq!(
        conditions,
        nodes::CONDITIONS
            .iter()
            .filter(|node| node.support == Support::Authorable)
            .map(|node| node.kind)
            .collect()
    );
    assert_eq!(
        effects,
        nodes::EFFECTS
            .iter()
            .filter(|node| node.support == Support::Authorable)
            .map(|node| node.kind)
            .collect()
    );
}

#[test]
fn every_scalar_effect_can_be_authored_serialized_compiled_and_read_back() {
    for entry in layout::EFFECT_LAYOUTS {
        let mut node = NativeNode::effect(entry.kind).unwrap();
        for field in entry.fields {
            use layout::FieldFormat::*;
            let value = match field.format {
                Byte => FactValue::Selector(2),
                Flag => FactValue::Flag(true),
                Mask8 => FactValue::Mask(0x81),
                Mask32 => FactValue::Mask(0x8000_0001),
                Key => FactValue::Key(0x1234_5678),
                Float => FactValue::Number(1.25),
                Seconds => FactValue::Seconds(1.25),
                Range => FactValue::Range(0.5, 2.5),
            };
            assert!(field.write(&mut node.bytes, &value));
        }
        let program = Program {
            trigger: Trigger::Always,
            actions: vec![Action::Native { node: node.clone() }],
            ..Program::default()
        };
        let json = serde_json::to_string(&program).unwrap();
        assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
        let compiled = assemble(&program, None).unwrap();
        let decoded = action::decode(&compiled.payload).unwrap();
        let effect = decoded.effects().next().unwrap();
        assert_eq!(effect.kind, entry.kind);
        assert_eq!(
            &compiled.payload[effect.offset..effect.offset + node.bytes.len()],
            node.bytes
        );
        let recovered = decompile::decompile(&decoded, &program.name, |tag| tag).unwrap();
        let rebuilt = assemble(&recovered, None).unwrap();
        assert!(
            decompile::fidelity(&compiled.payload, &rebuilt.payload)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn every_scalar_condition_can_be_authored_as_activation_and_removal() {
    for entry in layout::CONDITION_LAYOUTS {
        let node = NativeNode::condition(entry.kind).unwrap();
        let program = Program {
            trigger: Trigger::Native,
            native_trigger: Some(node.clone()),
            native_removal: Some(node),
            actions: vec![Action::Native {
                node: NativeNode::effect(43).unwrap(),
            }],
            ..Program::default()
        };
        let compiled = assemble(&program, None).unwrap();
        let decoded = action::decode(&compiled.payload).unwrap();
        assert_eq!(decoded.groups[0].activation[0].kind, entry.kind);
        assert_eq!(decoded.groups[0].removal[0].kind, entry.kind);
        assert_eq!(decoded.activation_event_mask, 1_u64 << entry.kind);
        assert_eq!(decoded.removal_event_mask, 1_u64 << entry.kind);
    }
}

#[test]
fn invalid_native_headers_and_numeric_values_are_rejected_before_emission() {
    let rounds = Program {
        trigger: Trigger::Always,
        actions: vec![Action::add_rounds(i32::MIN)],
        ..Program::default()
    };
    assert!(rounds.validate().is_err());
    let base = Program {
        trigger: Trigger::Native,
        native_trigger: Some(NativeNode::condition(6).unwrap()),
        actions: vec![Action::Native {
            node: NativeNode::effect(29).unwrap(),
        }],
        ..Program::default()
    };
    for (offset, value) in [(5, 9), (6, 2)] {
        let mut program = base.clone();
        program.native_trigger.as_mut().unwrap().bytes[offset] = value;
        assert!(program.validate().is_err(), "header byte {offset}");
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut program = base.clone();
        program.native_trigger.as_mut().unwrap().bytes[..4].copy_from_slice(&value.to_le_bytes());
        assert!(program.validate().is_err());
        let mut program = base.clone();
        let Action::Native { node } = &mut program.actions[0] else {
            unreachable!()
        };
        node.bytes[4..8].copy_from_slice(&value.to_le_bytes());
        assert!(program.validate().is_err());
    }
    for text in ["0x0€", "0x💥", "0xGG", "0xF"] {
        let value = serde_json::json!({"kind": 0, "bytes": text});
        assert!(serde_json::from_value::<NativeNode>(value).is_err());
    }
}

#[test]
fn inspected_stock_conditions_reuse_probability_and_complete_nested_values() {
    use crate::investment::native_content::conditions;
    for kind in [6, 26, 28] {
        let mut node = NativeNode::condition(kind).unwrap();
        node.bytes[..4].copy_from_slice(&0.125f32.to_le_bytes());
        node.bytes[4] = 255;
        let original = Program {
            trigger: Trigger::Native,
            native_trigger: Some(node),
            actions: vec![Action::add_rounds(3)],
            ..Program::default()
        };
        let compiled = assemble(&original, None).unwrap();
        let choices = conditions::from_payload(&compiled.payload).unwrap();
        let copied = choices.iter().find(|choice| choice.kind == kind).unwrap();
        let standalone = action::decode_condition_node(&copied.bytes).unwrap();
        let complete = action::decode(&compiled.payload).unwrap();
        let root = &complete.groups[0].activation[0];
        assert_eq!(standalone.description(), root.description());
        assert_eq!(standalone.facts, root.facts);
        assert_eq!(standalone.native, root.native);
        let custom = Program {
            native_trigger: Some(NativeNode {
                kind,
                bytes: copied.bytes.clone(),
            }),
            ..original
        };
        let rebuilt = assemble(&custom, None).unwrap();
        assert_eq!(rebuilt.payload, compiled.payload);
        let reread = action::decode(&rebuilt.payload).unwrap();
        assert_eq!(
            &reread.groups[0].activation[0].native[..4],
            &0.125f32.to_le_bytes()
        );
        if kind == 26 {
            assert!(
                choices.len() > 1,
                "nested conditions must also be discoverable"
            );
        }
    }
}

#[test]
fn rejected_field_writes_preserve_every_byte() {
    for (format, value) in [
        (layout::FieldFormat::Mask8, FactValue::Mask(256)),
        (layout::FieldFormat::Mask32, FactValue::Mask(1_u64 << 32)),
        (layout::FieldFormat::Float, FactValue::Number(f32::NAN)),
        (
            layout::FieldFormat::Range,
            FactValue::Range(1.0, f32::INFINITY),
        ),
    ] {
        let field = layout::Field {
            label: "Test",
            offset: 4,
            format,
        };
        let mut bytes = [0xAA; 16];
        assert!(!field.write(&mut bytes, &value));
        assert_eq!(bytes, [0xAA; 16]);
    }
}

#[test]
fn timer_extensions_are_routed_by_their_stored_effect_indices() {
    let extend = Action::ExtendTimers {
        extend_ms: 1000,
        cap_ms: 5000,
    };
    let program = Program {
        trigger: Trigger::WeaponKill,
        duration_ms: 5000,
        cooldown_ms: 1000,
        actions: vec![
            extend.clone(),
            Action::property(0x5EE2_66FC),
            extend,
            Action::add_rounds(1),
        ],
        ..Program::default()
    };
    let compiled = assemble(&program, Some((&[], [0; 40]))).unwrap();
    assert_eq!(
        crate::package_payload::u64_at(&compiled.payload, 0xA0).unwrap(),
        0b1010
    );
    let decoded = action::decode(&compiled.payload).unwrap();
    assert_eq!(decoded.timer_budget, 2);
    for effect in decoded.effects().filter(|effect| effect.kind == 32) {
        assert_eq!(effect.conditions[0].ordinal, 0xFF);
        assert_eq!(
            crate::package_payload::u64_at(&compiled.payload, effect.offset + 0x20).unwrap(),
            4
        );
    }
}

#[test]
fn native_timer_activation_and_removal_each_reserve_a_timer_slot() {
    let mut activation = NativeNode::condition(1).unwrap();
    activation.bytes[8..12].copy_from_slice(&1.5_f32.to_le_bytes());
    let mut removal = NativeNode::condition(1).unwrap();
    removal.bytes[8..12].copy_from_slice(&3.0_f32.to_le_bytes());
    let program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(activation),
        native_removal: Some(removal),
        cooldown_ms: 2500,
        actions: vec![Action::add_rounds(1)],
        ..Program::default()
    };
    let compiled = assemble(&program, None).unwrap();
    let decoded = action::decode(&compiled.payload).unwrap();
    assert_eq!(decoded.timer_budget, 3);
    assert_eq!(
        decoded
            .conditions()
            .iter()
            .map(|node| node.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let recovered = decompile::decompile(&decoded, &program.name, |tag| tag).unwrap();
    let rebuilt = assemble(&recovered, None).unwrap();
    assert!(
        decompile::fidelity(&compiled.payload, &rebuilt.payload)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn authored_accumulator_children_keep_values_and_rebuild_routing() {
    use crate::sandbox_perk::{action::native::Graph, nodes};
    let class = nodes::condition(26).unwrap().class;
    let mut node = NativeNode::condition(26).unwrap();
    let mut graph = Graph::read(&node.bytes, 0, class).unwrap();
    graph.create_target(0, 16, 0x80803E32, true).unwrap();
    let rows = graph.blocks[0].links[&16];
    graph.resize_array(rows, 1).unwrap();
    graph
        .create_target(rows, 0, nodes::condition(1).unwrap().class, false)
        .unwrap();
    let timer = graph.blocks[rows].links[&0];
    graph.blocks[timer].bytes[8..12].copy_from_slice(&2.5_f32.to_le_bytes());
    graph.blocks[rows].bytes[12..16].copy_from_slice(&1.25_f32.to_le_bytes());
    graph.blocks[rows].bytes[16..20].copy_from_slice(&0.75_f32.to_le_bytes());
    graph.resize_array(rows, 2).unwrap();
    graph
        .create_target(rows, 32, nodes::condition(30).unwrap().class, false)
        .unwrap();
    let event = graph.blocks[rows].links[&32];
    graph.blocks[event].bytes[8..12].copy_from_slice(&0x12345678_u32.to_le_bytes());
    node.bytes = graph.emit().unwrap();
    let program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(node),
        duration_ms: 1000,
        actions: vec![Action::add_rounds(1)],
        ..Program::default()
    };
    let serialized = serde_json::to_string(&program).unwrap();
    let program: Program = serde_json::from_str(&serialized).unwrap();
    let compiled = assemble(&program, None).unwrap();
    let decoded = action::decode(&compiled.payload).unwrap();
    let root = &decoded.groups[0].activation[0];
    assert_eq!(root.ordinal, 0);
    assert_eq!(
        root.children
            .iter()
            .map(|child| (child.kind, child.ordinal))
            .collect::<Vec<_>>(),
        [(1, 2), (30, 1)]
    );
    assert_eq!(
        decoded.activation_event_mask,
        (1 << 26) | (1 << 1) | (1 << 30)
    );
    let first = &root.children[0];
    assert_eq!(
        crate::package_payload::u32_at(&compiled.payload, first.offset + 8).unwrap(),
        2.5_f32.to_bits()
    );
    let rebuilt = Graph::read(&root.native, 0, class).unwrap();
    let rows = rebuilt.blocks[0].links[&16];
    assert_eq!(
        &rebuilt.blocks[rows].bytes[12..20],
        [1.25_f32.to_le_bytes(), 0.75_f32.to_le_bytes()].concat()
    );
    assert_eq!(
        crate::package_payload::u32_at(&compiled.payload, root.children[1].offset + 8).unwrap(),
        0x12345678
    );
}
