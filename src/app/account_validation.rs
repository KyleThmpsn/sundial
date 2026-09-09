use crate::app::account_workspace as account;

mod capacity;
pub(super) use capacity::{apply_with_bucket_limits, validate_new_bucket_overflows};

use std::collections::HashMap;

use crate::catalog::{Catalog, InventoryMetadata, ItemDef};

use super::{
    account_workspace::WorkspaceDocument,
    equipment::{self, EquippedItemPlugs, EquippedPlugValue},
    inventory::ItemPlugs,
};

trait AccountCatalog {
    fn item(&self, hash: u64) -> Option<&ItemDef>;
    fn inventory_metadata(&self, hash: u64) -> Option<&InventoryMetadata>;
    fn contains_plug(&self, hash: u64) -> bool;
}

impl AccountCatalog for Catalog {
    fn item(&self, hash: u64) -> Option<&ItemDef> {
        self.item(hash)
    }

    fn inventory_metadata(&self, hash: u64) -> Option<&InventoryMetadata> {
        self.inventory_metadata(hash)
    }

    fn contains_plug(&self, hash: u64) -> bool {
        self.contains_plug(hash)
    }
}

/// Rejects catalog-reference problems introduced by the edited account.
///
/// Existing problems remain saveable so an incomplete package scan or a legacy
/// unsupported definition cannot lock the user out of otherwise safe edits.
pub(super) fn validate_new_account_catalog_issues(
    candidate: &WorkspaceDocument,
    persisted: &WorkspaceDocument,
    catalog: &Catalog,
    allow_cross_class_subclasses: bool,
) -> Result<(), String> {
    validate_new_bucket_overflows(candidate, persisted, catalog)?;
    validate_new_issues(
        collect_catalog_issues(candidate, catalog, allow_cross_class_subclasses),
        collect_catalog_issues(persisted, catalog, allow_cross_class_subclasses),
    )
}

fn validate_new_issues(candidate: Vec<String>, persisted: Vec<String>) -> Result<(), String> {
    let mut baseline_counts = HashMap::<String, usize>::new();
    for issue in persisted {
        *baseline_counts.entry(issue).or_default() += 1;
    }
    let mut new_issues = Vec::new();
    for issue in candidate {
        if let Some(count) = baseline_counts.get_mut(&issue).filter(|count| **count > 0) {
            *count -= 1;
        } else {
            new_issues.push(issue);
        }
    }
    new_issues.sort();
    let Some(first) = new_issues.first() else {
        return Ok(());
    };
    let remaining = new_issues.len().saturating_sub(1);
    let suffix = if remaining == 0 {
        String::new()
    } else {
        format!(" (and {remaining} more new catalog-reference issue(s))")
    };
    Err(format!(
        "account data is incompatible with the installed catalog: {first}{suffix}"
    ))
}

fn collect_catalog_issues<C: AccountCatalog>(
    document: &WorkspaceDocument,
    catalog: &C,
    allow_cross_class_subclasses: bool,
) -> Vec<String> {
    let mut issues = Vec::new();

    match account::profile_items(document) {
        Ok(Some(items)) => {
            for item in &items {
                let context = format!("profile item 0x{:08X}", item.definition_hash);
                validate_profile_item(
                    catalog,
                    &context,
                    u64::from(item.definition_hash),
                    i64::from(item.quantity),
                    &mut issues,
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            issues.push(format!("profile items could not be read: {error}"));
        }
    }

    for character_index in 0..account::character_count(document) {
        let character_number = character_index + 1;
        let class_type = match account::character_metadata(document, character_index) {
            Ok(metadata) => Some(metadata.class_type),
            Err(error) => {
                issues.push(format!(
                    "character {character_number} metadata could not be read: {error}"
                ));
                None
            }
        };

        match account::character_inventory(document, character_index) {
            Ok(Some(items)) => {
                for item in &items {
                    let context = format!(
                        "character {character_number} inventory item 0x{:08X}",
                        item.definition_hash,
                    );
                    validate_character_item(
                        catalog,
                        CharacterItemReference {
                            context: &context,
                            hash: u64::from(item.definition_hash),
                            quantity: i64::from(item.quantity),
                            class_type,
                            expected_bucket: None,
                        },
                        allow_cross_class_subclasses,
                        &mut issues,
                    );
                    validate_inventory_plugs(
                        catalog,
                        &context,
                        u64::from(item.definition_hash),
                        &item.plugs,
                        &mut issues,
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                issues.push(format!(
                    "character {character_number} inventory could not be read: {error}"
                ));
            }
        }

        match account::equipped_item_snapshots(document, character_index) {
            Ok(items) => {
                for item in items {
                    let context = format!(
                        "character {character_number} equipped {}",
                        item.slot_label.to_lowercase()
                    );
                    if !item.issues.is_empty() {
                        issues.push(format!(
                            "{context} is malformed: {}",
                            item.issues.join(", ")
                        ));
                        continue;
                    }
                    let Some(hash) = item.definition_hash else {
                        issues.push(format!("{context} has no definition hash"));
                        continue;
                    };
                    let Some(quantity) = item.quantity else {
                        issues.push(format!("{context} has no quantity"));
                        continue;
                    };
                    validate_character_item(
                        catalog,
                        CharacterItemReference {
                            context: &format!("{context} (0x{hash:08X})"),
                            hash,
                            quantity,
                            class_type,
                            expected_bucket: Some(item.bucket_hash),
                        },
                        allow_cross_class_subclasses,
                        &mut issues,
                    );
                    validate_equipped_plugs(catalog, &context, hash, &item.plugs, &mut issues);
                }
            }
            Err(error) => {
                issues.push(format!(
                    "character {character_number} equipment could not be read: {error}"
                ));
            }
        }
    }

    issues
}

fn validate_profile_item<C: AccountCatalog>(
    catalog: &C,
    context: &str,
    hash: u64,
    quantity: i64,
    issues: &mut Vec<String>,
) {
    let Some(metadata) = catalog.inventory_metadata(hash) else {
        issues.push(format!(
            "{context} is not present in the installed inventory catalog"
        ));
        return;
    };
    if !metadata.is_profile_items_candidate() {
        issues.push(format!(
            "{context} is not valid for the profile-items inventory"
        ));
        return;
    }
    validate_quantity(context, quantity, metadata.max_stack_size, issues);
}

#[derive(Clone, Copy)]
struct CharacterItemReference<'a> {
    context: &'a str,
    hash: u64,
    quantity: i64,
    class_type: Option<u8>,
    expected_bucket: Option<u64>,
}

fn validate_character_item<C: AccountCatalog>(
    catalog: &C,
    reference: CharacterItemReference<'_>,
    allow_cross_class_subclasses: bool,
    issues: &mut Vec<String>,
) {
    let CharacterItemReference {
        context,
        hash,
        quantity,
        class_type,
        expected_bucket,
    } = reference;
    let Some(item) = catalog.item(hash) else {
        issues.push(format!(
            "{context} is not present in the installed item catalog"
        ));
        return;
    };
    let Some(metadata) = catalog.inventory_metadata(hash) else {
        issues.push(format!(
            "{context} has no decoded installed inventory definition"
        ));
        return;
    };
    if !metadata.is_character_inventory_candidate() {
        issues.push(format!("{context} is not valid for character inventory"));
        return;
    }

    validate_quantity(context, quantity, metadata.max_stack_size, issues);
    if let Some(class_type) = class_type
        && !equipment::item_class_is_compatible(
            item,
            u64::from(class_type),
            allow_cross_class_subclasses,
        )
    {
        issues.push(format!(
            "{context} is class {} but the character is class {class_type}",
            item.class_type
        ));
    }
    if let Some(expected_bucket) = expected_bucket
        && item.bucket_hash != expected_bucket
    {
        issues.push(format!(
            "{context} belongs to bucket 0x{:08X}, not the equipped slot bucket 0x{expected_bucket:08X}",
            item.bucket_hash
        ));
    }
}

fn validate_quantity(context: &str, quantity: i64, maximum: Option<u32>, issues: &mut Vec<String>) {
    let Ok(quantity) = u32::try_from(quantity) else {
        issues.push(format!("{context} has an invalid quantity of {quantity}"));
        return;
    };
    if quantity == 0 {
        issues.push(format!("{context} has an invalid quantity of 0"));
    } else if let Some(maximum) = maximum
        && quantity > maximum
    {
        issues.push(format!(
            "{context} quantity {quantity} exceeds the installed maximum of {maximum}"
        ));
    }
}

fn validate_inventory_plugs<C: AccountCatalog>(
    catalog: &C,
    context: &str,
    item_hash: u64,
    plugs: &ItemPlugs,
    issues: &mut Vec<String>,
) {
    if let ItemPlugs::Authored(plugs) = plugs {
        validate_socket_count(catalog, context, item_hash, plugs.len(), issues);
        for (index, hash) in plugs.iter().enumerate() {
            if let Some(hash) = hash {
                validate_plug(catalog, context, index, u64::from(*hash), issues);
            }
        }
    }
}

fn validate_equipped_plugs<C: AccountCatalog>(
    catalog: &C,
    context: &str,
    item_hash: u64,
    plugs: &EquippedItemPlugs,
    issues: &mut Vec<String>,
) {
    match plugs {
        EquippedItemPlugs::NativeDefaults => {}
        EquippedItemPlugs::Authored(plugs) => {
            validate_socket_count(catalog, context, item_hash, plugs.len(), issues);
            for (index, value) in plugs.iter().enumerate() {
                match value {
                    EquippedPlugValue::Empty => {}
                    EquippedPlugValue::Hash(hash) => {
                        validate_plug(catalog, context, index, *hash, issues);
                    }
                    EquippedPlugValue::Malformed(value) => {
                        issues.push(format!(
                            "{context} plug {} is malformed: {value}",
                            index + 1
                        ));
                    }
                }
            }
        }
        EquippedItemPlugs::Missing => {
            issues.push(format!("{context} has no plug selection"));
        }
        EquippedItemPlugs::Malformed(value) => {
            issues.push(format!("{context} plug selection is malformed: {value}"));
        }
    }
}

fn validate_socket_count<C: AccountCatalog>(
    catalog: &C,
    context: &str,
    item_hash: u64,
    authored_count: usize,
    issues: &mut Vec<String>,
) {
    if let Some(item) = catalog.item(item_hash) {
        let expected = item.default_plugs.len();
        if authored_count != expected {
            issues.push(format!(
                "{context} has {authored_count} authored socket entries but the installed item requires {expected}. Restore default plugs or use exactly {expected} entries",
            ));
        }
    }
}

fn validate_plug<C: AccountCatalog>(
    catalog: &C,
    context: &str,
    index: usize,
    hash: u64,
    issues: &mut Vec<String>,
) {
    if !catalog.contains_plug(hash) {
        issues.push(format!(
            "{context} plug {} (0x{hash:08X}) is not present in the installed plug catalog",
            index + 1
        ));
    }
}

#[cfg(test)]
mod tests {
    mod baseline;

    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::catalog::{AbilityOptions, InventoryScope, ItemStackability};

    #[derive(Default)]
    struct FakeCatalog {
        items: HashMap<u64, ItemDef>,
        inventory: HashMap<u64, InventoryMetadata>,
        plugs: HashSet<u64>,
    }

    impl AccountCatalog for FakeCatalog {
        fn item(&self, hash: u64) -> Option<&ItemDef> {
            self.items.get(&hash)
        }

        fn inventory_metadata(&self, hash: u64) -> Option<&InventoryMetadata> {
            self.inventory.get(&hash)
        }

        fn contains_plug(&self, hash: u64) -> bool {
            self.plugs.contains(&hash)
        }
    }

    fn definition(hash: u64, bucket_hash: u64, class_type: u64) -> ItemDef {
        ItemDef {
            hash,
            name: format!("Item {hash}"),
            type_name: String::new(),
            bucket_hash,
            class_type,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: AbilityOptions::default(),
        }
    }

    fn character_metadata(max_stack_size: u32) -> InventoryMetadata {
        InventoryMetadata {
            scope: InventoryScope::Character,
            native_bucket_id: 0,
            stackability: ItemStackability::Instanced,
            max_stack_size: Some(max_stack_size),
            bucket_capacity: Some(10),
        }
    }

    fn profile_metadata(max_stack_size: u32) -> InventoryMetadata {
        InventoryMetadata {
            scope: InventoryScope::Profile,
            native_bucket_id: 0,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(max_stack_size),
            bucket_capacity: Some(10),
        }
    }

    #[test]
    fn character_reference_checks_item_class_stack_and_equipped_bucket() {
        let hash = 10;
        let expected_bucket = 20;
        let mut catalog = FakeCatalog::default();
        catalog
            .items
            .insert(hash, definition(hash, expected_bucket, 0));
        catalog.inventory.insert(hash, character_metadata(1));

        let mut issues = Vec::new();
        validate_character_item(
            &catalog,
            CharacterItemReference {
                context: "test item",
                hash,
                quantity: 1,
                class_type: Some(0),
                expected_bucket: Some(expected_bucket),
            },
            false,
            &mut issues,
        );
        assert!(issues.is_empty());

        validate_character_item(
            &catalog,
            CharacterItemReference {
                context: "bad item",
                hash,
                quantity: 2,
                class_type: Some(2),
                expected_bucket: Some(30),
            },
            false,
            &mut issues,
        );
        assert!(issues.iter().any(|issue| issue.contains("quantity 2")));
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("character is class 2"))
        );
        assert!(issues.iter().any(|issue| issue.contains("slot bucket")));
    }

    #[test]
    fn baseline_catalog_issues_do_not_block_unrelated_edits() {
        let persisted = Vec::from(["existing unsupported definition".to_owned()]);
        let candidate = persisted.clone();
        assert!(validate_new_issues(candidate, persisted).is_ok());

        let candidate = Vec::from([
            "existing unsupported definition".to_owned(),
            "new bad definition".to_owned(),
        ]);
        let persisted = Vec::from(["existing unsupported definition".to_owned()]);
        let error = validate_new_issues(candidate, persisted).unwrap_err();
        assert!(error.contains("new bad definition"));
    }

    #[test]
    fn profile_only_inventory_definitions_are_valid_catalog_references() {
        let hash = 5;
        let mut catalog = FakeCatalog::default();
        catalog.inventory.insert(hash, profile_metadata(999));
        let mut issues = Vec::new();

        validate_profile_item(&catalog, "profile-only item", hash, 1, &mut issues);

        assert!(issues.is_empty());
    }

    #[test]
    fn plug_references_use_the_plug_catalog_instead_of_the_equipment_catalog() {
        let plug_hash = 20;
        let item_hash = 30;
        let mut catalog = FakeCatalog::default();
        catalog.plugs.insert(plug_hash);
        catalog.items.insert(item_hash, definition(item_hash, 0, 3));
        let mut issues = Vec::new();

        validate_plug(&catalog, "valid plug", 0, plug_hash, &mut issues);
        assert!(issues.is_empty());

        validate_plug(&catalog, "item used as plug", 0, item_hash, &mut issues);
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("item used as plug"))
        );
    }

    #[test]
    fn character_and_plug_hashes_must_exist() {
        let catalog = FakeCatalog::default();
        let mut issues = Vec::new();
        validate_profile_item(&catalog, "missing profile item", 5, 1, &mut issues);
        validate_character_item(
            &catalog,
            CharacterItemReference {
                context: "missing item",
                hash: 10,
                quantity: 1,
                class_type: Some(0),
                expected_bucket: None,
            },
            false,
            &mut issues,
        );
        validate_plug(&catalog, "test item", 0, 20, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("missing profile item"))
        );
        assert!(issues.iter().any(|issue| issue.contains("missing item")));
        assert!(issues.iter().any(|issue| issue.contains("plug 1")));
    }

    #[test]
    fn authored_socket_count_must_match_installed_shape_for_equipped_and_stored_items() {
        let mut catalog = FakeCatalog::default();
        let mut item = definition(10, 0, 3);
        item.default_plugs = vec![None; 4];
        catalog.items.insert(10, item);
        for count in [0, 3, 4, 5] {
            let mut stored_issues = Vec::new();
            validate_inventory_plugs(
                &catalog,
                "stored item",
                10,
                &ItemPlugs::Authored(vec![None; count]),
                &mut stored_issues,
            );
            let mut equipped_issues = Vec::new();
            validate_equipped_plugs(
                &catalog,
                "equipped item",
                10,
                &EquippedItemPlugs::Authored(vec![EquippedPlugValue::Empty; count]),
                &mut equipped_issues,
            );
            assert_eq!(stored_issues.is_empty(), count == 4);
            assert_eq!(equipped_issues.is_empty(), count == 4);
        }
        let mut issues = Vec::new();
        validate_inventory_plugs(
            &catalog,
            "stored item",
            10,
            &ItemPlugs::NativeDefaults,
            &mut issues,
        );
        validate_equipped_plugs(
            &catalog,
            "equipped item",
            10,
            &EquippedItemPlugs::NativeDefaults,
            &mut issues,
        );
        assert!(issues.is_empty());

        catalog.items.get_mut(&10).unwrap().default_plugs.clear();
        validate_inventory_plugs(
            &catalog,
            "socketless item",
            10,
            &ItemPlugs::Authored(vec![]),
            &mut issues,
        );
        assert!(issues.is_empty());
        validate_inventory_plugs(
            &catalog,
            "socketless item",
            10,
            &ItemPlugs::Authored(vec![None]),
            &mut issues,
        );
        assert!(issues.iter().any(|issue| issue.contains("requires 0")));
    }
}
