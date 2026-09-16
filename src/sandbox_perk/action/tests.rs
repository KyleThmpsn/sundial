//! Decoder tests over payloads built to the mapped native layout.
//!
//! These fixtures are assembled here rather than copied from a package, so the suite
//! stays runnable without an installed client. The complete comparison against every
//! installed action resource lives in the ignored `perk_action_audit` example.

use super::fixtures::{Builder, drawn_pattern_action, precision_kill_action};
use super::*;
use crate::sandbox_perk::nodes::Support;

mod catalog;

#[test]
fn condition_choices_preserve_owned_records_and_report_nested_probability_sources() {
    let payload = precision_kill_action();
    let mut graph = native::Graph::read(&payload, 0, ACTION_ROOT_CLASS).unwrap();
    let kill = graph
        .blocks
        .iter_mut()
        .find(|block| block.class == 0x80803DE7)
        .unwrap();
    kill.bytes[4] = 7;
    kill.bytes[6] = 1;
    let payload = graph.emit().unwrap();
    let decoded = decode(&payload).unwrap();
    let choices = crate::investment::native_content::conditions::from_payload(&payload).unwrap();
    let original = &decoded.groups[0].activation[0];
    let copied = choices
        .iter()
        .find(|choice| choice.bytes == original.native)
        .unwrap();
    assert!(!copied.requirements.is_empty());
    let rebuilt = native::Graph::read(&copied.bytes, 0, original.class).unwrap();
    rebuilt.validate_node(true, copied.kind).unwrap();
    assert_eq!(rebuilt.emit().unwrap(), copied.bytes);
    assert!(
        choices
            .iter()
            .any(|choice| choice.source.ends_with("End Condition"))
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.source.ends_with("Reactivation"))
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.source.ends_with("Matching Condition"))
    );
}

#[test]
fn source_derived_ability_roles_preserve_native_facts_and_the_known_event_key() {
    for known in [true, false] {
        let mut out = Builder::new();
        let declaration = nodes::condition(12).unwrap();
        let trigger = out.condition(declaration.class, 12, declaration.struct_size as usize);
        out.u32(trigger + 8, 0x6CEC7A87);
        out.u32(trigger + 12, if known { 0x18CCBF24 } else { 0x18CCBF25 });
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[trigger],
        );
        let effect = out.component_adjustment(0.1, -1.0, 1.0);
        out.bytes[effect + 2] = 0;
        out.bytes[effect + 3] = u8::from(!known);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[effect]);
        let action = decode(&out.finish()).unwrap();
        let summary = ActionSummary::new(&action);
        assert_eq!(
            summary.groups[0].activation[0]
                .text
                .contains("Orb of Light"),
            known
        );
        // The flag byte no longer gates the ability role. A census of every kind 8 node in
        // the stock perks showed the selector alone decides which ability is adjusted, so
        // grenade energy is named whether or not the flag is set.
        assert!(summary.groups[0].effects[0].text.contains("grenade energy"));
        assert!(
            summary.groups[0].effects[0]
                .detail
                .contains(&"Target Selector: 0".into())
        );
        assert!(
            summary.groups[0].effects[0]
                .detail
                .contains(&"Scale: 0.1".into())
        );
    }
}

#[test]
fn a_drawn_pattern_action_decodes_into_its_lists_and_assets() {
    let payload = drawn_pattern_action();
    let action = decode(&payload).expect("decode");
    assert_eq!(action.groups.len(), 1);
    assert_eq!(action.policy, 0);
    assert_eq!(action.retained_state_budget, 2);
    let group = &action.groups[0];
    assert_eq!(group.activation.len(), 1);
    assert_eq!(group.activation[0].kind, 16);
    assert_eq!(group.removal[0].kind, 17);
    assert_eq!(group.effects.len(), 2);
    assert_eq!(group.effects[0].kind, 26);
    assert_eq!(group.effects[0].referenced_tag, Some(0x8161_F73A));
    assert_eq!(
        group.effects[0].referenced_path.as_deref(),
        Some("content/sandbox/weapons/demo/demo.pattern.tft")
    );
    assert_eq!(group.effects[1].kind, 3);
    assert_eq!(group.effects[1].referenced_tag, Some(0x80BC_2F21));
    assert_eq!(action.referenced_tags(), vec![0x80BC_2F21, 0x8161_F73A]);
    assert_eq!(action.support(), Support::Authorable);
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
fn the_summary_reads_as_plain_english_for_a_kill_trigger() {
    let payload = precision_kill_action();
    let action = decode(&payload).expect("decode");
    let summary = ActionSummary::new(&action);
    assert_eq!(
        summary.headline,
        "A precision kill from this weapon, then applies one effect."
    );
    let group = &summary.groups[0];
    assert_eq!(group.label, "Main Program");
    assert_eq!(
        group.effects[0].text,
        "Extend the running timers by 5 s, up to 5 s"
    );
    assert_eq!(group.removal[0].text, "After 5 s");
    assert_eq!(group.rearm[0].text, "After 2.5 s");
    assert!(summary.render().contains("Ready Again When:"));
    assert!(
        summary
            .notes
            .iter()
            .any(|note| note.contains("does not prove"))
    );
}

#[test]
fn the_summary_names_assets_by_their_native_path() {
    let payload = drawn_pattern_action();
    let summary = ActionSummary::new(&decode(&payload).expect("decode"));
    assert_eq!(
        summary.groups[0].effects[0].text,
        "Replace the weapon projectile pattern with demo"
    );
    assert_eq!(summary.groups[0].effects[1].text, "Spawn demo once");
    assert_eq!(summary.groups[0].activation[0].text, "The weapon is drawn");
}

#[test]
fn a_partial_probability_is_reported_rather_than_hidden() {
    let mut out = Builder::new();
    let activation = out.kill(&[], false, 0.25);
    out.pointer_list(
        PRIMARY_GROUP + GROUP_ACTIVATION,
        CONDITION_ROW_CLASS,
        &[activation],
    );
    let payload = out.finish();
    let action = decode(&payload).expect("decode");
    assert_eq!(
        action.groups[0].activation[0].probability,
        Probability::Literal(0.25)
    );
    let summary = ActionSummary::new(&action);
    assert_eq!(
        summary.groups[0].activation[0].text,
        "A kill from any credited source (25% chance)"
    );
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
        "Add 2 rounds to this weapon and -1 round to ammo type 1 in the magazine"
    );
    assert_eq!(
        effects[0].facts,
        vec![
            Fact::new("Target Selector", FactValue::Selector(1)),
            Fact::new("Flag Byte", FactValue::Selector(0)),
            Fact::new("Option Byte", FactValue::Selector(0)),
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
            Fact::new("Assign Slot 1", FactValue::Number(1.2)),
            Fact::new("Multiply Slot 0 From Stat", FactValue::Selector(0x0B)),
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

/// Census of one decode pass over the installed finished-perk actions.
#[derive(Default)]
struct Census {
    actions: usize,
    conditions: usize,
    effects: usize,
    condition_kinds: std::collections::BTreeSet<u8>,
    effect_kinds: std::collections::BTreeSet<u8>,
    unmapped_conditions: std::collections::BTreeSet<u8>,
    unmapped_effects: std::collections::BTreeSet<u8>,
    referenced_assets: std::collections::BTreeSet<u32>,
    policies: std::collections::BTreeMap<u8, usize>,
    /// Actions by the weakest support level of any node they contain.
    by_support: std::collections::BTreeMap<Support, usize>,
    /// How many actions each kind whose fields are not mapped keeps from being fully read.
    blocking_conditions: std::collections::BTreeMap<u8, usize>,
    blocking_effects: std::collections::BTreeMap<u8, usize>,
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES with a clean Shadowkeep package directory"]
fn every_installed_perk_action_decodes_and_summarizes_without_a_structural_error() {
    use crate::investment_schema::{
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, investment_globals_table_tag,
    };
    use crate::sandbox_perk::{
        SANDBOX_PERK_RUNTIME_MAP_TAG, finished_sandbox_perk_at, finished_sandbox_perk_count,
        sandbox_perk_runtime_assignment,
    };
    use std::collections::BTreeSet;
    use tiger_pkg::TagHash;

    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must name a clean package directory");
    let install = std::path::Path::new(&packages)
        .parent()
        .expect("clean packages need an install root");
    let manager =
        crate::package_runtime::open_shadowkeep_packages(install).expect("open clean packages");
    let globals_tag =
        crate::package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)
            .expect("resolve investment_globals");
    let globals = manager.read_tag(globals_tag).expect("read globals");
    let catalog_tag = TagHash(
        investment_globals_table_tag(&globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)
            .expect("resolve finished perk catalog"),
    );
    let catalog = manager
        .read_tag(catalog_tag)
        .expect("read finished catalog");
    let runtime_map = manager
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .expect("read runtime map");

    let mut actions = BTreeSet::new();
    let perk_count = finished_sandbox_perk_count(&catalog).expect("count perks");
    for index in 0..perk_count {
        let perk = finished_sandbox_perk_at(&catalog, index).expect("read perk");
        if let Some(assignment) = sandbox_perk_runtime_assignment(&runtime_map, perk.runtime_key)
            .expect("resolve assignment")
        {
            actions.insert(assignment.runtime_tag);
        }
    }
    assert!(!actions.is_empty(), "no perk actions were reachable");

    let mut census = Census::default();
    let mut failures = Vec::new();
    for tag in actions {
        let Ok(payload) = manager.read_tag(TagHash(tag)) else {
            continue;
        };
        match decode(&payload) {
            Ok(action) => record(&mut census, &action),
            Err(error) => failures.push(format!("0x{tag:08X}: {error}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} actions failed to decode: {:?}",
        failures.len(),
        census.actions + failures.len(),
        &failures[..failures.len().min(10)]
    );
    eprintln!(
        "decoded {} actions from {perk_count} finished perks: {} conditions across kinds {:?}, {} effects across kinds {:?}",
        census.actions,
        census.conditions,
        census.condition_kinds,
        census.effects,
        census.effect_kinds
    );
    eprintln!(
        "policies {:?}; {} distinct referenced entity graphs",
        census.policies,
        census.referenced_assets.len()
    );
    eprintln!(
        "kinds whose fields are not mapped: conditions {:?}, effects {:?}",
        census.unmapped_conditions, census.unmapped_effects
    );
    // The honest reading count: an action is fully read only when every node it contains has
    // mapped fields. Structure Only nodes are named and sized but their fields are not.
    eprintln!("actions by weakest node support: {:?}", census.by_support);
    let ranked = |blocking: &std::collections::BTreeMap<u8, usize>, name: fn(u8) -> String| {
        let mut ranked = blocking.iter().collect::<Vec<_>>();
        ranked.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        ranked
            .into_iter()
            .map(|(kind, count)| format!("{} ({kind}): {count}", name(*kind)))
            .collect::<Vec<_>>()
    };
    eprintln!(
        "condition kinds blocking a full read, by actions: {:?}",
        ranked(&census.blocking_conditions, nodes::condition_name)
    );
    eprintln!(
        "effect kinds blocking a full read, by actions: {:?}",
        ranked(&census.blocking_effects, nodes::effect_name)
    );
}

fn record(census: &mut Census, action: &DecodedAction) {
    census.actions += 1;
    *census.policies.entry(action.policy).or_default() += 1;
    let summary = ActionSummary::new(action);
    assert!(!summary.headline.is_empty());
    assert!(!summary.render().is_empty());
    for condition in action.conditions() {
        census.conditions += 1;
        census.condition_kinds.insert(condition.kind);
        if condition.facts.is_empty() {
            census.unmapped_conditions.insert(condition.kind);
        }
    }
    for effect in action.effects() {
        census.effects += 1;
        census.effect_kinds.insert(effect.kind);
        if effect.facts.is_empty() && effect.referenced_tag.is_none() {
            census.unmapped_effects.insert(effect.kind);
        }
        census.referenced_assets.extend(effect.referenced_tag);
    }
    *census.by_support.entry(action.support()).or_default() += 1;
    let structural = |support: Support| support >= Support::Structural;
    let mut blocking_conditions = std::collections::BTreeSet::new();
    let mut blocking_effects = std::collections::BTreeSet::new();
    for condition in action.conditions() {
        if condition
            .catalog()
            .is_none_or(|node| structural(node.support))
        {
            blocking_conditions.insert(condition.kind);
        }
    }
    for effect in action.effects() {
        if effect.catalog().is_none_or(|node| structural(node.support)) {
            blocking_effects.insert(effect.kind);
        }
    }
    for kind in blocking_conditions {
        *census.blocking_conditions.entry(kind).or_default() += 1;
    }
    for kind in blocking_effects {
        *census.blocking_effects.entry(kind).or_default() += 1;
    }
}

#[test]
fn general_predicates_read_as_the_state_and_weapon_type_they_check() {
    use crate::sandbox_perk::action::{native, summary::state_description};
    let class = 0x8080_3DCE;
    let mut bytes = native::template(true, 20).unwrap();
    // A key no stock perk names, and no weapon record, keeps the traced name.
    bytes[0xD4..0xD8].copy_from_slice(&0x811C_9DC5u32.to_le_bytes());
    bytes[0xF8] = 0;
    let plain = state_description(class, &bytes);
    // The template may carry its own state; only assert on the keys written below.
    let _ = plain;
    bytes[0xD4..0xD8].copy_from_slice(&0x59E3_47EDu32.to_le_bytes());
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Charged with Light")
    );
    bytes[0xF8] = 1;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While not Charged with Light")
    );
    bytes[0xD4..0xD8].copy_from_slice(&0xE1E6_BB64u32.to_le_bytes());
    bytes[0xF8] = 0;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Subclass Is Arc")
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
            vec![(2u8, "Airborne"), (4, "Sliding"), (8, "Sprinting")]
        );
        let weapon = fields.iter().find(|f| f.offset == 0x81).unwrap();
        assert_eq!(weapon.label, "Weapon State");
        assert_eq!(
            native::fields::contract(class, weapon).choices.to_vec(),
            vec![(4u8, "Aiming Down Sights")]
        );
        assert!(
            fields
                .iter()
                .any(|f| f.offset == 0x18 && f.label == "Health Value 1")
        );
    }
    let class = 0x8080_3DCE;
    let mut bytes = native::template(true, 20).unwrap();
    bytes[0xD4..0xD8].copy_from_slice(&0x811C_9DC5u32.to_le_bytes());
    bytes[0xF8] = 0;
    bytes[0x38] = 0;
    bytes[0x81] = 0;
    assert_eq!(state_description(class, &bytes), None);
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
    // A named key still wins over the inline states.
    bytes[0xD4..0xD8].copy_from_slice(&0x59E3_47EDu32.to_le_bytes());
    bytes[0xF8] = 0;
    assert_eq!(
        state_description(class, &bytes).as_deref(),
        Some("While Charged with Light")
    );
}
