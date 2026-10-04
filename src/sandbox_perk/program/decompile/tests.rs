//! Tests for recovering stock actions as programs and for the exact conversion check.
use super::*;
use crate::sandbox_perk::program::AttachmentTarget;

mod coverage;
use crate::sandbox_perk::action::fixtures::{Builder, drawn_pattern_action, precision_kill_action};
use crate::sandbox_perk::action::{
    CONDITION_ROW_CLASS, EFFECT_ROW_CLASS, GROUP_ACTIVATION, GROUP_EFFECTS, GROUP_REARM,
    GROUP_REMOVAL, PRIMARY_GROUP,
};

fn kill_attach_action(chance: f32) -> Vec<u8> {
    let mut out = Builder::new();
    let activation = out.kill(&[PRECISION], true, chance);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let attach = out.attach(
        0x80BC_5810,
        "content/sandbox/effects/trail/trail.entity.tft",
    );
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
    let duration = out.timer(5.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let cooldown = out.timer(2.5);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REARM,
        CONDITION_ROW_CLASS,
        &[cooldown],
    );
    out.finish()
}

#[test]
fn auxiliary_records_are_carried_verbatim_through_the_round_trip() {
    use crate::sandbox_perk::action::{AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS};
    // The three leaf classes stock actions list at the root, with representative bytes.
    let key_record = {
        let mut bytes = [0_u8; 24];
        bytes[..4].copy_from_slice(&2_u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0xA43A_8C2E_u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&1.5_f32.to_le_bytes());
        bytes
    };
    // Offset 0x08 is a declared reference and stays null in every stock record. The
    // resource tag sits at 0x10.
    let mut tag_record = [0_u8; 24];
    tag_record[..4].copy_from_slice(&3_u32.to_le_bytes());
    tag_record[16..20].copy_from_slice(&0x80BC_5810_u32.to_le_bytes());
    let mut small_record = [0_u8; 8];
    small_record[..4].copy_from_slice(&0x10FF_0000_u32.to_le_bytes());
    small_record[4..].copy_from_slice(&0x599B_031D_u32.to_le_bytes());

    let mut out = Builder {
        bytes: drawn_pattern_action(),
    };
    let mut nodes = Vec::new();
    for (class, bytes) in [
        (0x8080_4085, &key_record[..]),
        (0x8080_4087, &tag_record[..]),
        (0x8080_2A20, &small_record[..]),
    ] {
        let at = out.node(class, bytes.len());
        out.bytes[at..at + bytes.len()].copy_from_slice(bytes);
        nodes.push(at);
    }
    out.pointer_list(AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS, &nodes);
    let payload = out.finish();

    let action = decode(&payload).unwrap();
    assert_eq!(
        action
            .auxiliary
            .iter()
            .map(|record| (record.class, record.bytes.as_slice()))
            .collect::<Vec<_>>(),
        vec![
            (0x8080_4085, &key_record[..]),
            (0x8080_4087, &tag_record[..]),
            (0x8080_2A20, &small_record[..]),
        ]
    );
    let (program, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
    assert_eq!(recovery, Recovery::Typed);
    assert_eq!(program.auxiliary.len(), 3);
    assert_eq!(program.auxiliary[0].bytes, key_record.to_vec());

    let compiled = super::super::compiler::assemble(&program, None).unwrap();
    assert_eq!(
        decode(&compiled.payload).unwrap().auxiliary,
        action.auxiliary
    );
    // The fixture is not byte-faithful to compiler routing metadata. The records are.
    let differences = fidelity(&payload, &compiled.payload).unwrap();
    assert!(
        differences
            .iter()
            .all(|difference| !difference.node.starts_with("Auxiliary")),
        "{differences:?}"
    );

    // Dropping a record is a reported difference, not a refusal.
    let mut trimmed = program.clone();
    trimmed.auxiliary.pop();
    let compiled = super::super::compiler::assemble(&trimmed, None).unwrap();
    let differences = fidelity(&payload, &compiled.payload).unwrap();
    assert!(
        differences
            .iter()
            .any(|difference| difference.node == "Auxiliary Record List Length"),
        "{differences:?}"
    );

    // A record of the wrong size for its class is refused before it reaches the compiler.
    let mut wrong = program;
    wrong.auxiliary[2].bytes.push(0);
    assert!(wrong.validate().unwrap_err().contains("8"));
}

#[test]
fn alternative_conditions_are_carried_verbatim_beside_the_typed_reading() {
    let mut out = Builder::new();
    let draw = out.draw();
    let also_starts = out.event_key(0x1234_5678);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[draw, also_starts],
    );
    let pattern = out.pattern(0x8161_F73A, "content/sandbox/weapons/demo/demo.pattern.tft");
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[pattern]);
    let holster = out.holster();
    let also_ends = out.timer(4.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[holster, also_ends],
    );
    let payload = out.finish();

    let action = decode(&payload).unwrap();
    let program = decompile(&action, "Demo", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Drawn);
    assert_eq!(program.alternative_triggers.len(), 1);
    assert_eq!(program.alternative_triggers[0].kind, 30);
    assert_eq!(program.alternative_removals.len(), 1);
    assert_eq!(program.alternative_removals[0].kind, 1);
    assert_eq!(
        program.alternative_removals[0].bytes,
        action.groups[0].removal[1].native
    );

    let compiled = super::super::compiler::assemble(&program, None).unwrap();
    let again = decode(&compiled.payload).unwrap();
    let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
    assert_eq!(kinds(&again.groups[0].activation), vec![16, 30]);
    assert_eq!(kinds(&again.groups[0].removal), vec![17, 1]);
    // The alternative timer takes a timer slot, and both lists route their events.
    assert_eq!(again.timer_budget, 1);
    assert_eq!(again.activation_event_mask, (1 << 16) | (1 << 30));
    assert_eq!(again.removal_event_mask, (1 << 17) | (1 << 1));
    let differences = fidelity(&payload, &compiled.payload).unwrap();
    assert!(
        differences
            .iter()
            .all(|difference| !difference.node.contains("list length")),
        "{differences:?}"
    );

    // Dropping an alternative is reported, not refused.
    let mut trimmed = program.clone();
    trimmed.alternative_removals.clear();
    let compiled = super::super::compiler::assemble(&trimmed, None).unwrap();
    assert!(
        fidelity(&payload, &compiled.payload)
            .unwrap()
            .iter()
            .any(|difference| difference.node == "removal list length")
    );

    // An alternative of a kind without a recovered layout is refused before it reaches the
    // compiler.
    let mut wrong = program;
    wrong.alternative_removals[0].kind = 200;
    assert!(
        wrong
            .validate()
            .unwrap_err()
            .contains("no recovered native layout")
    );
}

#[test]
fn unnamed_label_filters_are_carried_in_native_nodes() {
    let mut out = Builder::new();
    let activation = out.kill(&[0x599B_031D], false, 0.5);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let rounds = out.fixed_ammunition(&[0x599B_031D], 1, 0);
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[rounds]);
    let duration = out.timer(5.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let payload = out.finish();

    let action = decode(&payload).unwrap();
    let program = decompile(&action, "Demo", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Native);
    let trigger = program.native_trigger.as_ref().unwrap();
    assert_eq!(trigger.kind, 2);
    assert_eq!(trigger.bytes, action.groups[0].activation[0].native);
    assert!(program.has_kill_trigger());
    assert_eq!(program.duration_ms, 5_000);
    let [Action::Native { node }] = program.actions.as_slice() else {
        panic!("{:?}", program.actions);
    };
    assert_eq!(node.kind, action.groups[0].effects[0].kind);
    assert_eq!(node.bytes, action.groups[0].effects[0].native);

    let compiled = super::super::compiler::assemble(&program, None).unwrap();
    let again = decode(&compiled.payload).unwrap();
    assert_eq!(
        again.groups[0].activation[0].native,
        action.groups[0].activation[0].native
    );
    assert_eq!(
        again.groups[0].effects[0].native,
        action.groups[0].effects[0].native
    );
}

#[test]
fn execution_policies_are_carried_verbatim_through_the_round_trip() {
    use crate::sandbox_perk::action::{POLICY_CONFIGURATION, POLICY_SELECTOR, ROOT_KEY};
    let mut out = Builder {
        bytes: drawn_pattern_action(),
    };
    out.bytes[POLICY_SELECTOR] = 1;
    out.bytes[POLICY_SELECTOR + 1] = 1;
    out.u32(ROOT_KEY, 0xC767_798C);
    // Selector 1 uses an 8-byte record: a key and a flag word.
    let configuration = out.node(0x8080_3E07, 8);
    out.u32(configuration, 0xDB33_855E);
    out.u32(configuration + 4, 1);
    out.pointer(POLICY_CONFIGURATION, configuration);
    let payload = out.finish();

    let action = decode(&payload).unwrap();
    let record = action.policy_configuration.as_ref().unwrap();
    assert_eq!(record.class, 0x8080_3E07);
    assert_eq!(record.bytes.len(), 8);
    let (program, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
    assert_eq!(recovery, Recovery::Typed);
    let policy = program.policy.as_ref().unwrap();
    assert_eq!(
        (policy.selector, policy.modifier, policy.key),
        (1, 1, 0xC767_798C)
    );
    assert_eq!(policy.configuration.as_ref().unwrap().bytes, record.bytes);

    let compiled = super::super::compiler::assemble(&program, None).unwrap();
    let again = decode(&compiled.payload).unwrap();
    assert_eq!(again.policy, 1);
    assert_eq!(again.policy_modifier, 1);
    assert_eq!(again.root_key, 0xC767_798C);
    assert_eq!(again.policy_configuration, action.policy_configuration);
    let differences = fidelity(&payload, &compiled.payload).unwrap();
    assert!(
        differences
            .iter()
            .all(|difference| !difference.node.starts_with("Policy")
                && difference.node != "Root Key"
                && !(difference.node == "Action Routing and State"
                    && (0x30..0x32).contains(&difference.offset))),
        "{differences:?}"
    );

    // Dropping the policy is reported, not refused.
    let mut plain = program.clone();
    plain.policy = None;
    let compiled = super::super::compiler::assemble(&plain, None).unwrap();
    let differences = fidelity(&payload, &compiled.payload).unwrap();
    assert!(
        differences
            .iter()
            .any(|difference| difference.node == "Policy Configuration Presence"),
        "{differences:?}"
    );

    // A default-policy action carries no policy, so the recipe stays minimal.
    let plain = decompile(&decode(&drawn_pattern_action()).unwrap(), "Demo", |tag| tag).unwrap();
    assert!(plain.policy.is_none());
}

#[test]
fn native_endings_and_rearms_are_carried_beside_kill_triggers() {
    let mut out = Builder::new();
    let activation = out.kill(&[PRECISION], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let attach = out.attach(
        0x80BC_5810,
        "content/sandbox/effects/trail/trail.entity.tft",
    );
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
    // The stock shape behind 51 perks: a kill perk that ends on an unconditional check.
    let ends = out.unconditional();
    out.pointer_list(PRIMARY_GROUP + GROUP_REMOVAL, CONDITION_ROW_CLASS, &[ends]);
    // A rearm that is not a timer, with a second alternative.
    let rearm = out.draw();
    let also = out.event_key(0x1234_5678);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REARM,
        CONDITION_ROW_CLASS,
        &[rearm, also],
    );
    let payload = out.finish();

    let action = decode(&payload).unwrap();
    let program = decompile(&action, "Demo", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::PrecisionKill);
    assert_eq!(program.duration_ms, 0);
    assert_eq!(
        program.native_removal.as_ref().map(|node| node.kind),
        Some(0)
    );
    assert_eq!(program.cooldown_ms, 0);
    assert_eq!(
        program.native_rearm.as_ref().map(|node| node.kind),
        Some(16)
    );
    assert_eq!(program.alternative_rearms.len(), 1);

    let mask = Some((&[PRECISION][..], [0_u8; 40]));
    let compiled = super::super::compiler::assemble(&program, mask).unwrap();
    let again = decode(&compiled.payload).unwrap();
    let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
    assert_eq!(kinds(&again.groups[0].removal), vec![0]);
    assert_eq!(kinds(&again.groups[0].rearm), vec![16, 30]);
    assert_eq!(again.rearm_event_mask, (1 << 16) | (1 << 30));

    // A cooldown and a native rearm cannot both apply.
    let mut both = program;
    both.cooldown_ms = 1_000;
    assert!(both.validate().unwrap_err().contains("not both"));
}

#[test]
fn recovery_prefers_the_typed_program_and_never_refuses() {
    let payload = drawn_pattern_action();
    let (typed, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
    assert_eq!(recovery, Recovery::Typed);
    assert!(typed.native.is_none());
    assert_eq!(typed.trigger, Trigger::Drawn);
    assert_eq!(typed.actions.len(), 2);

    // An activation probability outside 0 to 1 is outside the typed model. The same action
    // still opens, carried in native form, and the reason names the shape that was refused.
    let mut out_of_range = payload;
    let at = decode(&out_of_range).unwrap().groups[0].activation[0].offset;
    out_of_range[at..at + 4].copy_from_slice(&2.0_f32.to_le_bytes());
    let (native, recovery) = recover(&out_of_range, "Demo", |tag| tag).unwrap();
    assert!(native.native.is_some());
    assert!(native.actions.is_empty());
    match recovery {
        Recovery::NativeForm(reason) => {
            assert!(reason.contains("probability"), "{reason}")
        }
        Recovery::Typed => panic!("an out-of-range probability cannot be typed"),
    }
}

#[test]
fn a_drawn_pattern_action_recovers_its_program_in_authored_order() {
    let action = decode(&drawn_pattern_action()).unwrap();
    let program = decompile(&action, "Demo", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Drawn);
    assert_eq!(program.duration_ms, 0);
    assert_eq!(program.cooldown_ms, 0);
    assert_eq!(program.chance_permyriad, 10_000);
    // The payload stores effects last to first, so the authored order is reversed.
    assert!(matches!(program.actions[0], Action::Spawn { .. }));
    assert!(matches!(program.actions[1], Action::Pattern { .. }));
    assert_eq!(program.actions[0].asset().unwrap().graph, 0x80BC_2F21);
    assert_eq!(
        program.actions[1].asset().unwrap().path,
        "content/sandbox/weapons/demo/demo.pattern.tft"
    );
}

#[test]
fn an_extend_timers_effect_recovers_when_it_nests_the_trigger() {
    let mut out = Builder::new();
    let activation = out.kill(&[PRECISION], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let nested = out.kill(&[PRECISION], true, 1.0);
    let extend = out.extend_timers(5.0, 10.0, &[nested]);
    let attach = out.attach(0x80BC_5810, "content/sandbox/effects/buff/buff.entity.tft");
    // Stored last to first: the attach runs first, then the extension.
    out.pointer_list(
        PRIMARY_GROUP + GROUP_EFFECTS,
        EFFECT_ROW_CLASS,
        &[extend, attach],
    );
    let duration = out.timer(5.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let action = decode(&out.finish()).unwrap();
    let program = decompile(&action, "Refresh", |tag| tag).unwrap();
    assert!(matches!(program.actions[0], Action::Attach { .. }));
    assert_eq!(
        program.actions[1],
        Action::ExtendTimers {
            extend_ms: 5_000,
            cap_ms: 10_000
        }
    );
    // A nested condition that is not the trigger lies outside the typed action and is
    // carried verbatim, nested condition included.
    let mut out = Builder::new();
    let activation = out.kill(&[PRECISION], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let nested = out.kill(&[GRENADE], false, 1.0);
    let extend = out.extend_timers(5.0, 5.0, &[nested]);
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[extend]);
    let duration = out.timer(5.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let decoded = decode(&out.finish()).unwrap();
    let program = decompile(&decoded, "Refresh", |tag| tag).unwrap();
    let [Action::Native { node }] = program.actions.as_slice() else {
        panic!("{:?}", program.actions);
    };
    assert_eq!(node.kind, 32);
    assert_eq!(node.bytes, decoded.groups[0].effects[0].native);
}

/// Whether a fidelity difference is about list or program structure rather than bytes.
fn structural(difference: &Difference) -> bool {
    difference.node == "Program Count"
        || difference.node.contains("list length")
        || difference.node == "Effect List Length"
}

/// The drawn pattern action plus a second program: always active, spawning one entity,
/// ending on an event key.
fn two_program_action() -> Vec<u8> {
    use crate::sandbox_perk::action::{ADDITIONAL_GROUPS, GROUP_ROW_CLASS, GROUP_SIZE};
    let mut out = Builder {
        bytes: drawn_pattern_action(),
    };
    let rows = out.rows(ADDITIONAL_GROUPS, GROUP_ROW_CLASS, 1, GROUP_SIZE);
    let activation = out.unconditional();
    out.pointer_list(rows + GROUP_ACTIVATION, CONDITION_ROW_CLASS, &[activation]);
    let spawn = out.spawn(0x80BC_2F21, "content/sandbox/effects/demo/demo.entity.tft");
    out.pointer_list(rows + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[spawn]);
    let ends = out.event_key(0x1234_5678);
    out.pointer_list(rows + GROUP_REMOVAL, CONDITION_ROW_CLASS, &[ends]);
    out.finish()
}

#[test]
fn further_programs_are_recovered_beside_the_typed_one() {
    let payload = two_program_action();
    assert_eq!(decode(&payload).unwrap().groups.len(), 2);
    let (program, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
    assert_eq!(recovery, Recovery::Typed);
    assert_eq!(program.trigger, Trigger::Drawn);
    assert_eq!(program.actions.len(), 2);
    assert_eq!(program.additional_groups.len(), 1);
    let group = &program.additional_groups[0];
    assert_eq!(group.activation[0].kind, 0);
    assert_eq!(group.effects[0].kind, 3);
    assert_eq!(group.removal[0].kind, 30);
    assert!(group.rearm.is_empty());
}

#[test]
fn further_programs_compile_verbatim_with_their_routing_records() {
    let payload = two_program_action();
    let action = decode(&payload).unwrap();
    let program = decompile(&action, "Demo", |tag| tag).unwrap();
    let compiled = super::super::compiler::assemble(&program, None).unwrap();
    let again = decode(&compiled.payload).unwrap();
    assert_eq!(again.groups.len(), 2);
    let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
    assert_eq!(kinds(&again.groups[1].activation), vec![0]);
    assert_eq!(kinds(&again.groups[1].removal), vec![30]);
    assert_eq!(again.groups[1].effects[0].kind, 3);
    assert_eq!(
        again.groups[1].effects[0].native,
        action.groups[1].effects[0].native
    );
    // The compiled routing records exist, one per further program, for the rebuild.
    let (count, _, _, class) =
        crate::package_payload::native_array_at(&compiled.payload, 0xA8).unwrap();
    assert_eq!((count, class), (1, 0x8080_407B));
    let differences = fidelity(&payload, &compiled.payload).unwrap();
    assert!(!differences.iter().any(structural), "{differences:?}");

    // Dropping the further program is reported, not refused.
    let mut single = program;
    single.additional_groups.clear();
    let compiled = super::super::compiler::assemble(&single, None).unwrap();
    assert!(
        fidelity(&payload, &compiled.payload)
            .unwrap()
            .iter()
            .any(|difference| difference.node == "Program Count")
    );
}

#[test]
fn a_named_property_effect_recovers_its_constant_and_refuses_other_programs() {
    let mut out = Builder::new();
    let activation = out.unconditional();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let property = out.named_property(0x5EE2_66FC, 1.0, 2, 0, 1);
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[property]);
    let payload = out.finish();
    let program = decompile(&decode(&payload).unwrap(), "Flag", |tag| tag).unwrap();
    assert_eq!(
        program.actions[0],
        Action::Property {
            key: 0x5EE2_66FC,
            target: 2,
            operation_byte: 0,
            removal: 1,
            value_bits: 1.0_f32.to_bits(),
            restore_bits: 0,
            ability_mask: 0,
            input: 0,
            flag: 1,
        }
    );
    let json = serde_json::to_string(&program.actions[0]).unwrap();
    assert!(json.contains("\"key\":\"0x5EE266FC\""), "{json}");
    assert!(json.contains("\"value_bits\":1.0"), "{json}");
    assert_eq!(
        serde_json::from_str::<Action>(&json).unwrap(),
        program.actions[0]
    );
    // A nonzero fast-path selector means the program is not the plain constant, so the
    // node is carried verbatim instead of as a typed property.
    let mut other = payload.clone();
    other[property + 0x44] = 1;
    let decoded = decode(&other).unwrap();
    let program = decompile(&decoded, "Flag", |tag| tag).unwrap();
    assert!(
        matches!(&program.actions[0], Action::Native { node } if node.kind == 10),
        "{:?}",
        program.actions
    );
    // Fidelity compares the program's bytes, not only the node.
    let mut changed = payload.clone();
    let constants = crate::package_payload::relative_offset(
        property + 0x30,
        0,
        crate::package_payload::i64_at(&changed, property + 0x30).unwrap(),
    )
    .unwrap()
        + 16;
    changed[constants..constants + 4].copy_from_slice(&2.0_f32.to_le_bytes());
    let differences = fidelity(&payload, &changed).unwrap();
    assert_eq!(differences.len(), 1);
    assert!(differences[0].node.ends_with("value program"));
}

#[test]
fn ammunition_effects_recover_their_one_amount_and_carry_filters_and_spreads_natively() {
    let mut out = Builder::new();
    let activation = out.kill(&[], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    // Triple Tap's shape: one round into this weapon's magazine.
    let rounds = out.fixed_ammunition(&[], 1, 0);
    // A kill-to-reload shape: half the magazine capacity into the magazine.
    let fraction = out.proportional_ammunition(1, 1, 0.5);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_EFFECTS,
        EFFECT_ROW_CLASS,
        &[fraction, rounds],
    );
    let duration = out.timer(1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let payload = out.finish();
    let program = decompile(&decode(&payload).unwrap(), "Ammo", |tag| tag).unwrap();
    assert_eq!(program.actions[0], Action::add_rounds(1));
    assert_eq!(program.actions[1], Action::add_fraction(0.5));
    let json = serde_json::to_string(&program.actions[1]).unwrap();
    assert!(json.contains("\"fraction_bits\":0.5"), "{json}");
    assert!(!json.contains("overflow"), "{json}");
    assert_eq!(
        serde_json::from_str::<Action>(&json).unwrap(),
        program.actions[1]
    );
    // A source label filter lies outside the typed action and is carried verbatim.
    let mut out = Builder::new();
    let activation = out.kill(&[], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let filtered = out.fixed_ammunition(&[PRECISION], 1, 0);
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[filtered]);
    let duration = out.timer(1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let decoded = decode(&out.finish()).unwrap();
    let program = decompile(&decoded, "Ammo", |tag| tag).unwrap();
    let [Action::Native { node }] = program.actions.as_slice() else {
        panic!("{:?}", program.actions);
    };
    assert_eq!(node.bytes, decoded.groups[0].effects[0].native);
    // Two amounts in one node exceed the typed action, which carries one, so the node stays
    // native.
    let mut out = Builder::new();
    let activation = out.kill(&[], true, 1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let spread = out.fixed_ammunition(&[], 2, -1);
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[spread]);
    let duration = out.timer(1.0);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[duration],
    );
    let decoded = decode(&out.finish()).unwrap();
    let program = decompile(&decoded, "Ammo", |tag| tag).unwrap();
    assert!(
        matches!(&program.actions[0], Action::Native { node } if node.bytes == decoded.groups[0].effects[0].native),
        "{:?}",
        program.actions
    );
}

fn always_attach_action(repeat: Option<f32>) -> Vec<u8> {
    let mut out = Builder::new();
    let activation = out.unconditional();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let attach = out.attach(0x80BC_5810, "content/sandbox/effects/aura/aura.entity.tft");
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
    if let Some(seconds) = repeat {
        let timer = out.timer(seconds);
        out.pointer_list(PRIMARY_GROUP + GROUP_REARM, CONDITION_ROW_CLASS, &[timer]);
    }
    out.finish()
}

#[test]
fn an_always_active_action_recovers_with_its_repeat_interval() {
    let action = decode(&always_attach_action(Some(3.0))).unwrap();
    let program = decompile(&action, "Aura", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Always);
    assert_eq!(program.duration_ms, 0);
    assert_eq!(program.cooldown_ms, 3_000);
    assert!(matches!(program.actions[0], Action::Attach { .. }));
    let plain = decompile(
        &decode(&always_attach_action(None)).unwrap(),
        "Aura",
        |tag| tag,
    )
    .unwrap();
    assert_eq!(plain.cooldown_ms, 0);
}

#[test]
fn attach_technical_fields_survive_the_round_trip_and_default_when_absent() {
    let mut out = Builder::new();
    let activation = out.unconditional();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let attach = out.attach_with(
        0x80BC_5810,
        "content/sandbox/effects/aura/aura.entity.tft",
        3,
        [0x4113_6E32, 0x95E7_400C],
        [1.0; 4],
    );
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
    let action = decode(&out.finish()).unwrap();
    let program = decompile(&action, "Aura", |tag| tag).unwrap();
    let Action::Attach {
        mode,
        keys,
        float_bits,
        ..
    } = &program.actions[0]
    else {
        panic!("expected an attach action");
    };
    assert_eq!(*mode, AttachmentTarget::OtherCombatant);
    assert_eq!(*keys, [0x4113_6E32, 0x95E7_400C]);
    assert_eq!(*float_bits, [1.0_f32.to_bits(); 4]);
    let json = serde_json::to_string(&program).unwrap();
    assert!(json.contains("\"mode\":3"), "{json}");
    assert!(json.contains("\"0x41136E32\""), "{json}");
    assert!(json.contains("\"float_bits\":[1.0,1.0,1.0,1.0]"), "{json}");
    assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
    // A recipe written before these fields existed reads back as the compiler's old output.
    let legacy = r#"{"operation":"attach","asset":{"graph":1}}"#;
    let action = serde_json::from_str::<Action>(legacy).unwrap();
    assert_eq!(
        action,
        Action::attach(Asset {
            graph: 1,
            ..Asset::default()
        })
    );
    assert_eq!(
        serde_json::to_value(&action).unwrap(),
        serde_json::from_str::<serde_json::Value>(legacy).unwrap()
    );
}

#[test]
fn an_always_active_action_recovers_its_ending_event_key() {
    let mut out = Builder::new();
    let activation = out.unconditional();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let attach = out.attach(0x80BC_5810, "content/sandbox/effects/aura/aura.entity.tft");
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
    let ending = out.event_key(0xA628_8DD1);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[ending],
    );
    let timer = out.timer(3.0);
    out.pointer_list(PRIMARY_GROUP + GROUP_REARM, CONDITION_ROW_CLASS, &[timer]);
    let program = decompile(&decode(&out.finish()).unwrap(), "Aura", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Always);
    assert_eq!(program.removal_key, Some(0xA628_8DD1));
    assert_eq!(program.cooldown_ms, 3_000);
    let json = serde_json::to_string(&program).unwrap();
    assert!(json.contains("\"removal_key\":\"0xA6288DD1\""), "{json}");
    assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
    let mut plain = program.clone();
    plain.removal_key = None;
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("removal_key")
    );
    plain.trigger = Trigger::Drawn;
    plain.removal_key = Some(1);
    assert!(plain.validate().unwrap_err().contains("always-active"));
}

#[test]
fn an_empty_activation_list_reads_as_always_active() {
    let mut out = Builder::new();
    let attach = out.attach(0x80BC_5810, "content/sandbox/effects/aura/aura.entity.tft");
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
    let action = decode(&out.finish()).unwrap();
    let program = decompile(&action, "Aura", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Always);
}

#[test]
fn a_kill_action_recovers_its_trigger_timing_and_chance() {
    let action = decode(&kill_attach_action(0.25)).unwrap();
    let program = decompile(&action, "Trail", |tag| tag + 1).unwrap();
    assert_eq!(program.trigger, Trigger::PrecisionKill);
    assert_eq!(program.duration_ms, 5_000);
    assert_eq!(program.cooldown_ms, 2_500);
    assert_eq!(program.chance_permyriad, 2_500);
    assert_eq!(program.actions.len(), 1);
    assert_eq!(program.actions[0].asset().unwrap().graph, 0x80BC_5811);
}

#[test]
fn a_weighted_spawn_effect_recovers_its_complete_native_record() {
    // The fixture's nested condition matches its trigger, so the extension is authorable.
    let action = decode(&precision_kill_action()).unwrap();
    let program = decompile(&action, "Outlaw", |tag| tag).unwrap();
    assert!(matches!(program.actions[0], Action::ExtendTimers { .. }));
    // Weighted spawn records now have a complete native authoring path.
    let mut out = Builder::new();
    let activation = out.unconditional();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    // A label-free weighted spawn record is preserved even with empty choices.
    let unknown = out.node(0x8080_3E47, 64);
    out.bytes[unknown] = 13;
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[unknown]);
    let decoded = decode(&out.finish()).unwrap();
    let program = decompile(&decoded, "Weighted Spawn", |tag| tag).unwrap();
    assert!(
        matches!(&program.actions[0],Action::Native{node} if node.kind==13 && node.bytes==decoded.groups[0].effects[0].native)
    );
}

#[test]
fn plain_scalar_nodes_recover_verbatim_as_native_trigger_ending_and_effect() {
    let mut out = Builder::new();
    // A Two Event Flag Masks trigger with masks 3 and 4.
    let activation = out.condition(0x8080_3DFB, 6, 12);
    out.bytes[activation + 8] = 3;
    out.bytes[activation + 9] = 4;
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    // Three Weapon Float Overrides, retained, and a one-shot Publish Named Player Event.
    let overrides = out.node(0x8080_3E0B, 16);
    out.bytes[overrides] = 29;
    out.bytes[overrides + 1] = 1;
    out.f32(overrides + 4, 0.5);
    out.f32(overrides + 12, 2.0);
    let publish = out.node(0x8080_3E1E, 8);
    out.bytes[publish] = 43;
    out.u32(publish + 4, 0x5EE2_66FC);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_EFFECTS,
        EFFECT_ROW_CLASS,
        &[publish, overrides],
    );
    // An Event Key Match of kind 29 ends it, which the ending-key model does not cover.
    let ending = out.condition(0x8080_3DEC, 29, 12);
    out.u32(ending + 8, 0xA628_8DD1);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_REMOVAL,
        CONDITION_ROW_CLASS,
        &[ending],
    );
    let payload = out.finish();
    let decoded = decode(&payload).unwrap();
    let program = decompile(&decoded, "Native", |tag| tag).unwrap();
    assert_eq!(program.trigger, Trigger::Native);
    let trigger = program.native_trigger.as_ref().unwrap();
    assert_eq!(trigger.kind, 6);
    assert_eq!(&trigger.bytes, &payload[activation..activation + 12]);
    let removal = program.native_removal.as_ref().unwrap();
    assert_eq!(removal.kind, 29);
    assert_eq!(&removal.bytes[8..], &payload[ending + 8..ending + 12]);
    assert_eq!(program.actions.len(), 2);
    let Action::Native { node } = &program.actions[0] else {
        panic!("expected a native effect");
    };
    assert_eq!(node.kind, 29);
    assert_eq!(&node.bytes, &payload[overrides..overrides + 16]);
    assert_eq!(program.actions[1].label(), "Send a Game Signal");
    assert!(!program.actions[1].retained());
    // The rebuilt nodes compile back to the same bytes, header included.
    let mut compiled = crate::sandbox_perk::program::compiler::Payload::new();
    let node = compiled
        .native_condition(program.native_trigger.as_ref().unwrap(), 0)
        .unwrap();
    assert_eq!(
        &compiled.bytes[node..node + 12],
        &payload[activation..activation + 12]
    );
}

#[test]
fn fidelity_reports_only_unmasked_native_differences() {
    let stock = kill_attach_action(1.0);
    let mut compiled = kill_attach_action(1.0);
    assert!(fidelity(&stock, &compiled).unwrap().is_empty());
    let action = decode(&compiled).unwrap();
    let attach = action.groups[0].effects[0].offset;
    compiled[attach + 0x20] = 0x40;
    let differences = fidelity(&stock, &compiled).unwrap();
    assert_eq!(differences.len(), 1);
    assert_eq!(differences[0].offset, 0x20);
    assert_eq!(differences[0].node, "Create Entity");
    // The compiled ordinal byte is derived and never counts as a difference.
    let activation = action.groups[0].activation[0].offset;
    compiled[activation + 7] = 9;
    assert_eq!(fidelity(&stock, &compiled).unwrap().len(), 1);
}
