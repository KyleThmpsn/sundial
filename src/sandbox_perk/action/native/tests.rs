use super::*;

#[test]
fn typed_callback_references_cannot_be_edited_as_independent_numbers() {
    let class = 0x8080_3E3C;
    let mut graph = Graph::read(&template(false, 33).unwrap(), 0, class).unwrap();
    let before = graph.clone();
    let fields = fields::describe(class).unwrap();
    for offset in [0xA0, 0xA8, 0xAC, 0xB0] {
        let field = fields.iter().find(|f| f.offset == offset).unwrap();
        assert!(!field.editable);
        assert!(
            field
                .write(&mut graph.blocks[0], 0, &vec![0xFF; field.width])
                .is_err()
        );
    }
    assert_eq!(graph, before);
}
use crate::sandbox_perk::nodes;

#[test]
fn mapped_gameplay_values_edit_only_their_native_lanes() {
    for (kind, offset, format, value) in [
        (1, 0x20, fields::Format::Float, (-0.0f32).to_bits()),
        (1, 0x2C, fields::Format::Float, 2.5f32.to_bits()),
        (1, 0x30, fields::Format::Key, 0xE49D_441F),
        (8, 8, fields::Format::Float, 0.12345679f32.to_bits()),
        (8, 12, fields::Format::Float, (-0.0f32).to_bits()),
        (14, 0x6C, fields::Format::Integer, (-3i32) as u32),
        (15, 0x6C, fields::Format::Float, 0.25f32.to_bits()),
        (32, 4, fields::Format::Float, 1.75f32.to_bits()),
        (5, 4, fields::Format::Unsigned, u32::MAX),
    ] {
        let class = nodes::effect(kind).unwrap().class;
        let mut graph = Graph::read(&template(false, kind).unwrap(), 0, class).unwrap();
        if kind == 1 {
            // Existing non-finite parameter lanes survive edits to adjacent fields.
            graph.blocks[0].bytes[0x24..0x28].copy_from_slice(&0x7FC12345u32.to_le_bytes());
        }
        if kind == 8 {
            // Unknown padding and an untouched NaN payload must survive nearby numeric edits.
            graph.blocks[0].bytes[5..8].copy_from_slice(&[0xCC, 0x21, 0xFA]);
            if offset == 8 {
                graph.blocks[0].bytes[12..16].copy_from_slice(&0x7FC12345u32.to_le_bytes());
            }
        }
        let mut expected = graph.clone();
        expected.blocks[0].bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let field = fields::describe(class)
            .unwrap()
            .into_iter()
            .find(|field| field.offset == offset)
            .unwrap();
        assert_eq!(field.format, format);
        field
            .write(&mut graph.blocks[0], 0, &value.to_le_bytes())
            .unwrap();
        assert_eq!(graph, expected);
        assert_eq!(
            Graph::read(&graph.emit().unwrap(), 0, class).unwrap(),
            expected
        );
    }
}

#[test]
fn range_and_mask_edits_preserve_adjacent_native_bits() {
    for (kind, offset, expected_format, bits) in [
        (34, 0x18, fields::Format::Float, (-0.0f32).to_bits()),
        (9, 8, fields::Format::Mask32, 0x80000001),
    ] {
        let class = nodes::condition(kind).unwrap().class;
        let mut graph = Graph::read(&template(true, kind).unwrap(), 0, class).unwrap();
        if kind == 34 {
            graph.blocks[0].bytes[0x1C..0x20].copy_from_slice(&0x7FC12345u32.to_le_bytes());
        }
        let mut expected = graph.clone();
        expected.blocks[0].bytes[offset..offset + 4].copy_from_slice(&bits.to_le_bytes());
        let field = fields::describe(class)
            .unwrap()
            .into_iter()
            .find(|f| f.offset == offset)
            .unwrap();
        assert_eq!(field.format, expected_format);
        field
            .write(&mut graph.blocks[0], 0, &bits.to_le_bytes())
            .unwrap();
        assert_eq!(graph, expected);
        assert_eq!(
            Graph::read(&graph.emit().unwrap(), 0, class).unwrap(),
            expected
        );
    }
}

#[test]
fn nested_modifier_edits_reach_the_decoder_without_touching_other_rows() {
    use crate::sandbox_perk::action::{self, fixtures::Builder};
    let mut fixture = Builder::new();
    let offset = fixture.event_modifier(&[], 0.5, 17);
    let class = nodes::effect(40).unwrap().class;
    let mut graph = Graph::read(&fixture.bytes, offset, class).unwrap();
    let assignments = graph.blocks[0].links[&0x128];
    let multipliers = graph.blocks[0].links[&0x138];
    // The engine ignores these three bytes when reading the field selector. They must
    // neither hide the decoded fact nor be overwritten by an adjacent value edit.
    graph.blocks[assignments].bytes[1..4].copy_from_slice(&[0xA5, 0x5A, 0xFF]);
    let multiplier_before = graph.blocks[multipliers].clone();
    let field = fields::describe(0x80803E22)
        .unwrap()
        .into_iter()
        .find(|f| f.offset == 4)
        .unwrap();
    field
        .write(&mut graph.blocks[assignments], 0, &1.25f32.to_le_bytes())
        .unwrap();
    let payload = graph.emit().unwrap();
    let facts = action::fields::effect_facts(&payload, 0, 40).unwrap();
    assert!(
        facts
            .iter()
            .any(|fact| fact.label == "Assign Precision Bonus"
                && fact.value == action::FactValue::Number(1.25))
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.label == "Multiply Base Damage Scale from Stat"
                && fact.value == action::FactValue::Selector(17))
    );
    assert_eq!(graph.blocks[multipliers], multiplier_before);
    assert_eq!(Graph::read(&payload, 0, class).unwrap(), graph);
}

#[test]
fn runtime_operation_path_is_a_managed_reference_not_an_editable_number() {
    let class = nodes::effect(48).unwrap().class;
    let mut graph = Graph::read(&template(false, 48).unwrap(), 0, class).unwrap();
    let field = fields::describe(class)
        .unwrap()
        .into_iter()
        .find(|f| f.offset == 8)
        .unwrap();
    assert_eq!(field.format, fields::Format::Pointer);
    assert!(
        field
            .write(&mut graph.blocks[0], 0, &123u64.to_le_bytes())
            .is_err()
    );
    let target = graph.blocks[0].links[&8];
    assert_eq!(graph.blocks[target].class, 0);
    assert!(
        std::str::from_utf8(&graph.blocks[target].bytes)
            .unwrap()
            .starts_with("content\\")
    );
    let facts =
        crate::sandbox_perk::action::fields::effect_facts(&graph.emit().unwrap(), 0, 48).unwrap();
    assert!(facts.iter().any(|fact| fact.label == "Has Resource Path"
        && fact.value == crate::sandbox_perk::action::FactValue::Flag(true)));
    assert!(facts.iter().all(|fact| fact.label != "Operation Value"));
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn bundled_templates_match_nodes_in_the_reference_installation() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("reference packages");
    let manager =
        crate::package_authoring::open_shadowkeep_package_manager(std::path::Path::new(&packages))
            .unwrap();
    let rows: Vec<(bool, u8, u32, usize, String)> =
        serde_json::from_str(include_str!("templates.json")).unwrap();
    for (condition, kind, tag, offset, _) in &rows {
        let payload = manager.read_tag(*tag).unwrap();
        let node = if *condition {
            nodes::condition(*kind)
        } else {
            nodes::effect(*kind)
        }
        .unwrap();
        let actual = Graph::read(&payload, *offset, node.class).unwrap();
        let bundled = Graph::read(&template(*condition, *kind).unwrap(), 0, node.class).unwrap();
        assert_eq!(
            actual, bundled,
            "{} from source 0x{tag:08X} at {offset}",
            node.name
        );
    }
    eprintln!(
        "{} bundled templates match reference source nodes",
        rows.len()
    );
}

#[test]
fn every_observed_kind_has_a_complete_relocatable_template() {
    let mut count = 0;
    for (condition, entries) in [
        (true, nodes::CONDITIONS.as_slice()),
        (false, nodes::EFFECTS.as_slice()),
    ] {
        for entry in entries.iter().filter(|e| e.observed()) {
            let bytes = template(condition, entry.kind).expect("observed template");
            let graph = Graph::read(&bytes, 0, entry.class)
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            graph.validate_node(condition, entry.kind).unwrap();
            let emitted = graph.emit().unwrap();
            assert_eq!(
                Graph::read(&emitted, 0, entry.class).unwrap(),
                graph,
                "{}",
                entry.name
            );
            for block in graph.blocks.iter().filter(|b| b.class != 0) {
                let fields = fields::describe(block.class).unwrap();
                let mut coverage = vec![0; schema::record(block.class).unwrap().size];
                for field in fields {
                    for byte in &mut coverage[field.offset..field.offset + field.width] {
                        *byte += 1;
                    }
                }
                assert!(coverage.iter().all(|n| *n == 1), "0x{:08X}", block.class);
            }
            count += 1;
        }
    }
    assert_eq!(count, 82);
}

#[test]
fn malformed_pointers_and_array_headers_are_rejected() {
    let class = nodes::condition(26).unwrap().class;
    let bytes = template(true, 26).unwrap();
    let mut broken = bytes.clone();
    broken[16..24].copy_from_slice(&i64::MAX.to_le_bytes());
    assert!(Graph::read(&broken, 0, class).is_err());
    let mut graph = Graph::read(&bytes, 0, class).unwrap();
    let field = 16;
    graph.blocks[0].links.insert(field, usize::MAX);
    assert!(graph.emit().is_err());
}

#[test]
fn added_accumulator_rows_own_independent_children() {
    let class = nodes::condition(26).unwrap().class;
    let mut graph = Graph::read(&template(true, 26).unwrap(), 0, class).unwrap();
    graph.create_target(0, 16, 0x80803E32, true).unwrap();
    let rows = graph.blocks[0].links[&16];
    graph.resize_array(rows, 1).unwrap();
    graph
        .create_target(rows, 0, nodes::condition(1).unwrap().class, false)
        .unwrap();
    graph.resize_array(rows, 2).unwrap();
    let first = graph.blocks[rows].links[&0];
    let second = graph.blocks[rows].links[&32];
    assert_ne!(first, second);
    graph.blocks[second].bytes[8..12].copy_from_slice(&2.5_f32.to_le_bytes());
    assert_ne!(graph.blocks[first].bytes, graph.blocks[second].bytes);
    let bytes = graph.emit().unwrap();
    assert_eq!(
        Graph::read(&bytes, 0, class).unwrap().emit().unwrap(),
        bytes
    );
}

#[test]
fn zero_array_pointer_requires_a_zero_count() {
    let class = nodes::condition(26).unwrap().class;
    let mut bytes = template(true, 26).unwrap();
    bytes[8..16].copy_from_slice(&1u64.to_le_bytes());
    bytes[16..24].fill(0);
    assert!(Graph::read(&bytes, 0, class).is_err());
}

#[test]
fn inline_label_arrays_offer_all_four_operations_even_when_stock_lists_are_empty() {
    for entry in nodes::CONDITIONS
        .iter()
        .chain(&nodes::EFFECTS)
        .filter(|entry| entry.observed())
    {
        for (offset, class, _) in schema::inline(entry.class).unwrap() {
            if class != labels::SOURCE_CLASS {
                continue;
            }
            for operation in 0..4 {
                assert_eq!(
                    schema::choices(entry.class, offset + operation * 16 + 8),
                    [(0x808094B3, true)],
                    "{} operation {operation}",
                    entry.name
                );
            }
        }
    }
}

#[test]
fn typed_references_reject_wrong_rows_and_allow_new_condition_kinds() {
    let class = nodes::condition(26).unwrap().class;
    let mut graph = Graph::read(&template(true, 26).unwrap(), 0, class).unwrap();
    let before = graph.clone();
    assert!(graph.create_target(0, 16, 0x808094B3, true).is_err());
    assert_eq!(graph, before);
    graph.create_target(0, 16, 0x80803E32, true).unwrap();
    let rows = graph.blocks[0].links[&16];
    graph.resize_array(rows, 1).unwrap();
    for entry in nodes::CONDITIONS.iter().filter(|entry| entry.observed()) {
        graph.create_target(rows, 0, entry.class, false).unwrap();
    }
    assert!(
        graph
            .create_target(rows, 0, nodes::effect(43).unwrap().class, false)
            .is_err()
    );
}

#[test]
fn reserve_transfer_programs_edit_independently_and_keep_native_bits() {
    use crate::sandbox_perk::action::RESERVE_TRANSFER_PROGRAMS;
    let class = nodes::effect(16).unwrap().class;
    let mut graph = Graph::read(&template(false, 16).unwrap(), 0, class).unwrap();
    let mut expected = Vec::new();
    for (slot, (offset, _, _)) in RESERVE_TRANSFER_PROGRAMS.iter().enumerate() {
        let mut program = value::Program::read(&graph, 0, *offset).unwrap();
        program.constants = vec![[(slot as f32 * 0.125).to_bits(); 4]];
        program.constants.push([0; 4]);
        program.write(&mut graph, 0, *offset).unwrap();
        // Seed source bits directly. Authoring new nonfinite values is disallowed.
        let constants = graph.blocks[0].links[&(offset + 24)];
        for (lane, bits) in [0x80000000u32, 0x7FC12345, 0xFFC12345, 0]
            .into_iter()
            .enumerate()
        {
            let start = 16 + lane * 4;
            graph.blocks[constants].bytes[start..start + 4].copy_from_slice(&bits.to_le_bytes());
        }
        expected.push(value::Program::read(&graph, 0, *offset).unwrap());
    }
    expected[1].constants[0] = [0.2f32.to_bits(); 4];
    expected[1]
        .write(&mut graph, 0, RESERVE_TRANSFER_PROGRAMS[1].0)
        .unwrap();
    let reloaded = Graph::read(&graph.emit().unwrap(), 0, class).unwrap();
    for ((offset, _, _), expected) in RESERVE_TRANSFER_PROGRAMS.iter().zip(expected) {
        assert_eq!(
            value::Program::read(&reloaded, 0, *offset).unwrap(),
            expected
        );
    }
    let before = graph.clone();
    let mut invalid = value::Program::read(&graph, 0, 0x78).unwrap();
    invalid.constants[0][0] = 0x7FC56789;
    assert!(invalid.write(&mut graph, 0, 0x78).is_err());
    assert_eq!(graph, before);
}

#[test]
fn native_value_program_writes_preserve_polynomial_mode_and_reject_bad_operands() {
    let class = nodes::effect(8).unwrap().class;
    let mut graph = Graph::read(&template(false, 8).unwrap(), 0, class).unwrap();
    let offset = schema::inline(class)
        .unwrap()
        .into_iter()
        .find(|(_, c, _)| *c == value::CLASS)
        .unwrap()
        .0;
    let mut value = value::Program::read(&graph, 0, offset).unwrap();
    value.constants = vec![[
        0.1_f32.to_bits(),
        0.2_f32.to_bits(),
        0.3_f32.to_bits(),
        0.4_f32.to_bits(),
    ]];
    value.fast_path = 1;
    value.write(&mut graph, 0, offset).unwrap();
    assert_eq!(value::Program::read(&graph, 0, offset).unwrap(), value);
    let before = graph.clone();
    value.instructions = vec![
        value::Instruction {
            opcode: 52,
            operand: Some(255),
        },
        value::Instruction {
            opcode: 62,
            operand: Some(0),
        },
    ];
    assert!(value.write(&mut graph, 0, offset).is_err());
    assert_eq!(before, graph);
}

/// Swashbuckler's shape: its maximum times the stacks over five, saturated. It gives a fifth of
/// the maximum a stack and holds the maximum past five. A comparison, whose direction is not
/// settled, gives no value rather than a guess.
#[test]
fn stock_ramps_evaluate_by_stack_and_hold_at_their_cap() {
    let code = [
        52, 0, 60, 0, 34, 0, 52, 1, 3, 35, 35, 34, 0, 52, 2, 15, 35, 34, 0, 3, 62, 0,
    ];
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let opcode = code[at];
        let operand = (opcode == 34 || opcode >= 52).then(|| code[at + 1]);
        at += 1 + usize::from(operand.is_some());
        instructions.push(value::Instruction { opcode, operand });
    }
    let row = |lanes: [f32; 4]| lanes.map(f32::to_bits);
    let mut ramp = value::Program {
        instructions,
        constants: vec![
            row([1.0 / 3.0; 4]),
            row([0.2, 1.0, 1.0, 1.0]),
            row([0.0, 0.0, 1.0, 0.0]),
        ],
        fast_path: 0,
    };
    ramp.validate().unwrap();
    let values = [0.0, 1.0, 5.0, 8.0].map(|stacks| ramp.evaluate(stacks).unwrap());
    for (value, expected) in values
        .into_iter()
        .zip([0.0, 1.0 / 15.0, 1.0 / 3.0, 1.0 / 3.0])
    {
        assert!((value - expected).abs() < 1e-6, "{values:?}");
    }
    // The multiplication by a fifth becomes a comparison.
    ramp.instructions[4].opcode = 10;
    ramp.validate().unwrap();
    assert_eq!(ramp.evaluate(1.0), None);
}

#[test]
fn label_edits_rebuild_both_predicate_forms_and_added_label_sets() {
    let registry = crate::package_runtime::labels::fixture::registry();
    for kind in [37, 54] {
        let class = nodes::effect(kind).unwrap().class;
        let mut graph = Graph::read(&template(false, kind).unwrap(), 0, class).unwrap();
        let (source, predicate) = labels::bindings(class).unwrap()[0];
        graph
            .create_target(0, source + 8, 0x808094B3, true)
            .unwrap();
        let rows = graph.blocks[0].links[&(source + 8)];
        graph.resize_array(rows, 1).unwrap();
        graph.blocks[rows].bytes[..4].copy_from_slice(&0x962EA19Bu32.to_le_bytes());
        labels::compile(&mut graph, &registry).unwrap();
        assert_eq!(labels::effective(&graph, 0, predicate).unwrap()[0][1], 2);
        graph
            .create_target(0, source + 24, 0x808094B3, true)
            .unwrap();
        let rows = graph.blocks[0].links[&(source + 24)];
        graph.resize_array(rows, 1).unwrap();
        graph.blocks[rows].bytes[..4].copy_from_slice(&0x962EA19Bu32.to_le_bytes());
        let (add, mask) = if kind == 37 {
            (0x98, 0xA8)
        } else {
            (0x68, 0x78)
        };
        graph.create_target(0, add + 8, 0x808094B3, true).unwrap();
        let rows = graph.blocks[0].links[&(add + 8)];
        graph.resize_array(rows, 1).unwrap();
        graph.blocks[rows].bytes[..4].copy_from_slice(&0x962EA19Bu32.to_le_bytes());
        labels::compile(&mut graph, &registry).unwrap();
        assert_eq!(graph.blocks[0].bytes[predicate], 1);
        assert_eq!(labels::effective(&graph, 0, predicate).unwrap()[1][1], 2);
        assert_eq!(graph.blocks[0].bytes[mask + 1], 2);
    }
}

#[test]
#[ignore = "requires PARHELION_PERK_SURVEY and SUNDIAL_LABEL_REGISTRY with captured native data"]
fn captured_actions_preserve_every_node_and_runtime_label_predicate() {
    let root = std::path::PathBuf::from(
        std::env::var_os("PARHELION_PERK_SURVEY").expect("survey directory"),
    );
    let inventory: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("inventory.json")).unwrap()).unwrap();
    let registry =
        std::fs::read(std::env::var_os("SUNDIAL_LABEL_REGISTRY").expect("label registry path"))
            .unwrap();
    let mut action_count = 0;
    let mut node_count = 0;
    let mut predicates = 0;
    let mut programs = 0;
    for action in inventory["actions"].as_array().unwrap() {
        let tag = action["tag"].as_u64().unwrap();
        let data = std::fs::read(root.join(format!("actions/{tag:08X}.bin"))).unwrap();
        let decoded = crate::sandbox_perk::action::decode(&data)
            .unwrap_or_else(|error| panic!("0x{tag:08X}: {error}"));
        for (class, kind, condition, bytes) in decoded
            .conditions()
            .into_iter()
            .map(|n| (n.class, n.kind, true, &n.native))
            .chain(
                decoded
                    .effects()
                    .map(|n| (n.class, n.kind, false, &n.native)),
            )
        {
            let graph = Graph::read(bytes, 0, class).unwrap();
            graph
                .validate_node(condition, kind)
                .unwrap_or_else(|error| panic!("0x{tag:08X} kind {kind}: {error}"));
            assert_eq!(
                Graph::read(&graph.emit().unwrap(), 0, class).unwrap(),
                graph
            );
            node_count += 1;
        }
        let graph = Graph::read(&data, 0, crate::sandbox_perk::action::ACTION_ROOT_CLASS).unwrap();
        let mut rebuilt = graph.clone();
        labels::compile(&mut rebuilt, &registry)
            .unwrap_or_else(|error| panic!("0x{tag:08X}: {error}"));
        assert_eq!(
            graph, rebuilt,
            "Unedited native label data changed in 0x{tag:08X}"
        );
        for (block, record) in graph
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.class != 0)
        {
            let stride = schema::record(record.class).unwrap().size;
            for row in 0..record.count.unwrap_or(1) {
                for (offset, class, _) in schema::inline(record.class).unwrap() {
                    let offset = row * stride + offset;
                    if class == labels::PREDICATE_CLASS {
                        assert!(
                            labels::bindings(record.class)
                                .unwrap()
                                .iter()
                                .any(|(_, predicate)| row * stride + predicate == offset),
                            "Unbound runtime predicate in 0x{tag:08X} class 0x{:08X}",
                            record.class
                        );
                        assert_eq!(
                            labels::effective(&graph, block, offset).unwrap(),
                            labels::effective(&rebuilt, block, offset).unwrap(),
                            "0x{tag:08X} block {block} +0x{offset:X}"
                        );
                        predicates += 1;
                    }
                    if class == value::CLASS {
                        value::validate(&graph, block, offset).unwrap();
                        programs += 1;
                    }
                }
            }
        }
        action_count += 1;
    }
    assert_eq!(
        (action_count, node_count, predicates, programs),
        (1632, 6432, 2218, 690)
    );
    eprintln!(
        "Verified {action_count} actions, {node_count} nodes, {predicates} predicates and {programs} value programs."
    );
}

#[test]
fn event_keys_are_named_from_the_perks_that_use_them() {
    use super::fields::keys::{self, SITES};
    for (class, offset, entries) in SITES {
        assert!(!entries.is_empty(), "{class:08X}+{offset:X} has no keys");
        let mut seen = std::collections::BTreeSet::new();
        for key in *entries {
            assert!(
                seen.insert(key.hash),
                "{class:08X}+{offset:X} lists 0x{:08X} twice",
                key.hash
            );
            assert!(!key.name.trim().is_empty() && !key.evidence.trim().is_empty());
            assert_eq!(keys::name(key.hash), Some(key.name));
        }
        assert_eq!(keys::known(*class, *offset), *entries);
        // The site is a key field of an observed node class, or the resource reference the
        // ability conditions store, whose stock values are ability patterns named the same way.
        let fields = super::fields::describe(*class).unwrap();
        assert!(
            fields.iter().any(|field| field.offset == *offset
                && matches!(
                    field.format,
                    super::fields::Format::Key | super::fields::Format::Tag
                )),
            "{class:08X}+{offset:X} is not a key or reference field"
        );
    }
    assert_eq!(keys::name(0x6CEC_7A87), Some("Orb of Light Picked Up"));
    assert!(keys::known(0x8080_3E03, 8).is_empty());
}

#[test]
fn promoted_condition_defaults_carry_the_stock_event_bytes() {
    use crate::sandbox_perk::{action::layout::stock_defaults, program::NativeNode};
    let reload = NativeNode::condition(19).unwrap();
    assert_eq!(
        (reload.bytes[8], reload.bytes[9], reload.bytes[0xA]),
        (1, 1, 0)
    );
    assert_eq!(NativeNode::condition(22).unwrap().bytes[8], 1);
    let aim = NativeNode::condition(23).unwrap();
    assert_eq!((aim.bytes[8], aim.bytes[0xA]), (1, 1));
    assert_eq!(NativeNode::condition(27).unwrap().bytes[8], 1);
    assert_eq!(NativeNode::condition(42).unwrap().bytes[8], 1);
    // A kind with no plain reading starts as the template leaves it.
    assert!(stock_defaults(true, 20).is_empty());
    assert!(stock_defaults(false, 19).is_empty());
    // The reload flag is a described field, so the editor shows it by name.
    let fields = super::fields::describe(0x8080_3DE2).unwrap();
    assert!(
        fields
            .iter()
            .any(|field| field.offset == 9 && field.label == "On Reload")
    );
}

#[test]
fn ability_slot_bits_and_radar_range_are_named_from_the_perks_that_set_them() {
    use crate::sandbox_perk::program::NativeNode;
    // Kind 9's slot mask is a 32-bit field whose named bits follow kind 8's numbering.
    let fields = super::fields::describe(0x8080_3E00).unwrap();
    let mask = fields
        .iter()
        .find(|field| field.offset == 8)
        .expect("kind 9's slot mask is described");
    assert_eq!(mask.format, super::fields::Format::Mask32);
    assert_eq!(
        super::fields::contract(0x8080_3E00, mask).choices.to_vec(),
        vec![
            (2, "Super"),
            (4, "Melee"),
            (8, "Jump"),
            (128, "Class Ability")
        ]
    );
    // Kind 18's third float is the radar range, and a fresh node leaves all three at -1,
    // the value the callback leaves unchanged, rather than writing zero over them.
    let fields = super::fields::describe(0x8080_3E23).unwrap();
    assert!(
        fields
            .iter()
            .any(|field| field.offset == 0x0C && field.label == "Radar Detection Range")
    );
    let node = NativeNode::effect(18).unwrap();
    for at in [4usize, 8, 12] {
        let value = f32::from_le_bytes(node.bytes[at..at + 4].try_into().unwrap());
        assert_eq!(value, -1.0, "float at +{at:X}");
    }
    // The new named action must add radar detail, rather than starting with the
    // subtractive operation inherited from the old anonymous zero-filled node.
    assert_eq!(NativeNode::effect(20).unwrap().bytes[2], 1);
    // Kind 13's weights are the ammo types.
    let fields = super::fields::describe(0x8080_3E47).unwrap();
    for (offset, label) in [
        (0x20, "Primary Ammo Weight"),
        (0x2C, "Special Ammo Weight"),
        (0x38, "Heavy Ammo Weight"),
    ] {
        assert!(
            fields
                .iter()
                .any(|field| field.offset == offset && field.label == label),
            "{label}"
        );
    }
}

/// Every client-recovered key name must hash to the key it is filed under. The names came
/// from matching FNV-1 hashes in client memory, so this is the same check that found them
/// and it fails loudly if an entry is ever mistyped.
#[test]
fn client_recovered_key_names_hash_to_their_keys() {
    for (hash, name) in super::fields::keys::CLIENT_NAMES {
        assert_eq!(crate::hash::fnv1_name_hash(name), *hash, "{name}");
        assert_eq!(super::fields::keys::client_name(*hash), Some(*name));
    }
}

/// Every named key whose evidence cites an engine name must hash from that name, whether the
/// name came from the client scan or from hashing the checking perks' own words.
#[test]
fn engine_names_cited_as_key_evidence_hash_to_their_keys() {
    let mut cited = 0;
    for (_, _, entries) in super::fields::keys::SITES {
        for key in *entries {
            let Some(rest) = key.evidence.strip_prefix("The engine name ") else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            assert_eq!(crate::hash::fnv1_name_hash(&name), key.hash, "{name}");
            cited += 1;
        }
    }
    assert!(cited > 0, "no key cites an engine name");
}

/// Every edit allocates its target afresh rather than writing through one that another node
/// may share, so each one leaves the allocation it replaced behind. An authored program is
/// stored as its block list, and `validate` counts every block against the allocation cap,
/// so without compaction an editing session grows its own saved recipe until no frame can
/// validate it and the program can no longer be edited at all.
#[test]
fn repeated_edits_do_not_grow_a_graph_past_what_its_root_reaches() {
    let class = nodes::condition(2).unwrap().class;
    let mut graph = Graph::read(&template(true, 2).unwrap(), 0, class).unwrap();
    let opened = graph.blocks.len();
    // The kill node's label array, reallocated the way every label edit reallocates it.
    for _ in 0..64 {
        graph.create_target(0, 0xD0 + 8, 0x808094B3, true).unwrap();
    }
    assert!(
        graph.blocks.len() > opened + 60,
        "an edit should leave its replaced allocation behind, found {} blocks",
        graph.blocks.len()
    );
    let edited = graph.emit().unwrap();
    // What the program actually consists of: emitting writes the blocks the root reaches and
    // nothing else, so reading those bytes back is the graph with no allocation left over.
    let reachable = Graph::read(&edited, 0, class).unwrap().blocks.len();
    graph.compact();
    assert_eq!(graph.blocks.len(), reachable);
    // Compaction drops only what the root cannot reach, so the program is the same one.
    assert_eq!(graph.emit().unwrap(), edited);
    graph.validate().unwrap();
}

/// Reading a native object memoizes by resource position, so two nodes that pointed at one
/// resource share a single block here. Writing through that block would change both nodes,
/// and the other one is usually elsewhere in the program entirely.
#[test]
fn an_edit_through_a_shared_allocation_copies_it_first() {
    let class = nodes::condition(2).unwrap().class;
    let mut graph = Graph::read(&template(true, 2).unwrap(), 0, class).unwrap();
    let (field, target) = graph.blocks[0]
        .links
        .iter()
        .map(|(field, target)| (*field, *target))
        .next()
        .expect("the kill node owns an allocation");
    // A second owner sharing all of the first one's allocations, as a read of two nodes
    // naming one resource produces.
    let sibling = graph.blocks.len();
    graph.blocks.push(graph.blocks[0].clone());
    let untouched = graph.blocks[target].bytes.clone();

    let copy = graph.make_unique(sibling, field).unwrap();
    assert_ne!(
        copy, target,
        "a shared allocation must be copied before a write"
    );
    assert_eq!(
        graph.blocks[0].links[&field], target,
        "the other owner keeps its own"
    );
    graph.blocks[copy].bytes.fill(0xAB);
    assert_eq!(graph.blocks[target].bytes, untouched);

    // Nothing else reaches it now, so the next edit writes in place instead of copying again.
    assert_eq!(graph.make_unique(sibling, field).unwrap(), copy);
}

/// The node editors re-emit the record they were given on every frame, so reading one has to
/// return the bytes it was read from. If it did not, merely opening a stock effect would read
/// as an edit and adopt it into an authored program the reader never asked for.
#[test]
fn reading_and_re_emitting_a_node_template_returns_the_same_bytes() {
    let mut checked = 0;
    for (condition, node) in nodes::CONDITIONS
        .iter()
        .map(|node| (true, node))
        .chain(nodes::EFFECTS.iter().map(|node| (false, node)))
    {
        let Some(bytes) = template(condition, node.kind) else {
            continue;
        };
        let graph = Graph::read(&bytes, 0, node.class).unwrap();
        assert_eq!(
            graph.emit().unwrap(),
            bytes,
            "kind {} ({}) does not survive a read and re-emit",
            node.kind,
            node.name
        );
        checked += 1;
    }
    assert!(checked > 20, "only {checked} templates were covered");
}
