use super::*;
use crate::catalog::{CollectionConditionDef, CollectionConditionTokenDef};
use serde_json::json;

fn collectible(index: u16, slot: u32, inverse: bool) -> CollectibleDef {
    let mut tokens = vec![CollectionConditionTokenDef {
        kind: FLAG_INSTRUCTION,
        operand: slot,
    }];
    if inverse {
        tokens.push(CollectionConditionTokenDef {
            kind: 2,
            operand: 0,
        });
    }
    CollectibleDef {
        index,
        hash: u64::from(index) + 100,
        item_definition_index: index,
        item_hash: u64::from(index) + 200,
        material_requirement_set_index: None,
        material_requirement_set_hash: 0,
        material_requirements: Vec::new(),
        name: format!("Item {index}"),
        type_name: "Weapon".into(),
        paths: Vec::new(),
        conditions: vec![CollectionConditionDef { field: 4, tokens }],
        parent_nodes: Vec::new(),
    }
}

fn catalog() -> Catalog {
    Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        (0..3)
            .map(|slot| UnlockDefinition {
                code: 1,
                compact_slot: Some(slot),
                ..Default::default()
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn shared_flags_are_counted_once_per_collectible_and_conflicts_roll_back() {
    let catalog = catalog();
    let mut document = json!({});
    let same_flag = [collectible(0, 0, false), collectible(1, 0, false)];
    assert_eq!(
        apply_rows(&mut document, &catalog, &same_flag, true)
            .unwrap()
            .changed,
        2
    );
    let before = document.clone();
    let conflicting = [collectible(0, 0, false), collectible(1, 0, true)];
    assert!(apply_rows(&mut document, &catalog, &conflicting, true).is_err());
    assert_eq!(document, before);
}

#[test]
fn bulk_acquisition_persists_in_the_selected_json_or_sqlite_account() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = catalog();
    let definitions = [collectible(0, 0, false), collectible(1, 1, false)];
    for runtime in 0..3 {
        let native = runtime == 1;
        let dawn = runtime == 2;
        let directory = crate::test_support::TestDirectory::new("bulk-account-routing");
        let path = directory.0.join("settings.json");
        let json = json!({"version":if dawn {6} else if native {18} else {8},"state":{"characters":[],"unlocks":{"future":true}}});
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        if native {
            crate::persistence::sqlite_account::tests::create_fixture(
                &directory.0.join("data/investment.sqlite3"),
                3,
            );
        }
        if dawn {
            crate::persistence::dawn_account::tests::create_fixture(
                &directory.0.join("player-state.db"),
            );
        }
        let mut workspace = WorkspaceDocument::load(json.clone(), &path, dawn);
        for acquired in [true, false] {
            let mut view = workspace.progression_view(0);
            assert_eq!(
                apply_rows(&mut view, &catalog, &definitions, acquired)
                    .unwrap()
                    .changed,
                2
            );
            workspace.apply_progression_view(0, view).unwrap();
            if dawn {
                assert_eq!(workspace.json(), &json);
                workspace.save_dawn().unwrap();
            } else if native {
                assert_eq!(workspace.json(), &json);
                crate::persistence::sqlite_account::tests::save_fixture_document(
                    workspace.native_account_mut().unwrap(),
                    &directory.0.join(format!("backup-{acquired}.sqlite3")),
                );
            } else {
                std::fs::write(&path, serde_json::to_vec(workspace.json()).unwrap()).unwrap();
            }
            workspace = WorkspaceDocument::load(
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
                &path,
                dawn,
            );
            let snapshot = collection_state_snapshot(&workspace.progression_view(0)).unwrap();
            for definition in &definitions {
                assert_eq!(
                    collectible_acquired_state(definition, &snapshot, &catalog),
                    Some(acquired)
                );
            }
        }
    }
}
