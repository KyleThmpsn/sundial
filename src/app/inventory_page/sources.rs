//! Reads used for capacity decisions must distinguish a failed read from an empty list.
use crate::app::{
    account_workspace::{self as account, WorkspaceDocument},
    equipment::EquippedItemSnapshot,
    inventory::InventoryItemSnapshot,
};

pub(super) struct CharacterItems {
    pub stored: Vec<InventoryItemSnapshot>,
    pub equipped: Vec<EquippedItemSnapshot>,
}

pub(super) fn character_items(
    document: &WorkspaceDocument,
    index: usize,
) -> Result<CharacterItems, String> {
    let stored = account::character_inventory(document, index)
        .map_err(|error| format!("Stored inventory could not be read: {error}"))?
        .unwrap_or_default();
    let equipped = account::equipped_item_snapshots(document, index)
        .map_err(|error| format!("Equipped items could not be read: {error}"))?;
    Ok(CharacterItems { stored, equipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn empty_inventory_and_failed_reads_remain_distinct() {
        let mut json: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"
        ))
        .unwrap();
        json["state"]["characters"][0]["inventory"] = json!([]);
        json["state"]["characters"][0]["equipment"] = json!({});
        let empty = WorkspaceDocument::json_only(json.clone());
        let items = character_items(&empty, 0).unwrap();
        assert!(items.stored.is_empty());
        for field in ["inventory", "equipment"] {
            let mut broken = json.clone();
            broken["state"]["characters"][0][field] = json!("malformed");
            let error = character_items(&WorkspaceDocument::json_only(broken), 0)
                .err()
                .expect("A malformed collection must not become an empty one");
            assert!(error.contains(field), "{error}");
            assert!(error.contains("could not be read"), "{error}");
        }
    }
}
