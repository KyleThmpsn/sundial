use super::*;
use crate::package_payload::native_array_at;
use crate::sandbox_perk::action::native::{Graph, labels};
use crate::sandbox_perk::nodes;

/// A synthetic installation: the fixture label registry and a fixed set of live
/// resource tags. Nothing here reads a game package.
struct Synthetic {
    registry: Vec<u8>,
    live: Vec<u32>,
}

impl Synthetic {
    /// The label globals are always live: every event condition refers to them.
    fn new(live: &[u32]) -> Self {
        let mut live = live.to_vec();
        live.push(LABEL_GLOBALS);
        Self {
            registry: crate::package_runtime::labels::fixture::registry(),
            live,
        }
    }
}

impl NativeResolver for Synthetic {
    fn registry(&mut self) -> Result<&[u8], String> {
        Ok(&self.registry)
    }

    fn validate_resources(&self, graph: &Graph) -> Result<(), String> {
        for (_, tag) in referenced_resources(graph)? {
            if !self.live.contains(&tag) {
                return Err(format!("Native resource 0x{tag:08X} is missing."));
            }
        }
        Ok(())
    }
}

const KILL: u8 = 2;
const PRECISION: u32 = 0x962E_A19B;
const DEAD_RESOURCE: u32 = 0x8ABC_0001;

#[test]
fn editable_draft_preserves_symbolic_filters_extra_groups_and_native_bits() {
    let mut node = NativeNode::effect(47).unwrap();
    node.bytes[4..8].copy_from_slice(&0xFEEDABCDu32.to_le_bytes());
    let program = Program {
        trigger: Trigger::PrecisionKill,
        actions: vec![
            Action::UpdateAccumulator {
                mode: 0,
                value_bits: 0x80000000,
            },
            Action::Native { node: node.clone() },
        ],
        additional_groups: vec![NativeGroup {
            effects: vec![node],
            ..NativeGroup::default()
        }],
        ..Program::default()
    };
    let mut native = draft(&program).unwrap();
    let registry = crate::package_runtime::labels::fixture::registry();
    labels::compile(&mut native.graph, &registry).unwrap();
    let expected = assemble(
        &program,
        Some((
            trigger_labels(program.trigger),
            compile_labels(&registry, &[PRECISION]).unwrap(),
        )),
    )
    .unwrap();
    assert!(
        super::super::decompile::native_fidelity(&expected.payload, &native.graph.emit().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(native.assets.len(), 0);
}

#[test]
fn draft_keeps_component_overrides_and_refuses_to_merge_distinct_edits() {
    use crate::runtime::{
        WeaponRuntimeFieldLocator, WeaponRuntimeRootKind, WeaponRuntimeValue,
        WeaponRuntimeValueOverride,
    };
    let first = Asset {
        graph: 0x815282E1,
        path: "content/projectile.pattern.tft".into(),
        values: vec![WeaponRuntimeValueOverride {
            locator: WeaponRuntimeFieldLocator {
                graph_tag: Some(0x815282E1.into()),
                binding_hash: 1.into(),
                resource_index: 0,
                root: WeaponRuntimeRootKind::ComponentInstance,
                root_schema: 0x80803B73.into(),
                path: vec![],
                type_handle: 2.into(),
                value_offset: 0x144,
                byte_size: 8,
            },
            value: WeaponRuntimeValue::Bytes(
                [0x7FC12345_u32.to_le_bytes(), 0x80000000_u32.to_le_bytes()].concat(),
            ),
        }],
        damage_type: None,
        hud_status: None,
        rows: Vec::new(),
        script: None,
    };
    let program = Program {
        actions: vec![Action::Pattern {
            asset: first.clone(),
        }],
        ..Program::default()
    };
    assert_eq!(
        draft(&program).unwrap().assets,
        std::slice::from_ref(&first)
    );
    let mut conflicting = program;
    let mut second = first;
    second.values.clear();
    conflicting.actions.push(Action::attach(second));
    assert!(draft(&conflicting).is_err());
}

fn kill_class() -> u32 {
    nodes::condition(KILL).unwrap().class
}

/// A Kill Event whose first source label list names a precision kill while its compiled
/// masks are still those of the template, exactly as a label edit leaves a node before
/// it is compiled.
fn kill_with_stale_masks() -> NativeNode {
    let mut node = NativeNode::condition(KILL).unwrap();
    let mut graph = Graph::read(&node.bytes, 0, kill_class()).unwrap();
    let (source, _) = labels::bindings(kill_class()).unwrap()[0];
    graph
        .create_target(0, source + 8, 0x808094B3, true)
        .unwrap();
    let rows = graph.blocks[0].links[&(source + 8)];
    graph.resize_array(rows, 1).unwrap();
    graph.blocks[rows].bytes[..4].copy_from_slice(&PRECISION.to_le_bytes());
    node.bytes = graph.emit().unwrap();
    node
}

/// A Kill Event whose resource lane names a tag no installation holds.
fn kill_with_dead_resource() -> NativeNode {
    let mut node = NativeNode::condition(KILL).unwrap();
    node.bytes[80..84].copy_from_slice(&DEAD_RESOURCE.to_le_bytes());
    node
}

fn compiled_precision_mask(node: &NativeNode) -> [u8; 40] {
    let graph = Graph::read(&node.bytes, 0, kill_class()).unwrap();
    let (_, predicate) = labels::bindings(kill_class()).unwrap()[0];
    labels::effective(&graph, 0, predicate).unwrap()[0]
}

type Read = fn(&Program) -> &NativeNode;

/// Every position that carries a verbatim condition, each holding one copy of the node,
/// with a way to read that copy back after preparation. The primary trigger comes first
/// and is the reference the other positions must match.
fn condition_positions(node: &NativeNode) -> Vec<(String, Program, Read)> {
    let base = Program {
        trigger: Trigger::Always,
        duration_ms: 1000,
        actions: vec![Action::native(43).unwrap()],
        ..Program::default()
    };
    let group = |group: NativeGroup| Program {
        additional_groups: vec![group],
        ..base.clone()
    };
    let positions: Vec<(&str, Program, Read)> = vec![
        (
            "native_trigger",
            Program {
                trigger: Trigger::Native,
                native_trigger: Some(node.clone()),
                ..base.clone()
            },
            |program| program.native_trigger.as_ref().unwrap(),
        ),
        (
            "alternative_triggers",
            Program {
                alternative_triggers: vec![node.clone()],
                ..base.clone()
            },
            |program| &program.alternative_triggers[0],
        ),
        (
            "native_removal",
            Program {
                native_removal: Some(node.clone()),
                ..base.clone()
            },
            |program| program.native_removal.as_ref().unwrap(),
        ),
        (
            "alternative_removals",
            Program {
                alternative_removals: vec![node.clone()],
                ..base.clone()
            },
            |program| &program.alternative_removals[0],
        ),
        (
            "native_rearm",
            Program {
                native_rearm: Some(node.clone()),
                ..base.clone()
            },
            |program| program.native_rearm.as_ref().unwrap(),
        ),
        (
            "alternative_rearms",
            Program {
                alternative_rearms: vec![node.clone()],
                ..base.clone()
            },
            |program| &program.alternative_rearms[0],
        ),
        (
            "additional_groups.activation",
            group(NativeGroup {
                activation: vec![node.clone()],
                ..NativeGroup::default()
            }),
            |program| &program.additional_groups[0].activation[0],
        ),
        (
            "additional_groups.removal",
            group(NativeGroup {
                removal: vec![node.clone()],
                ..NativeGroup::default()
            }),
            |program| &program.additional_groups[0].removal[0],
        ),
        (
            "additional_groups.rearm",
            group(NativeGroup {
                rearm: vec![node.clone()],
                ..NativeGroup::default()
            }),
            |program| &program.additional_groups[0].rearm[0],
        ),
    ];
    positions
        .into_iter()
        .map(|(name, program, read)| (name.to_string(), program, read))
        .collect()
}

#[test]
fn label_edits_compile_in_every_native_condition_position() {
    let node = kill_with_stale_masks();
    assert_eq!(
        compiled_precision_mask(&node),
        [0; 40],
        "fixture masks are stale"
    );
    let registry = crate::package_runtime::labels::fixture::registry();
    let expected = compile_labels(&registry, &[PRECISION]).unwrap();
    assert_ne!(expected, [0; 40]);
    let mut reference = None;
    for (position, mut program, read) in condition_positions(&node) {
        let mut resolver = Synthetic::new(&[]);
        prepare_native_nodes(&mut program, &mut resolver)
            .unwrap_or_else(|error| panic!("{position}: {error}"));
        let prepared = read(&program);
        assert_eq!(
            compiled_precision_mask(prepared),
            expected,
            "{position} kept its stale masks"
        );
        match &reference {
            None => reference = Some(prepared.bytes.clone()),
            Some(bytes) => assert_eq!(
                &prepared.bytes, bytes,
                "{position} compiled differently from the primary trigger"
            ),
        }
    }
}

#[test]
fn missing_resources_are_rejected_in_every_native_condition_position() {
    let node = kill_with_dead_resource();
    let graph = Graph::read(&node.bytes, 0, kill_class()).unwrap();
    assert_eq!(
        referenced_resources(&graph).unwrap(),
        vec![(kill_class(), DEAD_RESOURCE), (kill_class(), LABEL_GLOBALS)]
    );
    for (position, program, _) in condition_positions(&node) {
        let mut missing = Synthetic::new(&[]);
        let error = prepare_native_nodes(&mut program.clone(), &mut missing).expect_err(&position);
        assert!(error.contains("0x8ABC0001"), "{position}: {error}");
        let mut live = Synthetic::new(&[DEAD_RESOURCE]);
        prepare_native_nodes(&mut program.clone(), &mut live)
            .unwrap_or_else(|error| panic!("{position} with the resource live: {error}"));
    }
}

#[test]
fn missing_resources_are_rejected_in_group_effects_like_native_actions() {
    let mut node = NativeNode::effect(13).unwrap();
    node.bytes[16..20].copy_from_slice(&DEAD_RESOURCE.to_le_bytes());
    let graph = Graph::read(&node.bytes, 0, nodes::effect(13).unwrap().class).unwrap();
    assert!(
        referenced_resources(&graph)
            .unwrap()
            .iter()
            .any(|(_, tag)| *tag == DEAD_RESOURCE)
    );
    let as_action = Program {
        trigger: Trigger::Always,
        actions: vec![Action::Native { node: node.clone() }],
        ..Program::default()
    };
    let as_group_effect = Program {
        trigger: Trigger::Always,
        actions: vec![Action::native(43).unwrap()],
        additional_groups: vec![NativeGroup {
            effects: vec![node],
            ..NativeGroup::default()
        }],
        ..Program::default()
    };
    for (position, program) in [("actions", as_action), ("group effects", as_group_effect)] {
        let error = prepare_native_nodes(&mut program.clone(), &mut Synthetic::new(&[]))
            .expect_err(position);
        assert!(error.contains("0x8ABC0001"), "{position}: {error}");
        prepare_native_nodes(&mut program.clone(), &mut Synthetic::new(&[DEAD_RESOURCE]))
            .unwrap_or_else(|error| panic!("{position}: {error}"));
    }
}

#[test]
fn native_nodes_reach_every_verbatim_position() {
    let condition = NativeNode::condition(KILL).unwrap();
    let effect = NativeNode::effect(43).unwrap();
    let program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(condition.clone()),
        native_removal: Some(condition.clone()),
        native_rearm: Some(condition.clone()),
        alternative_triggers: vec![condition.clone()],
        alternative_removals: vec![condition.clone()],
        alternative_rearms: vec![condition.clone()],
        actions: vec![Action::Native {
            node: effect.clone(),
        }],
        additional_groups: vec![NativeGroup {
            activation: vec![condition.clone()],
            effects: vec![effect.clone()],
            removal: vec![condition.clone()],
            rearm: vec![condition.clone()],
        }],
        ..Program::default()
    };
    let (conditions, effects): (Vec<_>, Vec<_>) = program
        .native_nodes()
        .partition(|(condition, _)| condition.is_condition());
    assert_eq!(conditions.len(), 9);
    assert_eq!(effects.len(), 2);
    assert!(conditions.iter().all(|(_, node)| **node == condition));
    assert!(effects.iter().all(|(_, node)| **node == effect));
}

fn attach_node(action: &Action) -> Vec<u8> {
    let mut out = Payload::new();
    let at = out.action(action, None).unwrap();
    out.bytes[at..at + 64].to_vec()
}

#[test]
fn a_named_property_node_matches_the_surveyed_constant_shape() {
    let mut out = Payload::new();
    let at = out
        .action(
            &Action::Property {
                key: 0x5EE2_66FC,
                target: 2,
                operation_byte: 0,
                removal: 1,
                value_bits: 1.0_f32.to_bits(),
                restore_bits: 0,
                ability_mask: 0,
                input: 0,
                flag: 1,
            },
            None,
        )
        .unwrap();
    let node = &out.bytes[at..at + 80];
    assert_eq!(&node[..4], &[10, 1, 2, 1]);
    assert_eq!(
        u32::from_le_bytes(node[8..12].try_into().unwrap()),
        0x5EE2_66FC
    );
    assert_eq!(u64::from_le_bytes(node[0x18..0x20].try_into().unwrap()), 4);
    assert_eq!(u64::from_le_bytes(node[0x28..0x30].try_into().unwrap()), 1);
    assert_eq!(
        &node[0x38..0x48],
        &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(&node[0x48..0x4C], &[0, 0, 1, 0]);
    let decoded = crate::sandbox_perk::action::constant_program_value(&out.bytes, at + 0x18);
    assert_eq!(decoded, Some(1.0));
}

#[test]
fn an_always_active_program_ends_on_its_event_key() {
    let program = Program {
        trigger: Trigger::Always,
        duration_ms: 0,
        cooldown_ms: 3_000,
        removal_key: Some(0xA628_8DD1),
        actions: vec![Action::attach(Asset {
            graph: 0x80BC_5810,
            path: String::new(),
            values: Vec::new(),
            damage_type: None,
            rows: Vec::new(),
            hud_status: None,
            script: None,
        })],
        ..Program::default()
    };
    let mut out = Payload::new();
    assert!(out.removal_and_rearm(&program).unwrap());
    // A pointer row stores an offset relative to itself, and the node precedes the row.
    let pointed = |row: usize| {
        let relative = i64::from_le_bytes(out.bytes[row..row + 8].try_into().unwrap());
        usize::try_from(row as i64 + relative).unwrap()
    };
    let (count, _, rows, class) = native_array_at(&out.bytes, 0x48).unwrap();
    assert_eq!((count, class), (1, CONDITION_ROWS));
    let node = pointed(rows);
    assert_eq!(
        &out.bytes[node..node + 12],
        &[0, 0, 0x80, 0x3F, 0xFF, 30, 0, 1, 0xD1, 0x8D, 0x28, 0xA6]
    );
    assert_eq!(
        u64::from_le_bytes(out.bytes[0x90..0x98].try_into().unwrap()),
        1 << 30
    );
    assert_eq!(
        u64::from_le_bytes(out.bytes[0x98..0xA0].try_into().unwrap()),
        2
    );
    assert_eq!(&out.bytes[0xCC..0xCE], &[2, 1]);
    let (rearm_count, _, rearm_rows, _) = native_array_at(&out.bytes, 0x58).unwrap();
    let rearm = pointed(rearm_rows);
    assert_eq!((rearm_count, out.bytes[rearm + 7]), (1, 2));
}

#[test]
fn ammunition_nodes_match_the_label_free_stock_shape() {
    let mut out = Payload::new();
    let at = out.action(&Action::add_rounds(2), None).unwrap();
    let node = &out.bytes[at..at + 136];
    assert_eq!(&node[..4], &[14, 0, 0, 0]);
    assert!(node[4..0x48].iter().all(|byte| *byte == 0));
    assert_eq!(
        u64::from_le_bytes(node[0x50..0x58].try_into().unwrap()),
        u64::from(LABEL_GLOBALS)
    );
    assert_eq!(node[0x58], 0xFF);
    assert_eq!(&node[0x68..0x6C], &[1, 0, 0, 0]);
    assert_eq!(u32::from_le_bytes(node[0x6C..0x70].try_into().unwrap()), 2);
    assert!(node[0x70..].iter().all(|byte| *byte == 0));
    let at = out
        .action(
            &Action::AddFraction {
                fraction_bits: 0.25_f32.to_bits(),
                target: AmmunitionTarget::Category3,
                store: AmmunitionStore::Reserves,
                capacity: AmmunitionStore::Reserves,
                overflow: true,
                action_scaled: false,
            },
            None,
        )
        .unwrap();
    let node = &out.bytes[at..at + 136];
    assert_eq!(&node[..2], &[15, 0]);
    assert_eq!(&node[0x68..0x6C], &[0, 1, 0, 0]);
    assert!(node[0x6C..0x84].iter().all(|byte| *byte == 0));
    assert_eq!(
        f32::from_le_bytes(node[0x84..0x88].try_into().unwrap()),
        0.25
    );
    // The decoder reads the compiled node back as the same facts a stock node gives.
    let decoded = crate::sandbox_perk::action::decode(&{
        let mut program = Program {
            trigger: Trigger::Always,
            duration_ms: 0,
            actions: vec![Action::add_rounds(2)],
            ..Program::default()
        };
        program.name = "Ammo".into();
        let mut out = Payload::new();
        let activation = out.unconditional(0);
        out.nodes(0x20, CONDITION_ROWS, &[activation]);
        let effect = out.action(&program.actions[0], None).unwrap();
        out.nodes(0x38, EFFECT_ROWS, &[effect]);
        out.removal_and_rearm(&program).unwrap();
        out.u64(0, out.bytes.len() as u64);
        out.bytes
    })
    .unwrap();
    let effect = &decoded.groups[0].effects[0];
    assert_eq!(effect.kind, 14);
    assert!(
        effect
            .facts
            .iter()
            .any(|fact| fact.label == "Owning Slot Amount"),
        "{:?}",
        effect.facts
    );
    assert!(!effect.facts.iter().any(|fact| matches!(
        fact.value,
        crate::sandbox_perk::action::FactValue::Labels(_)
    )));
}

#[test]
fn explicit_end_conditions_override_trigger_defaults_and_rebuild_event_masks() {
    for trigger in [Trigger::Drawn, Trigger::Equipped, Trigger::WeaponKill] {
        let mut ending = NativeNode::condition(1).unwrap();
        ending.bytes[8..12].copy_from_slice(&2.75f32.to_le_bytes());
        let program = Program {
            trigger,
            native_removal: Some(ending),
            actions: vec![Action::add_rounds(1)],
            ..Program::default()
        };
        program.validate_structure().unwrap();
        let mut out = Payload::new();
        let (at, kind) = out.removal(&program).unwrap().unwrap();
        assert_eq!(kind, 1);
        assert_eq!(&out.bytes[at + 8..at + 12], &2.75f32.to_le_bytes());
        out.removal_and_rearm(&program).unwrap();
        assert_eq!(
            u64::from_le_bytes(out.bytes[0x90..0x98].try_into().unwrap()),
            1 << 1
        );
    }
}

/// A requirement the workbench adds starts with no event mask. Every stock requirement
/// row stores the mask of its own conditions, so a kill requirement carries the kill bit.
#[test]
fn requirement_rows_carry_the_event_mask_of_their_own_conditions() {
    use crate::sandbox_perk::action;
    let class = |kind| nodes::condition(kind).unwrap().class;
    let template = NativeNode::condition(31).unwrap();
    let mut graph = Graph::read(&template.bytes, 0, class(31)).unwrap();
    // The template requires two General Predicates. The first requirement becomes a kill.
    let rows = graph.blocks[0].links[&0x10];
    let first = graph.blocks[rows].links[&0x10];
    let kill = NativeNode::condition(2).unwrap();
    let kill = graph
        .append(&Graph::read(&kill.bytes, 0, class(2)).unwrap())
        .unwrap();
    graph.blocks[first].links.insert(0, kill);
    for row in 0..2 {
        let at = row * 0x20 + action::SUBGROUP_EVENT_MASK;
        graph.blocks[rows].bytes[at..at + 8].fill(0);
    }
    let program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(NativeNode {
            kind: 31,
            bytes: graph.emit().unwrap(),
        }),
        actions: vec![Action::add_rounds(1)],
        ..Default::default()
    };
    let compiled = assemble(&program, None).unwrap();
    let decoded = action::decode(&compiled.payload).unwrap();
    let masks = decoded.groups[0].activation[0]
        .subgroups
        .iter()
        .map(|subgroup| {
            let at = subgroup.offset + action::SUBGROUP_EVENT_MASK;
            u64::from_le_bytes(compiled.payload[at..at + 8].try_into().unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(masks, [1 << 2, 1 << 20]);
}

#[test]
fn native_nodes_are_written_verbatim_under_a_compiler_owned_header() {
    let mut trigger = NativeNode::condition(6).unwrap();
    trigger.bytes[8] = 0x03;
    trigger.bytes[9] = 0x04;
    let mut ending = NativeNode::condition(29).unwrap();
    ending.bytes[8..12].copy_from_slice(&0xA628_8DD1_u32.to_le_bytes());
    let mut event = NativeNode::effect(43).unwrap();
    event.bytes[4..8].copy_from_slice(&0x5EE2_66FC_u32.to_le_bytes());
    let program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(trigger),
        native_removal: Some(ending),
        duration_ms: 0,
        cooldown_ms: 2_000,
        actions: vec![Action::Native { node: event }, Action::native(30).unwrap()],
        ..Program::default()
    };
    program.validate().unwrap();
    let mut out = Payload::new();
    let activation = out
        .native_condition(program.native_trigger.as_ref().unwrap(), 0)
        .unwrap();
    assert_eq!(
        &out.bytes[activation..activation + 12],
        &[0, 0, 0x80, 0x3F, 0xFF, 6, 0, 0, 3, 4, 0, 0]
    );
    out.nodes(0x20, CONDITION_ROWS, &[activation]);
    let effects = program
        .actions
        .iter()
        .rev()
        .map(|action| out.action(action, None).unwrap())
        .collect::<Vec<_>>();
    let publish = effects[1];
    assert_eq!(
        &out.bytes[publish..publish + 8],
        &[43, 0, 0, 0, 0xFC, 0x66, 0xE2, 0x5E]
    );
    let count = effects[0];
    assert_eq!(&out.bytes[count..count + 3], &[30, 1, 1]);
    out.nodes(0x38, EFFECT_ROWS, &effects);
    assert!(out.removal_and_rearm(&program).unwrap());
    assert_eq!(
        u64::from_le_bytes(out.bytes[0x88..0x90].try_into().unwrap()),
        1 << 6
    );
    assert_eq!(
        u64::from_le_bytes(out.bytes[0x90..0x98].try_into().unwrap()),
        1 << 29
    );
    // One retained action, no duration timer, one cooldown timer.
    assert_eq!(&out.bytes[0xCC..0xCE], &[2, 1]);
    let json = serde_json::to_string(&program).unwrap();
    assert!(json.contains("\"trigger\":\"native\""), "{json}");
    assert!(json.contains("\"bytes\":\"0x2B000000FC66E25E\""), "{json}");
    assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
}

/// The retained byte belongs to the effect kind, not to the authored action: every stock
/// node of a kind agrees on it. The compiler writes it from `Action::retained`, so each
/// typed action has to agree with the captured stock template of the kind it compiles to.
/// A disagreement makes the compiled node unlike every stock one, and makes the decompile
/// that reproduces the kind refuse every stock node of it, which is how `TransmatContext`
/// became unreachable while still compiling.
#[test]
fn every_typed_action_writes_the_stock_retained_byte() {
    let asset = || Asset {
        graph: 0x80BC_2F21,
        path: String::new(),
        values: Vec::new(),
        damage_type: None,
        rows: Vec::new(),
        hud_status: None,
        script: None,
    };
    let kill = Activation::Kill {
        trigger: Trigger::WeaponKill,
        labels: &[],
        mask: [0; 40],
        chance: 10_000,
    };
    let typed = [
        Action::Spawn {
            asset: asset(),
            position: Position::default(),
        },
        Action::attach(asset()),
        Action::Pattern { asset: asset() },
        Action::ExtendTimers {
            extend_ms: 5_000,
            cap_ms: 5_000,
        },
        Action::property(0x5EE2_66FC),
        Action::adjust_component(AbilityTarget::Grenade),
        Action::update_accumulator(1.0),
        Action::ability_property(AbilityTarget::Grenade),
        Action::transmat_context(0x1234_5678),
        Action::override_host_key(0x1234_5678),
        Action::set_damage_type(DamageMode::Solar),
        Action::weapon_reference_count(0),
        Action::add_rounds(1),
        Action::add_fraction(0.5),
    ];
    for action in typed {
        let mut out = Payload::new();
        let at = out.action(&action, Some(&kill)).unwrap();
        let kind = out.bytes[at];
        let stock = crate::sandbox_perk::action::native::template(NativeNodeKind::Effect(kind))
            .and_then(|template| template.get(1).copied())
            .unwrap_or_else(|| panic!("{} has no stock template", action.label()));
        assert_eq!(
            out.bytes[at + 1],
            stock,
            "{} (effect kind {kind}) writes a retained byte no stock node of the kind carries",
            action.label()
        );
    }
}

#[test]
fn extend_timers_needs_a_kill_trigger_to_nest() {
    let mut out = Payload::new();
    let error = out
        .action(
            &Action::ExtendTimers {
                extend_ms: 5_000,
                cap_ms: 5_000,
            },
            None,
        )
        .unwrap_err();
    assert!(error.contains("kill trigger"));
}

#[test]
fn recovered_kill_triggers_preserve_filters_and_chance_in_timer_extensions() {
    use crate::sandbox_perk::action::{self, native::Graph};
    let mut source = Payload::new();
    let at = source.kill_condition(Trigger::PrecisionKill, &[0x962E_A19B], [0; 40], 3750, 0);
    // Retain an additional opaque native requirement and an untouched float lane.
    source.bytes[at + 0x140] = 1;
    source.u32(at + 0x154, (-0.0f32).to_bits());
    let bytes = Graph::read(&source.bytes, at, 0x80803DE7)
        .unwrap()
        .emit()
        .unwrap();
    let program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(NativeNode {
            kind: 2,
            bytes: bytes.clone(),
        }),
        actions: vec![
            Action::ExtendTimers {
                extend_ms: 3000,
                cap_ms: 7000,
            },
            Action::Spawn {
                asset: Asset {
                    graph: 1,
                    ..Default::default()
                },
                position: Position::Event,
            },
        ],
        ..Default::default()
    };
    let compiled = assemble(&program, None).unwrap();
    let decoded = action::decode(&compiled.payload).unwrap();
    let kills = decoded
        .conditions()
        .into_iter()
        .filter(|condition| condition.kind == 2)
        .collect::<Vec<_>>();
    assert_eq!(kills.len(), 2);
    for condition in kills {
        let mut actual = condition.native.clone();
        actual[7] = bytes[7]; // Only the compiled ordinal belongs to the receiving program.
        assert_eq!(actual, bytes);
    }
    let spawn = decoded.effects().find(|effect| effect.kind == 3).unwrap();
    assert_eq!(spawn.native[4], 1);
    let mut no_kill = program;
    no_kill.native_trigger = NativeNode::condition(6);
    assert!(no_kill.validate().is_err());
}

#[test]
fn attach_technical_fields_are_written_verbatim() {
    let node = attach_node(&Action::Attach {
        asset: Asset {
            graph: 0x80BC_5810,
            path: String::new(),
            values: Vec::new(),
            damage_type: None,
            rows: Vec::new(),
            hud_status: None,
            script: None,
        },
        mode: AttachmentTarget::OtherCombatant,
        keys: [0x4113_6E32, 0x95E7_400C],
        float_bits: [1.0_f32.to_bits(); 4],
    });
    assert_eq!(node[2], 3);
    assert_eq!(
        u32::from_le_bytes(node[0x18..0x1C].try_into().unwrap()),
        0x4113_6E32
    );
    assert_eq!(
        u32::from_le_bytes(node[0x1C..0x20].try_into().unwrap()),
        0x95E7_400C
    );
    for offset in [0x20, 0x24, 0x28, 0x2C] {
        assert_eq!(
            f32::from_le_bytes(node[offset..offset + 4].try_into().unwrap()),
            1.0
        );
    }
    assert_eq!(
        u32::from_le_bytes(node[0x30..0x34].try_into().unwrap()),
        EMPTY_KEY
    );
}
