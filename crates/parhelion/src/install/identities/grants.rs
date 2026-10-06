//! Native bucket facts for the authored items an install adds to the account.
use super::*;
use crate::subclass::{native, tables};
use std::collections::HashMap;
use sundial::package_authoring::account::{
    AuthoredGrantReport, AuthoredGrantTarget, AuthoredItemGrant, grant_authored_items,
};
use sundial::package_authoring::stock_subclass_list_classes;

/// The stack an authored shader arrives as, within its own stack limit.
pub(crate) const SHADER_STACK: i32 = 777;

/// An authored item the install adds to the account, as the manifest records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::install) struct GrantedItem {
    pub(in crate::install) item_hash: u32,
    pub(in crate::install) definition_tag: TagHash,
    /// The class of the characters that receive it, or `None` for a profile stack.
    pub(in crate::install) class_type: Option<u8>,
    /// A subclass every class's characters receive, equipped only on `class_type`'s.
    pub(in crate::install) every_class: bool,
}

/// The authored subclasses and shaders an installed generation holds, read from its native
/// item tables, so no manifest is needed. A subclass names its class through its socket-entry
/// list, as the catalog reads it: every list keeps its base's class-base entry, whose pool
/// differs by class, so the stock list sharing that pool names the class.
pub(in crate::install) fn installed_grants(
    target: &Path,
    hashes: &BTreeSet<u32>,
) -> Result<Vec<GrantedItem>, String> {
    with_generation(target, target, |generation| {
        let tables = Tables::open(&generation.manager)?;
        let mut pools = None;
        let mut grants = Vec::new();
        for (hash, definition_tag) in tables.item_rows()? {
            if !hashes.contains(&hash) {
                continue;
            }
            let (definition, kind) = tables.definition(hash, definition_tag)?;
            let class_type = match kind {
                Some(crate::ItemKind::Subclass) => {
                    Some(subclass_class(&tables, &mut pools, hash, &definition)?)
                }
                Some(crate::ItemKind::Shader) => None,
                _ => continue,
            };
            grants.push(GrantedItem {
                item_hash: hash,
                definition_tag,
                class_type,
                // Equip conditions persist the choice even when the manifest is unavailable.
                every_class: kind == Some(crate::ItemKind::Subclass)
                    && sundial::package_authoring::investment_schema::subclass_equipment_class(
                        &definition,
                    )
                    .is_ok_and(|class| class.is_none()),
            });
        }
        Ok(grants)
    })
}

/// The class an installed subclass is for: the class of the stock list whose class-base pool
/// its list keeps. The stock pools are read once, on the first subclass.
fn subclass_class(
    tables: &Tables<'_>,
    pools: &mut Option<BTreeMap<u32, u8>>,
    hash: u32,
    definition: &[u8],
) -> Result<u8, String> {
    let pool = tables.class_base_pool(definition)?;
    if pools.is_none() {
        *pools = Some(tables.stock_class_pools()?);
    }
    pools
        .as_ref()
        .and_then(|pools| pools.get(&pool).copied())
        .ok_or_else(|| {
            format!(
                "Installed subclass 0x{hash:08X} names no class: no stock subclass list shares its class-base pool 0x{pool:08X}"
            )
        })
}

/// The installed generation's item and socket-entry-list tables.
struct Tables<'a> {
    manager: &'a sundial::package_authoring::PackageManager,
    items: Vec<u8>,
    lists: Vec<u8>,
}

impl<'a> Tables<'a> {
    fn open(manager: &'a sundial::package_authoring::PackageManager) -> Result<Self, String> {
        let (root, items) = placement::tables(manager)?;
        let lists = manager
            .read_tag(TagHash(investment_root_table_tag(
                &root,
                ROOT_SOCKET_ENTRY_LIST_TABLE_SLOT,
            )?))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            manager,
            items,
            lists,
        })
    }

    fn read(&self, tag: TagHash) -> Result<Vec<u8>, String> {
        self.manager.read_tag(tag).map_err(|e| e.to_string())
    }

    /// Each item's hash and definition tag, in table order.
    fn item_rows(&self) -> Result<Vec<(u32, TagHash)>, String> {
        rows(
            &self.items,
            ITEM_DEFINITION_INDEX_ROW_CLASS,
            ITEM_INDEX_ROW_SIZE,
        )?
        .iter()
        .map(|row| {
            Ok((
                read_u32(row, 0).map_err(|e| e.to_string())?,
                TagHash(read_u32(row, 16).map_err(|e| e.to_string())?),
            ))
        })
        .collect()
    }

    /// An item's definition, checked to carry its hash, with the kind whose bucket it holds.
    fn definition(
        &self,
        hash: u32,
        tag: TagHash,
    ) -> Result<(Vec<u8>, Option<crate::ItemKind>), String> {
        let definition = self.read(tag)?;
        if read_u32(&definition, ITEM_DEFINITION_HASH_OFFSET).map_err(|e| e.to_string())? != hash {
            return Err(format!(
                "Installed item 0x{hash:08X} has an inconsistent definition"
            ));
        }
        let bucket = definition
            .get(ITEM_INVENTORY_SLOT_OFFSET)
            .copied()
            .ok_or_else(|| format!("Installed item 0x{hash:08X} has no inventory bucket"))?;
        let kind = [crate::ItemKind::Subclass, crate::ItemKind::Shader]
            .into_iter()
            .find(|kind| kind.native_slots().iter().any(|(slot, _)| *slot == bucket));
        Ok((definition, kind))
    }

    /// The pool of the class-base entry in the socket-entry list at `index`.
    fn list_class_base_pool(&self, index: u16) -> Result<u32, String> {
        let tag = tables::list_tag(&self.lists, index).map_err(|e| e.to_string())?;
        native::class_base_pool(&self.read(tag)?).map_err(|e| e.to_string())
    }

    /// The pool of the class-base entry in a subclass definition's socket-entry list.
    fn class_base_pool(&self, definition: &[u8]) -> Result<u32, String> {
        let index = native::list_index(definition).map_err(|e| e.to_string())?;
        self.list_class_base_pool(index)
    }

    /// The class each class-base pool belongs to, read from the stock subclass lists.
    fn stock_class_pools(&self) -> Result<BTreeMap<u32, u8>, String> {
        let mut pools = BTreeMap::new();
        for (index, class) in stock_subclass_list_classes() {
            let pool = self.list_class_base_pool(index)?;
            if pools
                .insert(pool, class)
                .is_some_and(|known| known != class)
            {
                return Err(format!(
                    "Stock subclass lists of two classes share class-base pool 0x{pool:08X}"
                ));
            }
        }
        Ok(pools)
    }
}

/// Reads each item's bucket from the installed generation, then adds what the account lacks.
pub(in crate::install) fn grant_items(
    game_root: &Path,
    target: &Path,
    items: &[GrantedItem],
) -> Result<AuthoredGrantReport, String> {
    with_generation(target, target, |generation| {
        let manager = &generation.manager;
        let (root, table) = placement::tables(manager)?;
        let definition = |hash: u32, tag: TagHash| -> Result<Vec<u8>, String> {
            let item = manager.read_tag(tag).map_err(|e| e.to_string())?;
            if read_u32(&item, ITEM_DEFINITION_HASH_OFFSET).map_err(|e| e.to_string())? != hash {
                return Err(format!("Item 0x{hash:08X} has an inconsistent definition"));
            }
            Ok(item)
        };
        let bucket = |item: &[u8], hash: u32| {
            item.get(ITEM_INVENTORY_SLOT_OFFSET)
                .copied()
                .ok_or_else(|| format!("Item 0x{hash:08X} has no inventory bucket"))
        };
        // A character equips the project's first subclass for its class. A subclass for every
        // class reaches the other classes' characters too, unequipped, since another class
        // using it is untested.
        let mut equipping = BTreeSet::new();
        let mut grants = Vec::new();
        for item in items {
            let payload = definition(item.item_hash, item.definition_tag)?;
            let targets = match item.class_type {
                Some(class_type) if item.every_class => (0..3)
                    .map(|class| AuthoredGrantTarget::Class {
                        class_type: class,
                        equip: class == class_type && equipping.insert(class),
                    })
                    .collect(),
                Some(class_type) => vec![AuthoredGrantTarget::Class {
                    class_type,
                    equip: equipping.insert(class_type),
                }],
                None => {
                    let limit = read_u32(&payload, ITEM_MAX_STACK_SIZE_OFFSET)
                        .map_err(|e| e.to_string())?;
                    let limit = i32::try_from(limit).unwrap_or(i32::MAX);
                    vec![AuthoredGrantTarget::Profile(SHADER_STACK.min(limit).max(1))]
                }
            };
            let bucket = bucket(&payload, item.item_hash)?;
            let capacity = sundial::package_authoring::inventory_bucket_capacity(
                manager,
                &root,
                bucket,
                item.class_type.is_none(),
            )?;
            grants.extend(targets.into_iter().map(|target| AuthoredItemGrant {
                item_hash: item.item_hash,
                bucket,
                capacity,
                target,
            }));
        }
        // The account's other definitions, each read once and only when counted.
        let mut tags: Option<BTreeMap<u32, TagHash>> = None;
        let mut known = HashMap::<u32, Option<u8>>::new();
        grant_authored_items(game_root, &grants, &mut |hash| {
            if let Some(found) = known.get(&hash) {
                return Ok(*found);
            }
            if tags.is_none() {
                tags = Some(definition_tags(&table)?);
            }
            let found = match tags.as_ref().and_then(|tags| tags.get(&hash)) {
                Some(tag) => Some(bucket(&definition(hash, *tag)?, hash)?),
                None => None,
            };
            known.insert(hash, found);
            Ok(found)
        })
    })
}

fn definition_tags(table: &[u8]) -> Result<BTreeMap<u32, TagHash>, String> {
    rows(table, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE)?
        .into_iter()
        .map(|row| {
            Ok((
                read_u32(row, 0).map_err(|e| e.to_string())?,
                TagHash(read_u32(row, 16).map_err(|e| e.to_string())?),
            ))
        })
        .collect()
}
