//! Private gear-art metadata rows and the assignment map that resolves their keys.
use super::*;
use crate::tag_payload::{read_u64, relative_target, write_i64, write_u64};

/// The stock assignment map: gear-art key to relation tag.
pub(super) const ASSIGNMENT_TABLE: TagHash = TagHash(0x80EC3F61);

fn array(data: &[u8], at: usize) -> AuthoringResult<(usize, usize, usize, u32)> {
    sundial::package_authoring::native_payload::native_array_at(data, at).map_err(invalid)
}
fn point(data: &mut [u8], at: usize, target: usize) -> AuthoringResult<()> {
    write_i64(data, at, target as i64 - at as i64)
}
pub(super) fn append(data: &mut Vec<u8>, class: u32, count: usize, bytes: &[u8]) -> usize {
    let header = (data.len() + 19) & !15;
    data.resize(header - 4, 0);
    data.extend_from_slice(&0x80809FBDu32.to_le_bytes());
    data.extend_from_slice(&(count as u64).to_le_bytes());
    data.extend_from_slice(&(class as u64).to_le_bytes());
    data.extend_from_slice(bytes);
    header
}

const LAYOUT: KeyedAuxiliaryLayout = KeyedAuxiliaryLayout {
    row_size: ITEM_METADATA_ROW_SIZE,
    row_class: ITEM_METADATA_ROW_CLASS,
    nested_offset: ITEM_METADATA_NESTED_OFFSET,
    nested_class: ITEM_METADATA_NESTED_CLASS,
    secondary_class: Some(ITEM_METADATA_SECONDARY_CLASS),
    index_row_size: ITEM_METADATA_INDEX_ROW_SIZE,
    index_row_class: ITEM_METADATA_INDEX_ROW_CLASS,
    description: "private artwork metadata",
};

/// The metadata row owned by `item`, cloned from the donor row at `donor_row` when absent.
pub(super) fn prepare_row_at(
    emission: &mut PackageEmission,
    item: u32,
    donor_row: usize,
) -> AuthoringResult<usize> {
    let (count, _, rows, _) = array(&emission.item_metadata, 8)?;
    if donor_row >= count {
        return Err(invalid("Artwork donor row is outside the metadata table"));
    }
    let donor_hash = read_u32(&emission.item_metadata, rows + donor_row * 32)?;
    validate_keyed_auxiliary_structure(
        &emission.item_metadata,
        &emission.item_metadata_index,
        LAYOUT,
    )?;
    if !(0..count).any(|i| read_u32(&emission.item_metadata, rows + i * 32).ok() == Some(item)) {
        (emission.item_metadata, emission.item_metadata_index) = append_keyed_auxiliary_pair_at(
            std::mem::take(&mut emission.item_metadata),
            std::mem::take(&mut emission.item_metadata_index),
            donor_hash,
            item,
            donor_row,
            LAYOUT,
        )?;
    }
    let (count, _, rows, _) = array(&emission.item_metadata, 8)?;
    (0..count)
        .find(|i| read_u32(&emission.item_metadata, rows + i * 32).ok() == Some(item))
        .ok_or_else(|| invalid("Private artwork metadata missing after allocation"))
}

/// The metadata row owned by `item`, cloned from the row that `native_item` owns. A reissued
/// native item can own several rows; `source_key` then picks the one listing that key.
#[cfg(feature = "d2-model-importer")]
pub(super) fn prepare_row(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    item: u32,
    native_item: u32,
    source_key: u32,
) -> AuthoringResult<(usize, u32)> {
    let (count, _, rows, _) = array(&emission.item_metadata, 8)?;
    let direct = (0..count)
        .find(|i| read_u32(&emission.item_metadata, rows + i * 32).ok() == Some(native_item));
    let donor = if let Some(index) = direct {
        index
    } else {
        // Reissued weapons can select an older variant's artwork row by index.
        let (n, _, item_rows, _) = array(&emission.item_table, 8)?;
        let matches = (0..n)
            .filter(|i| {
                read_u32(&emission.item_table, item_rows + i * 24).ok() == Some(native_item)
            })
            .collect::<Vec<_>>();
        let [index] = matches.as_slice() else {
            return Err(invalid("Native artwork item missing or ambiguous"));
        };
        let tag = TagHash(read_u32(&emission.item_table, item_rows + index * 24 + 16)?);
        let definition = manager
            .read_tag(tag)
            .map_err(|error| invalid(error.to_string()))?;
        let (n, _, art_rows, class) = array(&definition, relative_target(&definition, 0x88)?)?;
        if class != 0x808077B5 {
            return Err(invalid("Native artwork selector class changed"));
        }
        let mut candidates = std::collections::BTreeSet::new();
        for i in 0..n {
            let index = crate::tag_payload::read_u16(&definition, art_rows + i * 4 + 2)? as usize;
            if index >= count {
                return Err(invalid("Native artwork selector is outside metadata"));
            }
            if row_keys(&emission.item_metadata, rows + index * 32)?.contains(&source_key) {
                candidates.insert(index);
            }
        }
        if candidates.len() != 1 {
            return Err(invalid(
                "Native artwork alias does not select one matching source assignment",
            ));
        }
        *candidates.first().unwrap()
    };
    let donor_hash = read_u32(&emission.item_metadata, rows + donor * 32)?;
    Ok((prepare_row_at(emission, item, donor)?, donor_hash))
}

/// Every assignment key a metadata row lists: both singles and every slot alternative.
pub(super) fn row_keys(data: &[u8], row: usize) -> AuthoringResult<Vec<u32>> {
    let mut keys = vec![read_u32(data, row + 8)?, read_u32(data, row + 12)?];
    if read_u64(data, row + 16)? != 0 {
        let (slots, _, entries, _) = array(data, row + 16)?;
        for slot in 0..slots {
            let resource = relative_target(data, entries + slot * 8)?;
            let (n, _, assignments, _) = array(data, resource + 8)?;
            for j in 0..n {
                keys.push(read_u32(data, assignments + j * 4)?);
            }
        }
    }
    keys.retain(|key| !matches!(*key, 0 | u32::MAX | 0x811C_9DC5));
    Ok(keys)
}

/// Give the row at `row` its own copy of the assignments of the donor row at `donor`,
/// rewritten by `policy`. The policy sees the two single keys and every slot as
/// `(selector, keys)`; it may change keys but not the layout.
pub(super) fn rewrite(
    data: &mut Vec<u8>,
    row: usize,
    donor: usize,
    policy: impl FnOnce(&mut [u32; 2], &mut Vec<(u64, Vec<u32>)>) -> AuthoringResult<()>,
) -> AuthoringResult<()> {
    let (count, _, rows, _) = array(data, 8)?;
    if donor < rows || (donor - rows) % 32 != 0 || (donor - rows) / 32 >= count {
        return Err(invalid("Artwork donor row offset is not a metadata row"));
    }
    let (n, _, multi, multi_class) = array(data, donor + 16)?;
    if n > 32 {
        return Err(invalid("Unsupported native slot count"));
    }
    let mut singles = [read_u32(data, donor + 8)?, read_u32(data, donor + 12)?];
    if n == 0 {
        policy(&mut singles, &mut Vec::new())?;
        for (i, value) in singles.into_iter().enumerate() {
            write_u32(data, row + 8 + i * 4, value)?;
        }
        data[row + 16..row + 32].fill(0);
        return Ok(());
    }
    let (resource_count, resource_header, resources, _) = array(data, 24)?;
    let end = resources + resource_count * 24;
    let extra = n * 24;
    let mut copies = Vec::new();
    let mut slots = Vec::new();
    for i in 0..n {
        let source = relative_target(data, multi + i * 8)?;
        if source < resources || source + 24 > end || (source - resources) % 24 != 0 {
            return Err(invalid("Artwork slot is outside the resource table"));
        }
        let (cnt, _, entries, class) = array(data, source + 8)?;
        let keys = (0..cnt)
            .map(|j| read_u32(data, entries + j * 4))
            .collect::<AuthoringResult<Vec<_>>>()?;
        copies.push((read_u64(data, source)?, cnt, class));
        slots.push((read_u64(data, source)?, keys));
    }
    policy(&mut singles, &mut slots)?;
    if slots.len() != n
        || slots
            .iter()
            .zip(&copies)
            .any(|(slot, copy)| slot.1.len() != copy.1)
    {
        return Err(invalid("Artwork policy changed the slot layout"));
    }
    // Insert new registered resource rows before the assignment arrays. Existing
    // multi-slot pointers keep their targets; each assignment-array pointer moves.
    let mut targets = Vec::new();
    // Earlier private rows append their multi-slot arrays after the resource table.
    // A subsequent insertion moves those arrays, including the pointer fields
    // inside them. Relocate both ends, not just the resource assignment arrays.
    for i in 0..count {
        let item = rows + i * 32;
        let (slots, header, entries, _) = array(data, item + 16)?;
        if slots == 0 {
            continue;
        }
        targets.push((item + 24, header));
        for slot in 0..slots {
            let at = entries + slot * 8;
            targets.push((at, relative_target(data, at)?));
        }
    }
    for i in 0..resource_count {
        let o = resources + i * 24;
        if read_u64(data, o + 8)? != 0 {
            let target = relative_target(data, o + 16)?;
            if target < end {
                return Err(invalid("Unexpected resource array placement"));
            }
            targets.push((o + 16, target));
        }
    }
    if multi >= end {
        return Err(invalid("Unexpected multiple-assignment placement"));
    }
    data.splice(end..end, std::iter::repeat_n(0, extra));
    let moved = |offset: usize| {
        if offset >= end {
            offset + extra
        } else {
            offset
        }
    };
    for (o, target) in targets {
        point(data, moved(o), moved(target))?;
    }
    write_u64(data, 24, (resource_count + n) as u64)?;
    write_u64(data, resource_header, (resource_count + n) as u64)?;
    let mh = append(data, multi_class, n, &vec![0; n * 8]);
    for (i, ((selector, cnt, class), (_, keys))) in copies.into_iter().zip(slots).enumerate() {
        let resource = end + i * 24;
        write_u64(data, resource, selector)?;
        write_u64(data, resource + 8, cnt as u64)?;
        let bytes = keys
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let ah = append(data, class, cnt, &bytes);
        point(data, resource + 16, ah)?;
        point(data, mh + 16 + i * 8, resource)?;
    }
    for (i, value) in singles.into_iter().enumerate() {
        write_u32(data, row + 8 + i * 4, value)?;
    }
    write_u64(data, row + 16, n as u64)?;
    point(data, row + 24, mh)?;
    let size = data.len();
    write_u64(data, 0, size as u64)?;
    Ok(())
}

/// The assignment map with `entries` added. `original` is the table as this build last left
/// it, so several private groups can extend it in one pass.
pub(super) fn insert_assignments(
    original: &[u8],
    entries: &[(u32, TagHash)],
) -> AuthoringResult<ReplacementSpec> {
    let (count, _, start, _) = array(original, 8)?;
    let mut rows = BTreeMap::new();
    for i in 0..count {
        let o = start + i * 8;
        rows.insert(read_u32(original, o)?, read_u32(original, o + 4)?);
    }
    for (key, tag) in entries {
        if matches!(*key, 0 | u32::MAX | 0x811C_9DC5) || rows.insert(*key, tag.0).is_some() {
            return Err(invalid(format!(
                "Private art assignment 0x{key:08X} collides with an existing key"
            )));
        }
    }
    let mut table = original[..start].to_vec();
    for (k, v) in &rows {
        table.extend_from_slice(&k.to_le_bytes());
        table.extend_from_slice(&v.to_le_bytes());
    }
    let size = table.len();
    write_u64(&mut table, 0, size as u64)?;
    write_u64(&mut table, 8, rows.len() as u64)?;
    let header = relative_target(&table, 16)?;
    write_u64(&mut table, header, rows.len() as u64)?;
    Ok(ReplacementSpec {
        tag: ASSIGNMENT_TABLE,
        payload: table,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rewritten_rows_keep_their_own_slot_copies() {
        let mut data = vec![0; 48];
        let ih = append(&mut data, 0x80805DFD, 3, &[0; 96]);
        write_u64(&mut data, 8, 3).unwrap();
        point(&mut data, 16, ih).unwrap();
        let rows = ih + 16;
        for (i, key) in [111, 222, 333].into_iter().enumerate() {
            write_u32(&mut data, rows + i * 32, key).unwrap();
        }
        let mh = append(&mut data, 0x80805DFE, 2, &[0; 16]);
        let rh = append(&mut data, 0x80805DFF, 2, &[0; 48]);
        write_u64(&mut data, 24, 2).unwrap();
        point(&mut data, 32, rh).unwrap();
        for (i, key) in [10u32, 20].into_iter().enumerate() {
            let resource = rh + 16 + i * 24;
            write_u64(&mut data, resource, i as u64).unwrap();
            write_u64(&mut data, resource + 8, 1).unwrap();
            let ah = append(&mut data, 0x80805E01, 1, &key.to_le_bytes());
            point(&mut data, resource + 16, ah).unwrap();
            point(&mut data, mh + 16 + i * 8, resource).unwrap();
        }
        write_u64(&mut data, rows + 16, 2).unwrap();
        point(&mut data, rows + 24, mh).unwrap();
        let substitute = |from: u32, to: u32| {
            move |_: &mut [u32; 2], slots: &mut Vec<(u64, Vec<u32>)>| {
                for (_, keys) in slots.iter_mut() {
                    for key in keys {
                        if *key == from {
                            *key = to;
                        }
                    }
                }
                Ok(())
            }
        };
        rewrite(&mut data, rows + 32, rows, substitute(20, 200)).unwrap();
        rewrite(&mut data, rows + 64, rows, substitute(10, 300)).unwrap();
        assert_eq!(row_keys(&data, rows).unwrap(), vec![10, 20]);
        assert_eq!(row_keys(&data, rows + 32).unwrap(), vec![10, 200]);
        assert_eq!(row_keys(&data, rows + 64).unwrap(), vec![300, 20]);
        let bad = rewrite(&mut data, rows + 64, rows, |_, slots| {
            slots.pop();
            Ok(())
        });
        assert!(bad.is_err());
    }

    #[test]
    fn assignment_map_grows_sorted_and_rejects_collisions() {
        let mut table = vec![0; 32];
        let header = append(&mut table, 0x808056EC, 1, &[5, 0, 0, 0, 9, 0, 0, 0]);
        write_u64(&mut table, 8, 1).unwrap();
        point(&mut table, 16, header).unwrap();
        let size = table.len();
        write_u64(&mut table, 0, size as u64).unwrap();
        let grown = insert_assignments(&table, &[(3, TagHash(0x8100_0001))]).unwrap();
        let (count, _, rows, _) = array(&grown.payload, 8).unwrap();
        assert_eq!(count, 2);
        assert_eq!(read_u32(&grown.payload, rows).unwrap(), 3);
        assert_eq!(read_u32(&grown.payload, rows + 8).unwrap(), 5);
        assert!(insert_assignments(&table, &[(5, TagHash(0x8100_0001))]).is_err());
        assert!(insert_assignments(&table, &[(0x811C_9DC5, TagHash(0x8100_0001))]).is_err());
    }
}
