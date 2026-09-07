use std::{collections::BTreeSet, ops::Range};

use sha1::{Digest, Sha1};
use sundial::package_authoring::is_valid_package_tag;
use tiger_pkg::TagHash;

use crate::package_profile::{
    BUILD_SIGNATURE_OFFSET, PACKAGE_ID_OFFSET, PATCH_ID_OFFSET, SHADOWKEEP_HEADER_VERSION,
    VERSION_OFFSET,
};
use crate::{AuthoringResult, error::invalid};

pub(crate) const BLOCK_SIZE: usize = 0x40000;
const HEADER_SIZE: usize = 0x170;
const PLATFORM_OFFSET: usize = 0x02;
pub(crate) const W64_PLATFORM: u16 = 2;
const LOCALE_CHECK_ENABLE_OFFSET: usize = 0x06;
const MUST_BE_ZERO_OFFSET: usize = 0x07;
const BUILD_TIME_OFFSET: usize = 0x10;
const CONTENT_BUILD_OFFSET: usize = 0x18;
const CONTENT_REVISION_OFFSET: usize = 0x1C;
const HEADER_VARIANT_OFFSET: usize = 0x1A;
const PREBL_HEADER_VARIANT: u8 = 1;
const HEADER_SIGNATURE_OFFSET_OFFSET: usize = 0xB0;
pub(crate) const ENTRY_COUNT_OFFSET: usize = 0xB4;
const BETA_ENTRY_TABLE_OFFSET: usize = 0xB8;
const BLOCK_COUNT_OFFSET: usize = 0xD0;
const BETA_BLOCK_TABLE_OFFSET: usize = 0xD4;
const EXTENDED_DATA_POINTER_OFFSET: usize = 0xF0;
const EXTENDED_DATA_SIZE_OFFSET: usize = 0xF4;
const EXTENDED_DATA_HASH_OFFSET: usize = 0xF8;
const PACKAGE_TABLES_DATA_POINTER_OFFSET: usize = 0x110;
const PACKAGE_TABLES_DATA_SIZE_OFFSET: usize = 0x114;
const PACKAGE_TABLES_DATA_HASH_OFFSET: usize = 0x118;
const REGION_HASH_SIZE: usize = 20;
const PACKAGE_TABLES_THIS_SIZE_OFFSET: usize = 0;
const PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET: usize = 0x10;
const PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET: usize = 0x20;
const PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET: usize = 0x30;
const DYNAMIC_ARRAY_POINTER_OFFSET: usize = 8;
const DYNAMIC_ARRAY_HEADER_SIZE: usize = 16;
const SHARED_TAG_TABLE_MARKER: u32 = 0x8080_9FBD;
const SHARED_TAG_TABLE_CLASS: u64 = 0x0000_0000_8080_9A13;
const SHARED_TAG_TABLE_ROW_SIZE: usize = 8;
pub(crate) const SHARED_TAG_COMPANION_CLASS: u32 = 0x8080_9EF9;
// Every package in the supported installed set stores the start of the same final 0x800-byte
// opaque region here. The decoded header leaves +0x160 unnamed, so this is deliberately an
// empirical source-profile constraint rather than a claimed package-format field.
const OPAQUE_TRAILER_OFFSET_FIELD: usize = 0x160;
const FILE_SIZE_OFFSET: usize = 0x164;
const LOCALE_TOKEN_OFFSET: usize = 0x168;
const LOCALE_ID_OFFSET: usize = 0x16C;
const ENTRY_TABLE_POINTER_ADJUSTMENT: usize = 96;
const ENTRY_TABLE_COUNT_BACK: usize = 16;
const BLOCK_TABLE_COUNT_BACK: usize = 16;
pub(crate) const ENTRY_HEADER_SIZE: usize = 16;
pub(crate) const BLOCK_HEADER_SIZE: usize = 48;
const BLOCK_PAYLOAD_ALIGNMENT: usize = 0x800;
pub(crate) const PACKAGE_FILE_ALIGNMENT: usize = 0x800;
const HEADER_SIGNATURE_SIZE: usize = 0x100;
const OPAQUE_TRAILER_MARKER: [u8; 4] = [0xEF, 0xBE, 0xAD, 0xDE];
const MIN_PACKAGE_ID: u16 = 0x100;
const MAX_PACKAGE_ID: u16 = 0xCFF;
const MAX_ENTRY_COUNT: usize = 8192;
pub(crate) const MAX_BLOCK_COUNT: usize = 16384;
const MAX_PATCH_ID: u16 = 0xFF;
const LOCALE_TOKENS: [u32; 2] = [0x2811_41FD, 0xFB43_8DF4];

/// Deterministic build generation used by Sundial-authored package rows.
///
/// Sunrise includes this exact value in the generated ContentConfig row for each authored package.
/// It must be distinct from every earlier patch in the same package chain. Reusing the stock
/// patch-four generation deadlocks registration because these investment chains already use that
/// generation on patch one.
pub const SUNDIAL_BUILD_SIGNATURE: u64 = 0x1236_7498_D1CB_BA35;
const SUNDIAL_BUILD_TIME: u64 = 0x5F44_0756;
const SHADOWKEEP_CONTENT_BUILD: u32 = 0x0001_5281;
const SHADOWKEEP_CONTENT_REVISION: u32 = 2;

#[derive(Clone, Debug)]
pub(crate) struct PackageLayout {
    pub package_id: u16,
    pub content_build: u32,
    pub content_revision: u32,
    pub patch_id: u16,
    pub entry_count: usize,
    pub entry_capacity: usize,
    pub block_count: usize,
    pub block_capacity: usize,
    pub entry_table_offset: usize,
    pub entry_table_trailer_size: usize,
    pub block_table_offset: usize,
    pub package_tables_data_offset: usize,
    pub package_tables_data_size: usize,
    pub opaque_trailer_offset: usize,
    pub opaque_trailer_size: usize,
    shared_tag_table: Option<SharedTagTableLayout>,
}

#[derive(Clone, Debug)]
struct SharedTagTableLayout {
    count: usize,
    header_offset: usize,
    rows_offset: usize,
}

impl PackageLayout {
    pub(crate) const ENTRY_TABLE_TRAILER_SIZE: usize = 32;

    pub fn parse(bytes: &[u8]) -> AuthoringResult<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(invalid("Package is shorter than the Shadowkeep header"));
        }
        let version = read_u16(bytes, VERSION_OFFSET)?;
        if version != SHADOWKEEP_HEADER_VERSION {
            return Err(invalid(format!(
                "Package header version {version} is not Shadowkeep version 38"
            )));
        }
        let platform = read_u16(bytes, PLATFORM_OFFSET)?;
        if platform != W64_PLATFORM {
            return Err(invalid(format!(
                "Package platform {platform} is not the w64 platform {W64_PLATFORM}"
            )));
        }
        if bytes[HEADER_VARIANT_OFFSET] != PREBL_HEADER_VARIANT {
            return Err(invalid(format!(
                "Package header variant {} is not the supported d2_prebl variant {PREBL_HEADER_VARIANT}",
                bytes[HEADER_VARIANT_OFFSET]
            )));
        }
        if read_u32(bytes, BETA_ENTRY_TABLE_OFFSET)? != 0
            || read_u32(bytes, BETA_BLOCK_TABLE_OFFSET)? != 0
        {
            return Err(invalid(
                "A d2_prebl package must not use the beta directory offsets",
            ));
        }
        let package_id = read_u16(bytes, PACKAGE_ID_OFFSET)?;
        if !(MIN_PACKAGE_ID..=MAX_PACKAGE_ID).contains(&package_id) {
            return Err(invalid(format!(
                "Package id {package_id:04X} is outside the supported {MIN_PACKAGE_ID:03X}..{MAX_PACKAGE_ID:03X} range"
            )));
        }
        if bytes[MUST_BE_ZERO_OFFSET] != 0 {
            return Err(invalid("Package header byte +0x07 must be zero"));
        }
        let patch_id = read_u16(bytes, PATCH_ID_OFFSET)?;
        if patch_id > MAX_PATCH_ID {
            return Err(invalid(format!(
                "Package patch id {patch_id} exceeds {MAX_PATCH_ID}"
            )));
        }
        validate_locale_fields(bytes)?;

        let entry_count = read_u32(bytes, ENTRY_COUNT_OFFSET)? as usize;
        if !(1..=MAX_ENTRY_COUNT).contains(&entry_count) {
            return Err(invalid(format!(
                "Package entry count {entry_count} is outside 1..={MAX_ENTRY_COUNT}"
            )));
        }
        let block_count = read_u32(bytes, BLOCK_COUNT_OFFSET)? as usize;
        if block_count > MAX_BLOCK_COUNT {
            return Err(invalid(format!(
                "Package block count {block_count} exceeds {MAX_BLOCK_COUNT}"
            )));
        }

        let header_signature_offset = read_u32(bytes, HEADER_SIGNATURE_OFFSET_OFFSET)? as usize;
        if header_signature_offset != PACKAGE_FILE_ALIGNMENT {
            return Err(invalid(
                "Package header signature is not at the native Shadowkeep offset",
            ));
        }
        let opaque_trailer_offset = read_u32(bytes, OPAQUE_TRAILER_OFFSET_FIELD)? as usize;
        let file_size = read_u32(bytes, FILE_SIZE_OFFSET)? as usize;
        if file_size != bytes.len() || opaque_trailer_offset > file_size {
            return Err(invalid(
                "Package opaque-trailer boundary and file-size fields disagree",
            ));
        }
        let opaque_trailer_size = file_size - opaque_trailer_offset;
        if opaque_trailer_size != PACKAGE_FILE_ALIGNMENT
            || bytes.get(opaque_trailer_offset..opaque_trailer_offset + 4)
                != Some(OPAQUE_TRAILER_MARKER.as_slice())
        {
            return Err(invalid(
                "Package does not match the supported opaque trailing-region profile",
            ));
        }

        validate_hashed_region(
            bytes,
            EXTENDED_DATA_POINTER_OFFSET,
            EXTENDED_DATA_SIZE_OFFSET,
            EXTENDED_DATA_HASH_OFFSET,
            opaque_trailer_offset,
            "extended header",
            false,
        )?;
        let package_tables = validate_hashed_region(
            bytes,
            PACKAGE_TABLES_DATA_POINTER_OFFSET,
            PACKAGE_TABLES_DATA_SIZE_OFFSET,
            PACKAGE_TABLES_DATA_HASH_OFFSET,
            opaque_trailer_offset,
            "package tables data",
            true,
        )?;
        let package_tables_data_offset =
            read_u32(bytes, PACKAGE_TABLES_DATA_POINTER_OFFSET)? as usize;
        let package_tables_data_size = read_u32(bytes, PACKAGE_TABLES_DATA_SIZE_OFFSET)? as usize;
        let entry_table_offset = package_tables_data_offset
            .checked_add(ENTRY_TABLE_POINTER_ADJUSTMENT)
            .ok_or_else(|| invalid("Entry table offset overflows"))?;
        let encoded_entry_capacity = entry_table_offset
            .checked_sub(ENTRY_TABLE_COUNT_BACK)
            .ok_or_else(|| invalid("Entry table offset underflows its count field"))
            .and_then(|offset| read_u32(bytes, offset))?
            as usize;
        if !(entry_count..=MAX_ENTRY_COUNT).contains(&encoded_entry_capacity) {
            return Err(invalid(format!(
                "Package entry-region count {encoded_entry_capacity} is outside {entry_count}..={MAX_ENTRY_COUNT}"
            )));
        }
        let entry_bytes = entry_count
            .checked_mul(ENTRY_HEADER_SIZE)
            .ok_or_else(|| invalid("Entry table size overflows"))?;
        let entry_capacity_bytes = encoded_entry_capacity
            .checked_mul(ENTRY_HEADER_SIZE)
            .ok_or_else(|| invalid("Entry table capacity overflows"))?;
        let entry_table_end = entry_table_offset
            .checked_add(entry_capacity_bytes)
            .ok_or_else(|| invalid("Entry table end overflows"))?;
        let block_table_offset = entry_table_end
            .checked_add(Self::ENTRY_TABLE_TRAILER_SIZE)
            .ok_or_else(|| invalid("Block table offset overflows"))?;
        let block_header = relative_target(
            bytes,
            package_tables_data_offset
                + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
                + DYNAMIC_ARRAY_POINTER_OFFSET,
        )?;
        if block_header.checked_add(DYNAMIC_ARRAY_HEADER_SIZE) != Some(block_table_offset) {
            return Err(invalid(
                "Block descriptor disagrees with the documented pre-BL block-table offset",
            ));
        }
        let entry_table_trailer_size = Self::ENTRY_TABLE_TRAILER_SIZE;
        let block_bytes = block_count
            .checked_mul(BLOCK_HEADER_SIZE)
            .ok_or_else(|| invalid("Block table size overflows"))?;
        let entry_range = checked_range(bytes, entry_table_offset, entry_bytes, "entry table")?;
        ensure_contained(&entry_range, &package_tables, "entry table")?;
        let entry_region = checked_range(
            bytes,
            entry_table_offset,
            entry_capacity_bytes,
            "allocated entry region",
        )?;
        ensure_contained(&entry_region, &package_tables, "allocated entry region")?;
        let block_range = checked_range(bytes, block_table_offset, block_bytes, "block table")?;
        ensure_contained(&block_range, &package_tables, "block table")?;
        let encoded_block_capacity = block_table_offset
            .checked_sub(BLOCK_TABLE_COUNT_BACK)
            .ok_or_else(|| invalid("Block table offset underflows its count field"))
            .and_then(|offset| read_u64(bytes, offset))?
            as usize;
        if encoded_block_capacity < block_count || encoded_block_capacity > MAX_BLOCK_COUNT {
            return Err(invalid(format!(
                "Package block capacity {encoded_block_capacity} is outside {block_count}..={MAX_BLOCK_COUNT}"
            )));
        }
        let allocated_block_bytes = encoded_block_capacity
            .checked_mul(BLOCK_HEADER_SIZE)
            .ok_or_else(|| invalid("Allocated block table size overflows"))?;
        let allocated_block_range = checked_range(
            bytes,
            block_table_offset,
            allocated_block_bytes,
            "allocated block table",
        )?;
        ensure_contained(
            &allocated_block_range,
            &package_tables,
            "allocated block table",
        )?;
        let package_tables_this_size = read_u64(
            bytes,
            package_tables_data_offset + PACKAGE_TABLES_THIS_SIZE_OFFSET,
        )? as usize;
        let descriptor_entry_count = read_u64(
            bytes,
            package_tables_data_offset + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET,
        )? as usize;
        let descriptor_block_count = read_u64(
            bytes,
            package_tables_data_offset + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET,
        )? as usize;
        if descriptor_entry_count != entry_count
            || descriptor_block_count != block_count
            || package_tables_this_size != package_tables_data_size
        {
            return Err(invalid("Package table counts or sizes disagree"));
        }
        let entry_header = relative_target(
            bytes,
            package_tables_data_offset
                + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET
                + DYNAMIC_ARRAY_POINTER_OFFSET,
        )?;
        if entry_header.checked_add(DYNAMIC_ARRAY_HEADER_SIZE) != Some(entry_table_offset) {
            return Err(invalid(
                "Entry descriptor does not point to the pre-BL entry array",
            ));
        }
        let shared_tag_descriptor =
            package_tables_data_offset + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET;
        let shared_tag_count_u64 = read_u64(bytes, shared_tag_descriptor)?;
        let shared_tag_pointer = shared_tag_descriptor + DYNAMIC_ARRAY_POINTER_OFFSET;
        let shared_tag_relative = read_i64(bytes, shared_tag_pointer)?;
        let mut enrolled_type16_indices = BTreeSet::new();
        let shared_tag_table = if shared_tag_count_u64 == 0 {
            if shared_tag_relative != 0 || allocated_block_range.end != package_tables.end {
                return Err(invalid(
                    "Empty shared-tag table does not match the native package-table layout",
                ));
            }
            None
        } else {
            let count = usize::try_from(shared_tag_count_u64)
                .map_err(|_| invalid("Shared-tag table count exceeds this platform"))?;
            let header_offset = relative_target(bytes, shared_tag_pointer)?;
            let expected_header_offset = allocated_block_range
                .end
                .checked_add(DYNAMIC_ARRAY_HEADER_SIZE)
                .ok_or_else(|| invalid("Shared-tag table header offset overflows"))?;
            if header_offset != expected_header_offset || header_offset < size_of::<u32>() {
                return Err(invalid(
                    "Shared-tag table does not immediately follow the allocated block table",
                ));
            }
            if read_u32(bytes, header_offset - size_of::<u32>())? != SHARED_TAG_TABLE_MARKER
                || read_u64(bytes, header_offset)? != shared_tag_count_u64
                || read_u64(bytes, header_offset + size_of::<u64>())? != SHARED_TAG_TABLE_CLASS
            {
                return Err(invalid(format!(
                    "Package {package_id:04X} shared-tag table at 0x{header_offset:X}: marker 0x{:08X}, repeated count {} (expected {shared_tag_count_u64}), class 0x{:016X} is invalid",
                    read_u32(bytes, header_offset - size_of::<u32>())?,
                    read_u64(bytes, header_offset)?,
                    read_u64(bytes, header_offset + size_of::<u64>())?,
                )));
            }
            let rows_offset = header_offset
                .checked_add(DYNAMIC_ARRAY_HEADER_SIZE)
                .ok_or_else(|| invalid("Shared-tag table rows offset overflows"))?;
            let rows_size = count
                .checked_mul(SHARED_TAG_TABLE_ROW_SIZE)
                .ok_or_else(|| invalid("Shared-tag table size overflows"))?;
            let rows = checked_range(bytes, rows_offset, rows_size, "shared-tag table rows")?;
            ensure_contained(&rows, &package_tables, "shared-tag table rows")?;
            if rows.end != package_tables.end {
                return Err(invalid(
                    "Shared-tag table rows do not terminate the package metadata",
                ));
            }
            for row_index in 0..count {
                let row = rows_offset + row_index * SHARED_TAG_TABLE_ROW_SIZE;
                let owner = TagHash(read_u32(bytes, row)?);
                let companion = TagHash(read_u32(bytes, row + size_of::<u32>())?);
                let owner_index = owner.entry_index() as usize;
                if !is_valid_package_tag(owner)
                    || !is_valid_package_tag(companion)
                    || owner.pkg_id() != package_id
                    || owner_index >= entry_count
                {
                    return Err(invalid(format!(
                        "Shared-tag table row {row_index} does not contain a valid local owner and valid companion"
                    )));
                }
                let owner_entry = entry_table_offset + owner_index * ENTRY_HEADER_SIZE;
                let owner_type_info = read_u32(bytes, owner_entry + size_of::<u32>())?;
                let owner_file_type = ((owner_type_info >> 9) & 0x7F) as u8;
                let owner_file_subtype = ((owner_type_info >> 6) & 0x07) as u8;
                if owner_file_type != 0x10 || owner_file_subtype != 0 {
                    return Err(invalid(format!(
                        "Shared-tag table row {row_index} does not name a local type-16 owner"
                    )));
                }

                // A shared-memory companion is allowed to live in another package. Only a local
                // companion indexes this package's entry directory and can be validated here.
                if companion.pkg_id() == package_id {
                    let companion_index = companion.entry_index() as usize;
                    if companion_index >= entry_count {
                        return Err(invalid(format!(
                            "Shared-tag table row {row_index} names an out-of-range local companion"
                        )));
                    }
                    let companion_entry = entry_table_offset + companion_index * ENTRY_HEADER_SIZE;
                    let companion_type_info = read_u32(bytes, companion_entry + size_of::<u32>())?;
                    let companion_file_type = ((companion_type_info >> 9) & 0x7F) as u8;
                    let companion_file_subtype = ((companion_type_info >> 6) & 0x07) as u8;
                    if read_u32(bytes, companion_entry)? != SHARED_TAG_COMPANION_CLASS
                        || companion_file_type != 0x08
                        || companion_file_subtype != 0
                    {
                        return Err(invalid(format!(
                            "Shared-tag table row {row_index} does not name a local type-8 companion"
                        )));
                    }
                }
                enrolled_type16_indices.insert(owner_index);
            }
            Some(SharedTagTableLayout {
                count,
                header_offset,
                rows_offset,
            })
        };
        for entry_index in 0..entry_count {
            let entry = entry_table_offset + entry_index * ENTRY_HEADER_SIZE;
            let type_info = read_u32(bytes, entry + size_of::<u32>())?;
            let file_type = ((type_info >> 9) & 0x7F) as u8;
            if file_type == 0x10 && !enrolled_type16_indices.contains(&entry_index) {
                return Err(invalid(format!(
                    "Type-16 package entry {entry_index} is missing from the shared-tag table"
                )));
            }
        }
        validate_entry_block_ranges(bytes, &entry_range, block_count)?;
        validate_block_records(bytes, &block_range, patch_id, opaque_trailer_offset)?;

        // Header bytes [0, 0x170) are publisher-signed. Parhelion deliberately performs only the
        // structural checks here; the emitted build plan keeps signature relaxation explicit.
        Ok(Self {
            package_id,
            content_build: read_u32(bytes, CONTENT_BUILD_OFFSET)?,
            content_revision: read_u32(bytes, CONTENT_REVISION_OFFSET)?,
            patch_id,
            entry_count,
            entry_capacity: encoded_entry_capacity,
            block_count,
            block_capacity: encoded_block_capacity,
            entry_table_offset,
            entry_table_trailer_size,
            block_table_offset,
            package_tables_data_offset,
            package_tables_data_size,
            opaque_trailer_offset,
            opaque_trailer_size,
            shared_tag_table,
        })
    }

    fn set_authored_generation(&self, bytes: &mut [u8]) -> AuthoringResult<()> {
        // The final supported HUD bank is still on its earlier UI content generation.
        // Admit that audited package/patch only; other chains retain the existing constraint.
        let hud_predecessor = (
            self.package_id,
            self.patch_id,
            self.content_build,
            self.content_revision,
        ) == (0x037E, 5, 0x0001_4B68, 0);
        let investment_predecessor = self.content_build == SHADOWKEEP_CONTENT_BUILD
            && self.content_revision == SHADOWKEEP_CONTENT_REVISION;
        if !investment_predecessor && !hud_predecessor {
            return Err(invalid(format!(
                "Source package generation is not the expected final Shadowkeep predecessor: \
                 content_build=0x{:08X}, content_revision={}",
                self.content_build, self.content_revision
            )));
        }
        write_u64(bytes, BUILD_SIGNATURE_OFFSET, SUNDIAL_BUILD_SIGNATURE)?;
        Ok(())
    }

    pub(crate) fn set_extended_counts(
        &self,
        bytes: &mut Vec<u8>,
        patch_id: u8,
        entry_count: usize,
        block_count: usize,
        block_table_offset: usize,
        shared_tag_enrollments: &[(u32, u32)],
    ) -> AuthoringResult<()> {
        let inserted_entry_bytes = entry_count
            .checked_sub(self.entry_count)
            .and_then(|count| count.checked_mul(ENTRY_HEADER_SIZE))
            .ok_or_else(|| invalid("Extended entry-table shift overflows"))?;
        let inserted_block_bytes = block_count
            .checked_sub(self.block_count)
            .and_then(|count| count.checked_mul(BLOCK_HEADER_SIZE))
            .ok_or_else(|| invalid("Extended block-table shift overflows"))?;
        let inserted_shared_tag_row_bytes = shared_tag_enrollments
            .len()
            .checked_mul(SHARED_TAG_TABLE_ROW_SIZE)
            .ok_or_else(|| invalid("Extended shared-tag table size overflows"))?;
        let creates_shared_tag_table =
            !shared_tag_enrollments.is_empty() && self.shared_tag_table.is_none();
        let inserted_shared_tag_envelope_bytes = if creates_shared_tag_table {
            DYNAMIC_ARRAY_HEADER_SIZE
                .checked_mul(2)
                .ok_or_else(|| invalid("Extended shared-tag table envelope overflows"))?
        } else {
            0
        };
        let inserted_shared_tag_bytes = inserted_shared_tag_envelope_bytes
            .checked_add(inserted_shared_tag_row_bytes)
            .ok_or_else(|| invalid("Extended shared-tag table size overflows"))?;
        let original_allocated_block_table_end = self
            .block_table_offset
            .checked_add(
                self.block_capacity
                    .checked_mul(BLOCK_HEADER_SIZE)
                    .ok_or_else(|| invalid("Source allocated block-table size overflows"))?,
            )
            .ok_or_else(|| invalid("Source allocated block-table end overflows"))?;
        let package_tables_data_end = self
            .package_tables_data_offset
            .checked_add(self.package_tables_data_size)
            .ok_or_else(|| invalid("Package tables data end overflows"))?;
        if package_tables_data_end < original_allocated_block_table_end {
            return Err(invalid(
                "Package tables data does not contain the source allocated block table",
            ));
        }
        let mut first_shared_tag_header_offset = None;
        if !shared_tag_enrollments.is_empty() {
            if let Some(table) = &self.shared_tag_table {
                let source_rows_end = table
                    .rows_offset
                    .checked_add(
                        table
                            .count
                            .checked_mul(SHARED_TAG_TABLE_ROW_SIZE)
                            .ok_or_else(|| invalid("Source shared-tag table size overflows"))?,
                    )
                    .ok_or_else(|| invalid("Source shared-tag table end overflows"))?;
                if source_rows_end != package_tables_data_end {
                    return Err(invalid(
                        "Source shared-tag table does not terminate the package metadata",
                    ));
                }
            } else if package_tables_data_end != original_allocated_block_table_end {
                return Err(invalid(
                    "Empty source shared-tag table does not immediately follow the allocated block table",
                ));
            }
            let insertion_offset = package_tables_data_end
                .checked_add(inserted_entry_bytes)
                .and_then(|offset| offset.checked_add(inserted_block_bytes))
                .ok_or_else(|| invalid("Extended shared-tag insertion offset overflows"))?;
            if insertion_offset > bytes.len() {
                return Err(invalid(
                    "Extended shared-tag insertion offset exceeds the package",
                ));
            }
            let mut encoded = Vec::with_capacity(inserted_shared_tag_bytes);
            if creates_shared_tag_table {
                encoded.resize(inserted_shared_tag_envelope_bytes, 0);
                first_shared_tag_header_offset = Some(
                    insertion_offset
                        .checked_add(DYNAMIC_ARRAY_HEADER_SIZE)
                        .ok_or_else(|| invalid("First shared-tag header offset overflows"))?,
                );
            }
            for (owner, companion) in shared_tag_enrollments {
                encoded.extend_from_slice(&owner.to_le_bytes());
                encoded.extend_from_slice(&companion.to_le_bytes());
            }
            bytes.splice(insertion_offset..insertion_offset, encoded);
            if let Some(header_offset) = first_shared_tag_header_offset {
                let count = u64::try_from(shared_tag_enrollments.len())
                    .map_err(|_| invalid("Extended shared-tag count exceeds 64 bits"))?;
                write_u32(
                    bytes,
                    header_offset - size_of::<u32>(),
                    SHARED_TAG_TABLE_MARKER,
                )?;
                write_u64(bytes, header_offset, count)?;
                write_u64(
                    bytes,
                    header_offset + size_of::<u64>(),
                    SHARED_TAG_TABLE_CLASS,
                )?;
            }
        }
        let package_tables_data_size = self
            .package_tables_data_size
            .checked_add(inserted_entry_bytes)
            .and_then(|size| size.checked_add(inserted_block_bytes))
            .and_then(|size| size.checked_add(inserted_shared_tag_bytes))
            .ok_or_else(|| invalid("Extended package-tables size exceeds 32 bits"))?;
        let package_tables_data_size_u32 = u32::try_from(package_tables_data_size)
            .map_err(|_| invalid("Extended package-tables size exceeds 32 bits"))?;
        let package_tables_data_size_u64 = u64::try_from(package_tables_data_size)
            .map_err(|_| invalid("Extended package-tables size exceeds 64 bits"))?;
        let entry_count_u64 = u64::try_from(entry_count)
            .map_err(|_| invalid("Extended entry count exceeds 64 bits"))?;
        let block_count_u64 = u64::try_from(block_count)
            .map_err(|_| invalid("Extended block count exceeds 64 bits"))?;
        let entry_capacity = self
            .entry_capacity
            .checked_add(inserted_entry_bytes / ENTRY_HEADER_SIZE)
            .ok_or_else(|| invalid("Extended entry capacity overflows"))?;
        let block_capacity = self
            .block_capacity
            .checked_add(inserted_block_bytes / BLOCK_HEADER_SIZE)
            .ok_or_else(|| invalid("Extended block capacity overflows"))?;
        if entry_capacity > MAX_ENTRY_COUNT {
            return Err(invalid(format!(
                "Extended entry-region count exceeds {MAX_ENTRY_COUNT}"
            )));
        }
        if block_capacity > MAX_BLOCK_COUNT {
            return Err(invalid(format!(
                "Extended block capacity exceeds {MAX_BLOCK_COUNT}"
            )));
        }
        let entry_capacity_u32 = u32::try_from(entry_capacity)
            .map_err(|_| invalid("Extended entry capacity exceeds 32 bits"))?;
        let block_capacity_u64 = u64::try_from(block_capacity)
            .map_err(|_| invalid("Extended block capacity exceeds 64 bits"))?;
        write_u64(
            bytes,
            self.package_tables_data_offset + PACKAGE_TABLES_THIS_SIZE_OFFSET,
            package_tables_data_size_u64,
        )?;
        write_u32(
            bytes,
            PACKAGE_TABLES_DATA_SIZE_OFFSET,
            package_tables_data_size_u32,
        )?;
        write_u64(
            bytes,
            self.package_tables_data_offset + PACKAGE_TABLES_ENTRY_DESCRIPTOR_OFFSET,
            entry_count_u64,
        )?;
        write_u64(
            bytes,
            self.package_tables_data_offset + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET,
            block_count_u64,
        )?;
        adjust_relative_pointer(
            bytes,
            self.package_tables_data_offset
                + PACKAGE_TABLES_BLOCK_DESCRIPTOR_OFFSET
                + DYNAMIC_ARRAY_POINTER_OFFSET,
            inserted_entry_bytes,
        )?;
        let shared_tag_pointer = self.package_tables_data_offset
            + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET
            + DYNAMIC_ARRAY_POINTER_OFFSET;
        if self.shared_tag_table.is_some() {
            adjust_relative_pointer(
                bytes,
                shared_tag_pointer,
                inserted_entry_bytes + inserted_block_bytes,
            )?;
        } else if let Some(header_offset) = first_shared_tag_header_offset {
            let header_offset = i64::try_from(header_offset)
                .map_err(|_| invalid("First shared-tag header offset exceeds 64 bits"))?;
            let pointer = i64::try_from(shared_tag_pointer)
                .map_err(|_| invalid("Shared-tag descriptor offset exceeds 64 bits"))?;
            let relative = header_offset
                .checked_sub(pointer)
                .ok_or_else(|| invalid("First shared-tag descriptor displacement overflows"))?;
            write_i64(bytes, shared_tag_pointer, relative)?;
        }
        if !shared_tag_enrollments.is_empty() {
            let final_count = self
                .shared_tag_table
                .as_ref()
                .map_or(0, |table| table.count)
                .checked_add(shared_tag_enrollments.len())
                .ok_or_else(|| invalid("Extended shared-tag count overflows"))?;
            let final_count = u64::try_from(final_count)
                .map_err(|_| invalid("Extended shared-tag count exceeds 64 bits"))?;
            write_u64(
                bytes,
                self.package_tables_data_offset + PACKAGE_TABLES_SHARED_TAG_DESCRIPTOR_OFFSET,
                final_count,
            )?;
            let shifted_header_offset = if let Some(table) = &self.shared_tag_table {
                table
                    .header_offset
                    .checked_add(inserted_entry_bytes)
                    .and_then(|offset| offset.checked_add(inserted_block_bytes))
                    .ok_or_else(|| invalid("Extended shared-tag header offset overflows"))?
            } else {
                first_shared_tag_header_offset
                    .expect("creating a shared-tag table records its header offset")
            };
            write_u64(bytes, shifted_header_offset, final_count)?;
        }
        let entry_count_u32 = u32::try_from(entry_count)
            .map_err(|_| invalid("Extended entry count exceeds 32 bits"))?;
        let block_count_u32 = u32::try_from(block_count)
            .map_err(|_| invalid("Extended block count exceeds 32 bits"))?;
        self.set_authored_generation(bytes)?;
        write_u16(bytes, PATCH_ID_OFFSET, u16::from(patch_id))?;
        write_u32(bytes, ENTRY_COUNT_OFFSET, entry_count_u32)?;
        write_u32(
            bytes,
            self.entry_table_offset - ENTRY_TABLE_COUNT_BACK,
            entry_capacity_u32,
        )?;
        write_u32(bytes, BLOCK_COUNT_OFFSET, block_count_u32)?;
        write_u64(
            bytes,
            block_table_offset - BLOCK_TABLE_COUNT_BACK,
            block_capacity_u64,
        )?;
        Ok(())
    }

    pub(crate) fn shared_tag_enrollment_count(&self) -> usize {
        self.shared_tag_table
            .as_ref()
            .map_or(0, |table| table.count)
    }

    pub(crate) fn shared_tag_enrollment_rows<'a>(
        &self,
        bytes: &'a [u8],
    ) -> AuthoringResult<&'a [u8]> {
        let Some(table) = &self.shared_tag_table else {
            return Ok(&bytes[0..0]);
        };
        let size = table
            .count
            .checked_mul(SHARED_TAG_TABLE_ROW_SIZE)
            .ok_or_else(|| invalid("Shared-tag table size overflows"))?;
        let range = checked_range(bytes, table.rows_offset, size, "shared-tag table rows")?;
        Ok(&bytes[range])
    }

    /// Returns the complete logical package metadata without carrying this patch's block store.
    ///
    /// A later patch repeats the effective entry and block directories, but an inherited block
    /// record continues to name the older patch file that owns its bytes. The only physical bytes
    /// a new overlay needs after this prefix are blocks whose `patch_id` is the new patch.
    pub(crate) fn sparse_overlay_metadata_prefix<'a>(
        &self,
        bytes: &'a [u8],
    ) -> AuthoringResult<&'a [u8]> {
        let metadata_end = self
            .package_tables_data_offset
            .checked_add(self.package_tables_data_size)
            .ok_or_else(|| invalid("Package metadata boundary overflows"))?;
        let signature_end = PACKAGE_FILE_ALIGNMENT
            .checked_add(HEADER_SIGNATURE_SIZE)
            .ok_or_else(|| invalid("Package header-signature boundary overflows"))?;
        let extended_offset = read_u32(bytes, EXTENDED_DATA_POINTER_OFFSET)? as usize;
        let extended_size = read_u32(bytes, EXTENDED_DATA_SIZE_OFFSET)? as usize;
        let extended_end = extended_offset
            .checked_add(extended_size)
            .ok_or_else(|| invalid("Package extended-header boundary overflows"))?;
        let required_prefix_end = signature_end.max(extended_end);
        if required_prefix_end > metadata_end {
            return Err(invalid(
                "Package metadata does not follow every required header-described region",
            ));
        }

        let mut owned_ranges = Vec::new();
        for index in 0..self.block_count {
            let row = self.block_table_offset + index * BLOCK_HEADER_SIZE;
            let file_offset = read_u32(bytes, row)? as usize;
            let stored_size = read_u32(bytes, row + 4)? as usize;
            let patch_id = read_u16(bytes, row + 8)?;
            if patch_id != self.patch_id || stored_size == 0 {
                continue;
            }
            let range = checked_range(
                bytes,
                file_offset,
                stored_size,
                &format!("patch-owned block {index}"),
            )?;
            if range.start < metadata_end || range.end > self.opaque_trailer_offset {
                return Err(invalid(format!(
                    "Patch-owned block {index} overlaps package metadata or the opaque trailer"
                )));
            }
            owned_ranges.push(range);
        }
        owned_ranges.sort_unstable_by_key(|range| (range.start, range.end));

        // Everything after the metadata must be either this patch's block bytes or alignment
        // padding. Failing closed here prevents us from silently discarding an unknown region.
        let mut covered_end = metadata_end;
        for range in owned_ranges {
            if range.start > covered_end
                && bytes[covered_end..range.start]
                    .iter()
                    .any(|byte| *byte != 0)
            {
                return Err(invalid(
                    "Package contains unclassified nonzero data after its metadata",
                ));
            }
            covered_end = covered_end.max(range.end);
        }
        if bytes[covered_end..self.opaque_trailer_offset]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(invalid(
                "Package contains unclassified nonzero data before its opaque trailer",
            ));
        }

        let prefix = checked_range(bytes, 0, metadata_end, "sparse overlay metadata prefix")?;
        Ok(&bytes[prefix])
    }

    pub(crate) fn set_file_size(&self, bytes: &mut [u8]) -> AuthoringResult<()> {
        self.write_file_sizes(bytes)
    }

    pub(crate) fn update_package_tables_hash(
        &self,
        bytes: &mut [u8],
    ) -> AuthoringResult<Range<usize>> {
        let package_tables_data_size = read_u32(bytes, PACKAGE_TABLES_DATA_SIZE_OFFSET)? as usize;
        let package_tables = checked_range(
            bytes,
            self.package_tables_data_offset,
            package_tables_data_size,
            "package tables data",
        )?;
        let digest = Sha1::digest(&bytes[package_tables]);
        let hash = checked_range(
            bytes,
            PACKAGE_TABLES_DATA_HASH_OFFSET,
            REGION_HASH_SIZE,
            "package tables hash",
        )?;
        bytes[hash.clone()].copy_from_slice(&digest);
        Ok(hash)
    }

    pub(crate) fn opaque_trailer<'a>(&self, bytes: &'a [u8]) -> AuthoringResult<&'a [u8]> {
        checked_range(
            bytes,
            self.opaque_trailer_offset,
            self.opaque_trailer_size,
            "package opaque trailer",
        )
        .map(|range| &bytes[range])
    }

    fn write_file_sizes(&self, bytes: &mut [u8]) -> AuthoringResult<()> {
        let file_size = u32::try_from(bytes.len())
            .map_err(|_| invalid("Overlay package exceeds the 32-bit package size field"))?;
        let trailer_size = u32::try_from(self.opaque_trailer_size)
            .map_err(|_| invalid("Package opaque trailer exceeds 32 bits"))?;
        let trailer_offset = file_size
            .checked_sub(trailer_size)
            .ok_or_else(|| invalid("Overlay package is shorter than its opaque trailer"))?;
        write_u32(bytes, OPAQUE_TRAILER_OFFSET_FIELD, trailer_offset)?;
        write_u32(bytes, FILE_SIZE_OFFSET, file_size)
    }
}

pub(crate) fn append_aligned(bytes: &mut Vec<u8>, payload: &[u8]) -> usize {
    let padding =
        (BLOCK_PAYLOAD_ALIGNMENT - bytes.len() % BLOCK_PAYLOAD_ALIGNMENT) % BLOCK_PAYLOAD_ALIGNMENT;
    bytes.resize(bytes.len() + padding, 0);
    let offset = bytes.len();
    bytes.extend_from_slice(payload);
    offset
}

pub(crate) fn append_opaque_trailer(bytes: &mut Vec<u8>, trailer: &[u8]) -> AuthoringResult<()> {
    if trailer.len() != PACKAGE_FILE_ALIGNMENT
        || trailer.get(..4) != Some(OPAQUE_TRAILER_MARKER.as_slice())
    {
        return Err(invalid(
            "Cannot append an unsupported package trailing region",
        ));
    }
    let padding =
        (PACKAGE_FILE_ALIGNMENT - bytes.len() % PACKAGE_FILE_ALIGNMENT) % PACKAGE_FILE_ALIGNMENT;
    bytes.resize(bytes.len() + padding, 0);
    bytes.extend_from_slice(trailer);
    Ok(())
}

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

fn validate_locale_fields(bytes: &[u8]) -> AuthoringResult<()> {
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
fn validate_hashed_region(
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

fn ensure_contained(
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

fn validate_entry_block_ranges(
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

fn validate_block_records(
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

#[cfg(test)]
pub(crate) fn build_test_package_with_physical_payload(
    package_id: u16,
    patch_id: u16,
    payload: &[u8],
) -> AuthoringResult<Vec<u8>> {
    if payload.is_empty() || payload.len() > BLOCK_SIZE {
        return Err(invalid(
            "A test package payload must fit one nonempty block",
        ));
    }
    let mut bytes = build_standalone_package_skeleton(package_id, &[[0; 8]], 1, &[])?;
    let layout = PackageLayout::parse(&bytes)?;
    let trailer = layout.opaque_trailer(&bytes)?.to_vec();
    bytes.truncate(layout.opaque_trailer_offset);

    let payload_offset = append_aligned(&mut bytes, payload);
    let block_row = layout.block_table_offset;
    write_u32(
        &mut bytes,
        block_row,
        u32::try_from(payload_offset)
            .map_err(|_| invalid("Test payload offset exceeds 32 bits"))?,
    )?;
    write_u32(
        &mut bytes,
        block_row + 4,
        u32::try_from(payload.len()).map_err(|_| invalid("Test payload size exceeds 32 bits"))?,
    )?;
    write_u16(&mut bytes, block_row + 8, patch_id)?;
    bytes[block_row + 12..block_row + 12 + REGION_HASH_SIZE]
        .copy_from_slice(&Sha1::digest(payload));
    write_u16(&mut bytes, PATCH_ID_OFFSET, patch_id)?;
    write_u64(
        &mut bytes,
        layout.entry_table_offset + 8,
        (payload.len() as u64) << 28,
    )?;
    layout.update_package_tables_hash(&mut bytes)?;
    append_opaque_trailer(&mut bytes, &trailer)?;
    layout.set_file_size(&mut bytes)?;
    PackageLayout::parse(&bytes)?;
    Ok(bytes)
}

fn checked_range(
    bytes: &[u8],
    offset: usize,
    size: usize,
    field: &str,
) -> AuthoringResult<Range<usize>> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| invalid(format!("{field} range overflows")))?;
    if end > bytes.len() {
        return Err(invalid(format!("{field} extends beyond the package")));
    }
    Ok(offset..end)
}

fn read_u16(bytes: &[u8], offset: usize) -> AuthoringResult<u16> {
    let range = checked_range(bytes, offset, size_of::<u16>(), "16-bit field")?;
    Ok(u16::from_le_bytes(
        bytes[range]
            .try_into()
            .expect("a checked two-byte range has the requested size"),
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> AuthoringResult<u32> {
    let range = checked_range(bytes, offset, size_of::<u32>(), "32-bit field")?;
    Ok(u32::from_le_bytes(bytes[range].try_into().expect(
        "a checked four-byte range has the requested size",
    )))
}

fn read_u64(bytes: &[u8], offset: usize) -> AuthoringResult<u64> {
    let range = checked_range(bytes, offset, size_of::<u64>(), "64-bit field")?;
    Ok(u64::from_le_bytes(bytes[range].try_into().expect(
        "a checked eight-byte range has the requested size",
    )))
}

fn read_i64(bytes: &[u8], offset: usize) -> AuthoringResult<i64> {
    let range = checked_range(bytes, offset, size_of::<i64>(), "signed 64-bit field")?;
    Ok(i64::from_le_bytes(bytes[range].try_into().expect(
        "a checked signed eight-byte range has the requested size",
    )))
}

fn relative_target(bytes: &[u8], pointer: usize) -> AuthoringResult<usize> {
    let relative = read_i64(bytes, pointer)?;
    if relative >= 0 {
        pointer
            .checked_add(relative as usize)
            .ok_or_else(|| invalid("Relative package pointer overflows"))
    } else {
        pointer
            .checked_sub(relative.unsigned_abs() as usize)
            .ok_or_else(|| invalid("Relative package pointer points before the file"))
    }
}

fn adjust_relative_pointer(
    bytes: &mut [u8],
    pointer: usize,
    forward: usize,
) -> AuthoringResult<()> {
    if forward == 0 {
        return Ok(());
    }
    let relative = read_i64(bytes, pointer)?;
    if relative == 0 {
        return Ok(());
    }
    let forward = i64::try_from(forward)
        .map_err(|_| invalid("Relative package pointer adjustment exceeds 64 bits"))?;
    let adjusted = relative
        .checked_add(forward)
        .ok_or_else(|| invalid("Relative package pointer adjustment overflows"))?;
    write_i64(bytes, pointer, adjusted)
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> AuthoringResult<()> {
    let range = checked_range(bytes, offset, size_of::<u16>(), "16-bit field")?;
    bytes[range].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> AuthoringResult<()> {
    let range = checked_range(bytes, offset, size_of::<u32>(), "32-bit field")?;
    bytes[range].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) -> AuthoringResult<()> {
    let range = checked_range(bytes, offset, size_of::<u64>(), "64-bit field")?;
    bytes[range].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_i64(bytes: &mut [u8], offset: usize, value: i64) -> AuthoringResult<()> {
    let range = checked_range(bytes, offset, size_of::<i64>(), "signed 64-bit field")?;
    bytes[range].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests;
