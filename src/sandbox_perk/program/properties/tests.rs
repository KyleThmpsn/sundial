use super::*;
use crate::sandbox_perk::action::{self, fixtures::Builder};

pub(super) fn payload(key: u32, value: f32) -> Vec<u8> {
    let mut out = Builder::new();
    let activation = out.event_key(99);
    out.pointer_list(
        action::PRIMARY_GROUP + action::GROUP_ACTIVATION,
        action::CONDITION_ROW_CLASS,
        &[activation],
    );
    let property = out.named_property(key, value, 2, 1, 0);
    out.pointer_list(
        action::PRIMARY_GROUP + action::GROUP_EFFECTS,
        action::EFFECT_ROW_CLASS,
        &[property, property],
    );
    let removal = out.event_key(7);
    out.pointer_list(
        action::PRIMARY_GROUP + action::GROUP_REMOVAL,
        action::CONDITION_ROW_CLASS,
        &[removal],
    );
    out.finish()
}

#[test]
fn shared_actions_count_nodes_once_and_resolve_names_from_the_selected_catalog() {
    let usage = KeyUsage::read(&action::decode(&payload(42, 2.5)).unwrap());
    let index = KeyIndex {
        perks: vec![(1, 100), (2, 100), (3, 101)],
        actions: [(100, usage.clone()), (101, usage)].into(),
        ..KeyIndex::default()
    };
    let keys = KeyCatalog::from_index(&index, |index| vec![format!("Test Perk {index}")]);
    let property = keys.property_key(42).unwrap();
    assert_eq!(
        property.nodes, 2,
        "Repeated pointers and shared perks are not additional nodes"
    );
    assert_eq!(property.values, [2.5]);
    assert_eq!(property.targets, [2]);
    assert_eq!(property.operations, [1]);
    assert_eq!(property.removals, [0]);
    assert_eq!(
        property.perks,
        ["Test Perk 1", "Test Perk 2", "Test Perk 3"]
    );
    assert_eq!(keys.removal_key(7).unwrap().nodes, 2);
    assert!(
        keys.removal_key(99).is_none(),
        "Activation keys are not ending keys"
    );
    let renamed = KeyCatalog::from_index(&index, |_| vec!["Local Test Perk".into()]);
    assert_eq!(renamed.property_key(42).unwrap().perks, ["Local Test Perk"]);
    let encoded = serde_json::to_string(&index).unwrap();
    assert!(
        !encoded.contains("Test Perk"),
        "Names must never be saved with structural observations"
    );
    let unnamed = KeyCatalog::from_index(&index, |_| Vec::new());
    assert_eq!(
        unnamed.property_key(42).unwrap().seen_in(),
        "2 installed nodes, no named perk"
    );
}

#[test]
fn ambiguous_and_nonconstant_values_do_not_become_a_false_shared_default() {
    let mut a = KeyUsage::read(&action::decode(&payload(42, -3.0)).unwrap());
    let mut b = KeyUsage::read(&action::decode(&payload(42, 2.0)).unwrap());
    b.properties[0].target = 1;
    let mut decoded = action::decode(&payload(77, 1.0)).unwrap();
    for node in &mut decoded.groups[0].effects {
        node.facts
            .retain(|fact| fact.label != NAMED_PROPERTY_LABELS.value);
    }
    a.properties.extend(KeyUsage::read(&decoded).properties);
    let index = KeyIndex {
        actions: [(100, a), (101, b)].into(),
        ..KeyIndex::default()
    };
    let keys = KeyCatalog::from_index(&index, |_| Vec::new());
    assert_eq!(keys.property_key(42).unwrap().values, [-3.0, 2.0]);
    assert_eq!(
        KeyEvidence::single(&keys.property_key(42).unwrap().targets),
        None
    );
    assert!(keys.property_key(77).unwrap().values.is_empty());
    assert_eq!(keys.property_keys()[0].nodes, 2);
}
