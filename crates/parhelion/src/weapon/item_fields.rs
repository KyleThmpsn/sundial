//! Native item fields operations with independent validation.
use super::*;

// The client checks these per-version icon overrides before the main icon's watermark layer.
// Stock Red War and Trials donors retain nonempty entries here even after cloning their icon.
const ITEM_STRING_WATERMARK_DESCRIPTOR: usize = 0xA8;
const ITEM_STRING_WATERMARK_ROW_CLASS: u32 = 0x8080_5F87;

pub(super) fn item_string_watermark_overrides(
    data: &[u8],
) -> AuthoringResult<std::ops::Range<usize>> {
    let (count, _, rows, class) = array_at(data, ITEM_STRING_WATERMARK_DESCRIPTOR)?;
    if class != ITEM_STRING_WATERMARK_ROW_CLASS || count > 16 {
        return Err(invalid(
            "Item watermark override array has an unsupported shape",
        ));
    }
    let end = rows
        .checked_add(count * size_of::<u16>())
        .filter(|end| *end <= data.len())
        .ok_or_else(|| invalid("Item watermark override rows extend beyond the payload"))?;
    Ok(rows..end)
}

pub(super) fn clear_item_string_watermark_overrides(data: &mut [u8]) -> AuthoringResult<()> {
    let rows = item_string_watermark_overrides(data)?;
    // FFFF is the native no-override value: use our private icon's watermark, for every version.
    data[rows].fill(0xFF);
    Ok(())
}

pub(super) fn weapon_inventory_slot(data: &[u8]) -> AuthoringResult<WeaponInventorySlot> {
    let root_value = *data
        .get(ITEM_INVENTORY_SLOT_OFFSET)
        .ok_or_else(|| invalid("Weapon inventory-slot root byte is unavailable"))?;
    let root_slot = WeaponInventorySlot::from_root_value(root_value).ok_or_else(|| {
        invalid(format!(
            "Weapon inventory-slot root byte {root_value} is not Kinetic, Energy, or Power"
        ))
    })?;
    Ok(root_slot)
}

pub(super) fn weapon_equipment_slot(data: &[u8]) -> AuthoringResult<WeaponInventorySlot> {
    let block = relative_target(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    if block < 4 || read_u32(data, block - 4)? != ITEM_EQUIPMENT_BLOCK_CLASS {
        return Err(invalid("Weapon has no recognized equipment-slot block"));
    }
    let equipment_value = read_u16(data, block + ITEM_EQUIPMENT_SLOT_OFFSET)?;
    let sentinel = read_u16(data, block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET)?;
    if sentinel != u16::MAX {
        return Err(invalid(
            "Weapon equipment-slot value is not followed by the native FFFF sentinel",
        ));
    }
    let equipment_slot =
        WeaponInventorySlot::from_equipment_value(equipment_value).ok_or_else(|| {
            invalid(format!(
                "Weapon equipment-slot value {equipment_value} is not Kinetic, Energy, or Power"
            ))
        })?;
    Ok(equipment_slot)
}

pub(super) fn set_weapon_inventory_slot(
    data: &mut [u8],
    inventory_slot: WeaponInventorySlot,
) -> AuthoringResult<()> {
    // Parse the donor pair before writing so an old-style or ambiguous block is never normalized
    // into something that merely looks authorable.
    let donor_inventory_slot = weapon_inventory_slot(data)?;
    let donor_equipment_slot = weapon_equipment_slot(data)?;
    if donor_inventory_slot != donor_equipment_slot {
        return Err(invalid(format!(
            "Weapon has a {donor_inventory_slot:?} inventory bucket but a {donor_equipment_slot:?} equipment slot"
        )));
    }
    let block = relative_target(data, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    write_bytes(
        data,
        ITEM_INVENTORY_SLOT_OFFSET,
        &[inventory_slot.root_value()],
    )?;
    write_u16(
        data,
        block + ITEM_EQUIPMENT_SLOT_OFFSET,
        inventory_slot.equipment_value(),
    )?;
    if read_u16(data, block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET)? != u16::MAX {
        return Err(validation(
            "Authoring the weapon inventory slot changed its FFFF sentinel",
        ));
    }
    if weapon_inventory_slot(data)? != inventory_slot
        || weapon_equipment_slot(data)? != inventory_slot
    {
        return Err(validation(
            "Authored weapon did not retain the requested inventory slot",
        ));
    }
    Ok(())
}

pub(super) fn item_string_client_classification(
    data: &[u8],
    expected_slot: WeaponInventorySlot,
) -> AuthoringResult<[u8; ITEM_STRING_CLIENT_CLASSIFICATION_SIZE]> {
    let end = ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET
        .checked_add(ITEM_STRING_CLIENT_CLASSIFICATION_SIZE)
        .ok_or_else(|| invalid("Item-string client classification range overflowed"))?;
    let tuple: [u8; ITEM_STRING_CLIENT_CLASSIFICATION_SIZE] = data
        .get(ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET..end)
        .ok_or_else(|| invalid("Item-string client classification tuple is truncated"))?
        .try_into()
        .map_err(|_| invalid("Item-string client classification tuple has the wrong size"))?;
    let bucket_hash = u32::from_le_bytes(tuple[0..4].try_into().map_err(|_| {
        invalid("Item-string client classification bucket hash has the wrong size")
    })?);
    let slot = WeaponInventorySlot::from_bucket_hash(bucket_hash).ok_or_else(|| {
        invalid(format!(
            "Item-string client classification uses unknown bucket hash 0x{bucket_hash:08X}"
        ))
    })?;
    if bucket_hash != expected_slot.bucket_hash() || slot != expected_slot {
        return Err(invalid(format!(
            "Item-string client classification uses {slot:?}, but the definition uses {expected_slot:?}"
        )));
    }
    let first_type_key = u32::from_le_bytes(
        tuple[4..8]
            .try_into()
            .map_err(|_| invalid("Item-string client type key has the wrong size"))?,
    );
    let second_type_key = u32::from_le_bytes(
        tuple[8..12]
            .try_into()
            .map_err(|_| invalid("Item-string duplicate type key has the wrong size"))?,
    );
    if first_type_key == 0 || first_type_key != second_type_key {
        return Err(invalid(format!(
            "Item-string client type keys are not the same nonzero value (0x{first_type_key:08X}, 0x{second_type_key:08X})"
        )));
    }
    Ok(tuple)
}

/// Slot and weapon type are independent: +B8 is the bucket hash, while +BC/+C0
/// are type-name hashes (for example FNV-1("sword")), not destination-slot keys.
pub(super) fn set_item_string_inventory_slot(
    data: &mut [u8],
    donor_slot: WeaponInventorySlot,
    authored_slot: WeaponInventorySlot,
) -> AuthoringResult<()> {
    item_string_client_classification(data, donor_slot)?;
    write_u32(
        data,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        authored_slot.bucket_hash(),
    )
}

pub(super) fn transplant_item_string_client_classification(
    target: &mut [u8],
    target_donor_slot: WeaponInventorySlot,
    source: &[u8],
    source_slot: WeaponInventorySlot,
    authored_slot: WeaponInventorySlot,
) -> AuthoringResult<()> {
    let target_tuple = item_string_client_classification(target, target_donor_slot)?;
    let mut source_tuple = item_string_client_classification(source, source_slot)?;
    // Preserve the appearance's type keys, but not its inventory placement.
    source_tuple[..4].copy_from_slice(&authored_slot.bucket_hash().to_le_bytes());
    let before = target.to_vec();
    write_bytes(
        target,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        &source_tuple,
    )?;
    if item_string_client_classification(target, authored_slot)? != source_tuple {
        return Err(validation(
            "Authored item-string did not retain the presentation donor classification tuple",
        ));
    }
    let mut normalized = target.to_vec();
    write_bytes(
        &mut normalized,
        ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET,
        &target_tuple,
    )?;
    if normalized != before {
        return Err(validation(
            "Item-string classification authoring changed bytes outside the audited 12-byte tuple",
        ));
    }
    Ok(())
}

pub(super) fn item_string_ammo_type(data: &[u8]) -> AuthoringResult<Option<WeaponAmmoType>> {
    if read_u32(data, ITEM_STRING_AMMO_CLASS_OFFSET)? != ITEM_STRING_AMMO_CLASS {
        return Err(invalid(
            "Item string has no canonical 0x80805D1A ammunition-classification field",
        ));
    }
    WeaponAmmoType::from_package_value(read_u16(data, ITEM_STRING_AMMO_TYPE_OFFSET)?)
}

pub(super) fn set_item_string_ammo_type(
    data: &mut [u8],
    ammo_type: WeaponAmmoType,
) -> AuthoringResult<()> {
    let original = item_string_ammo_type(data)?;
    let before = data.to_vec();
    write_u16(
        data,
        ITEM_STRING_AMMO_TYPE_OFFSET,
        ammo_type.package_value(),
    )?;
    if item_string_ammo_type(data)? != Some(ammo_type) {
        return Err(validation(
            "Authored item string did not retain the requested ammunition classification",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(
        &mut normalized,
        ITEM_STRING_AMMO_TYPE_OFFSET,
        original.map_or(0, WeaponAmmoType::package_value),
    )?;
    if normalized != before {
        return Err(validation(
            "Ammunition-classification authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(super) fn item_string_stat_group_field_offset(data: &[u8]) -> AuthoringResult<usize> {
    let resource = relative_target(data, ITEM_STRING_STAT_GROUP_POINTER_OFFSET)?;
    let field = resource
        .checked_add(ITEM_STRING_STAT_GROUP_INDEX_OFFSET)
        .ok_or_else(|| invalid("Item-string stat-display group field overflowed"))?;
    let end = field
        .checked_add(size_of::<i32>())
        .ok_or_else(|| invalid("Item-string stat-display group range overflowed"))?;
    if resource < size_of::<u32>()
        || end > data.len()
        || read_u32(data, resource - size_of::<u32>())? != ITEM_STRING_STAT_GROUP_RESOURCE_CLASS
    {
        return Err(invalid(
            "Item string has no canonical 0x80805CF1 stat-display resource",
        ));
    }
    Ok(field)
}

pub(super) fn item_string_stat_group_index(data: &[u8]) -> AuthoringResult<u16> {
    let field = item_string_stat_group_field_offset(data)?;
    u16::try_from(read_i32(data, field)?).map_err(|_| {
        invalid("Item-string stat-display group is negative or does not fit a 16-bit index")
    })
}

pub(super) fn set_item_string_stat_group_index(data: &mut [u8], index: u16) -> AuthoringResult<()> {
    let field = item_string_stat_group_field_offset(data)?;
    let original = read_i32(data, field)?;
    // Refuse to mutate a malformed donor resource even though the authored value itself is valid.
    item_string_stat_group_index(data)?;
    let before = data.to_vec();

    write_i32(data, field, i32::from(index))?;
    if item_string_stat_group_index(data)? != index {
        return Err(validation(
            "Authored item string did not retain the requested stat-display group",
        ));
    }

    let mut normalized = data.to_vec();
    write_i32(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Stat-display group authoring changed bytes outside the audited signed 32-bit field",
        ));
    }
    Ok(())
}

pub(super) fn weapon_translation_topology(
    data: &[u8],
) -> AuthoringResult<WeaponTranslationTopology> {
    let root = relative_target(data, ITEM_TRANSLATION_BLOCK_POINTER_OFFSET)?;
    let root_end = root
        .checked_add(ITEM_TRANSLATION_BLOCK_SIZE)
        .ok_or_else(|| invalid("Weapon translation block overflowed"))?;
    if root < 4
        || root_end > data.len()
        || root % 8 != 0
        || read_u32(data, root - 4)? != ITEM_TRANSLATION_BLOCK_CLASS
    {
        return Err(invalid(
            "Weapon has no canonical 0x808077AF translation block",
        ));
    }

    let (art_count, art_header, art_row, art_class) =
        array_at(data, root + TRANSLATION_ART_DESCRIPTOR_OFFSET)?;
    let art_end = art_row
        .checked_add(art_count.saturating_mul(TRANSLATION_ART_ROW_SIZE))
        .ok_or_else(|| invalid("Weapon translation art rows overflowed"))?;
    if !(1..=4).contains(&art_count)
        || art_class != TRANSLATION_ART_ROW_CLASS
        || art_header < root_end
        || art_end > data.len()
    {
        return Err(invalid(
            "Weapon translation block has no canonical one-to-four-row art array",
        ));
    }

    let mut descriptor_counts = [art_count, 0, 0, 0];
    let mut dye_rows = [None; TRANSLATION_DYE_DESCRIPTOR_OFFSETS.len()];
    for (position, offset) in TRANSLATION_DYE_DESCRIPTOR_OFFSETS.into_iter().enumerate() {
        let descriptor = root + offset;
        if data
            .get(descriptor..descriptor + 16)
            .is_some_and(|bytes| bytes == [0; 16])
        {
            continue;
        }
        let (count, header, rows, class) = array_at(data, descriptor)?;
        let rows_end = rows
            .checked_add(count.saturating_mul(TRANSLATION_DYE_ROW_SIZE))
            .ok_or_else(|| invalid("Weapon translation dye-reference rows overflowed"))?;
        if count == 0
            || class != TRANSLATION_DYE_ROW_CLASS
            || header < root_end
            || rows_end > data.len()
        {
            return Err(invalid(
                "Weapon translation block has a noncanonical dye-reference array",
            ));
        }
        descriptor_counts[position + 1] = count;
        dye_rows[position] = Some(rows);
    }

    Ok(WeaponTranslationTopology {
        root,
        art_rows: art_row,
        dye_rows,
        descriptor_counts,
    })
}

pub(super) fn weapon_presentation_tuple(
    data: &[u8],
    topology: &WeaponTranslationTopology,
) -> AuthoringResult<WeaponPresentationTuple> {
    let mut dye_arrays = Vec::with_capacity(TRANSLATION_DYE_DESCRIPTOR_OFFSETS.len());
    for position in 0..TRANSLATION_DYE_DESCRIPTOR_OFFSETS.len() {
        let count = topology.descriptor_counts[position + 1];
        let Some(rows) = topology.dye_rows[position] else {
            if count != 0 {
                return Err(validation(
                    "Weapon translation dye-reference topology lost a populated row array",
                ));
            }
            dye_arrays.push(Vec::new());
            continue;
        };
        let byte_count = count
            .checked_mul(TRANSLATION_DYE_ROW_SIZE)
            .ok_or_else(|| invalid("Weapon translation dye-reference byte count overflowed"))?;
        let end = rows
            .checked_add(byte_count)
            .ok_or_else(|| invalid("Weapon translation dye-reference row range overflowed"))?;
        dye_arrays.push(
            data.get(rows..end)
                .ok_or_else(|| invalid("Weapon translation dye-reference rows are truncated"))?
                .to_vec(),
        );
    }
    let dye_arrays = dye_arrays.try_into().map_err(|_| {
        validation("Weapon translation dye-reference topology has an unexpected descriptor count")
    })?;
    let art_byte_count = topology.descriptor_counts[0]
        .checked_mul(TRANSLATION_ART_ROW_SIZE)
        .ok_or_else(|| invalid("Weapon translation art byte count overflowed"))?;
    let art_end = topology
        .art_rows
        .checked_add(art_byte_count)
        .ok_or_else(|| invalid("Weapon translation art row range overflowed"))?;
    Ok(WeaponPresentationTuple {
        weapon_pattern_index: weapon_pattern_index(data)?,
        art_rows: data
            .get(topology.art_rows..art_end)
            .ok_or_else(|| invalid("Weapon translation art rows are truncated"))?
            .to_vec(),
        dye_arrays,
    })
}

pub(super) fn transplant_weapon_geometry(
    target: &mut Vec<u8>,
    source: &[u8],
) -> AuthoringResult<()> {
    let source_rows = weapon_art_arrangements(source)?;
    let preserved_pattern = weapon_pattern_index(target)?;
    set_weapon_art_arrangements(target, &source_rows)?;
    if weapon_art_arrangements(target)? != source_rows
        || weapon_pattern_index(target)? != preserved_pattern
    {
        return Err(validation(
            "Authored weapon did not retain the selected geometry while preserving its runtime pattern",
        ));
    }
    Ok(())
}

pub(super) fn transplant_weapon_render_gear(
    target: &mut Vec<u8>,
    source: &[u8],
) -> AuthoringResult<()> {
    let source_rows = weapon_render_dye_rows(source)?;
    let preserved_pattern = weapon_pattern_index(target)?;
    let preserved_geometry = weapon_art_arrangements(target)?;
    set_weapon_render_dye_rows(target, &source_rows)?;
    if weapon_render_dye_rows(target)? != source_rows
        || weapon_art_arrangements(target)? != preserved_geometry
        || weapon_pattern_index(target)? != preserved_pattern
    {
        return Err(validation(
            "Authored weapon did not retain the selected render dyes while preserving geometry and runtime behavior",
        ));
    }
    Ok(())
}

pub(super) fn replace_translation_array(
    data: &mut Vec<u8>,
    descriptor: usize,
    row_class: u32,
    rows: &[u8],
    row_size: usize,
) -> AuthoringResult<()> {
    if rows.len() % row_size != 0 {
        return Err(invalid("Translation array rows are not stride-aligned"));
    }
    let count = rows.len() / row_size;
    if count == 0 {
        write_bytes(data, descriptor, &[0; 16])?;
        return Ok(());
    }
    while data.len() % 16 != 0 {
        data.push(0);
    }
    let header = data.len();
    data.extend_from_slice(&[0; 16]);
    write_u32(data, header + 8, row_class)?;
    data.extend_from_slice(rows);
    set_array_count(data, descriptor, header, count)?;
    write_relative_pointer(data, descriptor + 8, header)?;
    Ok(())
}

pub(super) fn set_weapon_art_arrangements(
    data: &mut Vec<u8>,
    rows: &[WeaponArtArrangementOverride],
) -> AuthoringResult<()> {
    if rows.is_empty() || rows.len() > 4 {
        return Err(invalid(
            "Translation art must contain between one and four rows",
        ));
    }
    let mut classes = BTreeSet::new();
    let mut encoded = Vec::with_capacity(rows.len() * TRANSLATION_ART_ROW_SIZE);
    for row in rows {
        if !(-1..=2).contains(&row.character_class)
            || row.arrangement == u16::MAX
            || !classes.insert(row.character_class)
        {
            return Err(invalid(
                "Translation-art rows require distinct classes -1 through 2 and active arrangement indices",
            ));
        }
        encoded.push(row.character_class as u8);
        encoded.push(0);
        encoded.extend_from_slice(&row.arrangement.to_le_bytes());
    }
    let root = weapon_translation_topology(data)?.root;
    replace_translation_array(
        data,
        root + TRANSLATION_ART_DESCRIPTOR_OFFSET,
        TRANSLATION_ART_ROW_CLASS,
        &encoded,
        TRANSLATION_ART_ROW_SIZE,
    )?;
    let topology = weapon_translation_topology(data)?;
    let tuple = weapon_presentation_tuple(data, &topology)?;
    if tuple.art_rows != encoded {
        return Err(validation(
            "Authored translation-art rows did not retain every requested value",
        ));
    }
    Ok(())
}

pub(super) fn weapon_art_arrangements(
    data: &[u8],
) -> AuthoringResult<Vec<WeaponArtArrangementOverride>> {
    let topology = weapon_translation_topology(data)?;
    (0..topology.descriptor_counts[0])
        .map(|index| {
            let row = topology.art_rows + index * TRANSLATION_ART_ROW_SIZE;
            let character_class = read_u8(data, row)? as i8;
            if read_u8(data, row + 1)? != 0 {
                return Err(invalid("Translation-art row has a nonzero reserved byte"));
            }
            Ok(WeaponArtArrangementOverride {
                character_class,
                arrangement: read_u16(data, row + TRANSLATION_ART_VARIANT_OFFSET)?,
            })
        })
        .collect()
}

pub(super) fn set_weapon_render_dye_rows(
    data: &mut Vec<u8>,
    arrays: &[Vec<WeaponDyeReferenceOverride>; 3],
) -> AuthoringResult<()> {
    let root = weapon_translation_topology(data)?.root;
    for (array, rows) in arrays.iter().enumerate() {
        if rows.len() > 32 {
            return Err(invalid(format!(
                "Translation dye array {array} cannot contain more than 32 rows"
            )));
        }
        let mut encoded = Vec::with_capacity(rows.len() * TRANSLATION_DYE_ROW_SIZE);
        for row in rows {
            encoded.push(row.channel_index as u8);
            encoded.push(0);
            encoded.extend_from_slice(&row.dye_reference_index.to_le_bytes());
        }
        replace_translation_array(
            data,
            root + TRANSLATION_DYE_DESCRIPTOR_OFFSETS[array],
            TRANSLATION_DYE_ROW_CLASS,
            &encoded,
            TRANSLATION_DYE_ROW_SIZE,
        )?;
    }
    let topology = weapon_translation_topology(data)?;
    let tuple = weapon_presentation_tuple(data, &topology)?;
    for (array, expected) in arrays.iter().enumerate() {
        let mut encoded = Vec::with_capacity(expected.len() * TRANSLATION_DYE_ROW_SIZE);
        for row in expected {
            encoded.push(row.channel_index as u8);
            encoded.push(0);
            encoded.extend_from_slice(&row.dye_reference_index.to_le_bytes());
        }
        if tuple.dye_arrays[array] != encoded {
            return Err(validation(format!(
                "Authored render-dye array {array} did not retain every requested row"
            )));
        }
    }
    Ok(())
}

pub(super) fn weapon_render_dye_rows(
    data: &[u8],
) -> AuthoringResult<[Vec<WeaponDyeReferenceOverride>; 3]> {
    let topology = weapon_translation_topology(data)?;
    let mut arrays: [Vec<WeaponDyeReferenceOverride>; 3] = Default::default();
    for (array, output) in arrays.iter_mut().enumerate() {
        let count = topology.descriptor_counts[array + 1];
        let Some(rows) = topology.dye_rows[array] else {
            if count != 0 {
                return Err(validation(
                    "Translation dye-reference topology lost a populated array",
                ));
            }
            continue;
        };
        for index in 0..count {
            let row = rows + index * TRANSLATION_DYE_ROW_SIZE;
            if read_u8(data, row + 1)? != 0 {
                return Err(invalid(
                    "Translation dye-reference row has a nonzero reserved byte",
                ));
            }
            output.push(WeaponDyeReferenceOverride {
                channel_index: read_u8(data, row)? as i8,
                dye_reference_index: read_u16(data, row + 2)?,
            });
        }
    }
    Ok(arrays)
}

pub(super) fn weapon_pattern_index(data: &[u8]) -> AuthoringResult<Option<u16>> {
    let topology = weapon_translation_topology(data)?;
    let index = read_u16(
        data,
        topology.root + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
    )?;
    Ok((index != u16::MAX).then_some(index))
}

pub(super) fn set_weapon_pattern_index(data: &mut [u8], index: u16) -> AuthoringResult<()> {
    if index == u16::MAX {
        return Err(invalid(
            "Weapon-pattern index 65535 is a native disabled sentinel",
        ));
    }
    let topology = weapon_translation_topology(data)?;
    let field = topology.root + TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET;
    let original = weapon_pattern_index(data)?
        .ok_or_else(|| invalid("Weapon sandbox-pattern selector is disabled"))?;
    let before = data.to_vec();

    write_u16(data, field, index)?;
    if weapon_pattern_index(data)? != Some(index) {
        return Err(validation(
            "Authored weapon did not retain the requested weapon-pattern selector",
        ));
    }

    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon-pattern authoring changed bytes outside the audited 16-bit selector",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_weapon_collection_donor(
    manager: &PackageManager,
    items: &[u8],
    item_rows: usize,
    item_count: usize,
    item_strings: &[u8],
    string_rows: usize,
    collectibles: &[u8],
    collectible_rows: usize,
    collectible_count: usize,
    gameplay_collectible: usize,
    gameplay: &[u8],
    gameplay_strings: &[u8],
    rarity: AuthoredWeaponRarity,
    slot: WeaponInventorySlot,
) -> AuthoringResult<Vec<usize>> {
    let exotic = rarity == AuthoredWeaponRarity::Exotic;
    let prefer_gameplay = (weapon_rarity(gameplay)? == AuthoredWeaponRarity::Exotic) == exotic
        && (!exotic || weapon_inventory_slot(gameplay)? == slot);
    let family_hash = read_u32(gameplay_strings, ITEM_TYPE_REFERENCE_OFFSET + 4)?;
    // A native collectible can belong to the right page without contributing to
    // its acquired-count pools. Keep compatible alternatives for placement validation.
    let mut candidates = if prefer_gameplay {
        vec![gameplay_collectible]
    } else {
        Vec::new()
    };
    for index in 0..collectible_count {
        if prefer_gameplay && index == gameplay_collectible {
            continue;
        }
        let item = usize::from(read_u16(
            collectibles,
            collectible_rows + index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )?);
        if item >= item_count {
            continue;
        }
        let definition = read_tag(
            manager,
            TagHash(read_u32(items, item_rows + item * ITEM_ROW_SIZE + 16)?),
            "Collections placement exemplar",
        )?;
        let Ok(candidate_rarity) = weapon_rarity(&definition) else {
            continue;
        };
        if (candidate_rarity == AuthoredWeaponRarity::Exotic) != exotic {
            continue;
        }
        let Ok(candidate_slot) = weapon_inventory_slot(&definition) else {
            continue;
        };
        if exotic {
            if candidate_slot != slot {
                continue;
            }
        } else {
            let strings = read_tag(
                manager,
                TagHash(read_u32(
                    item_strings,
                    string_rows + item * ITEM_ROW_SIZE + 16,
                )?),
                "Collections exemplar type",
            )?;
            if read_u32(&strings, ITEM_TYPE_REFERENCE_OFFSET + 4)? != family_hash {
                continue;
            }
        }
        candidates.push(index);
    }
    if !candidates.is_empty() {
        return Ok(candidates);
    }
    Err(invalid(if exotic {
        format!("No stock Exotics Collections page is available for {slot:?} weapons")
    } else {
        "This weapon family has no non-Exotic Collections page in this game version. Keep Exotic rarity or choose another gameplay family.".to_owned()
    }))
}

pub(super) fn weapon_rarity(data: &[u8]) -> AuthoringResult<AuthoredWeaponRarity> {
    // Rarity is a root item field, but require the verified weapon translation topology so an
    // arbitrary item-like payload cannot pass through the weapon-only authoring path.
    weapon_translation_topology(data)?;
    AuthoredWeaponRarity::from_package_value(read_u8(data, ITEM_RARITY_OFFSET)?)
}

pub(super) const fn collection_material_set_for_rarity(rarity: AuthoredWeaponRarity) -> u16 {
    match rarity {
        AuthoredWeaponRarity::Exotic => COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET,
        AuthoredWeaponRarity::Common
        | AuthoredWeaponRarity::Uncommon
        | AuthoredWeaponRarity::Rare
        | AuthoredWeaponRarity::Legendary => COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET,
    }
}

pub(super) fn set_weapon_rarity(
    data: &mut [u8],
    rarity: AuthoredWeaponRarity,
) -> AuthoringResult<()> {
    let original = weapon_rarity(data)?.package_value();
    let before = data.to_vec();

    write_bytes(data, ITEM_RARITY_OFFSET, &[rarity.package_value()])?;
    if weapon_rarity(data)? != rarity {
        return Err(validation(
            "Authored weapon did not retain the requested rarity tier",
        ));
    }

    let mut normalized = data.to_vec();
    write_bytes(&mut normalized, ITEM_RARITY_OFFSET, &[original])?;
    if normalized != before {
        return Err(validation(
            "Weapon rarity authoring changed bytes outside the audited one-byte field",
        ));
    }
    Ok(())
}

pub(super) fn weapon_max_stack_size(data: &[u8]) -> AuthoringResult<u32> {
    weapon_translation_topology(data)?;
    let value = read_i32(data, ITEM_MAX_STACK_SIZE_OFFSET)?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("Weapon max stack size is not a positive signed 32-bit value"))
}

pub(super) fn set_weapon_max_stack_size(data: &mut [u8], value: u32) -> AuthoringResult<()> {
    let value_i32 = i32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("Weapon max stack size must fit signed 32 bits and be positive"))?;
    let original = weapon_max_stack_size(data)?;
    let before = data.to_vec();
    write_i32(data, ITEM_MAX_STACK_SIZE_OFFSET, value_i32)?;
    if weapon_max_stack_size(data)? != value {
        return Err(validation(
            "Authored weapon did not retain the requested max stack size",
        ));
    }
    let mut normalized = data.to_vec();
    write_i32(
        &mut normalized,
        ITEM_MAX_STACK_SIZE_OFFSET,
        i32::try_from(original).map_err(|_| invalid("Donor max stack size overflowed"))?,
    )?;
    if normalized != before {
        return Err(validation(
            "Weapon max-stack authoring changed bytes outside the audited signed 32-bit field",
        ));
    }
    Ok(())
}

pub(super) fn weapon_socket_entry_list_index(data: &[u8]) -> AuthoringResult<Option<u16>> {
    weapon_translation_topology(data)?;
    if read_i64(data, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)? == 0 {
        return Ok(None);
    }
    let block = relative_target(data, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)?;
    let end = block
        .checked_add(12)
        .ok_or_else(|| invalid("Weapon socket-entry-list holder overflows"))?;
    if end > data.len() {
        return Err(invalid(
            "Weapon socket-entry-list holder extends beyond its definition",
        ));
    }
    Ok(Some(read_u16(
        data,
        block + ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET,
    )?))
}

pub(super) fn set_weapon_socket_entry_list_index(
    data: &mut [u8],
    index: u16,
) -> AuthoringResult<()> {
    let Some(original) = weapon_socket_entry_list_index(data)? else {
        return Err(invalid(
            "Weapon has no native socket-entry-list holder to author",
        ));
    };
    let block = relative_target(data, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)?;
    let field = block + ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET;
    let before = data.to_vec();
    write_u16(data, field, index)?;
    if weapon_socket_entry_list_index(data)? != Some(index) {
        return Err(validation(
            "Authored weapon did not retain the requested socket-entry-list index",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Socket-entry-list authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(super) fn aligned_record_offset(
    data: &[u8],
    class: u32,
    start: usize,
    end: usize,
) -> Option<usize> {
    let limit = end.min(data.len());
    (start..limit.saturating_sub(3))
        .step_by(4)
        .find(|offset| read_u32(data, *offset).ok() == Some(class))
}

pub(super) fn weapon_plug_category_field(data: &[u8]) -> AuthoringResult<usize> {
    weapon_translation_topology(data)?;
    if let Some(block) = aligned_record_offset(
        data,
        ITEM_PLUG_BLOCK_CLASS,
        ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_BLOCK_SEARCH_END,
    ) {
        return block
            .checked_add(ITEM_PLUG_BLOCK_CATEGORY_OFFSET)
            .ok_or_else(|| invalid("Weapon plug-category field overflows"));
    }
    let end = ITEM_PLUG_CATEGORY_FALLBACK_OFFSET
        .checked_add(size_of::<u32>())
        .ok_or_else(|| invalid("Weapon fallback plug-category field overflows"))?;
    if end > data.len() {
        return Err(invalid(
            "Weapon has no native plug-category field to author",
        ));
    }
    Ok(ITEM_PLUG_CATEGORY_FALLBACK_OFFSET)
}

pub(super) fn weapon_plug_category_hash(data: &[u8]) -> AuthoringResult<u32> {
    read_u32(data, weapon_plug_category_field(data)?)
}

pub(super) fn set_weapon_plug_category_hash(data: &mut [u8], hash: u32) -> AuthoringResult<()> {
    let field = weapon_plug_category_field(data)?;
    let original = read_u32(data, field)?;
    let before = data.to_vec();
    write_u32(data, field, hash)?;
    if weapon_plug_category_hash(data)? != hash {
        return Err(validation(
            "Authored weapon did not retain the requested plug-category hash",
        ));
    }
    let mut normalized = data.to_vec();
    write_u32(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon plug-category authoring changed bytes outside the audited 32-bit field",
        ));
    }
    Ok(())
}

pub(super) fn weapon_roll_set_field(data: &[u8]) -> AuthoringResult<usize> {
    weapon_translation_topology(data)?;
    let block = aligned_record_offset(
        data,
        ITEM_PLUG_BLOCK_CLASS,
        ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_BLOCK_SEARCH_END,
    )
    .ok_or_else(|| invalid("Weapon has no native plug block containing a roll-set field"))?;
    block
        .checked_add(ITEM_PLUG_BLOCK_ROLL_SET_OFFSET)
        .ok_or_else(|| invalid("Weapon roll-set field overflows"))
}

pub(super) fn weapon_roll_set_index(data: &[u8]) -> AuthoringResult<u16> {
    read_u16(data, weapon_roll_set_field(data)?)
}

pub(super) fn set_weapon_roll_set_index(data: &mut [u8], index: u16) -> AuthoringResult<()> {
    let field = weapon_roll_set_field(data)?;
    let original = read_u16(data, field)?;
    let before = data.to_vec();
    write_u16(data, field, index)?;
    if weapon_roll_set_index(data)? != index {
        return Err(validation(
            "Authored weapon did not retain the requested roll-set index",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon roll-set authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(super) fn weapon_linked_plug_field(data: &[u8]) -> AuthoringResult<usize> {
    weapon_translation_topology(data)?;
    let block = aligned_record_offset(data, ITEM_LINKED_PLUG_BLOCK_CLASS, 0, data.len())
        .ok_or_else(|| invalid("Weapon has no native linked-plug block to author"))?;
    block
        .checked_add(ITEM_LINKED_PLUG_INDEX_OFFSET)
        .ok_or_else(|| invalid("Weapon linked-plug field overflows"))
}

pub(super) fn weapon_linked_plug_index(data: &[u8]) -> AuthoringResult<u16> {
    read_u16(data, weapon_linked_plug_field(data)?)
}

pub(super) fn set_weapon_linked_plug_index(data: &mut [u8], index: u16) -> AuthoringResult<()> {
    let field = weapon_linked_plug_field(data)?;
    let original = read_u16(data, field)?;
    let before = data.to_vec();
    write_u16(data, field, index)?;
    if weapon_linked_plug_index(data)? != index {
        return Err(validation(
            "Authored weapon did not retain the requested linked-plug index",
        ));
    }
    let mut normalized = data.to_vec();
    write_u16(&mut normalized, field, original)?;
    if normalized != before {
        return Err(validation(
            "Weapon linked-plug authoring changed bytes outside the audited 16-bit field",
        ));
    }
    Ok(())
}

pub(super) fn weapon_item_traits(data: &[u8]) -> AuthoringResult<Vec<u16>> {
    if data
        .get(ITEM_TRAITS_DESCRIPTOR_OFFSET..ITEM_TRAITS_DESCRIPTOR_OFFSET + 16)
        .is_some_and(|descriptor| descriptor == [0; 16])
    {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(data, ITEM_TRAITS_DESCRIPTOR_OFFSET)?;
    if class != ITEM_TRAIT_ROW_CLASS || count > 256 {
        return Err(invalid(
            "Weapon has an unknown or oversized item-trait array",
        ));
    }
    let traits = (0..count)
        .map(|index| read_u16(data, rows + index * ITEM_TRAIT_ROW_SIZE))
        .collect::<AuthoringResult<Vec<_>>>()?;
    if traits.contains(&u16::MAX)
        || traits.iter().copied().collect::<BTreeSet<_>>().len() != traits.len()
    {
        return Err(invalid(
            "Weapon item-trait array contains a disabled or duplicate index",
        ));
    }
    Ok(traits)
}

pub(super) fn set_weapon_item_traits(data: &mut Vec<u8>, traits: &[u16]) -> AuthoringResult<()> {
    if traits.len() > 256
        || traits.contains(&u16::MAX)
        || traits.iter().copied().collect::<BTreeSet<_>>().len() != traits.len()
    {
        return Err(invalid(
            "Item traits must contain at most 256 distinct active indices",
        ));
    }
    weapon_translation_topology(data)?;
    if traits.is_empty() {
        write_bytes(data, ITEM_TRAITS_DESCRIPTOR_OFFSET, &[0; 16])?;
    } else {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        let header = data.len();
        data.extend_from_slice(
            &u64::try_from(traits.len())
                .map_err(|_| invalid("Item-trait count does not fit 64 bits"))?
                .to_le_bytes(),
        );
        data.extend_from_slice(&ITEM_TRAIT_ROW_CLASS.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        for trait_index in traits {
            data.extend_from_slice(&trait_index.to_le_bytes());
        }
        write_u64(
            data,
            ITEM_TRAITS_DESCRIPTOR_OFFSET,
            u64::try_from(traits.len())
                .map_err(|_| invalid("Item-trait count does not fit 64 bits"))?,
        )?;
        write_relative_pointer(data, ITEM_TRAITS_DESCRIPTOR_OFFSET + 8, header)?;
    }
    if weapon_item_traits(data)? != traits {
        return Err(validation(
            "Authored weapon did not retain its requested item-trait indices",
        ));
    }
    Ok(())
}

pub(super) const fn fixed_damage_perk(
    perk: u16,
) -> Option<(WeaponDamageCarrierFamily, ModernDamageType)> {
    use sundial::package_authoring::native_weapon::{DamageFamily, Element, fixed_damage_marker};
    match fixed_damage_marker(perk) {
        Some((family, element)) => Some((
            match family {
                DamageFamily::Legacy => WeaponDamageCarrierFamily::LegacyFixed,
                DamageFamily::Modern => WeaponDamageCarrierFamily::ModernFixed,
            },
            match element {
                Element::Arc => ModernDamageType::Arc,
                Element::Solar => ModernDamageType::Solar,
                Element::Void => ModernDamageType::Void,
            },
        )),
        None => None,
    }
}

pub(super) const fn damage_plug_type(index: u16) -> Option<ModernDamageType> {
    match index {
        ARC_DAMAGE_PLUG_ITEM_INDEX => Some(ModernDamageType::Arc),
        SOLAR_DAMAGE_PLUG_ITEM_INDEX => Some(ModernDamageType::Solar),
        VOID_DAMAGE_PLUG_ITEM_INDEX => Some(ModernDamageType::Void),
        _ => None,
    }
}

pub(super) fn weapon_damage_socket_lanes(data: &[u8]) -> AuthoringResult<Vec<(usize, usize)>> {
    let resource = relative_target(data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, class) = array_at(data, resource)?;
    if class != ITEM_ORDINARY_SOCKET_ROW_CLASS || count > 64 {
        return Err(invalid("Weapon ordinary-socket rows are incompatible"));
    }
    Ok((0..count)
        .filter_map(|lane| {
            let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
            (read_u16(data, row).ok() == Some(ELEMENTAL_DAMAGE_SOCKET_TYPE)).then_some((lane, row))
        })
        .collect())
}

pub(super) fn weapon_damage_carrier(data: &[u8]) -> AuthoringResult<WeaponDamageCarrier> {
    use sundial::package_authoring::native_weapon::{BaseDamage, classify_fixed_damage};
    let perks = weapon_sandbox_perks(data)?;
    let fixed = match classify_fixed_damage(&perks) {
        BaseDamage::Fixed(_, _) => perks.iter().find_map(|perk| fixed_damage_perk(*perk)),
        BaseDamage::NoMarker => None,
        BaseDamage::Duplicate | BaseDamage::Variable => {
            return Err(invalid(
                "Weapon base sandbox-perk array contains duplicate or variable elemental damage markers",
            ));
        }
    };
    let plug_lanes = weapon_damage_socket_lanes(data)?;
    if plug_lanes.len() > 1 {
        return Err(invalid(
            "Weapon has more than one native type-68 elemental carrier socket",
        ));
    }
    if fixed.is_some() && !plug_lanes.is_empty() {
        return Err(invalid(
            "Weapon duplicates its elemental carrier in both the parent and a type-68 plug socket",
        ));
    }
    if let Some((family, damage_type)) = fixed {
        return Ok(WeaponDamageCarrier::Fixed {
            family,
            damage_type,
        });
    }
    if let Some(&(lane, row)) = plug_lanes.first() {
        let default = read_u16(data, row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET)?;
        let damage_type = damage_plug_type(default).ok_or_else(|| {
            invalid(format!(
                "Weapon type-68 elemental carrier lane {lane} has unsupported default plug index {default}"
            ))
        })?;
        return Ok(WeaponDamageCarrier::PlugDriven { damage_type, lane });
    }
    Ok(WeaponDamageCarrier::Empty)
}

pub(super) fn weapon_damage_descriptor(data: &[u8]) -> AuthoringResult<WeaponDamageDescriptor> {
    Ok(weapon_damage_carrier(data)?.descriptor())
}

pub(super) fn weapon_sandbox_perks(data: &[u8]) -> AuthoringResult<Vec<u16>> {
    sundial::package_authoring::native_weapon::base_sandbox_perks(data).map_err(invalid)
}

pub(super) fn replace_weapon_sandbox_perk_index(
    data: &mut [u8],
    source: u16,
    replacement: u16,
) -> AuthoringResult<()> {
    if source == u16::MAX || replacement == u16::MAX || source == replacement {
        return Err(invalid(
            "Private sandbox-perk replacement requires two distinct active indices",
        ));
    }
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Item has no recognized investment resource"));
    }
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    let (count, _, rows, class) = array_at(data, descriptor)?;
    if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
        return Err(invalid(
            "Item has an unknown or oversized base sandbox-perk array",
        ));
    }
    let matches = (0..count)
        .filter_map(|index| {
            let row = rows + index * ITEM_SANDBOX_PERK_ROW_SIZE;
            (read_u16(data, row).ok() == Some(source)).then_some(row)
        })
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err(invalid(format!(
            "Source finished sandbox-perk index {source} occurs {} times; exactly one is required",
            matches.len()
        )));
    };
    write_u16(data, *row, replacement)?;
    if weapon_sandbox_perks(data)?.contains(&source)
        || !weapon_sandbox_perks(data)?.contains(&replacement)
    {
        return Err(validation(
            "Private finished sandbox-perk replacement did not round-trip",
        ));
    }
    Ok(())
}

pub(super) fn weapon_sandbox_perk_rows(data: &[u8]) -> AuthoringResult<Vec<&[u8]>> {
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Weapon has no recognized investment resource"));
    }
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    if data
        .get(descriptor..descriptor + 16)
        .is_some_and(|bytes| bytes == [0; 16])
    {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(data, descriptor)?;
    if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
        return Err(invalid(
            "Weapon has an unknown or oversized base sandbox-perk array",
        ));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(index.saturating_mul(ITEM_SANDBOX_PERK_ROW_SIZE))
                .ok_or_else(|| invalid("Weapon sandbox-perk row offset overflowed"))?;
            data.get(row..row + ITEM_SANDBOX_PERK_ROW_SIZE)
                .ok_or_else(|| invalid("Weapon sandbox-perk row is truncated"))
        })
        .collect()
}

pub(super) fn item_string_sandbox_perk_descriptor(data: &[u8]) -> AuthoringResult<usize> {
    let resource = relative_target(data, ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET)?;
    if resource < size_of::<u32>()
        || read_u32(data, resource - size_of::<u32>())? != ITEM_STRING_SANDBOX_PERK_RESOURCE_CLASS
    {
        return Err(invalid(
            "Item-string tag has no recognized sandbox-perk companion resource",
        ));
    }
    resource
        .checked_add(ITEM_STRING_SANDBOX_PERK_DESCRIPTOR_OFFSET)
        .ok_or_else(|| invalid("Item-string sandbox-perk descriptor overflowed"))
}

pub(super) fn validate_item_string_sandbox_perk_segment(segment: &[u8]) -> AuthoringResult<()> {
    // The first eight bytes precede the relocation marker, not the array itself.
    // Coldheart's stock layout stores a neighboring relative pointer (0x10) there.
    // Preserve that context; only the marker, header and rows belong to this array.
    if segment.len() < 32
        || segment.get(8..16) != Some(&NESTED_ARRAY_TRAILER)
        || read_u32(segment, 24)? != ITEM_STRING_SANDBOX_PERK_ROW_CLASS
        || read_u32(segment, 28)? != 0
    {
        return Err(invalid(
            "Item-string sandbox-perk companion does not match the canonical stock row",
        ));
    }
    let count = usize::try_from(read_u64(segment, 16)?)
        .map_err(|_| invalid("Item-string sandbox-perk count does not fit memory"))?;
    let expected = 32_usize
        .checked_add(count.saturating_mul(ITEM_STRING_SANDBOX_PERK_ROW_SIZE))
        .ok_or_else(|| invalid("Item-string sandbox-perk segment overflowed"))?;
    if count > 64 || segment.len() != expected {
        return Err(invalid(
            "Item-string sandbox-perk companion has an invalid row count",
        ));
    }
    for index in 0..count {
        let row = 32 + index * ITEM_STRING_SANDBOX_PERK_ROW_SIZE;
        if read_u16(segment, row)? != u16::MAX
            || read_u16(segment, row + 2)? != 0
            || read_u32(segment, row + 4)? != ITEM_STRING_SANDBOX_PERK_EMPTY_EXPRESSION_TAG
            || segment
                .get(row + 8..row + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
                .is_none_or(|tail| tail.iter().any(|byte| *byte != 0))
        {
            return Err(invalid(
                "Item-string sandbox-perk companion contains a noncanonical row",
            ));
        }
    }
    Ok(())
}

pub(super) fn item_string_sandbox_perk_segment(data: &[u8]) -> AuthoringResult<Option<&[u8]>> {
    let descriptor = item_string_sandbox_perk_descriptor(data)?;
    if data
        .get(descriptor..descriptor + 16)
        .is_some_and(|bytes| bytes == [0; 16])
    {
        return Ok(None);
    }
    let (count, header, rows, class) = array_at(data, descriptor)?;
    let segment = header
        .checked_sub(16)
        .ok_or_else(|| invalid("Item-string sandbox-perk header has no framing prefix"))?;
    let row_bytes = count
        .checked_mul(ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
        .ok_or_else(|| invalid("Item-string sandbox-perk array overflowed"))?;
    let end = rows
        .checked_add(row_bytes)
        .ok_or_else(|| invalid("Item-string sandbox-perk array overflowed"))?;
    if count > 64 || class != ITEM_STRING_SANDBOX_PERK_ROW_CLASS || rows != header + 16 {
        return Err(invalid(
            "Item-string tag has an incompatible sandbox-perk companion array",
        ));
    }
    let segment = data
        .get(segment..end)
        .ok_or_else(|| invalid("Item-string sandbox-perk companion is truncated"))?;
    validate_item_string_sandbox_perk_segment(segment)?;
    Ok(Some(segment))
}

pub(super) fn item_string_sandbox_perk_count(data: &[u8]) -> AuthoringResult<usize> {
    item_string_sandbox_perk_segment(data)?
        .map(|segment| {
            usize::try_from(read_u64(segment, 16)?)
                .map_err(|_| invalid("Item-string sandbox-perk count does not fit memory"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) fn validate_weapon_sandbox_perk_parallelism(
    definition: &[u8],
    strings: &[u8],
    sandbox_perk_string_template: &[u8],
) -> AuthoringResult<()> {
    let definition_count = weapon_sandbox_perks(definition)?.len();
    let string_count = item_string_sandbox_perk_count(strings)?;
    if definition_count != string_count {
        return Err(validation(format!(
            "Weapon definition has {definition_count} sandbox-perk rows but its item-string companion has {string_count}"
        )));
    }
    if let Some(segment) = item_string_sandbox_perk_segment(strings)? {
        validate_item_string_sandbox_perk_segment(sandbox_perk_string_template)?;
        let template_row = sandbox_perk_string_template
            .get(32..32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
            .ok_or_else(|| invalid("Item-string sandbox-perk template row is truncated"))?;
        let framing_matches = segment.get(8..16) == sandbox_perk_string_template.get(8..16)
            && segment.get(24..32) == sandbox_perk_string_template.get(24..32);
        let rows_match = (0..string_count).all(|index| {
            let row = 32 + index * ITEM_STRING_SANDBOX_PERK_ROW_SIZE;
            segment.get(row..row + ITEM_STRING_SANDBOX_PERK_ROW_SIZE) == Some(template_row)
        });
        if !framing_matches || !rows_match {
            return Err(validation(
                "Weapon item-string sandbox-perk companion differs from the audited stock exemplar",
            ));
        }
    }
    Ok(())
}

pub(super) fn canonical_item_sandbox_perk_string_template(
    manager: &PackageManager,
    item_strings: &[u8],
) -> AuthoringResult<Vec<u8>> {
    let (count, _, rows, _) = array_at(item_strings, 8)?;
    for item_index in 0..count {
        let string_tag = TagHash(read_u32(
            item_strings,
            rows + item_index * ITEM_ROW_SIZE + 16,
        )?);
        let Ok(strings) = read_tag(manager, string_tag, "stock item-string exemplar") else {
            continue;
        };
        let Ok(Some(segment)) = item_string_sandbox_perk_segment(&strings) else {
            continue;
        };
        let mut template = segment
            .get(..32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
            .ok_or_else(|| invalid("Stock item-string sandbox-perk row is truncated"))?
            .to_owned();
        write_u64(&mut template, 16, 1)?;
        validate_item_string_sandbox_perk_segment(&template)?;
        return Ok(template);
    }
    Err(invalid(
        "No stock item-string row provides a sandbox-perk companion exemplar",
    ))
}

pub(super) fn canonical_weapon_sandbox_perk_row_template(
    manager: &PackageManager,
    item_table: &[u8],
) -> AuthoringResult<[u8; ITEM_SANDBOX_PERK_ROW_SIZE]> {
    let (count, _, rows, _) = array_at(item_table, 8)?;
    for item_index in 0..count {
        let definition_tag = TagHash(read_u32(
            item_table,
            rows + item_index * ITEM_ROW_SIZE + 16,
        )?);
        let Ok(definition) = read_tag(manager, definition_tag, "stock sandbox-perk exemplar")
        else {
            continue;
        };
        if !matches!(
            weapon_damage_carrier(&definition),
            Ok(WeaponDamageCarrier::Fixed { .. })
        ) {
            continue;
        }
        let Some(row) = weapon_sandbox_perk_rows(&definition)?.into_iter().next() else {
            continue;
        };
        return row
            .try_into()
            .map_err(|_| invalid("Stock sandbox-perk exemplar has the wrong row size"));
    }
    Err(invalid(
        "No stock weapon definition provides a compatible sandbox-perk row exemplar",
    ))
}

pub(super) fn set_item_string_sandbox_perk_count(
    data: &mut Vec<u8>,
    count: usize,
    one_row_template: &[u8],
) -> AuthoringResult<()> {
    if count > 64 {
        return Err(invalid(
            "An item-string sandbox-perk companion cannot exceed 64 rows",
        ));
    }
    validate_item_string_sandbox_perk_segment(one_row_template)?;
    if read_u64(one_row_template, 16)? != 1 {
        return Err(invalid(
            "Item-string sandbox-perk template must contain exactly one row",
        ));
    }
    if item_string_sandbox_perk_count(data)? == count {
        return Ok(());
    }
    let descriptor = item_string_sandbox_perk_descriptor(data)?;
    if count == 0 {
        write_bytes(data, descriptor, &[0; 16])?;
        return Ok(());
    }
    while data.len() % 16 != 0 {
        data.push(0);
    }
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = data.len();
    data.extend_from_slice(
        &u64::try_from(count)
            .map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?
            .to_le_bytes(),
    );
    data.extend_from_slice(&ITEM_STRING_SANDBOX_PERK_ROW_CLASS.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    let row = one_row_template
        .get(32..32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE)
        .ok_or_else(|| invalid("Item-string sandbox-perk template row is truncated"))?;
    for _ in 0..count {
        data.extend_from_slice(row);
    }
    write_u64(
        data,
        descriptor,
        u64::try_from(count).map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?,
    )?;
    write_relative_pointer(data, descriptor + 8, header)?;
    if item_string_sandbox_perk_count(data)? != count {
        return Err(validation(
            "Authored item-string sandbox-perk companion has the wrong row count",
        ));
    }
    Ok(())
}

pub(super) fn set_weapon_base_sandbox_perks(
    data: &mut Vec<u8>,
    perks: &[u16],
    row_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
) -> AuthoringResult<()> {
    if perks.len() > 64 || perks.contains(&u16::MAX) {
        return Err(invalid(
            "Base sandbox-perk rows must contain at most 64 active indices",
        ));
    }
    if perks.iter().copied().collect::<BTreeSet<_>>().len() != perks.len() {
        return Err(invalid("Base sandbox-perk rows cannot contain duplicates"));
    }
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
    if resource < 4 || read_u32(data, resource - 4)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err(invalid("Weapon has no recognized investment resource"));
    }
    let descriptor = resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET;
    let current_layout = if data
        .get(descriptor..descriptor + 16)
        .is_some_and(|bytes| bytes == [0; 16])
    {
        None
    } else {
        let (count, _, rows, class) = array_at(data, descriptor)?;
        if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
            return Err(invalid(
                "Weapon has an unknown or oversized base sandbox-perk array",
            ));
        }
        Some((count, rows))
    };
    if let Some((count, rows)) = current_layout.filter(|(count, _)| *count == perks.len()) {
        for (index, &perk) in perks.iter().enumerate() {
            write_u16(data, rows + index * ITEM_SANDBOX_PERK_ROW_SIZE, perk)?;
        }
        debug_assert_eq!(count, perks.len());
    } else if perks.is_empty() {
        write_bytes(data, descriptor, &[0; 16])?;
    } else {
        while data.len() % 16 != 0 {
            data.push(0);
        }
        // Nested arrays require the relocation marker before their count/class header.
        // Without it Sunrise's production reader rejects the otherwise valid perk rows.
        data.extend_from_slice(&[0; 8]);
        data.extend_from_slice(&NESTED_ARRAY_TRAILER);
        let header = data.len();
        data.extend_from_slice(
            &u64::try_from(perks.len())
                .map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?
                .to_le_bytes(),
        );
        data.extend_from_slice(&ITEM_SANDBOX_PERK_ROW_CLASS.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        for &perk in perks {
            let mut row = *row_template;
            write_u16(&mut row, 0, perk)?;
            data.extend_from_slice(&row);
        }
        write_u64(
            data,
            descriptor,
            u64::try_from(perks.len())
                .map_err(|_| invalid("Sandbox-perk row count does not fit 64 bits"))?,
        )?;
        write_relative_pointer(data, descriptor + 8, header)?;
    }
    if weapon_sandbox_perks(data)? != perks {
        return Err(validation(
            "Authored weapon did not retain its requested base sandbox-perk indices",
        ));
    }
    // Perk arrays also belong to plugs, which have no weapon socket topology.
    // Damage-carrier validation belongs to the weapon damage authoring operations.
    Ok(())
}

pub(super) fn set_weapon_base_sandbox_perks_with_strings(
    definition: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    perks: &[u16],
    definition_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    string_template: &[u8],
) -> AuthoringResult<()> {
    set_weapon_base_sandbox_perks(definition, perks, definition_template)?;
    set_item_string_sandbox_perk_count(strings, perks.len(), string_template)?;
    validate_weapon_sandbox_perk_parallelism(definition, strings, string_template)
}

pub(super) fn set_weapon_fixed_damage_type(
    data: &mut Vec<u8>,
    damage_type: ModernDamageType,
    family: WeaponDamageCarrierFamily,
    row_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
) -> AuthoringResult<()> {
    if fixed_damage_perk(read_u16(row_template, 0)?).is_none() {
        return Err(invalid(
            "Sandbox-perk row template is not a recognized elemental row",
        ));
    }
    let mut perks = weapon_sandbox_perks(data)?;
    let existing = perks
        .iter()
        .position(|perk| fixed_damage_perk(*perk).is_some());
    let requested = family.base_sandbox_perk_index(damage_type.shared());
    match (existing, requested) {
        (Some(index), Some(requested)) => perks[index] = requested,
        (None, Some(requested)) => perks.push(requested),
        (Some(index), None) => {
            perks.remove(index);
        }
        (None, None) => {}
    }
    set_weapon_base_sandbox_perks(data, &perks, row_template)?;
    if damage_type == ModernDamageType::Kinetic {
        if weapon_sandbox_perks(data)?
            .into_iter()
            .any(|perk| fixed_damage_perk(perk).is_some())
        {
            return Err(validation(
                "Authored weapon retained a fixed elemental damage perk",
            ));
        }
        return Ok(());
    }
    if weapon_damage_carrier(data)?
        != (WeaponDamageCarrier::Fixed {
            family,
            damage_type,
        })
    {
        return Err(validation(
            "Authored weapon did not retain the requested damage perk",
        ));
    }
    Ok(())
}

pub(super) fn validate_damage_plug_item_rows(
    item_table: &[u8],
    rows: usize,
    count: usize,
) -> AuthoringResult<()> {
    for (index, hash) in [
        (ARC_DAMAGE_PLUG_ITEM_INDEX, ARC_DAMAGE_PLUG_ITEM_HASH),
        (SOLAR_DAMAGE_PLUG_ITEM_INDEX, SOLAR_DAMAGE_PLUG_ITEM_HASH),
        (VOID_DAMAGE_PLUG_ITEM_INDEX, VOID_DAMAGE_PLUG_ITEM_HASH),
    ] {
        let index = usize::from(index);
        if index >= count || read_u32(item_table, rows + index * ITEM_ROW_SIZE)? != hash {
            return Err(invalid(format!(
                "Installed elemental carrier plug index {index} does not resolve to 0x{hash:08X}"
            )));
        }
    }
    Ok(())
}

pub(super) fn type_68_row_is_self_contained(data: &[u8], row: usize) -> AuthoringResult<bool> {
    for descriptor in [
        row + ITEM_ORDINARY_SOCKET_RANDOMIZED_SELECTION_PROGRAM_OFFSET,
        row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
    ] {
        if read_u64(data, descriptor)? != 0 || read_i64(data, descriptor + 8)? != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn set_weapon_plug_damage_type(
    data: &mut Vec<u8>,
    damage_type: ModernDamageType,
    topology_donor: Option<&[u8]>,
    row_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
) -> AuthoringResult<()> {
    if damage_type == ModernDamageType::Kinetic {
        return Err(invalid(
            "A type-68 elemental carrier cannot represent Kinetic damage",
        ));
    }
    let damage_lanes = weapon_damage_socket_lanes(data)?;
    let [(lane, row)] = damage_lanes.as_slice() else {
        return Err(invalid(
            "Plug-driven damage requires exactly one native type-68 carrier lane in the gameplay definition; Parhelion will not repurpose a disabled, mod, or unrelated socket",
        ));
    };
    let lane = *lane;
    let row = *row;
    if let Some(topology_donor) = topology_donor {
        let topology_lanes = weapon_damage_socket_lanes(topology_donor)?;
        let [(_, source_row)] = topology_lanes.as_slice() else {
            return Err(invalid(
                "The target-slot presentation donor does not have one unambiguous type-68 carrier lane",
            ));
        };
        if !type_68_row_is_self_contained(topology_donor, *source_row)? {
            return Err(invalid(
                "The target-slot type-68 carrier uses pointer-backed choices that cannot be transplanted independently",
            ));
        }
        let source = topology_donor
            .get(*source_row..*source_row + ITEM_ORDINARY_SOCKET_ROW_SIZE)
            .ok_or_else(|| invalid("Target-slot type-68 carrier row is truncated"))?;
        write_bytes(data, row, source)?;
    }
    write_u16(
        data,
        row + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
        WeaponDamageCarrierFamily::PlugDriven
            .default_plug_item_index(damage_type.shared())
            .ok_or_else(|| invalid("Requested damage has no stock type-68 carrier plug"))?,
    )?;
    set_weapon_fixed_damage_type(
        data,
        ModernDamageType::Kinetic,
        WeaponDamageCarrierFamily::ModernFixed,
        row_template,
    )?;
    if weapon_damage_carrier(data)? != (WeaponDamageCarrier::PlugDriven { damage_type, lane }) {
        return Err(validation(
            "Authored weapon did not retain its requested plug-driven damage carrier",
        ));
    }
    Ok(())
}

pub(super) fn apply_weapon_slot_and_damage_overrides(
    data: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    overrides: &WeaponCloneOverrides,
    target_slot_carrier: Option<&ResolvedDamageCarrierSource>,
    sandbox_perk_definition_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    sandbox_perk_string_template: &[u8],
) -> AuthoringResult<()> {
    if overrides.inventory_slot.is_none()
        && overrides.modern_damage_type.is_none()
        && overrides.base_sandbox_perks.is_none()
    {
        return Ok(());
    }
    validate_weapon_sandbox_perk_parallelism(data, strings, sandbox_perk_string_template)?;

    let donor_slot = weapon_inventory_slot(data)?;
    if weapon_equipment_slot(data)? != donor_slot {
        return Err(invalid(
            "Weapon inventory bucket and equipment slot disagree",
        ));
    }
    let donor_carrier = weapon_damage_carrier(data)?;

    if let Some(perks) = &overrides.base_sandbox_perks {
        set_weapon_base_sandbox_perks_with_strings(
            data,
            strings,
            perks,
            sandbox_perk_definition_template,
            sandbox_perk_string_template,
        )?;
    }

    let authored_slot = overrides.inventory_slot.unwrap_or(donor_slot);
    let base_damage = weapon_damage_descriptor(data)?;
    let requested_damage = overrides
        .modern_damage_type
        .map(|damage_type| {
            if damage_type == ModernDamageType::Kinetic {
                WeaponDamageDescriptor::Empty
            } else {
                WeaponDamageDescriptor::Elemental(damage_type)
            }
        })
        .unwrap_or(base_damage);

    if let Some(inventory_slot) = overrides.inventory_slot {
        set_weapon_inventory_slot(data, inventory_slot)?;
    }
    if let Some(damage_type) = overrides.modern_damage_type {
        // Damage topology follows the gameplay definition, not its inventory placement.
        // A stock source is needed only when introducing a previously absent carrier.
        let carrier_family = donor_carrier
            .family()
            .or_else(|| target_slot_carrier.map(|source| source.family));
        match (damage_type, carrier_family) {
            (ModernDamageType::Kinetic, _) => {
                if !weapon_damage_socket_lanes(data)?.is_empty() {
                    return Err(invalid(
                        "The gameplay definition carries elemental damage through a type-68 socket; removing that carrier for Kinetic damage is not package-proven",
                    ));
                }
                set_weapon_fixed_damage_type(
                    data,
                    damage_type,
                    WeaponDamageCarrierFamily::ModernFixed,
                    sandbox_perk_definition_template,
                )?;
            }
            (_, Some(WeaponDamageCarrierFamily::PlugDriven)) => {
                set_weapon_plug_damage_type(
                    data,
                    damage_type,
                    target_slot_carrier.and_then(|source| source.topology_definition.as_deref()),
                    sandbox_perk_definition_template,
                )?;
            }
            (
                _,
                Some(
                    family @ (WeaponDamageCarrierFamily::LegacyFixed
                    | WeaponDamageCarrierFamily::ModernFixed),
                ),
            ) => {
                if !weapon_damage_socket_lanes(data)?.is_empty() {
                    return Err(invalid(
                        "The gameplay definition already has a type-68 elemental carrier; adding a parent damage marker would duplicate the carrier",
                    ));
                }
                set_weapon_fixed_damage_type(
                    data,
                    damage_type,
                    family,
                    sandbox_perk_definition_template,
                )?;
            }
            (_, None) => {
                return Err(invalid(
                    "No compatible stock target-slot damage carrier topology was found",
                ));
            }
        }
    }
    let actual_damage = weapon_damage_descriptor(data)?;
    set_item_string_sandbox_perk_count(
        strings,
        weapon_sandbox_perks(data)?.len(),
        sandbox_perk_string_template,
    )?;
    if weapon_inventory_slot(data)? != authored_slot || actual_damage != requested_damage {
        return Err(validation(
            "Authored weapon slot/damage pair does not match its requested values",
        ));
    }
    validate_weapon_sandbox_perk_parallelism(data, strings, sandbox_perk_string_template)?;
    Ok(())
}

pub(super) fn weapon_version_array(data: &[u8]) -> AuthoringResult<Option<ItemVersionArray>> {
    item_version_array(data).map_err(invalid)
}

pub(super) fn set_weapon_power_cap(data: &mut [u8], power_cap_group: u16) -> AuthoringResult<()> {
    let version = weapon_version_array(data)?
        .ok_or_else(|| invalid("Weapon quality block has no version rows"))?;
    for index in 0..version.groups.len() {
        write_u16(
            data,
            version.rows + index * ITEM_VERSION_ROW_SIZE,
            power_cap_group,
        )?;
    }
    let authored = weapon_version_array(data)?;
    if authored.as_ref().is_none_or(|authored| {
        authored.groups.is_empty()
            || authored
                .groups
                .iter()
                .any(|group| *group != power_cap_group)
    }) {
        return Err(validation(
            "Authored weapon did not retain the requested power-cap group",
        ));
    }
    Ok(())
}

pub(super) fn set_weapon_power_cap_groups(data: &mut [u8], groups: &[u16]) -> AuthoringResult<()> {
    let version = weapon_version_array(data)?
        .ok_or_else(|| invalid("Weapon quality block has no version rows"))?;
    if groups.len() != version.groups.len() {
        return Err(invalid(format!(
            "Advanced power-cap override has {} version rows but the gameplay donor has {}",
            groups.len(),
            version.groups.len()
        )));
    }
    if groups.is_empty() {
        return Err(invalid(
            "Advanced power-cap groups must contain at least one table index",
        ));
    }
    for (index, group) in groups.iter().copied().enumerate() {
        write_u16(data, version.rows + index * ITEM_VERSION_ROW_SIZE, group)?;
    }
    if weapon_version_array(data)?.is_none_or(|authored| authored.groups != groups) {
        return Err(validation(
            "Authored weapon did not retain every requested power-cap version row",
        ));
    }
    Ok(())
}
