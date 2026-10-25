//! Decoder tests over payloads built to the mapped native layout.
//!
//! These fixtures are assembled here rather than copied from a package, so the suite
//! stays runnable without an installed client. The complete comparison against every
//! installed action resource lives in the ignored `perk_action_audit` example.

use super::fixtures::{Builder, drawn_pattern_action, precision_kill_action};
use super::*;
use crate::sandbox_perk::action::native::NodeKind as NativeNodeKind;
use crate::sandbox_perk::nodes::Support;

mod catalog;

#[test]
fn incoming_damage_summaries_distinguish_literal_stats_and_source_distance() {
    for (selector, distance, expected, bounded) in [
        (255, -1., "by 0.5", false),
        (255, 0., "by 0.5", true),
        (12, 5., "by native stat 12", true),
    ] {
        let mut out = Builder::new();
        let at = out.node(0x8080_3E3C, 0xB8);
        out.bytes[at] = 33;
        out.f32(at + 4, 0.5);
        out.bytes[at + 8] = selector;
        out.f32(at + 12, distance);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[at]);
        let decoded = decode(&out.finish()).unwrap();
        let effect = &decoded.groups[0].effects[0];
        let text = summary::describe_effect(effect);
        assert!(text.contains(expected), "{text}");
        assert_eq!(text.contains("source distance"), bounded);
        let fields = native::fields::describe(effect.class).unwrap();
        let stat = fields.iter().find(|f| f.offset == 8).unwrap();
        assert_eq!(
            native::fields::contract(effect.class, stat).choices,
            &[(255, "Fixed Multiplier")]
        );
        assert!(
            fields
                .iter()
                .filter(|f| matches!(f.offset, 0xA8 | 0xB0))
                .all(|f| !f.editable)
        );
    }
}

#[test]
fn a_precision_kill_action_decodes_its_labels_timers_and_nested_conditions() {
    let payload = precision_kill_action();
    let action = decode(&payload).expect("decode");
    let group = &action.groups[0];
    let activation = &group.activation[0];
    assert_eq!(activation.kind, 2);
    assert_eq!(
        activation.facts[0],
        Fact::new("Matches Any Label", FactValue::Labels(vec![0x962E_A19B]))
    );
    assert_eq!(
        activation.facts[1],
        Fact::new("Requires Owning Weapon", FactValue::Flag(true))
    );
    assert_eq!(group.removal[0].facts[0].value, FactValue::Seconds(5.0));
    assert_eq!(group.rearm[0].facts[0].value, FactValue::Seconds(2.5));
    let extend = &group.effects[0];
    assert_eq!(extend.kind, 32);
    assert_eq!(extend.conditions.len(), 1);
    assert_eq!(extend.conditions[0].kind, 2);
    assert_eq!(action.conditions().len(), 4);
    assert_eq!(action.activation_event_mask, 1 << 2);
}

#[test]
fn a_declared_size_that_disagrees_with_the_payload_is_rejected() {
    let mut payload = drawn_pattern_action();
    payload.push(0);
    assert!(decode(&payload).is_err());
    assert!(decode(&[0; 8]).is_err());
    assert!(decode(&[]).is_err());
}

#[test]
fn a_wrong_row_class_is_rejected_rather_than_read_as_nodes() {
    let mut out = Builder::new();
    let activation = out.draw();
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        EFFECT_ROW_CLASS,
        &[activation],
    );
    let payload = out.finish();
    let error = decode(&payload).expect_err("class mismatch");
    assert!(error.contains("class"), "{error}");
}

#[test]
fn structural_kinds_now_read_their_traced_fields() {
    let mut out = Builder::new();
    let activation = out.object_event_filter(&[0xC20D_D425], 0.01);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let adjustment = out.component_adjustment(0.1, -1.0, 2.5);
    let ammunition = out.fixed_ammunition(&[0x962E_A19B], 2, -1);
    let modifier = out.event_modifier(&[0xBF39_E12B], 1.2, 0x0B);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_EFFECTS,
        EFFECT_ROW_CLASS,
        &[adjustment, ammunition, modifier],
    );
    let action = decode(&out.finish()).expect("decode");
    assert_eq!(action.support(), Support::Authorable);
    let filter = &action.groups[0].activation[0];
    assert_eq!(filter.kind, 4);
    assert_eq!(
        filter.facts,
        vec![
            Fact::new("Matches Any Label", FactValue::Labels(vec![0xC20D_D425])),
            Fact::new("Value Threshold", FactValue::Number(0.01)),
            Fact::new("Object Filter Selector", FactValue::Selector(2)),
            Fact::new("Source Mask", FactValue::Mask(4)),
            Fact::new("Stateful Predicate", FactValue::Flag(false)),
        ]
    );
    let effects = &action.groups[0].effects;
    assert_eq!(
        effects[1].description(),
        "Add 2 rounds to this weapon and -1 round to primary ammo in the magazine"
    );
    assert_eq!(
        effects[0].facts,
        vec![
            Fact::new("Target Selector", FactValue::Selector(1)),
            Fact::new("Ability State", FactValue::Selector(0)),
            Fact::new("Ability Version", FactValue::Selector(0)),
            Fact::new("Scale", FactValue::Number(0.1)),
            Fact::new("Limit", FactValue::Number(-1.0)),
            Fact::new("Input Selector", FactValue::Selector(0xFF)),
            Fact::new("Constant Value", FactValue::Number(2.5)),
        ]
    );
    assert_eq!(
        effects[1].facts,
        vec![
            Fact::new("Excludes Any Label", FactValue::Labels(vec![0x962E_A19B])),
            Fact::new("Storage Path", FactValue::Selector(1)),
            Fact::new("Allow Magazine Overflow", FactValue::Flag(false)),
            Fact::new("Scale by Ammunition Unit", FactValue::Flag(false)),
            Fact::new("Scale by Action Value", FactValue::Flag(false)),
            Fact::new("Owning Slot Amount", FactValue::Integer(2)),
            Fact::new("Category 1 Amount", FactValue::Integer(-1)),
        ]
    );
    assert_eq!(
        effects[2].facts,
        vec![
            Fact::new("Excludes Any Label", FactValue::Labels(vec![0xBF39_E12B])),
            Fact::new("Assign Precision Bonus", FactValue::Number(1.2)),
            Fact::new(
                "Multiply Base Damage Scale from Stat",
                FactValue::Selector(0x0B)
            ),
        ]
    );
    let summary = ActionSummary::new(&action);
    assert!(
        !summary
            .notes
            .iter()
            .any(|note| note.contains("not its individual fields")),
        "{:?}",
        summary.notes
    );
}

#[test]
fn event_modifiers_distinguish_included_and_excluded_labels_and_decode_scalar_pairs() {
    // Micro-Missile uses these opposite source filters and literal pairs. The previous
    // reader omitted the include-any list and described exclusion as a requirement.
    let mut out = Builder::new();
    let mut nodes = Vec::new();
    for (source_offset, adjustment) in [(0x20, 1.4), (0, 0.5)] {
        let node = out.node(0x80802F16, 336);
        out.bytes[node] = 40;
        out.labels(node + 0xC0 + source_offset, &[0x3FBE_3C2A]);
        let pair = out.node(0x80802F1A, 8);
        out.f32(pair, adjustment);
        out.pointer(node + 0x140, pair);
        nodes.push(node);
    }
    out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &nodes);
    let decoded = decode(&out.finish()).unwrap();
    let effects = &decoded.groups[0].effects;
    assert!(effects[0].facts.contains(&Fact::new(
        "Excludes Any Label",
        FactValue::Labels(vec![0x3FBE_3C2A])
    )));
    assert!(effects[1].facts.contains(&Fact::new(
        "Matches Any Label",
        FactValue::Labels(vec![0x3FBE_3C2A])
    )));
    assert!(effects[0].facts.contains(&Fact::new(
        "Default Scalar Multiplier",
        FactValue::Number(2.4)
    )));
    assert!(effects[1].facts.contains(&Fact::new(
        "Default Scalar Multiplier",
        FactValue::Number(1.5)
    )));
    assert!(effects.iter().all(|effect| {
        !effect
            .facts
            .iter()
            .any(|fact| fact.label == "Alternate Scalar Multiplier")
    }));
}

#[test]
fn general_predicates_read_as_the_state_and_weapon_type_they_check() {
    use crate::sandbox_perk::action::{native, summary::state_description};
    let class = 0x8080_3DCE;
    let mut bytes = native::template(NativeNodeKind::Condition(20)).unwrap();
    // The template requires the weapon in hand and aimed. These titles read the key alone.
    bytes[0x38] = 0;
    bytes[0x81] = 0;
    bytes[0xF8] = 0;
    bytes[0xD4..0xD8].copy_from_slice(&0x59E3_47EDu32.to_le_bytes());
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("Meets the Charged with Light Stacks requirement")
    );
    bytes[0xF8] = 1;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("Does not meet the Charged with Light Stacks requirement")
    );
    bytes[0xD4..0xD8].copy_from_slice(&0xE1E6_BB64u32.to_le_bytes());
    bytes[0xF8] = 0;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Subclass Is Arc")
    );
    // A switch checked at 0 to 0 holds while its state does not. A count keeps its range.
    bytes[0xD8..0xE0].copy_from_slice(&[0; 8]);
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While not Subclass Is Arc")
    );
    bytes[0xD4..0xD8].copy_from_slice(&0x59E3_47EDu32.to_le_bytes());
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("Meets the Charged with Light Stacks requirement")
    );
    // The state keys hash from the engine's own variable names with the predicate fold.
    use crate::sandbox_perk::action::native::predicate::binding_key;
    assert_eq!(binding_key("is_arc"), 0xE1E6_BB64);
    assert_eq!(binding_key("is_void"), 0xEE5F_6482);
    assert_eq!(binding_key("super_active"), 0xD16E_1FA3);
    assert_eq!(binding_key("iron_sights"), 0xD9C4_6BC4);
    assert_eq!(binding_key("weapon_firing"), 0x59E4_FF8D);
    assert_eq!(binding_key("charged_with_light_stacks"), 0x59E3_47ED);
    assert_eq!(binding_key("kill_tag_gathered"), 0x7020_0731);
    assert_eq!(binding_key("near_bank"), 0x4FAA_5194);
    assert_eq!(binding_key("melee_energy"), 0x2814_F006);
}

#[test]
fn general_predicate_player_and_weapon_states_read_from_the_perks_that_set_them() {
    use crate::sandbox_perk::action::{native, summary::state_description};
    for class in [0x8080_3DCEu32, 0x8080_3DCC] {
        let fields = native::fields::describe(class).unwrap();
        let player = fields.iter().find(|f| f.offset == 0x38).unwrap();
        assert_eq!(player.label, "Player State");
        assert_eq!(
            native::fields::contract(class, player).choices.to_vec(),
            vec![
                (1u8, "Crouching"),
                (2, "Airborne"),
                (4, "Sliding"),
                (8, "Sprinting"),
                (16, "Missing Health Without Recovery"),
                (32, "Missing Shields Without Recovery"),
            ]
        );
        let weapon = fields.iter().find(|f| f.offset == 0x81).unwrap();
        assert_eq!(weapon.label, "Weapon State");
        assert_eq!(
            native::fields::contract(class, weapon).choices.to_vec(),
            vec![(1u8, "Holding the Weapon"), (4, "Aiming Down Sights")]
        );
        assert!(
            fields
                .iter()
                .any(|f| f.offset == 0x18 && f.label == "Minimum Health Fraction")
        );
    }
    let class = 0x8080_3DCE;
    let mut bytes = native::template(NativeNodeKind::Condition(20)).unwrap();
    bytes[0xD4..0xD8].copy_from_slice(&0x811C_9DC5u32.to_le_bytes());
    bytes[0xF8] = 0;
    bytes[0x38] = 0;
    bytes[0x81] = 0;
    assert_eq!(state_description(class, &bytes), None);
    bytes[0x38] = 1 | 16 | 32;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some(
            "While Crouching and Missing Health Without Recovery and Missing Shields Without Recovery"
        )
    );
    bytes[0x38] = 1 | 128;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Crouching and Player State 0x80")
    );
    bytes[0x38] = 2;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Airborne")
    );
    bytes[0x81] = 4;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Airborne and Aiming Down Sights")
    );
    bytes[0x38] = 0;
    bytes[0xF8] = 1;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While not Aiming Down Sights")
    );
    // A named key keeps the inline states beside it, since the predicate requires both.
    bytes[0xD4..0xD8].copy_from_slice(&0x59E3_47EDu32.to_le_bytes());
    bytes[0xF8] = 0;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("Meets the Charged with Light Stacks requirement while Aiming Down Sights")
    );
}
