use super::*;
use sundial::package_authoring::{
    sandbox_perk::dependencies::{Behavior, Perk},
    sandbox_perk::nodes,
    weapon_runtime::{native_member_names, native_type_name},
};

#[test]
fn ingredient_rows_identify_operations_and_keep_undecoded_actions_explicit() {
    let mut perk = Perk {
        index: 1967,
        hash: 1,
        runtime_key: 2,
        action: Some(0x8162C82C),
        graphs: Vec::new(),
        error: None,
        behavior: Some(Behavior {
            headline: String::new(),
            support: nodes::Support::Authorable,
            editable: true,
            condition_kinds: vec![8],
            effect_kinds: vec![2],
            details: Vec::new(),
            notes: Vec::new(),
        }),
    };
    let label = reading::identity(&perk);
    assert!(label.contains(nodes::effect_name(2).as_str()));
    assert!(label.contains("Effect 1967"));
    assert!(!label.contains("Action 0x"));
    perk.behavior = None;
    assert!(reading::identity(&perk).contains("Action 0x8162C82C"));
}

#[test]
fn known_type_roles_do_not_spread_to_unresolved_related_types() {
    assert_eq!(native_type_name(0x80803B73), Some("Projectile Movement"));
    assert_eq!(native_type_name(0x80803C56), None);
    assert_eq!(native_type_name(0x80804C7A), None);
    assert_eq!(native_type_name(0), None);
    assert!(native_member_names(0x80808506).contains(&"Query Marker Set".to_owned()));
    assert!(native_member_names(0x80804C7A).is_empty());
    let timer = nodes::condition(1).unwrap();
    assert_eq!(native_type_name(timer.class), Some(timer.name));
}
