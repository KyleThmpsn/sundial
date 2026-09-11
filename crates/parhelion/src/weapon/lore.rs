use super::*;

const ROOT_SLOT: usize = 52;
const DISPLAY_SLOT: usize = 34;
const STOCK_COUNT: usize = 1425;
const DEFINITION_CLASS: u32 = 0x8080_77F4;
const DISPLAY_CLASS: u32 = 0x8080_5ABA;
const BLOCK_CLASS: u32 = 0x8080_77ED;
const BLOCK_POINTER: usize = 0x28;

pub(super) struct Plan {
    pub definitions: ReplacementSpec,
    pub strings: ReplacementSpec,
}

pub(super) fn author(
    manager: &PackageManager,
    globals: &[u8],
    weapons: &[WeaponCloneSpec],
    items: &mut [NewTagSpec],
    collectibles: &mut [u8],
    placements: &[ProjectAuthoredRow],
) -> AuthoringResult<Option<Plan>> {
    if weapons.iter().all(|weapon| weapon.overrides.lore.is_none()) {
        return Ok(None);
    }
    if items.len() < weapons.len() || placements.len() != weapons.len() {
        return Err(invalid("Lore authoring does not match the weapon rows"));
    }
    let root = read_tag(manager, globals_child_tag(globals, 0)?, "investment root")?;
    let definition_tag = root_child_tag(&root, ROOT_SLOT)?;
    let display_tag = globals_child_tag(globals, DISPLAY_SLOT)?;
    let mut definitions = read_tag(manager, definition_tag, "lore definitions")?;
    let mut strings = read_tag(manager, display_tag, "lore display")?;
    validate_tables(&definitions, &strings, STOCK_COUNT)?;
    let (_, _, collectible_rows, _) = array_at(collectibles, 8)?;
    let mut count = STOCK_COUNT;
    for (ordinal, weapon) in weapons.iter().enumerate() {
        if weapon.overrides.lore.is_none() {
            continue;
        }
        let index = u16::try_from(count)
            .ok()
            .filter(|&index| index != u16::MAX)
            .ok_or_else(|| invalid("Custom lore exceeds the native table index capacity"))?;
        let hash = crate::presentation::text_hash(&weapon.namespace, "lore-entry");
        let (old_count, header, rows, _) = array_at(&definitions, 8)?;
        if contains_u32_at_offset(&definitions, rows, old_count, 16, 8, hash)? {
            return Err(invalid(
                "Custom lore identity collides with an existing entry",
            ));
        }
        let mut row = definitions[rows..rows + 16].to_vec();
        write_u32(&mut row, 8, hash)?;
        definitions.extend_from_slice(&row);
        set_array_count(&mut definitions, 8, header, count + 1)?;
        let (_, header, rows, _) = array_at(&strings, 8)?;
        let mut row = strings[rows..rows + 40].to_vec();
        write_u32(&mut row, 8, hash)?;
        write_localized_reference(
            &mut row,
            12,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            weapon.identity.name_hash,
        )?;
        write_localized_reference(
            &mut row,
            20,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            weapon.identity.flavor_hash,
        )?;
        write_localized_reference(
            &mut row,
            28,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            crate::presentation::text_hash(&weapon.namespace, "lore"),
        )?;
        write_u16(&mut row, 36, u16::MAX)?;
        strings.extend_from_slice(&row);
        set_array_count(&mut strings, 8, header, count + 1)?;
        set_item_lore(&mut items[ordinal].payload, index)?;
        let collectible = collectible_rows
            + placements[ordinal].authored_collectible_index * COLLECTIBLE_ROW_SIZE;
        write_u16(collectibles, collectible + 0x2C, index)?;
        count += 1;
    }
    validate_tables(&definitions, &strings, count)?;
    Ok(Some(Plan {
        definitions: ReplacementSpec {
            tag: definition_tag,
            payload: synchronize_payload_size(definitions)?,
        },
        strings: ReplacementSpec {
            tag: display_tag,
            payload: synchronize_payload_size(strings)?,
        },
    }))
}

fn validate_tables(definitions: &[u8], strings: &[u8], count: usize) -> AuthoringResult<()> {
    let mut hashes = Vec::new();
    for (data, stride, class) in [
        (definitions, 16, DEFINITION_CLASS),
        (strings, 40, DISPLAY_CLASS),
    ] {
        let (actual, _, rows, actual_class) = array_at(data, 8)?;
        if actual != count || actual_class != class || rows + count * stride != data.len() {
            return Err(invalid(
                "Lore tables do not match the audited native layout",
            ));
        }
        let keys = (0..count)
            .map(|i| {
                let row = rows + i * stride;
                if read_u64(data, row)? != stride as u64 {
                    return Err(invalid("Lore row size marker changed"));
                }
                read_u32(data, row + 8)
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        if !hashes.is_empty() && hashes != keys {
            return Err(invalid("Lore definitions and display rows are not aligned"));
        }
        hashes = keys;
    }
    Ok(())
}

fn set_item_lore(item: &mut Vec<u8>, index: u16) -> AuthoringResult<()> {
    if read_i64(item, BLOCK_POINTER)? != 0 {
        let start = relative_target(item, BLOCK_POINTER)?;
        if start < 4
            || item.get(start..start.saturating_add(8)).is_none()
            || read_u32(item, start - 4)? != BLOCK_CLASS
        {
            return Err(invalid("Item lore block has an unsupported native class"));
        }
        write_u16(item, start, index)?;
    } else {
        while item.len() % 16 != 12 {
            item.push(0);
        }
        item.extend_from_slice(&BLOCK_CLASS.to_le_bytes());
        let block = item.len();
        item.extend_from_slice(&[0; 8]);
        write_u16(item, block, index)?;
        write_relative_pointer(item, BLOCK_POINTER, block)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lore_block_addition_and_replacement_preserve_other_item_fields() {
        let mut item = vec![0; 192];
        item[0xB0] = 91;
        let before = item.clone();
        set_item_lore(&mut item, 1425).unwrap();
        let start = relative_target(&item, BLOCK_POINTER).unwrap();
        assert_eq!(read_u32(&item, start - 4).unwrap(), BLOCK_CLASS);
        assert_eq!(read_u16(&item, start).unwrap(), 1425);
        for i in 0..before.len() {
            if !(BLOCK_POINTER..BLOCK_POINTER + 8).contains(&i) {
                assert_eq!(item[i], before[i]);
            }
        }
        let size = item.len();
        set_item_lore(&mut item, 1426).unwrap();
        assert_eq!(item.len(), size);
        assert_eq!(read_u16(&item, start).unwrap(), 1426);
        write_u32(&mut item, start - 4, 0).unwrap();
        assert!(set_item_lore(&mut item, 1427).is_err());
    }
}
