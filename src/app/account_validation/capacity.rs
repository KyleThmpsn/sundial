//! Installed bucket limits count equipped and stored rows together.

use std::collections::BTreeMap;

use crate::catalog::{InventoryMetadata, InventoryScope};

use super::{AccountCatalog, WorkspaceDocument, account};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Profile,
    Character(usize),
}

struct Usage {
    rows: usize,
    metadata: InventoryMetadata,
}

type Buckets = BTreeMap<(Owner, u8), Usage>;

/// Commit an editor operation only if it does not introduce or worsen an overflow.
pub(in crate::app) fn apply_with_bucket_limits<T>(
    document: &mut WorkspaceDocument,
    catalog: &crate::catalog::Catalog,
    apply: impl FnOnce(&mut WorkspaceDocument) -> Result<T, String>,
) -> Result<T, String> {
    let mut candidate = document.clone();
    let result = apply(&mut candidate)?;
    validate_new_bucket_overflows(&candidate, document, catalog)?;
    *document = candidate;
    Ok(result)
}

pub(in crate::app) fn validate_new_bucket_overflows(
    candidate: &WorkspaceDocument,
    persisted: &WorkspaceDocument,
    catalog: &crate::catalog::Catalog,
) -> Result<(), String> {
    let before = collect(persisted, catalog);
    for (key, usage) in collect(candidate, catalog) {
        let Some(capacity) = usage.metadata.authored_row_capacity().map(usize::from) else {
            continue;
        };
        let previous = before.get(&key).map_or(0, |usage| usage.rows);
        // Allow unrelated edits and gradual repair of an existing overflow, but never increase it.
        if usage.rows <= capacity || usage.rows <= previous {
            continue;
        }
        let (owner, counting_note) = match key.0 {
            Owner::Profile => ("Profile".to_owned(), "Each stored stack occupies one slot."),
            Owner::Character(index) => (
                format!("Character {}", index + 1),
                "Equipped and stored items share this limit.",
            ),
        };
        return Err(format!(
            "{owner} {} contains {} items but the installed game allows only {capacity}. {counting_note} Remove or move {} items before saving",
            usage.metadata.bucket_label().to_lowercase(),
            usage.rows,
            usage.rows - capacity,
        ));
    }
    Ok(())
}

fn collect<C: AccountCatalog>(document: &WorkspaceDocument, catalog: &C) -> Buckets {
    let mut buckets = Buckets::new();
    if let Ok(Some(items)) = account::profile_items(document) {
        for item in items {
            count(
                &mut buckets,
                catalog,
                Owner::Profile,
                u64::from(item.definition_hash),
            );
        }
    }
    for index in 0..account::character_count(document) {
        let owner = Owner::Character(index);
        if let Some(native) = document.native_account() {
            for stack in native.character_stacks(index) {
                count(
                    &mut buckets,
                    catalog,
                    owner,
                    u64::from(stack.definition_hash),
                );
            }
        }
        if let Ok(Some(items)) = account::character_inventory(document, index) {
            for item in items {
                count(
                    &mut buckets,
                    catalog,
                    owner,
                    u64::from(item.definition_hash),
                );
            }
        }
        if let Ok(items) = account::equipped_item_snapshots(document, index) {
            for item in items {
                if let Some(hash) = item.definition_hash {
                    count(&mut buckets, catalog, owner, hash);
                }
            }
        }
    }
    buckets
}

fn count<C: AccountCatalog>(buckets: &mut Buckets, catalog: &C, owner: Owner, hash: u64) {
    let Some(&metadata) = catalog.inventory_metadata(hash) else {
        return;
    };
    let expected_scope = match owner {
        Owner::Profile => InventoryScope::Profile,
        Owner::Character(_) => InventoryScope::Character,
    };
    if metadata.scope == expected_scope {
        buckets
            .entry((owner, metadata.native_bucket_id))
            .or_insert(Usage { rows: 0, metadata })
            .rows += 1;
    }
}

#[cfg(test)]
mod tests;
