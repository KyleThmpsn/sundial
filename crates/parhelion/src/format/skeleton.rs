//! The empty, stock-shaped envelope of a newly registered patch-zero package.
use super::*;

const ENTRY_TABLE_CLASS: u32 = 0x8080_9EF3;
const BLOCK_TABLE_CLASS: u32 = 0x8080_9EEE;

/// Builds the empty, stock-shaped envelope for a newly registered patch-zero package.
///
/// Entry prefixes and shared-tag enrollment rows are already final. Payload locations and block
/// records remain zeroed so the caller can append plaintext blocks, update the metadata hash, and
/// relocate the opaque trailer without carrying bytes from any stock package generation.
pub(crate) fn build_standalone_package_skeleton(
    package_id: u16,
    entry_prefixes: &[[u8; 8]],
    block_count: usize,
    shared_tag_enrollments: &[(u32, u32)],
) -> AuthoringResult<Vec<u8>> {
    const PACKAGE_TABLES_OFFSET: usize = 0x1000;

    let entry_count = entry_prefixes.len();
    if !(MIN_PACKAGE_ID..=MAX_PACKAGE_ID).contains(&package_id) {
        return Err(invalid(format!(
            "Standalone package id {package_id:04X} is outside {MIN_PACKAGE_ID:03X}..={MAX_PACKAGE_ID:03X}"
        )));
    }
    if !(1..=MAX_ENTRY_COUNT).contains(&entry_count) {
        return Err(invalid(format!(
            "Standalone package entry count {entry_count} is outside 1..={MAX_ENTRY_COUNT}"
        )));
    }
    if block_count == 0 || block_count > MAX_BLOCK_COUNT {
        return Err(invalid(format!(
            "Standalone package block count {block_count} is outside 1..={MAX_BLOCK_COUNT}"
        )));
    }
    let entry_count_u32 = u32::try_from(entry_count)
        .map_err(|_| invalid("Standalone entry count exceeds 32 bits"))?;
    let entry_count_u64 = u64::try_from(entry_count)
        .map_err(|_| invalid("Standalone entry count exceeds 64 bits"))?;
    let block_count_u32 = u32::try_from(block_count)
        .map_err(|_| invalid("Standalone block count exceeds 32 bits"))?;
    let block_count_u64 = u64::try_from(block_count)
        .map_err(|_| invalid("Standalone block count exceeds 64 bits"))?;

    let entry_table_offset = PACKAGE_TABLES_OFFSET
        .checked_add(ENTRY_TABLE_POINTER_ADJUSTMENT)
        .ok_or_else(|| invalid("Standalone entry-table offset overflowed"))?;
    let entry_bytes = entry_count
        .checked_mul(ENTRY_HEADER_SIZE)
        .ok_or_else(|| invalid("Standalone entry-table size overflowed"))?;
    let block_table_offset = entry_table_offset
        .checked_add(entry_bytes)
        .and_then(|offset| offset.checked_add(PackageLayout::ENTRY_TABLE_TRAILER_SIZE))
        .ok_or_else(|| invalid("Standalone block-table offset overflowed"))?;
    let block_bytes = block_count
        .checked_mul(BLOCK_HEADER_SIZE)
        .ok_or_else(|| invalid("Standalone block-table size overflowed"))?;
    let block_table_end = block_table_offset
        .checked_add(block_bytes)
        .ok_or_else(|| invalid("Standalone block-table end overflowed"))?;
    let shared_tag_bytes = shared_tag_enrollments
        .len()
        .checked_mul(SHARED_TAG_TABLE_ROW_SIZE)
        .ok_or_else(|| invalid("Standalone shared-tag table size overflowed"))?;
    let shared_tag_header_offset = if shared_tag_enrollments.is_empty() {
        0
    } else {
        block_table_end
            .checked_add(DYNAMIC_ARRAY_HEADER_SIZE)
            .ok_or_else(|| invalid("Standalone shared-tag header offset overflowed"))?
    };
    let package_tables_end = if shared_tag_enrollments.is_empty() {
        block_table_end
    } else {
        shared_tag_header_offset
            .checked_add(DYNAMIC_ARRAY_HEADER_SIZE)
            .and_then(|offset| offset.checked_add(shared_tag_bytes))
            .ok_or_else(|| invalid("Standalone package-tables end overflowed"))?
    };
    let package_tables_size = package_tables_end
        .checked_sub(PACKAGE_TABLES_OFFSET)
        .ok_or_else(|| invalid("Standalone package-tables size underflowed"))?;
    let mut bytes = vec![0u8; package_tables_end];

    write_u16(&mut bytes, VERSION_OFFSET, SHADOWKEEP_HEADER_VERSION)?;
    write_u16(&mut bytes, PLATFORM_OFFSET, W64_PLATFORM)?;
    write_u16(&mut bytes, PACKAGE_ID_OFFSET, package_id)?;
    bytes[LOCALE_CHECK_ENABLE_OFFSET] = 1;
    write_u64(&mut bytes, BUILD_SIGNATURE_OFFSET, SUNDIAL_BUILD_SIGNATURE)?;
    write_u64(&mut bytes, BUILD_TIME_OFFSET, SUNDIAL_BUILD_TIME)?;
    write_u32(&mut bytes, CONTENT_BUILD_OFFSET, SHADOWKEEP_CONTENT_BUILD)?;
    write_u32(
        &mut bytes,
        CONTENT_REVISION_OFFSET,
        SHADOWKEEP_CONTENT_REVISION,
    )?;
    write_u16(&mut bytes, PATCH_ID_OFFSET, 0)?;
    write_u32(
        &mut bytes,
        HEADER_SIGNATURE_OFFSET_OFFSET,
        PACKAGE_FILE_ALIGNMENT as u32,
    )?;
    write_u32(&mut bytes, ENTRY_COUNT_OFFSET, entry_count_u32)?;
    write_u32(&mut bytes, BLOCK_COUNT_OFFSET, block_count_u32)?;
    write_u32(
        &mut bytes,
        PACKAGE_TABLES_DATA_POINTER_OFFSET,
        PACKAGE_TABLES_OFFSET as u32,
    )?;
    write_u32(
        &mut bytes,
        PACKAGE_TABLES_DATA_SIZE_OFFSET,
        u32::try_from(package_tables_size)
            .map_err(|_| invalid("Standalone package-tables size exceeds 32 bits"))?,
    )?;
    write_u32(&mut bytes, LOCALE_TOKEN_OFFSET, LOCALE_TOKENS[0])?;
    bytes[LOCALE_ID_OFFSET] = 0;

    write_u64(
        &mut bytes,
        PACKAGE_TABLES_OFFSET + PACKAGE_TABLES_THIS_SIZE_OFFSET,
        u64::try_from(package_tables_size)
            .map_err(|_| invalid("Standalone package-tables size exceeds 64 bits"))?,
    )?;
    write_u64(&mut bytes, PACKAGE_TABLES_OFFSET + 8, 1)?;
    write_u64(
        &mut bytes,
        PACKAGE_TABLES_OFFSET + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET,
        entry_count_u64,
    )?;
    let entry_pointer = PACKAGE_TABLES_OFFSET
        + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    write_i64(
        &mut bytes,
        entry_pointer,
        i64::try_from(entry_table_offset - DYNAMIC_ARRAY_HEADER_SIZE)
            .and_then(|target| i64::try_from(entry_pointer).map(|pointer| target - pointer))
            .map_err(|_| invalid("Standalone entry descriptor displacement overflowed"))?,
    )?;
    write_u64(
        &mut bytes,
        PACKAGE_TABLES_OFFSET + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET,
        block_count_u64,
    )?;
    let block_pointer = PACKAGE_TABLES_OFFSET
        + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    write_i64(
        &mut bytes,
        block_pointer,
        i64::try_from(block_table_offset - DYNAMIC_ARRAY_HEADER_SIZE)
            .and_then(|target| i64::try_from(block_pointer).map(|pointer| target - pointer))
            .map_err(|_| invalid("Standalone block descriptor displacement overflowed"))?,
    )?;
    write_u32(
        &mut bytes,
        entry_table_offset - ENTRY_TABLE_COUNT_BACK,
        entry_count_u32,
    )?;
    write_u64(
        &mut bytes,
        block_table_offset - BLOCK_TABLE_COUNT_BACK,
        block_count_u64,
    )?;
    // These physical directories use the same native array marker as the shared-tag table.
    // Sunrise checks both identities before it can read any entry's payload.
    for (offset, element_class) in [
        (entry_table_offset, ENTRY_TABLE_CLASS),
        (block_table_offset, BLOCK_TABLE_CLASS),
    ] {
        write_u32(
            &mut bytes,
            offset - DYNAMIC_ARRAY_HEADER_SIZE - size_of::<u32>(),
            SHARED_TAG_TABLE_MARKER,
        )?;
        write_u32(&mut bytes, offset - size_of::<u64>(), element_class)?;
    }
    for (index, prefix) in entry_prefixes.iter().enumerate() {
        let row = entry_table_offset + index * ENTRY_HEADER_SIZE;
        bytes[row..row + prefix.len()].copy_from_slice(prefix);
    }

    if !shared_tag_enrollments.is_empty() {
        let shared_count = u64::try_from(shared_tag_enrollments.len())
            .map_err(|_| invalid("Standalone shared-tag count exceeds 64 bits"))?;
        let shared_descriptor = PACKAGE_TABLES_OFFSET + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET;
        write_u64(&mut bytes, shared_descriptor, shared_count)?;
        write_i64(
            &mut bytes,
            shared_descriptor + DYNAMIC_ARRAY_POINTER_OFFSET,
            i64::try_from(shared_tag_header_offset)
                .and_then(|target| {
                    i64::try_from(shared_descriptor + DYNAMIC_ARRAY_POINTER_OFFSET)
                        .map(|pointer| target - pointer)
                })
                .map_err(|_| invalid("Standalone shared-tag displacement overflowed"))?,
        )?;
        write_u32(
            &mut bytes,
            shared_tag_header_offset - size_of::<u32>(),
            SHARED_TAG_TABLE_MARKER,
        )?;
        write_u64(&mut bytes, shared_tag_header_offset, shared_count)?;
        write_u64(
            &mut bytes,
            shared_tag_header_offset + size_of::<u64>(),
            SHARED_TAG_TABLE_CLASS,
        )?;
        let rows_offset = shared_tag_header_offset + DYNAMIC_ARRAY_HEADER_SIZE;
        for (index, (owner, companion)) in shared_tag_enrollments.iter().enumerate() {
            let row = rows_offset + index * SHARED_TAG_TABLE_ROW_SIZE;
            write_u32(&mut bytes, row, *owner)?;
            write_u32(&mut bytes, row + size_of::<u32>(), *companion)?;
        }
    }

    let package_tables_hash = Sha1::digest(&bytes[PACKAGE_TABLES_OFFSET..package_tables_end]);
    bytes[PACKAGE_TABLES_DATA_HASH_OFFSET..PACKAGE_TABLES_DATA_HASH_OFFSET + REGION_HASH_SIZE]
        .copy_from_slice(&package_tables_hash);
    let mut trailer = vec![0u8; PACKAGE_FILE_ALIGNMENT];
    trailer[..OPAQUE_TRAILER_MARKER.len()].copy_from_slice(&OPAQUE_TRAILER_MARKER);
    append_opaque_trailer(&mut bytes, &trailer)?;
    let file_size = u32::try_from(bytes.len())
        .map_err(|_| invalid("Standalone package exceeds the 32-bit file-size field"))?;
    write_u32(
        &mut bytes,
        OPAQUE_TRAILER_OFFSET_FIELD,
        file_size - PACKAGE_FILE_ALIGNMENT as u32,
    )?;
    write_u32(&mut bytes, FILE_SIZE_OFFSET, file_size)?;

    PackageLayout::parse(&bytes).map_err(|error| {
        invalid(format!(
            "Standalone package skeleton could not be reopened: {error}"
        ))
    })?;
    Ok(bytes)
}

pub(super) fn validate_locale_fields(bytes: &[u8]) -> AuthoringResult<()> {
    let check_enable = bytes[LOCALE_CHECK_ENABLE_OFFSET];
    let locale_token = read_u32(bytes, LOCALE_TOKEN_OFFSET)?;
    let locale_id = usize::from(bytes[LOCALE_ID_OFFSET]);
    if check_enable != 0 {
        let expected = LOCALE_TOKENS.get(locale_id).ok_or_else(|| {
            invalid(format!(
                "Package locale id {locale_id} has no supported locale token"
            ))
        })?;
        if locale_token != *expected {
            return Err(invalid(format!(
                "Package locale token 0x{locale_token:08X} does not match locale id {locale_id}"
            )));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_hashed_region(
    bytes: &[u8],
    offset_field: usize,
    size_field: usize,
    hash_field: usize,
    opaque_trailer_offset: usize,
    name: &str,
    required: bool,
) -> AuthoringResult<Range<usize>> {
    let offset = read_u32(bytes, offset_field)? as usize;
    let size = read_u32(bytes, size_field)? as usize;
    let hash = checked_range(bytes, hash_field, REGION_HASH_SIZE, &format!("{name} hash"))?;
    if offset == 0 && size == 0 {
        if required {
            return Err(invalid(format!("Package {name} region is absent")));
        }
        return Ok(0..0);
    }
    if offset == 0 || size == 0 {
        return Err(invalid(format!(
            "Package {name} region has an incomplete offset/size descriptor"
        )));
    }
    if offset < HEADER_SIZE {
        return Err(invalid(format!(
            "Package {name} region overlaps the signed header"
        )));
    }
    let region = checked_range(bytes, offset, size, name)?;
    if region.end > opaque_trailer_offset {
        return Err(invalid(format!(
            "Package {name} region extends into the opaque trailing region"
        )));
    }
    if Sha1::digest(&bytes[region.clone()]).as_slice() != &bytes[hash] {
        return Err(invalid(format!(
            "Package {name} SHA-1 does not match its data"
        )));
    }
    Ok(region)
}

pub(super) fn ensure_contained(
    range: &Range<usize>,
    container: &Range<usize>,
    name: &str,
) -> AuthoringResult<()> {
    if range.start < container.start || range.end > container.end {
        return Err(invalid(format!(
            "Package {name} is outside the metadata region"
        )));
    }
    Ok(())
}

pub(super) fn validate_entry_block_ranges(
    bytes: &[u8],
    entry_range: &Range<usize>,
    block_count: usize,
) -> AuthoringResult<()> {
    for (index, offset) in (entry_range.start..entry_range.end)
        .step_by(ENTRY_HEADER_SIZE)
        .enumerate()
    {
        let block_info = read_u64(bytes, offset + 8)?;
        let logical_size = block_info >> 28;
        if logical_size == 0 {
            continue;
        }
        let starting_block =
            usize::try_from(block_info & 0x3FFF).expect("a 14-bit starting block fits usize");
        let starting_byte = (block_info >> 14) & 0x3FFF;
        let starting_byte = starting_byte << 4;
        let covered_bytes = starting_byte
            .checked_add(logical_size)
            .ok_or_else(|| invalid(format!("Entry {index} block range overflows")))?;
        let blocks_used = covered_bytes.div_ceil(BLOCK_SIZE as u64);
        let end_block = (starting_block as u64)
            .checked_add(blocks_used)
            .ok_or_else(|| invalid(format!("Entry {index} ending block overflows")))?;
        if starting_block >= block_count || end_block > block_count as u64 {
            return Err(invalid(format!(
                "Entry {index} references blocks outside the package directory"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_block_records(
    bytes: &[u8],
    block_range: &Range<usize>,
    package_patch: u16,
    opaque_trailer_offset: usize,
) -> AuthoringResult<()> {
    for (index, offset) in (block_range.start..block_range.end)
        .step_by(BLOCK_HEADER_SIZE)
        .enumerate()
    {
        let file_offset = read_u32(bytes, offset)? as usize;
        let stored_size = read_u32(bytes, offset + 4)? as usize;
        let patch_id = read_u16(bytes, offset + 8)?;
        let flags = read_u16(bytes, offset + 10)?;
        if stored_size > BLOCK_SIZE {
            return Err(invalid(format!(
                "Block {index} stored size {stored_size} exceeds {BLOCK_SIZE}"
            )));
        }
        if patch_id > package_patch {
            return Err(invalid(format!(
                "Block {index} refers to future patch {patch_id}"
            )));
        }
        if flags & !0x000F != 0 {
            return Err(invalid(format!(
                "Block {index} uses undocumented flags 0x{flags:04X}"
            )));
        }
        if patch_id == package_patch
            && file_offset
                .checked_add(stored_size)
                .is_none_or(|end| end > opaque_trailer_offset)
        {
            return Err(invalid(format!(
                "Block {index} extends into the current patch opaque trailing region"
            )));
        }
        if stored_size != 0 {
            let recorded_hash = &bytes[offset + 12..offset + 12 + REGION_HASH_SIZE];
            if recorded_hash.iter().all(|byte| *byte == 0) {
                return Err(invalid(format!("Block {index} has an empty SHA-1")));
            }
            if patch_id != package_patch {
                continue;
            }
            let stored_bytes = checked_range(
                bytes,
                file_offset,
                stored_size,
                &format!("Block {index} stored bytes"),
            )?;
            if Sha1::digest(&bytes[stored_bytes]).as_slice() != recorded_hash {
                return Err(invalid(format!(
                    "Block {index} in the current patch failed SHA-1 validation"
                )));
            }
        }
    }
    Ok(())
}
