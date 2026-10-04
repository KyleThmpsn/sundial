use super::*;
use sundial::package_authoring::{
    sandbox_perk::dependencies::{Behavior, Perk},
    sandbox_perk::nodes,
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
            program: None,
            condition_kinds: vec![8],
            effect_kinds: vec![2],
            details: Vec::new(),
            notes: Vec::new(),
        }),
    };
    let label = reading::identity(&perk);
    assert!(label.contains("Dynamic Value"));
    assert!(label.contains("Effect 1967"));
    assert!(!label.contains("Action 0x"));
    perk.behavior = None;
    assert!(reading::identity(&perk).contains("Action 0x8162C82C"));
}
