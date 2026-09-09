//! Native socket defaults used to keep retained account instances aligned across generations.
use super::*;
use crate::tag_payload::{read_i64, relative_target};
use sundial::investment::MAX_WEAPON_SOCKETS;

pub(in crate::install) fn generation_socket_defaults(
    target: &Path,
    authored_directory: &Path,
    hashes: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, Vec<Option<u32>>>, String> {
    if hashes.is_empty() {
        return Ok(BTreeMap::new());
    }
    with_generation(target, authored_directory, |directory| {
        let manager = open_shadowkeep_package_manager(directory)?;
        let read = |tag| manager.read_tag(TagHash(tag)).map_err(|e| e.to_string());
        let globals = resolve_live_named_tag(&manager, "investment_globals", None)?;
        let globals = read(globals.0)?;
        let root = read(investment_globals_table_tag(&globals, 0)?)?;
        let table = read(investment_root_table_tag(
            &root,
            ROOT_ITEM_DEFINITION_TABLE_SLOT,
        )?)?;
        let rows = rows(&table, ITEM_DEFINITION_INDEX_ROW_CLASS, ITEM_INDEX_ROW_SIZE)?;
        let indices = rows
            .iter()
            .map(|row| read_u32(row, 0).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut result = BTreeMap::new();
        for (row, hash) in rows.iter().zip(&indices) {
            if !hashes.contains(hash) {
                continue;
            }
            let item = read(read_u32(row, 16).map_err(|e| e.to_string())?)?;
            if read_u32(&item, ITEM_DEFINITION_HASH_OFFSET).map_err(|e| e.to_string())? != *hash {
                return Err(format!(
                    "Authored item 0x{hash:08X} has an inconsistent native definition"
                ));
            }
            let defaults = decode_defaults(&item, &indices).map_err(|error| {
                format!("Cannot read native sockets for authored item 0x{hash:08X}: {error}")
            })?;
            if result.insert(*hash, defaults).is_some() {
                return Err(format!(
                    "Authored item 0x{hash:08X} has duplicate native definitions"
                ));
            }
        }
        if result.len() != hashes.len() {
            return Err(
                "A retained authored definition is missing from its native generation".into(),
            );
        }
        Ok(result)
    })
}

fn decode_defaults(item: &[u8], indices: &[u32]) -> Result<Vec<Option<u32>>, String> {
    if read_i64(item, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).map_err(|e| e.to_string())? == 0 {
        return Ok(Vec::new());
    }
    let resource =
        relative_target(item, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).map_err(|e| e.to_string())?;
    let (count, _, rows, class) = array_at(item, resource).map_err(|e| e.to_string())?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS || count > MAX_WEAPON_SOCKETS {
        return Err("A retained authored item has unsupported native ordinary sockets".into());
    }
    let end = count
        .checked_mul(ITEM_ORDINARY_SOCKET_ROW_SIZE)
        .and_then(|size| rows.checked_add(size))
        .ok_or("Native socket rows overflowed")?;
    let rows = item
        .get(rows..end)
        .ok_or("Native socket rows are truncated")?;
    rows.chunks_exact(ITEM_ORDINARY_SOCKET_ROW_SIZE)
        .map(|row| {
            let index = read_u16(row, ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET)
                .map_err(|e| e.to_string())?;
            if index == u16::MAX {
                return Ok(None);
            }
            let hash = indices
                .get(usize::from(index))
                .copied()
                .filter(|hash| *hash != 0 && *hash != u32::MAX)
                .ok_or("A native socket default references an invalid item index")?;
            Ok(Some(hash))
        })
        .collect()
}

#[cfg(test)]
mod tests;
