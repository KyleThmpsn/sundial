use super::*;
use crate::app::progression::{
    ProgressionScope, ProgressionValue, hierarchy::progression_display_rows,
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

#[test]
fn progression_columns_keep_missing_values_last_and_equal_names_stable() {
    let mut definitions = vec![
        progression(0, "bravo", 30),
        progression(1, "Alpha", 10),
        progression(2, "alpha", 20),
    ];
    definitions[0].scope_slot = Some(2);
    definitions[2].scope_slot = Some(1);
    definitions[0].steps = serde_json::from_value(json!([{"progress_total": 100}])).unwrap();
    definitions[1].steps = serde_json::from_value(json!([{"progress_total": 200}])).unwrap();
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        Vec::new(),
        Vec::new(),
        definitions,
    );
    let original = [
        ProgressionDisplayRow {
            definition_index: 0,
            lanes: Some([10, -1, 30]),
        },
        ProgressionDisplayRow {
            definition_index: 99,
            lanes: Some([20, 0, 0]),
        },
        ProgressionDisplayRow {
            definition_index: 1,
            lanes: None,
        },
        ProgressionDisplayRow {
            definition_index: 2,
            lanes: Some([5, 4, -3]),
        },
    ];
    for (column, descending, expected) in [
        (0, false, [0, 1, 2, 99]),
        (0, true, [99, 2, 1, 0]),
        (1, false, [1, 2, 0, 99]),
        (1, true, [0, 2, 1, 99]),
        (2, false, [1, 2, 0, 99]),
        (2, true, [0, 1, 2, 99]),
        (3, false, [2, 0, 99, 1]),
        (3, true, [0, 2, 99, 1]),
        (4, false, [2, 0, 99, 1]),
        (4, true, [99, 0, 2, 1]),
        (5, false, [0, 1, 99, 2]),
        (5, true, [1, 0, 99, 2]),
        (6, false, [0, 99, 2, 1]),
        (6, true, [2, 99, 0, 1]),
        (7, false, [2, 99, 0, 1]),
        (7, true, [0, 99, 2, 1]),
        (8, true, [0, 99, 1, 2]),
    ] {
        let mut rows = original;
        sort_progression_rows(&mut rows, &catalog, TableSort { column, descending });
        assert_eq!(
            rows.map(|row| row.definition_index),
            expected,
            "column {column}, descending {descending}"
        );
    }
}

#[test]
fn definition_names_sort_stably_in_both_directions() {
    let definitions = [
        progression(0, "bravo", 30),
        progression(1, "Alpha", 10),
        progression(2, "alpha", 20),
        progression(3, "", 40),
    ];
    for (descending, expected) in [(false, [1, 2, 0, 3]), (true, [0, 1, 2, 3])] {
        let mut rows = definitions.each_ref();
        sort_progression_definitions(
            &mut rows,
            TableSort {
                column: 2,
                descending,
            },
        );
        assert_eq!(rows.map(|row| row.definition_index), expected);
    }
}

#[test]
fn override_columns_preserve_signed_values_and_missing_definitions() {
    let definitions: Vec<UnlockDefinition> = [(30, "bravo"), (10, "Alpha"), (20, "alpha")]
        .into_iter()
        .map(|(hash, name)| UnlockDefinition {
            hash,
            name: Some(name.into()),
            code: 0,
            compact_slot: None,
            description: None,
            runtime_writers: Vec::new(),
            tested_by: Vec::new(),
        })
        .collect();
    for (column, descending, expected) in [
        (0, false, [0, 1, 2, 99]),
        (1, true, [0, 2, 1, 99]),
        (2, false, [1, 0, 99, 2]),
        (2, true, [2, 99, 0, 1]),
        (3, false, [1, 2, 0, 99]),
        (3, true, [0, 1, 2, 99]),
    ] {
        let mut rows = [(0, 0), (99, 5), (1, -10), (2, 20)];
        sort_overrides(
            &mut rows,
            TableSort { column, descending },
            |row| *row,
            |index| definitions.get(index),
        );
        assert_eq!(rows.map(|row| row.0), expected);
    }
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
