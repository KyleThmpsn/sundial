use super::*;
use crate::sandbox_perk::program::{self, decompile};

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn orb_generation_compiles_a_kill_event_position_and_keeps_native_asset_editable() {
    let path =
        std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = crate::package_authoring::open_shadowkeep_package_manager(&path).unwrap();
    let program = Program {
        name: "Orb Harvest".into(),
        trigger: program::Trigger::WeaponKill,
        cooldown_ms: 3000,
        actions: vec![program::Action::generate_orb(program::Position::Event)],
        ..Program::default()
    };
    let compiled = program::compile(&manager, &program).unwrap();
    let native = Program::from_native(&compiled.payload, "Orb Harvest").unwrap();
    let node = native
        .native
        .as_ref()
        .unwrap()
        .graph
        .blocks
        .iter()
        .find(|block| block.class == 0x80803E42)
        .unwrap();
    assert_eq!(node.bytes[2], 1, "activation event transform");
    assert_eq!(crate::package_payload::u32_at(&node.bytes, 4).unwrap(), 1);
    assert_eq!(
        crate::package_payload::u32_at(&node.bytes, 8).unwrap(),
        1f32.to_bits()
    );
    assert!(native.assets().any(|asset| asset.graph == 0x80EFAE02));
    assert_eq!(
        action::decode(&compiled.payload)
            .unwrap()
            .effects()
            .next()
            .unwrap()
            .kind,
        5
    );
    let compiled_native = program::compile(&manager, &native).unwrap();
    assert!(
        compiled_native
            .asset_offsets
            .iter()
            .any(|(_, offsets)| !offsets.is_empty())
    );
}

#[test]
fn complete_program_drafts_can_add_empty_lists_before_choosing_nodes() {
    let mut program = Program {
        native: Some(NativeProgram::empty()),
        ..Program::default()
    };
    let native = program.native.as_mut().unwrap();
    native
        .graph
        .create_target(0, 0x28, 0x808040BA, true)
        .unwrap();
    let rows = native.graph.blocks[0].links[&0x28];
    native.graph.resize_array(rows, 1).unwrap();
    program.validate_structure().unwrap();
    assert!(program.validate().is_err());
    let native = program.native.as_mut().unwrap();
    native
        .graph
        .create_target(
            rows,
            0,
            crate::sandbox_perk::nodes::condition(0).unwrap().class,
            false,
        )
        .unwrap();
    program.validate().unwrap();
    let restored: Program =
        serde_json::from_str(&serde_json::to_string(&program).unwrap()).unwrap();
    assert_eq!(restored, program);
    program.actions.push(program::Action::add_rounds(1));
    assert!(program.validate_structure().is_err());
}

#[test]
#[ignore = "requires the captured native action survey"]
fn captured_programs_preserve_all_groups_policies_and_auxiliary_records() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tmp/projectile-picker-20260910/runtime");
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("inventory.json")).unwrap()).unwrap();
    let mut counts = [0usize; 4];
    for row in index["actions"].as_array().unwrap() {
        let tag = row["tag"].as_u64().unwrap();
        let source = std::fs::read(root.join(format!("actions/{tag:08X}.bin"))).unwrap();
        let program = Program::from_native(&source, format!("{tag:08X}"))
            .unwrap_or_else(|error| panic!("{tag:08X}: {error}"));
        program.validate().unwrap();
        let emitted = program.native.as_ref().unwrap().graph.emit().unwrap();
        assert!(
            decompile::native_fidelity(&source, &emitted)
                .unwrap()
                .is_empty(),
            "{tag:08X}"
        );
        let action = action::decode(&emitted).unwrap();
        counts[0] += 1;
        counts[1] += usize::from(action.groups.len() > 1);
        counts[2] += usize::from(action.policy != 0);
        counts[3] += usize::from(!action.auxiliary.is_empty());
    }
    assert!(counts.iter().all(|count| *count > 0));
    println!(
        "Complete programs: {}, multiple groups: {}, policies: {}, auxiliary records: {}",
        counts[0], counts[1], counts[2], counts[3]
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn native_complete_programs_compile_multiple_groups_policies_and_auxiliary_data() {
    let path =
        std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = crate::package_authoring::open_shadowkeep_package_manager(&path).unwrap();
    let globals = manager
        .read_tag(
            crate::package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)
                .unwrap(),
        )
        .unwrap();
    for index in [26, 40, 104, 421, 453, 500, 501, 1019, 1178, 1283, 1683, 192] {
        let source =
            crate::sandbox_perk::load_sandbox_perk_runtime_action(&manager, &globals, index)
                .unwrap();
        let program =
            Program::from_native(&source.action_payload, format!("Perk {index}")).unwrap();
        let compiled =
            program::compile(&manager, &program).unwrap_or_else(|error| panic!("{index}: {error}"));
        let differences =
            decompile::native_fidelity(&source.action_payload, &compiled.payload).unwrap();
        assert!(differences.is_empty(), "{index}: {differences:?}");
        let before = action::decode(&source.action_payload).unwrap();
        let after = action::decode(&compiled.payload).unwrap();
        assert_eq!(before.groups.len(), after.groups.len());
        assert_eq!(before.policy, after.policy);
        assert_eq!(before.auxiliary, after.auxiliary);
        assert_eq!(before.activation_event_mask, after.activation_event_mask);
        assert_eq!(before.removal_event_mask, after.removal_event_mask);
        assert_eq!(before.rearm_event_mask, after.rearm_event_mask);
    }
}

#[test]
fn native_readiness_finds_required_assets_in_execution_order_and_keeps_optional_ones() {
    use program::{Action, NativeNode, Trigger};
    let missing_asset = |kind| {
        let mut node = NativeNode::effect(kind).unwrap();
        // Templates may already name a real stock entity. Exercise an explicitly empty choice.
        node.bytes[16..20].fill(0);
        node
    };
    for kind in [1, 2, 3, 26] {
        let draft = Program {
            trigger: Trigger::Always,
            actions: vec![Action::Native {
                node: missing_asset(kind),
            }],
            ..Program::default()
        };
        draft.validate_structure().unwrap();
        assert!(draft.validate().is_err());
        let native = program::native_draft(&draft).unwrap();
        let issue = native.authoring_issue().unwrap().unwrap();
        assert_eq!((issue.group, issue.action), (0, 0));
        let mut complete = Program {
            native: Some(native),
            ..Program::default()
        };
        complete.validate_structure().unwrap();
        assert!(complete.validate().is_err());
        let block = complete
            .native
            .as_mut()
            .unwrap()
            .graph
            .blocks
            .iter_mut()
            .find(|block| block.class == crate::sandbox_perk::nodes::effect(kind).unwrap().class)
            .unwrap();
        block.bytes[16..20].copy_from_slice(&0x80abcdefu32.to_le_bytes());
        complete.validate().unwrap();
    }
    let weighted = Program {
        actions: vec![Action::Native {
            node: missing_asset(13),
        }],
        ..Program::default()
    };
    weighted.validate().unwrap();
    assert!(
        program::native_draft(&weighted)
            .unwrap()
            .authoring_issue()
            .unwrap()
            .is_none()
    );
    let multiple = Program {
        trigger: Trigger::Always,
        actions: vec![
            Action::add_rounds(1),
            Action::Native {
                node: missing_asset(26),
            },
        ],
        ..Program::default()
    };
    assert_eq!(
        program::native_draft(&multiple)
            .unwrap()
            .authoring_issue()
            .unwrap()
            .unwrap()
            .action,
        1
    );
    let additional = Program {
        additional_groups: vec![program::NativeGroup {
            activation: Vec::new(),
            effects: vec![missing_asset(26)],
            removal: Vec::new(),
            rearm: Vec::new(),
        }],
        ..Program::default()
    };
    additional.validate_structure().unwrap();
    assert!(additional.validate().is_err());
    let issue = program::native_draft(&additional)
        .unwrap()
        .authoring_issue()
        .unwrap()
        .unwrap();
    assert_eq!((issue.group, issue.action), (1, 0));
}
