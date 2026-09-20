use super::by_key;
use crate::app::progression::{
    ProgressionDefinition, ProgressionScope, ProgressionValue, hierarchy::progression_display_rows,
    state::ProgressionDisplayRow,
};
use serde_json::json;

fn progression(index: u16, name: &str, hash: u32) -> ProgressionDefinition {
    serde_json::from_value(json!({
        "definition_index": index,
        "hash": hash,
        "name": name,
        "scope": "Account",
        "scope_slot": null,
        "repeat_last_step": false
    }))
    .unwrap()
}

/// Every sorted column in the progression tables reaches this one function, so the rule it
/// carries is asserted once here rather than per column: a row with no value sorts last in both
/// directions rather than sorting as though its value were zero, and rows that tie keep the order
/// they arrived in.
#[test]
fn rows_without_a_value_sort_last_in_both_directions_and_ties_keep_their_order() {
    let rows = || {
        vec![
            ("b", Some(2)),
            ("missing first", None),
            ("a", Some(1)),
            ("missing second", None),
            ("b tie", Some(2)),
        ]
    };

    let mut ascending = rows();
    by_key(&mut ascending, false, |row| row.1);
    assert_eq!(
        ascending.iter().map(|row| row.0).collect::<Vec<_>>(),
        ["a", "b", "b tie", "missing first", "missing second"]
    );

    let mut descending = rows();
    by_key(&mut descending, true, |row| row.1);
    assert_eq!(
        descending.iter().map(|row| row.0).collect::<Vec<_>>(),
        ["b", "b tie", "a", "missing first", "missing second"]
    );
}

#[test]
fn display_rows_retain_unknown_and_other_scope_saved_values() {
    let mut definitions = [
        progression(0, "Account", 10),
        progression(1, "Character", 20),
    ];
    definitions[1].scope = ProgressionScope::Character;
    let authored = [
        ProgressionValue {
            definition_index: 1,
            lanes: [1, 2, 3],
        },
        ProgressionValue {
            definition_index: 99,
            lanes: [0, 0, 0],
        },
        ProgressionValue {
            definition_index: 99,
            lanes: [4, 5, 6],
        },
    ];
    assert_eq!(
        progression_display_rows(&authored, &definitions, ProgressionScope::Account),
        [
            ProgressionDisplayRow {
                definition_index: 0,
                lanes: None
            },
            ProgressionDisplayRow {
                definition_index: 1,
                lanes: Some([1, 2, 3])
            },
            ProgressionDisplayRow {
                definition_index: 99,
                lanes: Some([4, 5, 6])
            },
        ]
    );
}
