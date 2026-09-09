use super::*;
use serde_json::json;

fn catalog_and_document() -> (Catalog, WorkspaceDocument) {
    let catalog = Catalog::for_test_with_inventory(
        vec![definition(10, 1498876634, 1)],
        HashMap::new(),
        HashMap::from([(10, character_metadata(1)), (20, profile_metadata(10))]),
    );
    let document = WorkspaceDocument::json_only(json!({
        "version": 16,
        "state": {
            "account": {"profile_items": [{"definition_hash": 20, "quantity": 11}]},
            "characters": [{
                "soid": 2, "class": 0, "race": 0, "gender": 0, "equipment": {},
                "inventory": [{
                    "instance_soid": 3, "definition_hash": 10,
                    "level": 106, "quantity": 1, "plugs": null,
                }],
            }],
        },
    }));
    (catalog, document)
}

#[test]
fn duplicating_invalid_inventory_rows_does_not_inherit_the_baseline_exemption() {
    let (catalog, before) = catalog_and_document();
    assert!(validate_new_account_catalog_issues(&before, &before, &catalog, false).is_ok());
    for path in [
        "/state/account/profile_items",
        "/state/characters/0/inventory",
    ] {
        let mut candidate = before.clone();
        let rows = candidate
            .json_mut()
            .pointer_mut(path)
            .unwrap()
            .as_array_mut()
            .unwrap();
        let mut duplicate = rows[0].clone();
        if duplicate.get("instance_soid").is_some() {
            duplicate["instance_soid"] = json!(4);
        }
        rows.push(duplicate);
        let error = validate_new_account_catalog_issues(&candidate, &before, &catalog, false)
            .expect_err("an existing bad row must not excuse another bad row");
        assert!(error.contains("catalog"), "{error}");
    }
}

#[test]
fn removing_invalid_rows_and_editing_unrelated_fields_remain_allowed() {
    let (catalog, before) = catalog_and_document();
    let mut candidate = before.clone();
    candidate.json_mut()["state"]["account"]["future_setting"] = json!({"preserve": true});
    assert!(validate_new_account_catalog_issues(&candidate, &before, &catalog, false).is_ok());
    candidate.json_mut()["state"]["account"]["profile_items"] = json!([]);
    candidate.json_mut()["state"]["characters"][0]["inventory"] = json!([]);
    assert!(validate_new_account_catalog_issues(&candidate, &before, &catalog, false).is_ok());
}

#[test]
fn reordering_rows_preserves_exemptions_but_another_invalid_quantity_does_not() {
    let (catalog, mut before) = catalog_and_document();
    before.json_mut()["state"]["account"]["profile_items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"definition_hash": 20, "quantity": 5}));
    let mut candidate = before.clone();
    candidate.json_mut()["state"]["account"]["profile_items"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert!(validate_new_account_catalog_issues(&candidate, &before, &catalog, false).is_ok());
    candidate.json_mut()["state"]["account"]["profile_items"][0]["quantity"] = json!(11);
    assert!(validate_new_account_catalog_issues(&candidate, &before, &catalog, false).is_err());
}
