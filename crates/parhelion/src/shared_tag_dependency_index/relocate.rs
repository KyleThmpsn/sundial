//! Relocate dependency membership without treating package indices or hashes as tag words.
use super::*;
use crate::tag_payload::write_u32;

fn destinations(
    original: &BTreeSet<(u16, u16)>,
    moves: &BTreeMap<u32, u32>,
) -> AuthoringResult<BTreeMap<(u16, u16), (u16, u16)>> {
    let mut relocated = BTreeMap::new();
    for (&old, &new) in moves {
        if old == new {
            continue;
        }
        let old = TagHash(old);
        let new = TagHash(new);
        for tag in [old, new] {
            if !(0x100..=crate::package_profile::MAX_AUTHORED_STANDALONE_PACKAGE_ID)
                .contains(&tag.pkg_id())
                || TagHash::new(tag.pkg_id(), tag.entry_index()) != tag
            {
                return Err(invalid(
                    "Loading-index relocation is not a canonical authored tag",
                ));
            }
        }
        let source = (old.pkg_id(), old.entry_index());
        if original.contains(&source) {
            relocated.insert(source, (new.pkg_id(), new.entry_index()));
        }
    }
    Ok(relocated)
}

fn remove(groups: &mut BTreeMap<u16, Group>, (package, entry): (u16, u16)) {
    let group = groups
        .get_mut(&package)
        .expect("validated dependency group");
    if let Some(word) = group.bitmap.get_mut(entry as usize / 32)
        && *word & (1 << (entry % 32)) != 0
    {
        *word &= !(1 << (entry % 32));
    } else {
        group.indices.retain(|&index| index != entry);
    }
    if group.indices.is_empty() && group.bitmap.iter().all(|&word| word == 0) {
        groups.remove(&package);
    }
}

fn insert(groups: &mut BTreeMap<u16, Group>, (package, entry): (u16, u16)) {
    let group = groups.entry(package).or_default();
    if group.bitmap.is_empty() {
        let position = group.indices.binary_search(&entry).unwrap_or_else(|p| p);
        group.indices.insert(position, entry);
    } else {
        group
            .bitmap
            .resize(group.bitmap.len().max(entry as usize / 32 + 1), 0);
        group.bitmap[entry as usize / 32] |= 1 << (entry % 32);
    }
}

pub(crate) fn remap(
    payload: &[u8],
    companion: TagHash,
    owner: TagHash,
    moves: &BTreeMap<u32, u32>,
) -> AuthoringResult<Vec<u8>> {
    let mut groups = parse(payload, companion, owner)?;
    let original = indexed_dependencies(&groups);
    if ![companion, owner]
        .iter()
        .all(|tag| original.contains(&(tag.pkg_id(), tag.entry_index())))
    {
        return Err(invalid(
            "Loading index does not include its companion and owner",
        ));
    }
    let relocated = destinations(&original, moves)?;
    if relocated.is_empty() {
        return Ok(payload.to_vec());
    }
    let expected = original
        .iter()
        .map(|entry| relocated.get(entry).copied().unwrap_or(*entry))
        .collect::<BTreeSet<_>>();
    if expected.len() != original.len() {
        return Err(invalid(
            "Loading-index relocation aliases distinct dependencies",
        ));
    }
    // Remove every old membership before inserting destinations, so swaps and overlapping
    // entry ranges cannot relocate a dependency twice or erase an incoming dependency.
    for &entry in relocated.keys() {
        remove(&mut groups, entry);
    }
    for &entry in relocated.values() {
        insert(&mut groups, entry);
    }
    // The encoder retains native tag64 values and unrelated package memberships.
    let mut authored = encode(payload, &groups)?;
    let companion = TagHash(moves.get(&companion.0).copied().unwrap_or(companion.0));
    let owner = TagHash(moves.get(&owner.0).copied().unwrap_or(owner.0));
    write_u32(&mut authored, 8, companion.0)?;
    write_u32(&mut authored, 12, owner.0)?;
    if indexed_dependencies(&parse(&authored, companion, owner)?) != expected {
        return Err(validation(
            "Relocated loading dependencies did not round-trip",
        ));
    }
    Ok(authored)
}
