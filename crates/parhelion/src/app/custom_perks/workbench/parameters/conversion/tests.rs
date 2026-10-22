use super::*;
use sundial::package_authoring::sandbox_perk::{
    action::{CONDITION_ROW_CLASS, DecodedCondition, EFFECT_ROW_CLASS, FactValue, REQUIRED_LABELS},
    nodes,
    program::{Action, Asset, NativeProgram, Trigger},
};

/// The entity the editor fixture's component graph belongs to.
const ENTITY: u32 = 0x8152_82E1;

#[test]
fn conversion_carries_component_edits_and_refuses_unrepresentable_references() {
    let mut loaded = super::super::tests::fixture();
    let tag = loaded.graphs[0].0;
    loaded.graphs[0].1.scope_fields();
    let field = loaded.graphs[0].1.fields().next().unwrap();
    let edit = WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: field.value.clone(),
    };
    let mut program = Program {
        actions: vec![Action::Pattern {
            asset: Asset {
                graph: tag,
                ..Asset::default()
            },
        }],
        ..Program::default()
    };
    transfer_values(&loaded, std::slice::from_ref(&edit), &mut program).unwrap();
    assert_eq!(
        program.actions[0].asset().unwrap().values,
        vec![edit.clone()]
    );
    program.actions = vec![Action::add_rounds(1)];
    assert!(
        transfer_values(&loaded, &[edit], &mut program)
            .unwrap_err()
            .contains("cannot carry")
    );
}

#[test]
fn conversion_uses_edited_action_values_and_rejects_stale_values() {
    let loaded = super::super::tests::fixture();
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.action_draft = actions::discover(&loaded.action_payload);
    editor.action_draft[0].value_bits = 2.0_f32.to_bits();
    let changed = effective_action(
        loaded.action_tag,
        &loaded.action_payload,
        &editor.conversion_input(),
        &[],
    )
    .unwrap();
    assert_eq!(
        actions::source_bits(&changed, &editor.action_draft[0]).unwrap(),
        2.0_f32.to_bits()
    );
    editor.action_draft[0].expected_bits = 1.0_f32.to_bits();
    assert!(
        effective_action(
            loaded.action_tag,
            &loaded.action_payload,
            &editor.conversion_input(),
            &[]
        )
        .is_err()
    );
}

/// An action assembled from node templates alone: an unconditional activation with the
/// given probability, one spawn effect that references the fixture entity, and two ending
/// conditions, an event key and a timer. Nothing in it comes from game packages.
fn synthetic_action(probability: f32) -> Vec<u8> {
    let mut graph = NativeProgram::empty().graph;
    let mut list = |field: usize, class: u32, kinds: &[(bool, u8)]| -> Vec<usize> {
        graph.create_target(0, field, class, true).unwrap();
        let rows = graph.blocks[0].links[&field];
        graph.resize_array(rows, kinds.len()).unwrap();
        let stride = graph.blocks[rows].bytes.len() / kinds.len();
        kinds
            .iter()
            .enumerate()
            .map(|(row, (condition, kind))| {
                let class = if *condition {
                    nodes::condition(*kind)
                } else {
                    nodes::effect(*kind)
                }
                .unwrap()
                .class;
                graph
                    .create_target(rows, row * stride, class, false)
                    .unwrap();
                graph.blocks[rows].links[&(row * stride)]
            })
            .collect()
    };
    // Each list's pointer follows its count, eight bytes into the group's descriptor.
    let activation = list(0x28, CONDITION_ROW_CLASS, &[(true, 0)])[0];
    let spawn = list(0x40, EFFECT_ROW_CLASS, &[(false, 3)])[0];
    list(0x50, CONDITION_ROW_CLASS, &[(true, 30), (true, 1)]);
    graph.blocks[activation].bytes[..4].copy_from_slice(&probability.to_le_bytes());
    graph.blocks[spawn].bytes[16..20].copy_from_slice(&ENTITY.to_le_bytes());
    graph.emit().unwrap()
}

/// The fixture editor state over a synthetic action, with one component edit in the draft.
fn synthetic_editor(payload: &[u8]) -> (PrivatePerkRuntimeGraph, WeaponRuntimeValueOverride) {
    let mut loaded = super::super::tests::fixture();
    loaded.action_payload = payload.to_vec();
    loaded.graphs[0].1.scope_fields();
    let field = loaded.graphs[0].1.fields().next().unwrap();
    let edit = WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: WeaponRuntimeValue::Bytes([4.25f32.to_le_bytes(), [0; 4]].concat()),
    };
    (loaded, edit)
}

#[test]
fn native_fallback_keeps_every_ending_and_component_edit_when_the_typed_model_refuses() {
    // An activation probability above one lies outside the typed model, so the typed route
    // refuses the action and the native form has to carry it.
    let payload = synthetic_action(2.0);
    let (loaded, edit) = synthetic_editor(&payload);
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.draft.push(edit.clone());
    let input = editor.conversion_input();
    let refusal = typed_program(&loaded, &input, &payload).unwrap_err();
    assert!(refusal.contains("probability"), "{refusal}");
    let program = native_program(&loaded, &input, &payload, [ENTITY]).unwrap();
    let emitted = program.native.as_ref().unwrap().graph.emit().unwrap();
    let decoded = action::decode(&emitted).unwrap();
    let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
    assert_eq!(kinds(&decoded.groups[0].removal), vec![30, 1]);
    assert_eq!(kinds(&decoded.groups[0].activation), vec![0]);
    assert_eq!(decompile::native_fidelity(&payload, &emitted), Ok(vec![]));
    let carried = program
        .assets()
        .flat_map(|asset| &asset.values)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].value, edit.value);
    assert_eq!(carried[0].locator.graph_tag, Some(ENTITY.into()));
    let preview = select(Err(refusal.clone()), || {
        Ok(Candidate {
            fidelity: decompile::native_fidelity(&payload, &emitted),
            program: program.clone(),
        })
    })
    .unwrap();
    assert_eq!(preview.recovery, decompile::Recovery::NativeForm(refusal));
    assert_eq!(preview.fidelity, Ok(vec![]));
    assert_eq!(preview.program, program);
}

/// The kill category a decoded condition filters on.
fn kill_filter(condition: &DecodedCondition) -> Option<PerkActivation> {
    let labels = condition
        .facts
        .iter()
        .find_map(|fact| match &fact.value {
            FactValue::Labels(labels) if fact.label == REQUIRED_LABELS => Some(labels.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let weapon = condition
        .facts
        .iter()
        .any(|fact| fact.label == "Requires Owning Weapon" && fact.value == FactValue::Flag(true));
    PerkActivation::from_filter(&labels, weapon)
}

/// How many conditions end the recovered program and which kill category starts it, read
/// the same way whether the program is typed or carried in native form.
fn endings_and_activation(program: &Program) -> (usize, Option<PerkActivation>) {
    if let Some(native) = &program.native {
        let decoded = action::decode(&native.graph.emit().unwrap()).unwrap();
        let group = &decoded.groups[0];
        return (
            group.removal.len(),
            group.activation.first().and_then(kill_filter),
        );
    }
    let primary = program.native_removal.is_some()
        || program.removal_key.is_some()
        || program.duration_ms > 0
        || matches!(program.trigger, Trigger::Equipped | Trigger::Drawn);
    let activation = match program.trigger {
        Trigger::WeaponKill => Some(PerkActivation::WeaponKill),
        Trigger::PrecisionKill => Some(PerkActivation::PrecisionWeaponKill),
        Trigger::MeleeKill => Some(PerkActivation::MeleeKill),
        Trigger::GrenadeKill => Some(PerkActivation::GrenadeKill),
        Trigger::AnyKill => Some(PerkActivation::AnyKill),
        Trigger::Native => program
            .native_trigger
            .as_ref()
            .and_then(|node| action::decode_condition_node(&node.bytes).ok())
            .and_then(|condition| kill_filter(&condition)),
        Trigger::Always | Trigger::Equipped | Trigger::Drawn => None,
    };
    (
        usize::from(primary) + program.alternative_removals.len(),
        activation,
    )
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn conversion_preserves_all_conditions_activation_and_component_values() {
    let path = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let key = PerkEditorKey {
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: 1,
        source_perk_index: 421,
    };
    let loaded = load_private_perk_runtime_graph(&path, key, &[]).unwrap();
    // Outlaw ends on either of two conditions. Whichever representation the round trip
    // settles on, both endings and the chosen kill category must survive exactly.
    for activation in PerkActivation::ALL {
        let mut editor = super::super::tests::editor(loaded.clone());
        editor.key = key;
        editor.activation = Some(activation);
        let input = editor.conversion_input();
        let preview = prepare(&manager, &loaded, &input).unwrap();
        assert_eq!(preview.fidelity, Ok(vec![]), "{activation:?}");
        assert_eq!(
            endings_and_activation(&preview.program),
            (2, Some(activation)),
            "{activation:?} as {:?}",
            preview.recovery
        );
    }
    let key = PerkEditorKey {
        source_perk_index: 1178,
        ..key
    };
    let loaded = load_private_perk_runtime_graph(&path, key, &[]).unwrap();
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.key = key;
    let (_, parameter) = movement::mapped(&loaded).into_iter().next().unwrap();
    parameter.set(&mut editor.draft, 4.25).unwrap();
    let input = editor.conversion_input();
    let preview = prepare(&manager, &loaded, &input).unwrap();
    let values = preview
        .program
        .assets()
        .flat_map(|asset| &asset.values)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(parameter.value(&values).unwrap(), 4.25);
    assert_eq!(preview.fidelity, Ok(vec![]));
}
