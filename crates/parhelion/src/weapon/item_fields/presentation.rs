use super::*;

// The client checks these per-version icon overrides before the main icon's watermark layer.
// Stock Red War and Trials donors retain nonempty entries here even after cloning their icon.
const ITEM_STRING_WATERMARK_DESCRIPTOR: usize = 0xA8;
const ITEM_STRING_WATERMARK_ROW_CLASS: u32 = 0x8080_5F87;

pub(in crate::weapon) fn item_string_watermark_overrides(
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

pub(in crate::weapon) fn clear_item_string_watermark_overrides(
    data: &mut [u8],
) -> AuthoringResult<()> {
    let rows = item_string_watermark_overrides(data)?;
    // FFFF is the native no-override value: use our private icon's watermark, for every version.
    data[rows].fill(0xFF);
    Ok(())
}

pub(in crate::weapon) fn weapon_translation_topology(
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

pub(in crate::weapon) fn weapon_presentation_tuple(
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

pub(in crate::weapon) fn validate_weapon_translation_markers(data: &[u8]) -> AuthoringResult<()> {
    let root = relative_target(data, ITEM_TRANSLATION_BLOCK_POINTER_OFFSET)?;
    for offset in
        std::iter::once(TRANSLATION_ART_DESCRIPTOR_OFFSET).chain(TRANSLATION_DYE_DESCRIPTOR_OFFSETS)
    {
        let descriptor = root + offset;
        if read_u64(data, descriptor)? == 0 {
            continue;
        }
        let (_, header, _, _) = array_at(data, descriptor)?;
        // Sunrise's serialized array reader requires a tag-class marker before
        // the header, even though the client's pointer-based reader does not.
        if header < 4 || read_u32(data, header - 4)? >> 16 != 0x8080 {
            return Err(validation(format!(
                "Authored weapon translation array at 0x{descriptor:X} is missing its native header marker"
            )));
        }
    }
    Ok(())
}

pub(in crate::weapon) fn transplant_weapon_geometry(
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

pub(in crate::weapon) fn transplant_weapon_render_gear(
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

pub(in crate::weapon) fn unlock_weapon_shader_dyes(data: &mut Vec<u8>) -> AuthoringResult<()> {
    let mut arrays = weapon_render_dye_rows(data)?;
    if arrays[2].is_empty() {
        return Ok(());
    }
    // Keep the original appearance when no shader is selected, but let custom
    // shader dyes win. Locked channels previously took precedence over defaults.
    let locked = std::mem::take(&mut arrays[2]);
    let channels = locked
        .iter()
        .map(|row| row.channel_index)
        .collect::<BTreeSet<_>>();
    arrays[1].retain(|row| !channels.contains(&row.channel_index));
    arrays[1].extend(locked);
    set_weapon_render_dye_rows(data, &arrays)
}

pub(in crate::weapon) fn replace_translation_array(
    data: &mut Vec<u8>,
    descriptor: usize,
    row_class: u32,
    rows: &[u8],
    row_size: usize,
) -> AuthoringResult<()> {
    if row_size == 0 || rows.len() % row_size != 0 {
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
    // Serialized readers require the native marker immediately before an array
    // header. The client can follow an unmarked array, but Sunrise omits it from
    // the equipped character's model and material selections.
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&NESTED_ARRAY_TRAILER);
    let header = data.len();
    data.extend_from_slice(&[0; 16]);
    write_u32(data, header + 8, row_class)?;
    data.extend_from_slice(rows);
    set_array_count(data, descriptor, header, count)?;
    write_relative_pointer(data, descriptor + 8, header)?;
    Ok(())
}

pub(in crate::weapon) fn set_weapon_art_arrangements(
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

pub(in crate::weapon) fn weapon_art_arrangements(
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

pub(in crate::weapon) fn set_weapon_render_dye_rows(
    data: &mut Vec<u8>,
    arrays: &[Vec<WeaponDyeReferenceOverride>; 3],
) -> AuthoringResult<()> {
    let root = weapon_translation_topology(data)?.root;
    let mut encoded_arrays: [Vec<u8>; 3] = Default::default();
    for (array, rows) in arrays.iter().enumerate() {
        if rows.len() > 32 {
            return Err(invalid(format!(
                "Translation dye array {array} cannot contain more than 32 rows"
            )));
        }
        let encoded = &mut encoded_arrays[array];
        encoded.reserve(rows.len() * TRANSLATION_DYE_ROW_SIZE);
        for row in rows {
            encoded.push(row.channel_index as u8);
            encoded.push(0);
            encoded.extend_from_slice(&row.dye_reference_index.to_le_bytes());
        }
    }
    for (array, encoded) in encoded_arrays.iter().enumerate() {
        replace_translation_array(
            data,
            root + TRANSLATION_DYE_DESCRIPTOR_OFFSETS[array],
            TRANSLATION_DYE_ROW_CLASS,
            encoded,
            TRANSLATION_DYE_ROW_SIZE,
        )?;
    }
    let topology = weapon_translation_topology(data)?;
    let tuple = weapon_presentation_tuple(data, &topology)?;
    for (array, encoded) in encoded_arrays.iter().enumerate() {
        if tuple.dye_arrays[array] != *encoded {
            return Err(validation(format!(
                "Authored render-dye array {array} did not retain every requested row"
            )));
        }
    }
    Ok(())
}

pub(in crate::weapon) fn weapon_render_dye_rows(
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
