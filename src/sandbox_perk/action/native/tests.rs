use super::*;
use crate::sandbox_perk::nodes;

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

#[test]
fn label_edits_rebuild_both_predicate_forms_and_added_label_sets() {
    let registry = include_bytes!("tests/label_registry.bin");
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
        labels::compile(&mut graph, registry).unwrap();
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
        labels::compile(&mut graph, registry).unwrap();
        assert_eq!(graph.blocks[0].bytes[predicate], 1);
        assert_eq!(labels::effective(&graph, 0, predicate).unwrap()[1][1], 2);
        assert_eq!(graph.blocks[0].bytes[mask + 1], 2);
    }
}

#[test]
#[ignore = "requires PARHELION_PERK_SURVEY with the captured runtime survey"]
fn captured_actions_preserve_every_node_and_runtime_label_predicate() {
    let root = std::path::PathBuf::from(
        std::env::var_os("PARHELION_PERK_SURVEY").expect("survey directory"),
    );
    let inventory: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("inventory.json")).unwrap()).unwrap();
    let registry = include_bytes!("tests/label_registry.bin");
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
        labels::compile(&mut rebuilt, registry)
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
