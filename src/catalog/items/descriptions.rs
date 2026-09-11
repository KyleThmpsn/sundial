//! Shared localized effect descriptions for socket plugs and inventory mods.
use std::collections::HashMap;
use tiger_pkg::{PackageManager, TagHash};

use super::{InventoryMetadata, InventoryScope, ItemPackageMetadata, perks::item_perk_indices};
use crate::{
    investment_localization::{LocalizedStringCache, resolve_string},
    investment_schema::{GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, investment_globals_table_tag},
    sandbox_perk::{
        FINISHED_SANDBOX_PERK_CATALOG_CLASS, finished_sandbox_perk_at, finished_sandbox_perk_count,
    },
};

pub(super) fn mod_description(
    item: &[u8],
    package: Option<&ItemPackageMetadata>,
    inventory: Option<InventoryMetadata>,
    descriptions: &HashMap<u16, String>,
) -> Option<String> {
    let is_mod = package.is_some_and(|metadata| metadata.plug_category_hash.is_some())
        || inventory.is_some_and(|metadata| {
            matches!(metadata.scope, InventoryScope::Profile) && metadata.native_bucket_id == 13
        });
    is_mod
        .then(|| item_perk_description(item, descriptions))
        .flatten()
}

/// Display descriptions also belong to perk rows omitted by the runtime-liveness filter.
pub(in crate::catalog) fn scan_perk_descriptions(
    manager: &PackageManager,
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
) -> Result<HashMap<u16, String>, String> {
    let tag = TagHash(investment_globals_table_tag(
        globals,
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT,
    )?);
    if manager
        .get_entry(tag)
        .is_none_or(|entry| entry.reference != FINISHED_SANDBOX_PERK_CATALOG_CLASS)
    {
        return Err("Finished perk descriptions have an invalid catalog class".into());
    }
    let data = manager
        .read_tag(tag)
        .map_err(|error| format!("Could not read perk descriptions: {error}"))?;
    let count = finished_sandbox_perk_count(&data)?;
    let mut descriptions = HashMap::new();
    for index in 0..count {
        let Ok(index) = u16::try_from(index) else {
            break;
        };
        let Some(detail) = finished_sandbox_perk_at(&data, usize::from(index))
            .ok()
            .and_then(|perk| perk.detail)
        else {
            continue;
        };
        if let Some(text) = resolve_string(manager, localized_tags, localized_cache, &detail, 8)
            .filter(|text| !text.trim().is_empty())
        {
            descriptions.insert(index, text);
        }
    }
    Ok(descriptions)
}

fn item_perk_description(item: &[u8], descriptions: &HashMap<u16, String>) -> Option<String> {
    let mut paragraphs = Vec::new();
    for index in item_perk_indices(item) {
        if let Some(text) = descriptions.get(&index).map(String::as_str)
            && !paragraphs.contains(&text)
        {
            paragraphs.push(text);
        }
    }
    (!paragraphs.is_empty()).then(|| paragraphs.join("\n\n"))
}
