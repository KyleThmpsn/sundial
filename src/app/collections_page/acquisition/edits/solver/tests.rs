use super::*;
use crate::catalog::{CollectibleDef, CollectionConditionDef, UnlockDefinition};
use serde_json::json;

fn token(kind: u32, operand: i32) -> Token {
    Token {
        kind,
        operand: operand as u32,
    }
}
fn catalog() -> Catalog {
    Catalog::for_test(vec![], Default::default()).with_test_progression(
        (0..32)
            .map(|slot| UnlockDefinition {
                code: 1,
                compact_slot: Some(slot),
                ..Default::default()
            })
            .collect(),
        (0..4)
            .map(|slot| UnlockDefinition {
                code: 1,
                compact_slot: Some(slot),
                ..Default::default()
            })
            .collect(),
        vec![],
    )
}
fn apply(tokens: Vec<Token>, native: bool) -> (Value, Catalog) {
    let catalog = catalog();
    let mut doc = json!({"future":{"preserve":true}});
    if native {
        doc["_native_progression"] = json!({});
    }
    let record = CollectibleDef {
        index: 0,
        hash: 100,
        item_definition_index: 0,
        item_hash: 200,
        material_requirement_set_index: None,
        material_requirement_set_hash: 0,
        material_requirements: vec![],
        name: "Solver Test".into(),
        type_name: String::new(),
        paths: vec![],
        conditions: vec![CollectionConditionDef {
            field: ACQUISITION_CONDITION_FIELD,
            tokens,
        }],
    };
    let snapshot = collection_state_snapshot(&doc).unwrap();
    set_collectible_acquisition_state(&mut doc, &record, &snapshot, &catalog, true).unwrap();
    assert_eq!(doc["future"]["preserve"], true);
    assert_eq!(
        acquisition_status(&record, &collection_state_snapshot(&doc).unwrap(), &catalog).state,
        AcquisitionState::Acquired
    );
    (doc, catalog)
}
#[test]
fn inverts_arithmetic_and_bitmasks_in_both_account_formats() {
    for native in [false, true] {
        for expr in [
            vec![
                token(10, 0),
                token(11, 2),
                token(19, 0),
                token(11, 10),
                token(8, 0),
            ],
            vec![
                token(10, 0),
                token(11, 3),
                token(20, 0),
                token(11, 7),
                token(8, 0),
            ],
            vec![
                token(10, 0),
                token(11, 8),
                token(25, 0),
                token(11, 8),
                token(8, 0),
            ],
            vec![
                token(10, 0),
                token(11, 9),
                token(17, 0),
                token(11, 4),
                token(19, 0),
                token(11, 56),
                token(8, 0),
            ],
        ] {
            apply(expr, native);
        }
    }
}
#[test]
fn solves_large_mixed_boolean_conditions_without_cartesian_enumeration() {
    let mut expr = vec![];
    for index in 0..24 {
        expr.push(token(1, index));
        if index % 2 == 0 {
            expr.push(token(2, 0));
        }
        if index > 0 {
            expr.push(token(4, 0));
        }
    }
    let (doc, catalog) = apply(expr, false);
    let snapshot = collection_state_snapshot(&doc).unwrap();
    for index in 0..24 {
        assert_eq!(
            snapshot.evaluated_flag(index, &catalog),
            Some(index % 2 == 1)
        );
    }
}
#[test]
fn does_not_offer_a_plan_for_contradictory_shared_inputs() {
    let catalog = catalog();
    let snapshot = collection_state_snapshot(&json!({})).unwrap();
    assert!(
        solve(
            &[token(1, 0), token(1, 0), token(2, 0), token(4, 0)],
            &snapshot,
            &catalog,
            true
        )
        .is_none()
    );
}

#[test]
fn an_unknown_branch_does_not_block_a_verified_writable_alternative() {
    let catalog = catalog();
    let snapshot = collection_state_snapshot(&json!({})).unwrap();
    let plan = solve(
        &[token(1, 999), token(1, 0), token(3, 0)],
        &snapshot,
        &catalog,
        true,
    )
    .unwrap();
    assert_eq!(
        plan,
        vec![CollectionStateEdit::Flag {
            definition_index: 0,
            set: true
        }]
    );
}
