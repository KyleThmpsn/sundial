//! Keep imported armor's rarity independent of its native geometry and runtime carrier.
use super::*;

pub(super) fn select(
    sources: &ProjectSources,
    spec: &WeaponCloneSpec,
    definition: &[u8],
    strings: &[u8],
    base: usize,
) -> AuthoringResult<usize> {
    let authored = spec.overrides.rarity.unwrap_or(rarity(definition)?);
    let exotic = authored == AuthoredWeaponRarity::Exotic;
    if !imported(spec)
        || spec.kind != ItemKind::Armor
        || exotic == (rarity(definition)? == AuthoredWeaponRarity::Exotic)
    {
        return Ok(base);
    }
    let class = read_u32(strings, ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET)?;
    let bucket = read_u8(definition, ITEM_INVENTORY_SLOT_OFFSET)?;
    let mut candidates = Vec::new();
    for index in 0..sources.stock_collectible_count {
        let row = sources.collectible_rows + index * COLLECTIBLE_ROW_SIZE;
        let item = usize::from(read_u16(
            &sources.stock_collectibles,
            row + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )?);
        if item >= sources.stock_item_count {
            continue;
        }
        let tag = read_u32(
            &sources.stock_item_table,
            sources.item_rows + item * ITEM_ROW_SIZE + 16,
        )?;
        let candidate = read_tag(&sources.manager, TagHash(tag), "Armor Collections exemplar")?;
        let Ok((slot, _)) = native_slot(&candidate, ItemKind::Armor) else {
            continue;
        };
        if (rarity(&candidate)? == AuthoredWeaponRarity::Exotic) != exotic {
            continue;
        }
        let tag = read_u32(
            &sources.stock_item_strings,
            sources.string_rows + item * ITEM_ROW_SIZE + 16,
        )?;
        let candidate = read_tag(&sources.manager, TagHash(tag), "Armor Collections class")?;
        if read_u32(&candidate, ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET)? != class {
            continue;
        }
        // Exotic class items have no older slot exemplar. The same class's Exotic armor
        // page still provides valid acquisition and placement without donating gameplay.
        let parents = template_presentation_parents(
            &sources.stock_nodes,
            &sources.stock_collectibles,
            index,
        )?;
        if crate::progression::gear_collection_page(&sources.stock_nodes, &parents).is_ok() {
            candidates.push((slot != bucket, index));
        }
    }
    candidates.sort_unstable();
    candidates.first().map(|(_, index)| *index).ok_or_else(|| {
        invalid("No armor Collections entry matches the imported item's class and rarity")
    })
}
