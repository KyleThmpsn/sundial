use super::*;

fn synthetic_package() -> Vec<u8> {
    synthetic_package_with_capacities(2, 3)
}

fn synthetic_package_with_capacities(entry_capacity: usize, block_capacity: usize) -> Vec<u8> {
    let entry_table_offset = 0x200usize;
    let entry_count = 2u32;
    let block_count = 3u32;
    let block_table_offset = entry_table_offset
        + entry_capacity * ENTRY_HEADER_SIZE
        + PackageLayout::ENTRY_TABLE_TRAILER_SIZE;
    let package_tables_data_offset = entry_table_offset - ENTRY_TABLE_POINTER_ADJUSTMENT;
    let shared_tag_count = 4usize;
    let shared_tag_header_offset =
        block_table_offset + block_capacity * BLOCK_HEADER_SIZE + DYNAMIC_ARRAY_HEADER_SIZE;
    let shared_tag_rows_offset = shared_tag_header_offset + DYNAMIC_ARRAY_HEADER_SIZE;
    let mut bytes =
        vec![0u8; shared_tag_rows_offset + shared_tag_count * SHARED_TAG_TABLE_ROW_SIZE];
    let package_tables_data_size = bytes.len() - package_tables_data_offset;
    bytes[VERSION_OFFSET..VERSION_OFFSET + 2]
        .copy_from_slice(&SHADOWKEEP_HEADER_VERSION.to_le_bytes());
    bytes[PLATFORM_OFFSET..PLATFORM_OFFSET + 2].copy_from_slice(&W64_PLATFORM.to_le_bytes());
    bytes[PACKAGE_ID_OFFSET..PACKAGE_ID_OFFSET + 2].copy_from_slice(&0x058fu16.to_le_bytes());
    bytes[LOCALE_CHECK_ENABLE_OFFSET] = 1;
    bytes[BUILD_SIGNATURE_OFFSET..BUILD_SIGNATURE_OFFSET + 8]
        .copy_from_slice(&0xE2CA_FC60_4440_ADDDu64.to_le_bytes());
    bytes[BUILD_TIME_OFFSET..BUILD_TIME_OFFSET + 8].copy_from_slice(&0x5F44_0756u64.to_le_bytes());
    bytes[CONTENT_BUILD_OFFSET..CONTENT_BUILD_OFFSET + 4]
        .copy_from_slice(&SHADOWKEEP_CONTENT_BUILD.to_le_bytes());
    bytes[CONTENT_REVISION_OFFSET..CONTENT_REVISION_OFFSET + 4]
        .copy_from_slice(&SHADOWKEEP_CONTENT_REVISION.to_le_bytes());
    bytes[PATCH_ID_OFFSET..PATCH_ID_OFFSET + 2].copy_from_slice(&2u16.to_le_bytes());
    bytes[HEADER_SIGNATURE_OFFSET_OFFSET..HEADER_SIGNATURE_OFFSET_OFFSET + 4]
        .copy_from_slice(&(PACKAGE_FILE_ALIGNMENT as u32).to_le_bytes());
    bytes[ENTRY_COUNT_OFFSET..ENTRY_COUNT_OFFSET + 4].copy_from_slice(&entry_count.to_le_bytes());
    bytes[BLOCK_COUNT_OFFSET..BLOCK_COUNT_OFFSET + 4].copy_from_slice(&block_count.to_le_bytes());
    bytes[PACKAGE_TABLES_DATA_POINTER_OFFSET..PACKAGE_TABLES_DATA_POINTER_OFFSET + 4]
        .copy_from_slice(
            &u32::try_from(package_tables_data_offset)
                .expect("synthetic offset should fit")
                .to_le_bytes(),
        );
    bytes[PACKAGE_TABLES_DATA_SIZE_OFFSET..PACKAGE_TABLES_DATA_SIZE_OFFSET + 4].copy_from_slice(
        &u32::try_from(package_tables_data_size)
            .expect("synthetic package-tables size should fit")
            .to_le_bytes(),
    );
    bytes[package_tables_data_offset + PACKAGE_TABLES_THIS_SIZE_OFFSET
        ..package_tables_data_offset + PACKAGE_TABLES_THIS_SIZE_OFFSET + 8]
        .copy_from_slice(
            &u64::try_from(package_tables_data_size)
                .expect("synthetic package-tables size should fit 64 bits")
                .to_le_bytes(),
        );
    bytes[package_tables_data_offset + 8..package_tables_data_offset + 16]
        .copy_from_slice(&1u64.to_le_bytes());
    bytes[package_tables_data_offset + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET
        ..package_tables_data_offset + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET + 8]
        .copy_from_slice(&u64::from(entry_count).to_le_bytes());
    let entry_pointer = package_tables_data_offset
        + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    bytes[entry_pointer..entry_pointer + 8].copy_from_slice(
        &i64::try_from(entry_table_offset - DYNAMIC_ARRAY_HEADER_SIZE - entry_pointer)
            .expect("synthetic entry pointer should fit")
            .to_le_bytes(),
    );
    bytes[package_tables_data_offset + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
        ..package_tables_data_offset + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET + 8]
        .copy_from_slice(&u64::from(block_count).to_le_bytes());
    let block_pointer = package_tables_data_offset
        + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    bytes[block_pointer..block_pointer + 8].copy_from_slice(
        &i64::try_from(block_table_offset - DYNAMIC_ARRAY_HEADER_SIZE - block_pointer)
            .expect("synthetic block pointer should fit")
            .to_le_bytes(),
    );
    let shared_tag_count_u64 = shared_tag_count as u64;
    bytes[package_tables_data_offset + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
        ..package_tables_data_offset + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET + 8]
        .copy_from_slice(&shared_tag_count_u64.to_le_bytes());
    let shared_tag_pointer = package_tables_data_offset
        + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    bytes[shared_tag_pointer..shared_tag_pointer + 8].copy_from_slice(
        &i64::try_from(shared_tag_header_offset - shared_tag_pointer)
            .expect("synthetic shared-tag pointer should fit")
            .to_le_bytes(),
    );
    let opaque_trailer_offset = bytes.len();
    bytes.extend_from_slice(&vec![0u8; PACKAGE_FILE_ALIGNMENT]);
    bytes[opaque_trailer_offset..opaque_trailer_offset + 4].copy_from_slice(&OPAQUE_TRAILER_MARKER);
    let file_size = u32::try_from(bytes.len()).expect("synthetic file size should fit");
    bytes[OPAQUE_TRAILER_OFFSET_FIELD..OPAQUE_TRAILER_OFFSET_FIELD + 4].copy_from_slice(
        &u32::try_from(opaque_trailer_offset)
            .expect("synthetic trailer offset should fit")
            .to_le_bytes(),
    );
    bytes[FILE_SIZE_OFFSET..FILE_SIZE_OFFSET + 4].copy_from_slice(&file_size.to_le_bytes());
    bytes[LOCALE_TOKEN_OFFSET..LOCALE_TOKEN_OFFSET + 4]
        .copy_from_slice(&LOCALE_TOKENS[0].to_le_bytes());
    bytes[LOCALE_ID_OFFSET] = 0;
    bytes[entry_table_offset - ENTRY_TABLE_COUNT_BACK
        ..entry_table_offset - ENTRY_TABLE_COUNT_BACK + 4]
        .copy_from_slice(
            &u32::try_from(entry_capacity)
                .expect("synthetic entry capacity should fit")
                .to_le_bytes(),
        );
    bytes[block_table_offset - BLOCK_TABLE_COUNT_BACK
        ..block_table_offset - BLOCK_TABLE_COUNT_BACK + 8]
        .copy_from_slice(
            &u64::try_from(block_capacity)
                .expect("synthetic block capacity should fit")
                .to_le_bytes(),
        );
    bytes[entry_table_offset..entry_table_offset + 4]
        .copy_from_slice(&0x8080_4A53u32.to_le_bytes());
    bytes[entry_table_offset + 4..entry_table_offset + 8]
        .copy_from_slice(&(0x10u32 << 9).to_le_bytes());
    bytes[entry_table_offset + ENTRY_HEADER_SIZE..entry_table_offset + ENTRY_HEADER_SIZE + 4]
        .copy_from_slice(&SHARED_TAG_COMPANION_CLASS.to_le_bytes());
    bytes[entry_table_offset + ENTRY_HEADER_SIZE + 4..entry_table_offset + ENTRY_HEADER_SIZE + 8]
        .copy_from_slice(&(0x08u32 << 9).to_le_bytes());
    bytes[shared_tag_header_offset - 4..shared_tag_header_offset]
        .copy_from_slice(&SHARED_TAG_TABLE_MARKER.to_le_bytes());
    bytes[shared_tag_header_offset..shared_tag_header_offset + 8]
        .copy_from_slice(&shared_tag_count_u64.to_le_bytes());
    bytes[shared_tag_header_offset + 8..shared_tag_header_offset + 16]
        .copy_from_slice(&SHARED_TAG_TABLE_CLASS.to_le_bytes());
    for row in 0..shared_tag_count {
        let offset = shared_tag_rows_offset + row * SHARED_TAG_TABLE_ROW_SIZE;
        bytes[offset..offset + 4]
            .copy_from_slice(&u32::from(TagHash::new(0x058f, 0)).to_le_bytes());
        bytes[offset + 4..offset + 8]
            .copy_from_slice(&u32::from(TagHash::new(0x058f, 1)).to_le_bytes());
    }
    let package_tables_hash = Sha1::digest(
        &bytes[package_tables_data_offset..package_tables_data_offset + package_tables_data_size],
    );
    bytes[PACKAGE_TABLES_DATA_HASH_OFFSET..PACKAGE_TABLES_DATA_HASH_OFFSET + REGION_HASH_SIZE]
        .copy_from_slice(&package_tables_hash);
    bytes
}

fn synthetic_patch_with_physical_payload(payload: &[u8]) -> Vec<u8> {
    build_test_package_with_physical_payload(0x0AA0, 3, payload)
        .expect("synthetic physical package should build")
}

fn rehash_package_tables(bytes: &mut [u8]) {
    let offset = read_u32(bytes, PACKAGE_TABLES_DATA_POINTER_OFFSET)
        .expect("package-tables offset should parse") as usize;
    let size = read_u32(bytes, PACKAGE_TABLES_DATA_SIZE_OFFSET)
        .expect("package-tables size should parse") as usize;
    let hash = Sha1::digest(&bytes[offset..offset + size]);
    bytes[PACKAGE_TABLES_DATA_HASH_OFFSET..PACKAGE_TABLES_DATA_HASH_OFFSET + REGION_HASH_SIZE]
        .copy_from_slice(&hash);
}

fn append_zero_length_shared_pair(source: &[u8], target_patch: u8) -> Vec<u8> {
    let layout = PackageLayout::parse(source).expect("source package should parse");
    let trailer = layout
        .opaque_trailer(source)
        .expect("source trailer should parse")
        .to_vec();
    let mut bytes = layout
        .sparse_overlay_metadata_prefix(source)
        .expect("source metadata should be reusable")
        .to_vec();
    let owner_index = layout.entry_count;
    let companion_index = owner_index + 1;
    let final_entry_count = layout.entry_count + 2;
    let inserted_entry_bytes = 2 * ENTRY_HEADER_SIZE;
    let entry_insert = layout.entry_table_offset + layout.entry_count * ENTRY_HEADER_SIZE;
    bytes.splice(entry_insert..entry_insert, [0].repeat(inserted_entry_bytes));
    write_u32(&mut bytes, entry_insert, 0x8080_4A53).expect("owner reference should fit");
    write_u32(&mut bytes, entry_insert + 4, 0x10u32 << 9).expect("owner type should fit");
    write_u32(
        &mut bytes,
        entry_insert + ENTRY_HEADER_SIZE,
        SHARED_TAG_COMPANION_CLASS,
    )
    .expect("companion reference should fit");
    write_u32(
        &mut bytes,
        entry_insert + ENTRY_HEADER_SIZE + 4,
        0x08u32 << 9,
    )
    .expect("companion type should fit");
    let final_entry_capacity = layout.entry_capacity + 2;
    let final_block_table_offset = layout.entry_table_offset
        + final_entry_capacity * ENTRY_HEADER_SIZE
        + layout.entry_table_trailer_size;
    let enrollment = [(
        u32::from(TagHash::new(layout.package_id, owner_index as u16)),
        u32::from(TagHash::new(layout.package_id, companion_index as u16)),
    )];

    layout
        .set_extended_counts(
            &mut bytes,
            target_patch,
            final_entry_count,
            layout.block_count,
            final_block_table_offset,
            &enrollment,
        )
        .expect("shared pair should extend the package tables");
    layout
        .update_package_tables_hash(&mut bytes)
        .expect("extended package tables should rehash");
    append_opaque_trailer(&mut bytes, &trailer).expect("source trailer should append");
    layout
        .set_file_size(&mut bytes)
        .expect("extended package size should update");
    bytes
}

#[test]
fn parses_shadowkeep_table_locations() {
    let bytes = synthetic_package();
    let layout = PackageLayout::parse(&bytes).expect("synthetic package should parse");

    assert_eq!(layout.package_id, 0x058f);
    assert_eq!(layout.content_build, SHADOWKEEP_CONTENT_BUILD);
    assert_eq!(layout.content_revision, SHADOWKEEP_CONTENT_REVISION);
    assert_eq!(layout.patch_id, 2);
    assert_eq!(layout.entry_count, 2);
    assert_eq!(layout.block_count, 3);
    assert_eq!(layout.entry_table_offset, 0x200);
    assert_eq!(layout.block_table_offset, 0x240);
    assert_eq!(layout.package_tables_data_offset, 0x1A0);
    assert_eq!(layout.package_tables_data_size, 0x170);
    assert_eq!(layout.opaque_trailer_offset, 0x310);
    assert_eq!(layout.opaque_trailer_size, PACKAGE_FILE_ALIGNMENT);
    assert_eq!(layout.shared_tag_enrollment_count(), 4);
}

#[test]
fn earlier_hud_generation_is_confined_to_its_audited_package_and_patch() {
    let mut bytes = synthetic_package();
    let mut layout = PackageLayout::parse(&bytes).unwrap();
    layout.package_id = 0x037E;
    layout.patch_id = 5;
    layout.content_build = 0x0001_4B68;
    layout.content_revision = 0;
    layout.set_authored_generation(&mut bytes).unwrap();
    assert_eq!(
        read_u64(&bytes, BUILD_SIGNATURE_OFFSET).unwrap(),
        SUNDIAL_BUILD_SIGNATURE
    );
    for (package, patch, build, revision) in [
        (0x058C, 5, 0x0001_4B68, 0),
        (0x037E, 4, 0x0001_4B68, 0),
        (0x037E, 5, 0x0001_4B69, 0),
        (0x037E, 5, 0x0001_4B68, 1),
    ] {
        layout.package_id = package;
        layout.patch_id = patch;
        layout.content_build = build;
        layout.content_revision = revision;
        let before = bytes.clone();
        assert!(layout.set_authored_generation(&mut bytes).is_err());
        assert_eq!(bytes, before);
    }
}

#[test]
fn parses_reserved_dynamic_array_capacity_without_treating_it_as_live_count() {
    let bytes = synthetic_package_with_capacities(8, 8);
    let layout =
        PackageLayout::parse(&bytes).expect("reserved dynamic-array capacity should parse");

    assert_eq!(layout.entry_count, 2);
    assert_eq!(layout.entry_capacity, 8);
    assert_eq!(layout.block_count, 3);
    assert_eq!(layout.block_capacity, 8);
    assert_eq!(layout.entry_table_trailer_size, 32);
    assert_eq!(layout.block_table_offset, 0x2A0);
}

#[test]
fn sparse_overlay_prefix_excludes_the_source_patch_block_store() {
    let marker = b"PARHELION-SOURCE-PAYLOAD-MUST-NOT-SURVIVE";
    let bytes = synthetic_patch_with_physical_payload(marker);
    let layout = PackageLayout::parse(&bytes).expect("synthetic physical package should parse");
    let prefix = layout
        .sparse_overlay_metadata_prefix(&bytes)
        .expect("source block store should be safely removable");

    assert_eq!(
        prefix.len(),
        layout.package_tables_data_offset + layout.package_tables_data_size
    );
    assert!(!prefix.windows(marker.len()).any(|window| window == marker));
    assert!(bytes.windows(marker.len()).any(|window| window == marker));
}

#[test]
fn sparse_overlay_prefix_rejects_unknown_nonzero_suffix_data() {
    let mut bytes = synthetic_patch_with_physical_payload(b"owned block bytes");
    let layout = PackageLayout::parse(&bytes).expect("synthetic physical package should parse");
    let metadata_end = layout.package_tables_data_offset + layout.package_tables_data_size;
    assert!(metadata_end < layout.opaque_trailer_offset);
    bytes[metadata_end] = 0xA5;
    let layout = PackageLayout::parse(&bytes)
        .expect("unmapped suffix bytes are outside the hashed package metadata");

    assert!(
        layout
            .sparse_overlay_metadata_prefix(&bytes)
            .expect_err("unknown suffix data must not be silently discarded")
            .to_string()
            .contains("unclassified nonzero data")
    );
}

#[test]
fn extended_tables_grow_and_rehash_the_native_package_tables() {
    let mut bytes = synthetic_package();
    let source_build_time = 0x0000_0000_5F44_4B94;
    write_u64(&mut bytes, BUILD_TIME_OFFSET, source_build_time)
        .expect("source build time should fit");
    let layout = PackageLayout::parse(&bytes).expect("synthetic package should parse");
    let final_entry_count = layout.entry_count + 2;
    let final_block_count = layout.block_count + 4;
    let final_block_table_offset = layout.entry_table_offset
        + final_entry_count * ENTRY_HEADER_SIZE
        + PackageLayout::ENTRY_TABLE_TRAILER_SIZE;
    let inserted_entry_bytes = 2 * ENTRY_HEADER_SIZE;
    let inserted_block_bytes = 4 * BLOCK_HEADER_SIZE;
    let shared_tag_enrollments = [(
        u32::from(TagHash::new(layout.package_id, 2)),
        u32::from(TagHash::new(layout.package_id, 3)),
    )];
    let original_shared_tag_rows = layout
        .shared_tag_enrollment_rows(&bytes)
        .expect("source shared-tag rows should parse")
        .to_vec();
    let entry_insert = layout.entry_table_offset + layout.entry_count * ENTRY_HEADER_SIZE;
    bytes.splice(entry_insert..entry_insert, [0].repeat(inserted_entry_bytes));
    bytes[entry_insert..entry_insert + 4].copy_from_slice(&0x8080_4A53u32.to_le_bytes());
    bytes[entry_insert + 4..entry_insert + 8].copy_from_slice(&(0x10u32 << 9).to_le_bytes());
    bytes[entry_insert + ENTRY_HEADER_SIZE..entry_insert + ENTRY_HEADER_SIZE + 4]
        .copy_from_slice(&SHARED_TAG_COMPANION_CLASS.to_le_bytes());
    bytes[entry_insert + ENTRY_HEADER_SIZE + 4..entry_insert + ENTRY_HEADER_SIZE + 8]
        .copy_from_slice(&(0x08u32 << 9).to_le_bytes());
    let block_insert = final_block_table_offset + layout.block_count * BLOCK_HEADER_SIZE;
    bytes.splice(block_insert..block_insert, [0].repeat(inserted_block_bytes));

    layout
        .set_extended_counts(
            &mut bytes,
            3,
            final_entry_count,
            final_block_count,
            final_block_table_offset,
            &shared_tag_enrollments,
        )
        .expect("extended table metadata should update");
    layout
        .update_package_tables_hash(&mut bytes)
        .expect("extended package tables should rehash");

    assert_eq!(
        read_u64(&bytes, BUILD_TIME_OFFSET).expect("build time should parse"),
        source_build_time,
        "an overlay must preserve its stock package's build timestamp"
    );

    assert_eq!(
        u32::from_le_bytes(
            bytes[PACKAGE_TABLES_DATA_SIZE_OFFSET..PACKAGE_TABLES_DATA_SIZE_OFFSET + 4]
                .try_into()
                .expect("package-tables size should be four bytes")
        ) as usize,
        layout.package_tables_data_size
            + inserted_entry_bytes
            + inserted_block_bytes
            + shared_tag_enrollments.len() * SHARED_TAG_TABLE_ROW_SIZE
    );
    assert_eq!(
        read_u64(
            &bytes,
            layout.package_tables_data_offset + PACKAGE_TABLES_THIS_SIZE_OFFSET
        )
        .expect("internal package-tables size should parse") as usize,
        layout.package_tables_data_size
            + inserted_entry_bytes
            + inserted_block_bytes
            + shared_tag_enrollments.len() * SHARED_TAG_TABLE_ROW_SIZE
    );
    assert_eq!(
        read_u64(
            &bytes,
            layout.package_tables_data_offset + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET
        )
        .expect("entry descriptor count should parse") as usize,
        final_entry_count
    );
    assert_eq!(
        read_u64(
            &bytes,
            layout.package_tables_data_offset + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
        )
        .expect("block descriptor count should parse") as usize,
        final_block_count
    );
    assert_eq!(
        relative_target(
            &bytes,
            layout.package_tables_data_offset
                + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
                + DYNAMIC_ARRAY_POINTER_OFFSET
        )
        .expect("block descriptor pointer should resolve")
            + DYNAMIC_ARRAY_HEADER_SIZE,
        final_block_table_offset
    );
    assert_eq!(
        relative_target(
            &bytes,
            layout.package_tables_data_offset
                + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
                + DYNAMIC_ARRAY_POINTER_OFFSET
        )
        .expect("shared-tag descriptor pointer should resolve"),
        0x2E0 + inserted_entry_bytes + inserted_block_bytes
    );
    let shifted_shared_tag_header = 0x2E0 + inserted_entry_bytes + inserted_block_bytes;
    let shifted_shared_tag_rows = shifted_shared_tag_header + DYNAMIC_ARRAY_HEADER_SIZE;
    assert_eq!(
        read_u64(
            &bytes,
            layout.package_tables_data_offset + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
        )
        .expect("shared-tag descriptor count should parse"),
        5
    );
    assert_eq!(
        read_u64(&bytes, shifted_shared_tag_header)
            .expect("repeated shared-tag count should parse"),
        5
    );
    assert_eq!(
        &bytes[shifted_shared_tag_rows..shifted_shared_tag_rows + original_shared_tag_rows.len()],
        original_shared_tag_rows
    );
    let appended_rows = shifted_shared_tag_rows + original_shared_tag_rows.len();
    assert_eq!(
        &bytes[appended_rows..appended_rows + 8],
        [
            u32::from(TagHash::new(layout.package_id, 2)).to_le_bytes(),
            u32::from(TagHash::new(layout.package_id, 3)).to_le_bytes(),
        ]
        .concat()
    );
    let size = read_u32(&bytes, PACKAGE_TABLES_DATA_SIZE_OFFSET)
        .expect("package-tables size should parse") as usize;
    assert_eq!(
        Sha1::digest(
            &bytes[layout.package_tables_data_offset..layout.package_tables_data_offset + size]
        )
        .as_slice(),
        &bytes[PACKAGE_TABLES_DATA_HASH_OFFSET..PACKAGE_TABLES_DATA_HASH_OFFSET + REGION_HASH_SIZE]
    );
}

#[test]
fn creates_the_first_shared_tag_table_and_preserves_it_on_later_growth() {
    let package_id = 0x0AA1;
    let source = build_standalone_package_skeleton(package_id, &[[0; 8]], 1, &[])
        .expect("empty shared-tag package should build");
    let source_layout =
        PackageLayout::parse(&source).expect("empty shared-tag package should parse");
    let source_shared_pointer = source_layout.package_tables_data_offset
        + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    assert_eq!(source_layout.shared_tag_enrollment_count(), 0);
    assert_eq!(read_i64(&source, source_shared_pointer).unwrap(), 0);

    let first = append_zero_length_shared_pair(&source, 1);
    let first_layout = PackageLayout::parse(&first).expect("first shared-tag table should reopen");
    assert_eq!(first_layout.shared_tag_enrollment_count(), 1);
    assert_eq!(
        first_layout.package_tables_data_size,
        source_layout.package_tables_data_size
            + 2 * ENTRY_HEADER_SIZE
            + 2 * DYNAMIC_ARRAY_HEADER_SIZE
            + SHARED_TAG_TABLE_ROW_SIZE
    );
    let first_pointer = first_layout.package_tables_data_offset
        + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
        + DYNAMIC_ARRAY_POINTER_OFFSET;
    let first_header =
        relative_target(&first, first_pointer).expect("first shared-tag pointer should resolve");
    let first_allocated_block_end =
        first_layout.block_table_offset + first_layout.block_capacity * BLOCK_HEADER_SIZE;
    assert_eq!(
        first_header,
        first_allocated_block_end + DYNAMIC_ARRAY_HEADER_SIZE
    );
    assert_eq!(
        read_u32(&first, first_header - size_of::<u32>()).unwrap(),
        SHARED_TAG_TABLE_MARKER
    );
    assert_eq!(read_u64(&first, first_header).unwrap(), 1);
    assert_eq!(
        read_u64(&first, first_header + size_of::<u64>()).unwrap(),
        SHARED_TAG_TABLE_CLASS
    );
    assert_eq!(
        first_layout
            .shared_tag_enrollment_rows(&first)
            .expect("first enrollment row should parse"),
        [
            u32::from(TagHash::new(package_id, 1)).to_le_bytes(),
            u32::from(TagHash::new(package_id, 2)).to_le_bytes(),
        ]
        .concat()
    );

    let second = append_zero_length_shared_pair(&first, 2);
    let second_layout =
        PackageLayout::parse(&second).expect("grown shared-tag table should reopen");
    assert_eq!(second_layout.shared_tag_enrollment_count(), 2);
    assert_eq!(
        second_layout.package_tables_data_size,
        first_layout.package_tables_data_size + 2 * ENTRY_HEADER_SIZE + SHARED_TAG_TABLE_ROW_SIZE,
        "the shared-tag envelope must only be emitted for the first table"
    );
    assert_eq!(
        second_layout
            .shared_tag_enrollment_rows(&second)
            .expect("both enrollment rows should parse"),
        [
            u32::from(TagHash::new(package_id, 1)).to_le_bytes(),
            u32::from(TagHash::new(package_id, 2)).to_le_bytes(),
            u32::from(TagHash::new(package_id, 3)).to_le_bytes(),
            u32::from(TagHash::new(package_id, 4)).to_le_bytes(),
        ]
        .concat()
    );
}

#[test]
fn rejects_truncated_or_wrong_variant_headers() {
    let mut truncated = synthetic_package();
    truncated.truncate(0x168);
    assert!(
        PackageLayout::parse(&truncated)
            .expect_err("the complete signed header is required")
            .to_string()
            .contains("shorter")
    );

    let mut beta = synthetic_package();
    beta[HEADER_VARIANT_OFFSET] = 0;
    assert!(
        PackageLayout::parse(&beta)
            .expect_err("the beta variant is not authorable by this writer")
            .to_string()
            .contains("d2_prebl")
    );
}

#[test]
fn rejects_invalid_platform_identity_patch_and_header_control_fields() {
    let mut wrong_platform = synthetic_package();
    wrong_platform[PLATFORM_OFFSET..PLATFORM_OFFSET + 2].copy_from_slice(&1u16.to_le_bytes());
    assert!(PackageLayout::parse(&wrong_platform).is_err());

    let mut wrong_package = synthetic_package();
    wrong_package[PACKAGE_ID_OFFSET..PACKAGE_ID_OFFSET + 2]
        .copy_from_slice(&0x0FFFu16.to_le_bytes());
    assert!(PackageLayout::parse(&wrong_package).is_err());

    let mut wrong_patch = synthetic_package();
    wrong_patch[PATCH_ID_OFFSET..PATCH_ID_OFFSET + 2].copy_from_slice(&0x0100u16.to_le_bytes());
    assert!(PackageLayout::parse(&wrong_patch).is_err());

    let mut wrong_zero = synthetic_package();
    wrong_zero[MUST_BE_ZERO_OFFSET] = 1;
    assert!(PackageLayout::parse(&wrong_zero).is_err());
}

#[test]
fn rejects_invalid_entry_counts_and_beta_offsets() {
    for count in [0u32, (MAX_ENTRY_COUNT + 1) as u32] {
        let mut bytes = synthetic_package();
        bytes[ENTRY_COUNT_OFFSET..ENTRY_COUNT_OFFSET + 4].copy_from_slice(&count.to_le_bytes());
        assert!(PackageLayout::parse(&bytes).is_err());
    }

    let mut bytes = synthetic_package();
    bytes[BETA_ENTRY_TABLE_OFFSET..BETA_ENTRY_TABLE_OFFSET + 4]
        .copy_from_slice(&0x200u32.to_le_bytes());
    assert!(PackageLayout::parse(&bytes).is_err());
}

#[test]
fn entry_region_count_is_u32_and_preserves_the_following_header_word() {
    let mut bytes = synthetic_package();
    let count_offset = 0x200 - ENTRY_TABLE_COUNT_BACK;
    let following = 0xA5C3_7E19u32.to_le_bytes();
    bytes[count_offset + 4..count_offset + 8].copy_from_slice(&following);
    rehash_package_tables(&mut bytes);
    let layout = PackageLayout::parse(&bytes)
        .expect("the word after a u32 region count is not part of the count");

    layout
        .set_extended_counts(
            &mut bytes,
            3,
            layout.entry_count,
            layout.block_count,
            layout.block_table_offset,
            &[],
        )
        .expect("rewriting counts should preserve adjacent metadata");

    assert_eq!(&bytes[count_offset + 4..count_offset + 8], &following);
}

#[test]
fn rejects_block_descriptor_disagreement_with_prebl_formula() {
    let mut bytes = synthetic_package();
    let pointer = 0x1A0 + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET + DYNAMIC_ARRAY_POINTER_OFFSET;
    let relative = read_i64(&bytes, pointer).expect("block pointer should parse");
    write_i64(&mut bytes, pointer, relative + 16).expect("block pointer should update");
    rehash_package_tables(&mut bytes);

    assert!(
        PackageLayout::parse(&bytes)
            .expect_err("the descriptor and documented formula must agree")
            .to_string()
            .contains("documented pre-BL")
    );
}

#[test]
fn rejects_type16_entries_missing_from_the_shared_tag_table() {
    let mut bytes = synthetic_package_with_capacities(4, 3);
    let package_tables = read_u32(&bytes, PACKAGE_TABLES_DATA_POINTER_OFFSET)
        .expect("package-tables offset should parse") as usize;
    let entry_table = package_tables + ENTRY_TABLE_POINTER_ADJUSTMENT;
    bytes[ENTRY_COUNT_OFFSET..ENTRY_COUNT_OFFSET + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[package_tables + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET
        ..package_tables + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET + 8]
        .copy_from_slice(&3u64.to_le_bytes());
    let unenrolled_entry = entry_table + 2 * ENTRY_HEADER_SIZE;
    bytes[unenrolled_entry..unenrolled_entry + 4].copy_from_slice(&0x8080_4A53u32.to_le_bytes());
    bytes[unenrolled_entry + 4..unenrolled_entry + 8]
        .copy_from_slice(&(0x10u32 << 9).to_le_bytes());
    rehash_package_tables(&mut bytes);

    assert!(
        PackageLayout::parse(&bytes)
            .expect_err("an unenrolled type-16 entry must fail validation")
            .to_string()
            .contains("missing from the shared-tag table")
    );
}

#[test]
fn accepts_cross_package_shared_tag_companions() {
    let mut bytes = synthetic_package();
    let package_tables = read_u32(&bytes, PACKAGE_TABLES_DATA_POINTER_OFFSET)
        .expect("package-tables offset should parse") as usize;
    let shared_pointer =
        package_tables + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET + DYNAMIC_ARRAY_POINTER_OFFSET;
    let shared_header =
        relative_target(&bytes, shared_pointer).expect("shared-tag pointer should resolve");
    let first_companion = shared_header + DYNAMIC_ARRAY_HEADER_SIZE + size_of::<u32>();
    write_u32(
        &mut bytes,
        first_companion,
        u32::from(TagHash::new(0x0933, 0x0123)),
    )
    .expect("cross-package companion should fit");
    rehash_package_tables(&mut bytes);

    PackageLayout::parse(&bytes)
        .expect("a valid cross-package shared-memory companion should parse");
}

#[test]
fn validates_both_extended_and_metadata_region_hashes() {
    let mut bytes = synthetic_package();
    let extended_offset = HEADER_SIZE;
    let extended_size = 0x20usize;
    bytes[extended_offset..extended_offset + extended_size].fill(0x5A);
    bytes[EXTENDED_DATA_POINTER_OFFSET..EXTENDED_DATA_POINTER_OFFSET + 4]
        .copy_from_slice(&(extended_offset as u32).to_le_bytes());
    bytes[EXTENDED_DATA_SIZE_OFFSET..EXTENDED_DATA_SIZE_OFFSET + 4]
        .copy_from_slice(&(extended_size as u32).to_le_bytes());
    let extended_hash = Sha1::digest(&bytes[extended_offset..extended_offset + extended_size]);
    bytes[EXTENDED_DATA_HASH_OFFSET..EXTENDED_DATA_HASH_OFFSET + REGION_HASH_SIZE]
        .copy_from_slice(&extended_hash);
    PackageLayout::parse(&bytes).expect("valid extended region should parse");

    bytes[extended_offset] ^= 1;
    assert!(
        PackageLayout::parse(&bytes)
            .expect_err("corrupt extended data must fail")
            .to_string()
            .contains("extended header SHA-1")
    );

    let mut metadata = synthetic_package();
    metadata[0x200] ^= 1;
    assert!(
        PackageLayout::parse(&metadata)
            .expect_err("corrupt metadata must fail")
            .to_string()
            .contains("package tables data SHA-1")
    );
}

#[test]
fn validates_locale_tokens_and_live_block_ranges() {
    let mut locale = synthetic_package();
    locale[LOCALE_TOKEN_OFFSET..LOCALE_TOKEN_OFFSET + 4]
        .copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    assert!(PackageLayout::parse(&locale).is_err());

    locale[LOCALE_CHECK_ENABLE_OFFSET] = 0;
    assert!(PackageLayout::parse(&locale).is_ok());

    locale[LOCALE_CHECK_ENABLE_OFFSET] = 2;
    assert!(PackageLayout::parse(&locale).is_err());
    locale[LOCALE_TOKEN_OFFSET..LOCALE_TOKEN_OFFSET + 4]
        .copy_from_slice(&LOCALE_TOKENS[0].to_le_bytes());
    assert!(PackageLayout::parse(&locale).is_ok());

    let mut block = synthetic_package();
    let block_offset = 0x240;
    let block_file_size = block.len() as u32;
    block[block_offset..block_offset + 4].copy_from_slice(&block_file_size.to_le_bytes());
    block[block_offset + 4..block_offset + 8].copy_from_slice(&1u32.to_le_bytes());
    block[block_offset + 8..block_offset + 10].copy_from_slice(&2u16.to_le_bytes());
    rehash_package_tables(&mut block);
    assert!(PackageLayout::parse(&block).is_err());
}

#[test]
fn current_patch_physical_blocks_require_their_recorded_sha1() {
    let payload = b"current patch block bytes";
    let valid = synthetic_patch_with_physical_payload(payload);
    PackageLayout::parse(&valid).expect("a matching physical-block SHA-1 should parse");

    let layout = PackageLayout::parse(&valid).expect("valid package layout should parse");
    let mut zero_hash = valid.clone();
    zero_hash[layout.block_table_offset + 12..layout.block_table_offset + 32].fill(0);
    rehash_package_tables(&mut zero_hash);
    assert!(
        PackageLayout::parse(&zero_hash)
            .expect_err("a current-patch physical block needs a nonzero SHA-1")
            .to_string()
            .contains("empty SHA-1")
    );

    let mut changed_payload = valid;
    let payload_offset = changed_payload
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("the physical payload should be present");
    changed_payload[payload_offset] ^= 1;
    assert!(
        PackageLayout::parse(&changed_payload)
            .expect_err("changed physical bytes must fail their recorded SHA-1")
            .to_string()
            .contains("failed SHA-1")
    );
}

#[test]
fn inherited_blocks_keep_their_digest_without_requiring_local_bytes() {
    let mut bytes = synthetic_patch_with_physical_payload(b"source-generation block");
    let layout = PackageLayout::parse(&bytes).expect("physical package should parse");
    write_u16(
        &mut bytes,
        layout.block_table_offset + 8,
        layout.patch_id - 1,
    )
    .expect("block should become inherited");
    rehash_package_tables(&mut bytes);

    PackageLayout::parse(&bytes).expect(
        "an inherited block may rely on its source package bytes while retaining its digest",
    );

    bytes[layout.block_table_offset + 12..layout.block_table_offset + 32].fill(0);
    rehash_package_tables(&mut bytes);
    assert!(
        PackageLayout::parse(&bytes)
            .expect_err("an inherited block still requires its recorded digest")
            .to_string()
            .contains("empty SHA-1")
    );
}

#[test]
fn pads_package_files_and_relocates_the_supported_opaque_trailer() {
    let mut bytes = vec![0xA5; PACKAGE_FILE_ALIGNMENT + 3];
    let mut trailer = vec![0u8; PACKAGE_FILE_ALIGNMENT];
    trailer[..4].copy_from_slice(&OPAQUE_TRAILER_MARKER);
    append_opaque_trailer(&mut bytes, &trailer).expect("supported opaque trailer should append");

    assert_eq!(bytes.len(), PACKAGE_FILE_ALIGNMENT * 3);
    assert_eq!(
        &bytes[..PACKAGE_FILE_ALIGNMENT + 3],
        vec![0xA5; PACKAGE_FILE_ALIGNMENT + 3]
    );
    assert!(
        bytes[PACKAGE_FILE_ALIGNMENT + 3..PACKAGE_FILE_ALIGNMENT * 2]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(&bytes[PACKAGE_FILE_ALIGNMENT * 2..], trailer);
}

#[test]
fn aligns_appended_block_payloads_to_the_native_0x800_boundary() {
    let mut bytes = vec![0xA5; PACKAGE_FILE_ALIGNMENT + 3];
    let offset = append_aligned(&mut bytes, b"payload");

    assert_eq!(offset, PACKAGE_FILE_ALIGNMENT * 2);
    assert_eq!(offset % PACKAGE_FILE_ALIGNMENT, 0);
    assert!(
        bytes[PACKAGE_FILE_ALIGNMENT + 3..offset]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(&bytes[offset..], b"payload");
}
