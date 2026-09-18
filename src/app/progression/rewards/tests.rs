use super::*;
use crate::catalog::{
    InventoryMetadata, InventoryScope, ItemDef, ItemPackageMetadata, ItemStackability,
};
use serde_json::json;

pub(in crate::app::progression) fn catalog() -> Catalog {
    let hashes = [100, 200, 300, 9000].into_iter().chain(1000..1034);
    let mut items = Vec::new();
    let mut packages = std::collections::HashMap::new();
    let mut inventory = std::collections::HashMap::new();
    for hash in hashes {
        items.push(ItemDef {
            hash,
            name: format!("Material {hash}"),
            type_name: "Material".into(),
            bucket_hash: 0,
            class_type: 3,
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        });
        packages.insert(
            hash,
            ItemPackageMetadata {
                definition_index: hash as u32,
                ..Default::default()
            },
        );
        inventory.insert(
            hash,
            InventoryMetadata {
                scope: if hash == 9000 {
                    InventoryScope::Profile
                } else {
                    InventoryScope::Character
                },
                native_bucket_id: if hash < 300 { 0 } else { 37 },
                stackability: if hash < 300 {
                    ItemStackability::Instanced
                } else {
                    ItemStackability::Stackable
                },
                max_stack_size: Some(10),
                bucket_capacity: Some(5),
            },
        );
    }
    Catalog::for_test_with_inventory(items, packages, inventory)
}

fn document() -> Value {
    json!({"_native_progression":{}, "_reward_context":{"character":0,"character_count":2,"class":0,"consumables":[],"inventory":[],"equipment":[],"next_serial":1}})
}

#[test]
fn consumable_plans_merge_to_the_cap_and_fail_atomically_on_overflow() {
    let catalog = catalog();
    let mut document = document();
    document["_reward_context"]["consumables"] = json!([{"definition_hash":1000,"quantity":6}]);
    queue(&mut document, &catalog, [(1000, 2), (1000, 2)]).unwrap();
    assert_eq!(direct_count(&Value::Null, &document), 2);
    assert_eq!(document["_progression_rewards"], json!([]));
    let before = document.clone();
    assert!(
        queue(&mut document, &catalog, [(9000, 1), (1000, 1)])
            .unwrap_err()
            .contains("stack cap 10")
    );
    assert_eq!(
        document, before,
        "Neither delivery path may partially grant a failed reward"
    );
}

#[test]
fn consumables_respect_bucket_capacity_and_existing_inventory_items() {
    let catalog = catalog();
    let mut document = document();
    document["_reward_context"]["inventory"] = json!([[77, 300, 8]]);
    queue(
        &mut document,
        &catalog,
        [(300, 2), (1000, 1), (1001, 1), (1002, 1), (1003, 1)],
    )
    .unwrap();
    let before = document.clone();
    let reason = queue(&mut document, &catalog, [(1004, 1)]).unwrap_err();
    assert!(reason.contains("No inventory space"));
    assert!(reason.contains("General inventory: 5/5 slots used, including planned rewards"));
    assert_eq!(document, before);
    assert!(queue(&mut document, &catalog, [(300, 1)]).is_err());
    assert_eq!(document, before);
}

#[test]
fn consumables_reject_invalid_duplicate_equipped_and_exhausted_inventory() {
    let catalog = catalog();
    for context in [
        json!({"consumables":[{"definition_hash":1000,"quantity":1}],"inventory":[[77,1000,1]]}),
        json!({"equipment":[[77,1000,1]]}),
        json!({"next_serial":i32::MAX}),
        json!({"consumables":[{"definition_hash":1000,"quantity":0}]}),
        json!({"consumables":(1000..1032).map(|hash|json!({"definition_hash":hash,"quantity":1})).collect::<Vec<_>>()}),
    ] {
        let mut document = document();
        document["_reward_context"]
            .as_object_mut()
            .unwrap()
            .extend(context.as_object().unwrap().clone());
        let before = document.clone();
        assert!(queue(&mut document, &catalog, [(1033, 1), (1000, 1)]).is_err());
        assert_eq!(document, before);
    }
    let mut legacy = json!({"version":8,"state":{"characters":[]}});
    let before = legacy.clone();
    assert!(queue(&mut legacy, &catalog, [(1000, 1)]).is_err());
    assert_eq!(legacy, before);
}

#[test]
fn consumables_and_pending_rewards_persist_together_for_the_selected_character() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = catalog();
    let directory = crate::test_support::TestDirectory::new("progression-consumables");
    let path = directory.0.join("settings.json");
    let settings = json!({"version":18,"future":17});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let dbpath = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&dbpath, 3);
    let db = rusqlite::Connection::open(&dbpath).unwrap();
    db.execute_batch("INSERT INTO characters SELECT 1,soid+1,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial FROM characters WHERE slot=0;
        ALTER TABLE character_stacks ADD COLUMN future TEXT NOT NULL DEFAULT 'new';
        INSERT INTO character_stacks VALUES(0,0,1000,8,3,'keep'),(1,0,1000,1,0,'other');").unwrap();
    let mut workspace = WorkspaceDocument::load(settings.clone(), &path, false);
    let mut view = workspace.progression_view(0);
    queue(
        &mut view,
        &catalog,
        [(9000, 2), (1000, 2), (1001, 3), (300, 1)],
    )
    .unwrap();
    let _ = super::super::mutations::set_unlock_flag(&mut view, "account_flag_runs", 200, true);
    let stale = view.clone();
    assert!(workspace.apply_progression_view(1, view.clone()).is_err());
    workspace.apply_progression_view(0, view).unwrap();
    assert_eq!(workspace.json(), &settings);
    let native = workspace.native_account().unwrap();
    assert_eq!(
        native
            .character_stacks(0)
            .iter()
            .map(|item| (item.definition_hash, item.quantity))
            .collect::<Vec<_>>(),
        vec![(1000, 10), (1001, 3)]
    );
    assert_eq!(native.character_stacks(1)[0].quantity, 1);
    assert_eq!(native.characters().characters()[0].inventory[0].quantity, 3);
    let next = native.progression_view(0)["_reward_context"]["next_serial"]
        .as_u64()
        .unwrap() as u32;
    assert_eq!(next, 7);
    assert!(
        native
            .character_stacks(0)
            .iter()
            .all(|item| (item.mutation_serial as u32) < next)
    );
    assert_eq!(native.pending_rewards().len(), 1);
    let after = workspace.progression_view(0);
    assert!(workspace.apply_progression_view(0, stale).is_err());
    assert_eq!(workspace.progression_view(0), after);
    crate::persistence::sqlite_account::tests::save_fixture_document(
        workspace.native_account_mut().unwrap(),
        &directory.0.join("backup.sqlite3"),
    );
    let reloaded = WorkspaceDocument::load(settings.clone(), &path, false);
    assert_eq!(reloaded.progression_view(0), after);
    let preserved: String = db
        .query_row(
            "SELECT future FROM character_stacks WHERE character_slot=0 AND position=0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, "keep");
    assert_eq!(reloaded.json(), &settings);
}

#[test]
fn direct_inventory_failure_rolls_back_pending_rewards_and_claim_flags() {
    use crate::app::account_workspace::WorkspaceDocument;
    let directory = crate::test_support::TestDirectory::new("progression-consumable-rollback");
    let path = directory.0.join("settings.json");
    let settings = json!({"version":18});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data/investment.sqlite3"),
        3,
    );
    let mut workspace = WorkspaceDocument::load(settings, &path, false);
    let before = workspace.progression_view(0);
    let mut view = before.clone();
    view["_progression_rewards"] = json!([{"kind":1,"hash":9000,"quantity":1}]);
    view["_progression_consumables"] =
        json!([{"hash":1000,"quantity":1,"maximum":10},{"hash":1001,"quantity":11,"maximum":10}]);
    let _ = super::super::mutations::set_unlock_flag(&mut view, "account_flag_runs", 200, true);
    assert!(workspace.apply_progression_view(0, view).is_err());
    assert_eq!(workspace.progression_view(0), before);
}
