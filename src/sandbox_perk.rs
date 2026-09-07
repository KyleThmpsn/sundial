//! Bounds-checked authoring for the finished sandbox-perk catalog and its runtime map.

use std::mem::size_of;

use crate::{
    investment_schema::{
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, NESTED_ARRAY_TRAILER,
        investment_globals_table_tag,
    },
    package_payload::{i64_at, relative_offset, u32_at, u64_at},
    weapon_entity::{
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        WEAPON_ENTITY_CLASS, validate_weapon_entity,
    },
};
use tiger_pkg::{PackageManager, TagHash};

pub mod activation;

/// Package class for the finished sandbox-perk catalog.
pub const FINISHED_SANDBOX_PERK_CATALOG_CLASS: u32 = 0x8080_5C97;
/// Primary row class in the finished sandbox-perk catalog.
pub const FINISHED_SANDBOX_PERK_ROW_CLASS: u32 = 0x8080_5C9D;
/// Size of one primary finished sandbox-perk row.
pub const FINISHED_SANDBOX_PERK_ROW_SIZE: usize = 0x18;
/// Secondary row class in the finished sandbox-perk catalog.
pub const FINISHED_SANDBOX_PERK_DETAIL_ROW_CLASS: u32 = 0x8080_5C9F;
/// Size of one secondary finished sandbox-perk row.
pub const FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE: usize = 0x1C;

/// Package class for the per-index sandbox-perk metadata table.
pub const SANDBOX_PERK_INDEX_CATALOG_CLASS: u32 = 0x8080_7AAA;
/// Row class for per-index sandbox-perk metadata.
pub const SANDBOX_PERK_INDEX_ROW_CLASS: u32 = 0x8080_7AAE;
/// Size of one per-index sandbox-perk metadata row.
pub const SANDBOX_PERK_INDEX_ROW_SIZE: usize = 0x08;

const FINISHED_SANDBOX_PERK_NAME_REFERENCE_OFFSET: usize = 0;

/// Advisory for Sunrise's replicated appearance-data reader, not a native package limit.
///
/// At Sunrise commit 26bfe280 (2026-09-05), `items::kSandboxPerkCapacity` and
/// build-data `details::Definition` retain four non-sentinel entries per item or plug.
/// Private clones replace existing entries, so their source plug's count still applies.
/// Do not truncate authored arrays or infer that all native consumers have this limit.
/// See `docs/sunrise-contracts.md` for provenance and the separate 16-entry weapon bank.
#[must_use]
pub fn sunrise_perk_projection_warning(perk_count: usize) -> Option<&'static str> {
    (perk_count > 4).then_some(
        "Sunrise compatibility: the reviewed runtime copies only the first 4 sandbox-perk entries per item or plug into replicated appearance data. Later entries are omitted from that path. This is not a package-format limit; your entries are preserved.",
    )
}

/// Package class for the shared sandbox-pattern/perk runtime-key map.
pub const SANDBOX_PERK_RUNTIME_MAP_CLASS: u32 = SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS;
/// Fixed live tag for the shared sandbox-pattern/perk runtime-key map.
pub const SANDBOX_PERK_RUNTIME_MAP_TAG: u32 = SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG;
/// Primary row class in the shared runtime-key map.
pub const SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_CLASS: u32 =
    SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS;
/// Size of one shared runtime-key assignment row.
pub const SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE: usize =
    SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
/// Auxiliary row class in the global sandbox-perk runtime-key map.
pub const SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_CLASS: u32 = 0x8080_000B;
/// Size of one auxiliary runtime-map row.
pub const SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_SIZE: usize = 0x04;

const PRIMARY_DESCRIPTOR: usize = 0x08;
const SECONDARY_DESCRIPTOR: usize = 0x18;
const ARRAY_HEADER_SIZE: usize = 0x10;
const ARRAY_TRAILER_SIZE: usize = NESTED_ARRAY_TRAILER.len();
const SINGLE_ARRAY_HEADER: usize = 0x20;
const ROOT_ARRAY_HEADER: usize = 0x30;

// A direct action-graph reference is stored as a 64-bit tag lane.  The native action
// member class occurs 0x14 bytes before the lane; this is the stock layout used by the
// finished-perk actions (including Micro-Missile).  Do not relax this to a general
// aligned-word search: a graph tag can coincidentally occur in scalar or hash data.
const ACTION_GRAPH_REFERENCE_MEMBER_CLASS_DELTA: usize = 0x14;

/// One row from the finished sandbox-perk catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinishedSandboxPerk {
    pub index: usize,
    pub perk_hash: u32,
    pub runtime_key: u32,
    pub trailing: u64,
    /// The pointed-to native detail row. A zero relative pointer represents no detail row.
    pub detail: Option<[u8; FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE]>,
}

/// A localized name owned by an authored finished sandbox-perk definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinishedSandboxPerkName {
    pub bank_index: u16,
    pub string_hash: u32,
}

/// Display fields for a private finished perk, independent of its action graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinishedSandboxPerkPresentation {
    pub name: Option<FinishedSandboxPerkName>,
    pub description: Option<FinishedSandboxPerkName>,
    /// Copy the native tooltip grouping field from a classified donor's visible perk.
    pub category_source_index: Option<usize>,
    /// Additional effects belong to the composite plug, not separate tooltip entries.
    pub hidden: bool,
}

/// One sorted entry in the global runtime-key map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxPerkRuntimeAssignment {
    pub index: usize,
    pub runtime_key: u32,
    pub runtime_tag: u32,
}

/// One runtime graph directly referenced by a finished sandbox-perk action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPerkRuntimeGraphSource {
    pub tag: TagHash,
    pub action_offsets: Vec<usize>,
    pub payload: Vec<u8>,
}

/// The action and graph resources reached by one finished sandbox-perk row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPerkRuntimeAction {
    pub finished_perk: FinishedSandboxPerk,
    pub action_tag: TagHash,
    pub action_payload: Vec<u8>,
    pub graphs: Vec<SandboxPerkRuntimeGraphSource>,
}

/// Resolves a boxed scalar owned by a polymorphic node in a finished-perk runtime action.
///
/// Native action payloads store the concrete node type immediately before the node bytes. A
/// self-relative pointer inside that node addresses the boxed scalar bytes, whose concrete type
/// is likewise stored immediately before the value. Selecting by both concrete types and the
/// occurrence among structurally valid nodes avoids depending on an absolute payload offset.
pub fn sandbox_perk_action_boxed_value_offset(
    action_payload: &[u8],
    node_type_handle: u32,
    node_occurrence: u16,
    value_pointer_offset: u32,
    value_type_handle: u32,
    value_size: usize,
) -> Result<usize, String> {
    validate_runtime_action_payload(action_payload, None)?;
    if !is_native_member_class(node_type_handle)
        || !is_native_member_class(value_type_handle)
        || value_size == 0
    {
        return Err("Sandbox-perk action value locator has an invalid native type or size".into());
    }

    let pointer_delta = usize::try_from(value_pointer_offset)
        .map_err(|_| "Sandbox-perk action value pointer offset is too large")?;
    let mut candidates = Vec::new();
    for class_offset in (0..action_payload.len().saturating_sub(3)).step_by(4) {
        if u32_at(action_payload, class_offset)? != node_type_handle {
            continue;
        }
        let node = class_offset
            .checked_add(size_of::<u32>())
            .ok_or("Sandbox-perk action node offset overflowed")?;
        let Some(pointer) = node.checked_add(pointer_delta) else {
            continue;
        };
        let Ok(relative) = i64_at(action_payload, pointer) else {
            continue;
        };
        if relative == 0 {
            continue;
        }
        let Ok(value) = relative_offset(pointer, 0, relative) else {
            continue;
        };
        let Some(value_class) = value.checked_sub(size_of::<u32>()) else {
            continue;
        };
        if u32_at(action_payload, value_class).ok() != Some(value_type_handle)
            || value
                .checked_add(value_size)
                .is_none_or(|end| end > action_payload.len())
        {
            continue;
        }
        candidates.push(value);
    }
    candidates.sort_unstable();
    candidates.dedup();
    candidates
        .get(usize::from(node_occurrence))
        .copied()
        .ok_or_else(|| {
            format!(
                "Sandbox-perk action has {} structurally valid 0x{node_type_handle:08X} -> 0x{value_type_handle:08X} boxed values, not occurrence {}",
                candidates.len(), node_occurrence
            )
        })
}

#[derive(Clone, Copy, Debug)]
struct NativeArray {
    count: usize,
    header: usize,
    rows: usize,
    row_class: u32,
}

#[derive(Clone, Copy, Debug)]
struct TwoArrayLayout {
    primary: NativeArray,
    primary_end: usize,
    secondary: NativeArray,
    secondary_end: usize,
}

/// Validates a finished sandbox-perk catalog payload.
pub fn validate_finished_sandbox_perk_catalog(payload: &[u8]) -> Result<(), String> {
    let layout = finished_catalog_layout(payload)?;
    for index in 0..layout.primary.count {
        let _ = finished_sandbox_perk_with_layout(payload, layout, index)?;
    }
    Ok(())
}

/// Parses one primary row and its optional pointed-to detail row.
pub fn finished_sandbox_perk_at(
    payload: &[u8],
    index: usize,
) -> Result<FinishedSandboxPerk, String> {
    let layout = finished_catalog_layout(payload)?;
    finished_sandbox_perk_with_layout(payload, layout, index)
}

/// Returns the number of finished sandbox-perk rows.
pub fn finished_sandbox_perk_count(payload: &[u8]) -> Result<usize, String> {
    Ok(finished_catalog_layout(payload)?.primary.count)
}

/// Clones one finished sandbox perk, assigns new identities, and appends it to the catalog.
///
/// The existing primary rows retain their order. Their non-null self-relative pointers are
/// rewritten after the structural shift, while a byte-identical copy of the donor's detail row is
/// appended to the secondary array.
pub fn clone_and_append_finished_sandbox_perk(
    payload: &[u8],
    donor_index: usize,
    new_perk_hash: u32,
    new_runtime_key: u32,
) -> Result<(Vec<u8>, usize), String> {
    clone_and_append_named_finished_sandbox_perk(
        payload,
        donor_index,
        new_perk_hash,
        new_runtime_key,
        None,
    )
}

/// Clones and appends a finished sandbox perk while optionally assigning it an authored name.
///
/// The first field in the pointed detail row is the native eight-byte localized-string reference.
/// Keeping this reference private prevents an authored perk identity from borrowing the donor's
/// display identity.
pub fn clone_and_append_named_finished_sandbox_perk(
    payload: &[u8],
    donor_index: usize,
    new_perk_hash: u32,
    new_runtime_key: u32,
    name: Option<FinishedSandboxPerkName>,
) -> Result<(Vec<u8>, usize), String> {
    clone_and_append_presented_finished_sandbox_perk(
        payload,
        donor_index,
        new_perk_hash,
        new_runtime_key,
        FinishedSandboxPerkPresentation {
            name,
            ..Default::default()
        },
    )
}

/// Clones gameplay identity while authoring the separately consumed tooltip detail row.
pub fn clone_and_append_presented_finished_sandbox_perk(
    payload: &[u8],
    donor_index: usize,
    new_perk_hash: u32,
    new_runtime_key: u32,
    presentation: FinishedSandboxPerkPresentation,
) -> Result<(Vec<u8>, usize), String> {
    let layout = finished_catalog_layout(payload)?;
    let donor = finished_sandbox_perk_with_layout(payload, layout, donor_index)?;
    let donor_detail = donor.detail.ok_or_else(|| {
        format!("Finished sandbox-perk row {donor_index} has no detail row to clone")
    })?;
    let mut authored_detail = donor_detail;
    if let Some(name) = presentation.name {
        write_u16(
            &mut authored_detail,
            FINISHED_SANDBOX_PERK_NAME_REFERENCE_OFFSET,
            name.bank_index,
        )?;
        write_u16(
            &mut authored_detail,
            FINISHED_SANDBOX_PERK_NAME_REFERENCE_OFFSET + 2,
            0,
        )?;
        write_u32(
            &mut authored_detail,
            FINISHED_SANDBOX_PERK_NAME_REFERENCE_OFFSET + 4,
            name.string_hash,
        )?;
    }
    if let Some(description) = presentation.description {
        write_u16(&mut authored_detail, 8, description.bank_index)?;
        write_u16(&mut authored_detail, 10, 0)?;
        write_u32(&mut authored_detail, 12, description.string_hash)?;
    }
    if let Some(index) = presentation.category_source_index {
        let source = finished_sandbox_perk_with_layout(payload, layout, index)?
            .detail
            .ok_or("Tooltip classification donor has no detail row")?;
        authored_detail[20..24].copy_from_slice(&source[20..24]);
    }
    if presentation.hidden {
        // Stock invisible effect 416 uses these absent localized references, icon
        // index and empty grouping hashes. Keep the runtime identity untouched.
        authored_detail = [
            0xFF, 0xFF, 0, 0, 0xC5, 0x9D, 0x1C, 0x81, 0xFF, 0xFF, 0, 0, 0xC5, 0x9D, 0x1C, 0x81,
            0xFF, 0xFF, 0, 0, 0xC5, 0x9D, 0x1C, 0x81, 0xC5, 0x9D, 0x1C, 0x81,
        ];
    }

    for index in 0..layout.primary.count {
        let row = primary_row(layout, index, FINISHED_SANDBOX_PERK_ROW_SIZE)?;
        let perk_hash = u32_at(payload, row)?;
        let runtime_key = u32_at(payload, row + 4)?;
        if perk_hash == new_perk_hash {
            return Err(format!(
                "Finished sandbox-perk hash 0x{new_perk_hash:08X} already exists at row {index}"
            ));
        }
        if runtime_key == new_runtime_key {
            return Err(format!(
                "Sandbox-perk runtime key 0x{new_runtime_key:08X} already exists at row {index}"
            ));
        }
    }

    let new_primary_count = layout
        .primary
        .count
        .checked_add(1)
        .ok_or("Finished sandbox-perk primary count overflowed")?;
    let new_secondary_count = layout
        .secondary
        .count
        .checked_add(1)
        .ok_or("Finished sandbox-perk detail count overflowed")?;
    let new_len = payload
        .len()
        .checked_add(FINISHED_SANDBOX_PERK_ROW_SIZE)
        .and_then(|size| size.checked_add(FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE))
        .ok_or("Finished sandbox-perk catalog size overflowed")?;

    let donor_row = primary_row(layout, donor_index, FINISHED_SANDBOX_PERK_ROW_SIZE)?;
    let mut authored = Vec::with_capacity(new_len);
    authored.extend_from_slice(&payload[..layout.primary_end]);
    let new_row = authored.len();
    authored.extend_from_slice(
        payload
            .get(donor_row..donor_row + FINISHED_SANDBOX_PERK_ROW_SIZE)
            .ok_or("Finished sandbox-perk donor row is out of bounds")?,
    );
    authored.extend_from_slice(&payload[layout.primary_end..]);
    let new_detail = authored.len();
    authored.extend_from_slice(&authored_detail);

    write_u64(&mut authored, 0, usize_as_u64(new_len, "catalog size")?)?;
    write_u64(
        &mut authored,
        PRIMARY_DESCRIPTOR,
        usize_as_u64(new_primary_count, "primary count")?,
    )?;
    write_u64(
        &mut authored,
        layout.primary.header,
        usize_as_u64(new_primary_count, "primary header count")?,
    )?;
    write_u64(
        &mut authored,
        SECONDARY_DESCRIPTOR,
        usize_as_u64(new_secondary_count, "detail count")?,
    )?;

    let shifted_secondary_header = layout
        .secondary
        .header
        .checked_add(FINISHED_SANDBOX_PERK_ROW_SIZE)
        .ok_or("Finished sandbox-perk detail header overflowed")?;
    write_relative_pointer(
        &mut authored,
        SECONDARY_DESCRIPTOR + 8,
        shifted_secondary_header,
    )?;
    write_u64(
        &mut authored,
        shifted_secondary_header,
        usize_as_u64(new_secondary_count, "detail header count")?,
    )?;

    for index in 0..layout.primary.count {
        let old_row = primary_row(layout, index, FINISHED_SANDBOX_PERK_ROW_SIZE)?;
        let old_pointer = i64_at(payload, old_row + 8)?;
        if old_pointer == 0 {
            write_i64(&mut authored, old_row + 8, 0)?;
            continue;
        }
        let old_target = detail_target(layout, old_row, old_pointer)?
            .ok_or("Non-null sandbox-perk detail pointer resolved as null")?;
        let shifted_target = old_target
            .checked_add(FINISHED_SANDBOX_PERK_ROW_SIZE)
            .ok_or("Finished sandbox-perk detail target overflowed")?;
        write_relative_pointer(&mut authored, old_row + 8, shifted_target)?;
    }

    write_u32(&mut authored, new_row, new_perk_hash)?;
    write_u32(&mut authored, new_row + 4, new_runtime_key)?;
    write_relative_pointer(&mut authored, new_row + 8, new_detail)?;

    let new_index = layout.primary.count;
    validate_finished_sandbox_perk_catalog(&authored)?;
    let appended = finished_sandbox_perk_at(&authored, new_index)?;
    if appended.perk_hash != new_perk_hash
        || appended.runtime_key != new_runtime_key
        || appended.trailing != donor.trailing
        || appended.detail != Some(authored_detail)
    {
        return Err("Authored finished sandbox-perk row did not round-trip".into());
    }
    Ok((authored, new_index))
}

/// Validates the per-index sandbox-perk metadata table paired with the finished-perk catalog.
pub fn validate_sandbox_perk_index_catalog(payload: &[u8]) -> Result<(), String> {
    let _ = sandbox_perk_index_layout(payload)?;
    Ok(())
}

/// Returns the number of rows in the per-index sandbox-perk metadata table.
pub fn sandbox_perk_index_count(payload: &[u8]) -> Result<usize, String> {
    Ok(sandbox_perk_index_layout(payload)?.count)
}

/// Returns the public perk hash stored at one metadata-table index.
pub fn sandbox_perk_index_hash_at(payload: &[u8], index: usize) -> Result<u32, String> {
    let layout = sandbox_perk_index_layout(payload)?;
    if index >= layout.count {
        return Err(format!(
            "Sandbox-perk metadata index {index} is outside the {}-row table",
            layout.count
        ));
    }
    let row = layout
        .rows
        .checked_add(
            index
                .checked_mul(SANDBOX_PERK_INDEX_ROW_SIZE)
                .ok_or("Sandbox-perk metadata row offset overflowed")?,
        )
        .ok_or("Sandbox-perk metadata row offset overflowed")?;
    u32_at(payload, row)
}

/// Clones the donor's per-index metadata row and appends it under a new perk hash.
///
/// The client addresses this table with the same perk index used for the finished-perk catalog.
/// Extending only one of the pair leaves the authored index out of bounds in inspection UI paths.
pub fn clone_and_append_sandbox_perk_index(
    payload: &[u8],
    donor_index: usize,
    new_perk_hash: u32,
) -> Result<(Vec<u8>, usize), String> {
    let layout = sandbox_perk_index_layout(payload)?;
    if donor_index >= layout.count {
        return Err(format!(
            "Sandbox-perk metadata donor index {donor_index} is outside the {}-row table",
            layout.count
        ));
    }
    for index in 0..layout.count {
        let row = layout
            .rows
            .checked_add(
                index
                    .checked_mul(SANDBOX_PERK_INDEX_ROW_SIZE)
                    .ok_or("Sandbox-perk metadata row offset overflowed")?,
            )
            .ok_or("Sandbox-perk metadata row offset overflowed")?;
        if u32_at(payload, row)? == new_perk_hash {
            return Err(format!(
                "Sandbox-perk metadata hash 0x{new_perk_hash:08X} already exists at row {index}"
            ));
        }
    }

    let donor_row = layout
        .rows
        .checked_add(
            donor_index
                .checked_mul(SANDBOX_PERK_INDEX_ROW_SIZE)
                .ok_or("Sandbox-perk metadata donor offset overflowed")?,
        )
        .ok_or("Sandbox-perk metadata donor offset overflowed")?;
    let donor_end = donor_row
        .checked_add(SANDBOX_PERK_INDEX_ROW_SIZE)
        .ok_or("Sandbox-perk metadata donor range overflowed")?;
    let mut authored = Vec::with_capacity(
        payload
            .len()
            .checked_add(SANDBOX_PERK_INDEX_ROW_SIZE)
            .ok_or("Sandbox-perk metadata size overflowed")?,
    );
    authored.extend_from_slice(payload);
    authored.extend_from_slice(
        payload
            .get(donor_row..donor_end)
            .ok_or("Sandbox-perk metadata donor row is out of bounds")?,
    );
    let new_index = layout.count;
    let new_row = payload.len();
    let new_count = layout
        .count
        .checked_add(1)
        .ok_or("Sandbox-perk metadata count overflowed")?;
    let authored_len = authored.len();
    write_u64(
        &mut authored,
        0,
        usize_as_u64(authored_len, "metadata catalog size")?,
    )?;
    write_u64(
        &mut authored,
        PRIMARY_DESCRIPTOR,
        usize_as_u64(new_count, "metadata count")?,
    )?;
    write_u64(
        &mut authored,
        layout.header,
        usize_as_u64(new_count, "metadata header count")?,
    )?;
    write_u32(&mut authored, new_row, new_perk_hash)?;

    let appended_layout = sandbox_perk_index_layout(&authored)?;
    let appended_row = appended_layout.rows + new_index * SANDBOX_PERK_INDEX_ROW_SIZE;
    if appended_layout.count != new_count
        || u32_at(&authored, appended_row)? != new_perk_hash
        || authored.get(appended_row + 4..appended_row + SANDBOX_PERK_INDEX_ROW_SIZE)
            != payload.get(donor_row + 4..donor_end)
    {
        return Err("Authored sandbox-perk metadata row did not round-trip".into());
    }
    Ok((authored, new_index))
}

/// Validates the global sandbox-perk runtime-key map.
pub fn validate_sandbox_perk_runtime_map(payload: &[u8]) -> Result<(), String> {
    let layout = runtime_map_layout(payload)?;
    if layout.secondary.count < layout.primary.count.div_ceil(32) {
        return Err(format!(
            "Runtime map has {} assignments but auxiliary storage covers only {} bits",
            layout.primary.count,
            layout.secondary.count * 32
        ));
    }
    let mut previous = None;
    for index in 0..layout.primary.count {
        let assignment = runtime_assignment_with_layout(payload, layout, index)?;
        if previous.is_some_and(|previous| previous >= assignment.runtime_key) {
            return Err(format!(
                "Sandbox-perk runtime keys are not strictly ascending at row {index}"
            ));
        }
        previous = Some(assignment.runtime_key);
    }
    Ok(())
}

/// Parses one row from the global sandbox-perk runtime-key map.
pub fn sandbox_perk_runtime_assignment_at(
    payload: &[u8],
    index: usize,
) -> Result<SandboxPerkRuntimeAssignment, String> {
    let layout = runtime_map_layout(payload)?;
    runtime_assignment_with_layout(payload, layout, index)
}

/// Returns the number of sorted runtime-key assignments.
pub fn sandbox_perk_runtime_assignment_count(payload: &[u8]) -> Result<usize, String> {
    Ok(runtime_map_layout(payload)?.primary.count)
}

/// Resolves one runtime key through the sorted runtime map.
pub fn sandbox_perk_runtime_assignment(
    payload: &[u8],
    runtime_key: u32,
) -> Result<Option<SandboxPerkRuntimeAssignment>, String> {
    let layout = runtime_map_layout(payload)?;
    let mut low = 0usize;
    let mut high = layout.primary.count;
    while low < high {
        let middle = low + (high - low) / 2;
        let assignment = runtime_assignment_with_layout(payload, layout, middle)?;
        match assignment.runtime_key.cmp(&runtime_key) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
            std::cmp::Ordering::Equal => return Ok(Some(assignment)),
        }
    }
    Ok(None)
}

/// Resolves a finished sandbox-perk row through its runtime map and direct graph references.
pub fn load_sandbox_perk_runtime_action(
    manager: &PackageManager,
    investment_globals: &[u8],
    finished_perk_index: usize,
) -> Result<SandboxPerkRuntimeAction, String> {
    let catalog_tag = TagHash(investment_globals_table_tag(
        investment_globals,
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT,
    )?);
    validate_structured_tag_entry(
        manager,
        catalog_tag,
        FINISHED_SANDBOX_PERK_CATALOG_CLASS,
        "finished sandbox-perk catalog",
    )?;
    let catalog = manager.read_tag(catalog_tag).map_err(|error| {
        format!("Could not read finished sandbox-perk catalog {catalog_tag}: {error}")
    })?;
    let finished_perk = finished_sandbox_perk_at(&catalog, finished_perk_index)?;
    let runtime_map_tag = TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG);
    validate_structured_tag_entry(
        manager,
        runtime_map_tag,
        SANDBOX_PERK_RUNTIME_MAP_CLASS,
        "sandbox-perk runtime map",
    )?;
    let runtime_map = manager.read_tag(runtime_map_tag).map_err(|error| {
        format!("Could not read sandbox-perk runtime map {runtime_map_tag}: {error}")
    })?;
    let assignment = sandbox_perk_runtime_assignment(&runtime_map, finished_perk.runtime_key)?
        .ok_or_else(|| {
            format!(
                "Finished sandbox-perk row {finished_perk_index} runtime key 0x{:08X} is not assigned",
                finished_perk.runtime_key
            )
        })?;
    let action_tag = TagHash(assignment.runtime_tag);
    let action_payload = manager.read_tag(action_tag).map_err(|error| {
        format!("Could not read sandbox-perk runtime action {action_tag}: {error}")
    })?;
    let action_entry = manager
        .get_entry(action_tag)
        .ok_or_else(|| format!("Sandbox-perk runtime action {action_tag} has no package entry"))?;
    if action_entry.file_type != 8 || !is_native_member_class(action_entry.reference) {
        return Err(format!(
            "Sandbox-perk runtime action {action_tag} is not a structured file-type 8 resource"
        ));
    }
    validate_runtime_action_payload(
        &action_payload,
        usize::try_from(action_entry.file_size).ok(),
    )?;
    let graphs = sandbox_perk_runtime_graph_sources(manager, &action_payload)?;
    Ok(SandboxPerkRuntimeAction {
        finished_perk,
        action_tag,
        action_payload,
        graphs,
    })
}

/// Finds structured weapon-entity graphs directly referenced by a runtime action payload.
pub fn sandbox_perk_runtime_graph_sources(
    manager: &PackageManager,
    action_payload: &[u8],
) -> Result<Vec<SandboxPerkRuntimeGraphSource>, String> {
    validate_runtime_action_payload(action_payload, None)?;
    let mut candidates = std::collections::BTreeMap::<u32, Vec<usize>>::new();
    for (tag, offset) in structured_action_graph_reference_candidates(action_payload)? {
        let Some(entry) = manager.get_entry(tag) else {
            continue;
        };
        if entry.file_type == 8 && entry.reference == WEAPON_ENTITY_CLASS {
            candidates.entry(tag.0).or_default().push(offset);
        }
    }

    let mut graphs = Vec::with_capacity(candidates.len());
    for (tag, action_offsets) in candidates {
        let tag = TagHash(tag);
        let payload = manager
            .read_tag(tag)
            .map_err(|error| format!("Could not read sandbox-perk runtime graph {tag}: {error}"))?;
        let entry = manager
            .get_entry(tag)
            .ok_or_else(|| format!("Sandbox-perk runtime graph {tag} has no package entry"))?;
        if usize::try_from(entry.file_size).ok() != Some(payload.len()) {
            return Err(format!(
                "Sandbox-perk runtime graph {tag} package size {} disagrees with decoded payload size {}",
                entry.file_size,
                payload.len()
            ));
        }
        if validate_weapon_entity(&payload).is_ok() {
            graphs.push(SandboxPerkRuntimeGraphSource {
                tag,
                action_offsets,
                payload,
            });
        }
    }
    Ok(graphs)
}

fn validate_structured_tag_entry(
    manager: &PackageManager,
    tag: TagHash,
    expected_class: u32,
    label: &str,
) -> Result<(), String> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("{label} {tag} has no package entry"))?;
    if entry.file_type != 8 || entry.reference != expected_class {
        return Err(format!(
            "{label} {tag} has type {}/0x{:08X}, expected 8/0x{expected_class:08X}",
            entry.file_type, entry.reference
        ));
    }
    Ok(())
}

fn validate_runtime_action_payload(
    payload: &[u8],
    package_file_size: Option<usize>,
) -> Result<(), String> {
    let declared_size = usize::try_from(u64_at(payload, 0)?)
        .map_err(|_| "Sandbox-perk runtime action size does not fit this platform")?;
    if declared_size != payload.len() {
        return Err(format!(
            "Sandbox-perk runtime action size field {declared_size} disagrees with payload size {}",
            payload.len()
        ));
    }
    if let Some(package_file_size) = package_file_size
        && package_file_size != payload.len()
    {
        return Err(format!(
            "Sandbox-perk runtime action package size {package_file_size} disagrees with decoded payload size {}",
            payload.len()
        ));
    }
    Ok(())
}

fn structured_action_graph_reference_candidates(
    action_payload: &[u8],
) -> Result<Vec<(TagHash, usize)>, String> {
    let mut candidates = Vec::new();
    for offset in (0..action_payload.len().saturating_sub(7)).step_by(4) {
        let member_class_offset =
            match offset.checked_sub(ACTION_GRAPH_REFERENCE_MEMBER_CLASS_DELTA) {
                Some(offset) => offset,
                None => continue,
            };
        let member_class = u32_at(action_payload, member_class_offset)?;
        let tag = TagHash(u32_at(action_payload, offset)?);
        let lane_high = u32_at(action_payload, offset + 4)?;
        if is_native_member_class(member_class) && lane_high == 0 {
            candidates.push((tag, offset));
        }
    }
    Ok(candidates)
}

fn is_native_member_class(value: u32) -> bool {
    // Runtime action member descriptors use the native 0x8080 namespace. It is distinct from
    // ordinary runtime package tags and rules out a tag-valued scalar being read as a class
    // handle one member later.
    value != 0 && TagHash(value).is_some() && (value & 0xFFFF_0000) == 0x8080_0000
}

/// Inserts one runtime-key assignment while preserving the map's sorted order and auxiliary data.
pub fn insert_sandbox_perk_runtime_assignment(
    payload: &[u8],
    runtime_key: u32,
    runtime_tag: u32,
) -> Result<Vec<u8>, String> {
    let layout = runtime_map_layout(payload)?;
    let mut keys = Vec::with_capacity(layout.primary.count);
    for index in 0..layout.primary.count {
        keys.push(runtime_assignment_with_layout(payload, layout, index)?.runtime_key);
    }
    if !keys.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err("Sandbox-perk runtime keys are not strictly ascending".into());
    }
    let insertion_index = match keys.binary_search(&runtime_key) {
        Ok(index) => {
            return Err(format!(
                "Sandbox-perk runtime key 0x{runtime_key:08X} already exists at row {index}"
            ));
        }
        Err(index) => index,
    };

    let new_count = layout
        .primary
        .count
        .checked_add(1)
        .ok_or("Sandbox-perk runtime assignment count overflowed")?;
    let new_len = payload
        .len()
        .checked_add(SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE)
        .ok_or("Sandbox-perk runtime map size overflowed")?;
    let insertion = layout
        .primary
        .rows
        .checked_add(
            insertion_index
                .checked_mul(SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE)
                .ok_or("Sandbox-perk runtime assignment offset overflowed")?,
        )
        .ok_or("Sandbox-perk runtime assignment offset overflowed")?;

    let mut authored = Vec::with_capacity(new_len);
    authored.extend_from_slice(&payload[..insertion]);
    authored.extend_from_slice(&runtime_key.to_le_bytes());
    authored.extend_from_slice(&runtime_tag.to_le_bytes());
    authored.extend_from_slice(&payload[insertion..]);

    write_u64(&mut authored, 0, usize_as_u64(new_len, "runtime-map size")?)?;
    write_u64(
        &mut authored,
        PRIMARY_DESCRIPTOR,
        usize_as_u64(new_count, "runtime assignment count")?,
    )?;
    write_u64(
        &mut authored,
        layout.primary.header,
        usize_as_u64(new_count, "runtime assignment header count")?,
    )?;
    let shifted_secondary_header = layout
        .secondary
        .header
        .checked_add(SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE)
        .ok_or("Sandbox-perk runtime-map auxiliary header overflowed")?;
    write_relative_pointer(
        &mut authored,
        SECONDARY_DESCRIPTOR + 8,
        shifted_secondary_header,
    )?;

    ensure_runtime_map_capacity(&mut authored)?;
    validate_sandbox_perk_runtime_map(&authored)?;
    let inserted = sandbox_perk_runtime_assignment_at(&authored, insertion_index)?;
    if inserted.runtime_key != runtime_key || inserted.runtime_tag != runtime_tag {
        return Err("Authored sandbox-perk runtime assignment did not round-trip".into());
    }
    Ok(authored)
}

/// Runtime loading marks one auxiliary bit per assignment. Inserting rows can
/// cross a word boundary even when the original auxiliary bytes are preserved.
pub(crate) fn ensure_runtime_map_capacity(payload: &mut Vec<u8>) -> Result<(), String> {
    let layout = runtime_map_layout(payload)?;
    let required = layout.primary.count.div_ceil(32);
    if required <= layout.secondary.count {
        return Ok(());
    }
    let growth = (required - layout.secondary.count)
        .checked_mul(SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_SIZE)
        .ok_or("Runtime map auxiliary size overflowed")?;
    let size = payload
        .len()
        .checked_add(growth)
        .ok_or("Runtime map size overflowed")?;
    payload.resize(size, 0);
    write_u64(
        payload,
        SECONDARY_DESCRIPTOR,
        usize_as_u64(required, "auxiliary count")?,
    )?;
    write_u64(
        payload,
        layout.secondary.header,
        usize_as_u64(required, "auxiliary count")?,
    )?;
    write_u64(payload, 0, usize_as_u64(size, "runtime-map size")?)?;
    Ok(())
}

fn finished_catalog_layout(payload: &[u8]) -> Result<TwoArrayLayout, String> {
    two_array_layout(
        payload,
        FINISHED_SANDBOX_PERK_ROW_CLASS,
        FINISHED_SANDBOX_PERK_ROW_SIZE,
        FINISHED_SANDBOX_PERK_DETAIL_ROW_CLASS,
        FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE,
        "Finished sandbox-perk catalog",
    )
}

fn sandbox_perk_index_layout(payload: &[u8]) -> Result<NativeArray, String> {
    let label = "Sandbox-perk metadata catalog";
    let declared_size = usize::try_from(u64_at(payload, 0)?)
        .map_err(|_| format!("{label} size does not fit this platform"))?;
    if declared_size != payload.len() {
        return Err(format!(
            "{label} size field {declared_size} disagrees with payload size {}",
            payload.len()
        ));
    }
    if payload.get(0x18..SINGLE_ARRAY_HEADER) != Some(NESTED_ARRAY_TRAILER.as_slice()) {
        return Err(format!("{label} has an unexpected array trailer"));
    }
    let array = native_array(payload, PRIMARY_DESCRIPTOR, label)?;
    if array.header != SINGLE_ARRAY_HEADER {
        return Err(format!(
            "{label} header is at 0x{:X}, expected 0x{SINGLE_ARRAY_HEADER:X}",
            array.header
        ));
    }
    if array.row_class != SANDBOX_PERK_INDEX_ROW_CLASS {
        return Err(format!(
            "{label} row class is 0x{:08X}, expected 0x{SANDBOX_PERK_INDEX_ROW_CLASS:08X}",
            array.row_class
        ));
    }
    let end = checked_rows_end(array, SANDBOX_PERK_INDEX_ROW_SIZE, payload.len(), label)?;
    if end != payload.len() {
        return Err(format!(
            "{label} has {} unexpected trailing bytes",
            payload.len() - end
        ));
    }
    Ok(array)
}

fn runtime_map_layout(payload: &[u8]) -> Result<TwoArrayLayout, String> {
    two_array_layout(
        payload,
        SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_CLASS,
        SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE,
        SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_CLASS,
        SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_SIZE,
        "Sandbox-perk runtime map",
    )
}

fn two_array_layout(
    payload: &[u8],
    primary_class: u32,
    primary_stride: usize,
    secondary_class: u32,
    secondary_stride: usize,
    label: &str,
) -> Result<TwoArrayLayout, String> {
    let declared_size = usize::try_from(u64_at(payload, 0)?)
        .map_err(|_| format!("{label} size does not fit this platform"))?;
    if declared_size != payload.len() {
        return Err(format!(
            "{label} size field {declared_size} disagrees with payload size {}",
            payload.len()
        ));
    }
    let initial_trailer = payload
        .get(0x28..ROOT_ARRAY_HEADER)
        .ok_or_else(|| format!("{label} header is truncated"))?;
    if initial_trailer != NESTED_ARRAY_TRAILER {
        return Err(format!("{label} has an unexpected root array trailer"));
    }

    let primary = native_array(payload, PRIMARY_DESCRIPTOR, label)?;
    let secondary = native_array(payload, SECONDARY_DESCRIPTOR, label)?;
    if primary.header != ROOT_ARRAY_HEADER {
        return Err(format!(
            "{label} primary header is at 0x{:X}, expected 0x{ROOT_ARRAY_HEADER:X}",
            primary.header
        ));
    }
    if primary.row_class != primary_class {
        return Err(format!(
            "{label} primary row class is 0x{:08X}, expected 0x{primary_class:08X}",
            primary.row_class
        ));
    }
    if secondary.row_class != secondary_class {
        return Err(format!(
            "{label} secondary row class is 0x{:08X}, expected 0x{secondary_class:08X}",
            secondary.row_class
        ));
    }
    let primary_end = checked_rows_end(primary, primary_stride, payload.len(), label)?;
    let expected_secondary_header = primary_end
        .checked_add(ARRAY_TRAILER_SIZE)
        .ok_or_else(|| format!("{label} inter-array offset overflowed"))?;
    if secondary.header != expected_secondary_header {
        return Err(format!(
            "{label} arrays are not separated by exactly one native trailer"
        ));
    }
    if payload.get(primary_end..secondary.header) != Some(NESTED_ARRAY_TRAILER.as_slice()) {
        return Err(format!("{label} has an unexpected inter-array trailer"));
    }
    let secondary_end = checked_rows_end(secondary, secondary_stride, payload.len(), label)?;
    if secondary_end != payload.len() {
        return Err(format!(
            "{label} has {} unexpected trailing bytes",
            payload.len() - secondary_end
        ));
    }
    Ok(TwoArrayLayout {
        primary,
        primary_end,
        secondary,
        secondary_end,
    })
}

fn native_array(payload: &[u8], descriptor: usize, label: &str) -> Result<NativeArray, String> {
    let count = usize::try_from(u64_at(payload, descriptor)?)
        .map_err(|_| format!("{label} array count does not fit this platform"))?;
    let pointer = descriptor
        .checked_add(8)
        .ok_or_else(|| format!("{label} array pointer overflowed"))?;
    let header = relative_offset(descriptor, 8, i64_at(payload, pointer)?)?;
    if u64_at(payload, header)?
        != u64::try_from(count).map_err(|_| format!("{label} array count does not fit u64"))?
    {
        return Err(format!(
            "{label} descriptor and header counts disagree at 0x{descriptor:X}"
        ));
    }
    let rows = header
        .checked_add(ARRAY_HEADER_SIZE)
        .ok_or_else(|| format!("{label} array rows overflowed"))?;
    Ok(NativeArray {
        count,
        header,
        rows,
        row_class: u32_at(payload, header + 8)?,
    })
}

fn checked_rows_end(
    array: NativeArray,
    stride: usize,
    payload_len: usize,
    label: &str,
) -> Result<usize, String> {
    let end = array
        .count
        .checked_mul(stride)
        .and_then(|size| array.rows.checked_add(size))
        .ok_or_else(|| format!("{label} row extent overflowed"))?;
    if end > payload_len {
        return Err(format!("{label} rows extend beyond the payload"));
    }
    Ok(end)
}

fn primary_row(layout: TwoArrayLayout, index: usize, stride: usize) -> Result<usize, String> {
    if index >= layout.primary.count {
        return Err(format!(
            "Sandbox-perk row index {index} is outside the {}-row table",
            layout.primary.count
        ));
    }
    layout
        .primary
        .rows
        .checked_add(
            index
                .checked_mul(stride)
                .ok_or("Sandbox-perk row offset overflowed")?,
        )
        .ok_or_else(|| "Sandbox-perk row offset overflowed".into())
}

fn finished_sandbox_perk_with_layout(
    payload: &[u8],
    layout: TwoArrayLayout,
    index: usize,
) -> Result<FinishedSandboxPerk, String> {
    let row = primary_row(layout, index, FINISHED_SANDBOX_PERK_ROW_SIZE)?;
    let pointer = i64_at(payload, row + 8)?;
    let detail = detail_target(layout, row, pointer)?
        .map(|target| read_array(payload, target))
        .transpose()?;
    Ok(FinishedSandboxPerk {
        index,
        perk_hash: u32_at(payload, row)?,
        runtime_key: u32_at(payload, row + 4)?,
        trailing: u64_at(payload, row + 16)?,
        detail,
    })
}

fn detail_target(
    layout: TwoArrayLayout,
    primary_row: usize,
    pointer: i64,
) -> Result<Option<usize>, String> {
    if pointer == 0 {
        return Ok(None);
    }
    let target = relative_offset(primary_row, 8, pointer)?;
    if target < layout.secondary.rows || target >= layout.secondary_end {
        return Err(format!(
            "Finished sandbox-perk detail pointer at 0x{:X} points outside the detail array",
            primary_row + 8
        ));
    }
    let displacement = target - layout.secondary.rows;
    if displacement % FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE != 0
        || target
            .checked_add(FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE)
            .is_none_or(|end| end > layout.secondary_end)
    {
        return Err(format!(
            "Finished sandbox-perk detail pointer at 0x{:X} is not aligned to a complete detail row",
            primary_row + 8
        ));
    }
    Ok(Some(target))
}

fn runtime_assignment_with_layout(
    payload: &[u8],
    layout: TwoArrayLayout,
    index: usize,
) -> Result<SandboxPerkRuntimeAssignment, String> {
    let row = primary_row(layout, index, SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE)?;
    Ok(SandboxPerkRuntimeAssignment {
        index,
        runtime_key: u32_at(payload, row)?,
        runtime_tag: u32_at(payload, row + 4)?,
    })
}

fn usize_as_u64(value: usize, label: &str) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("Sandbox-perk {label} does not fit u64"))
}

fn write_relative_pointer(payload: &mut [u8], pointer: usize, target: usize) -> Result<(), String> {
    let pointer = i64::try_from(pointer).map_err(|_| "Relative pointer does not fit i64")?;
    let target = i64::try_from(target).map_err(|_| "Relative target does not fit i64")?;
    write_i64(
        payload,
        usize::try_from(pointer).map_err(|_| "Relative pointer is negative")?,
        target
            .checked_sub(pointer)
            .ok_or("Relative pointer difference overflowed")?,
    )
}

fn write_u32(payload: &mut [u8], offset: usize, value: u32) -> Result<(), String> {
    write_bytes(payload, offset, &value.to_le_bytes())
}

fn write_u16(payload: &mut [u8], offset: usize, value: u16) -> Result<(), String> {
    write_bytes(payload, offset, &value.to_le_bytes())
}

fn write_u64(payload: &mut [u8], offset: usize, value: u64) -> Result<(), String> {
    write_bytes(payload, offset, &value.to_le_bytes())
}

fn write_i64(payload: &mut [u8], offset: usize, value: i64) -> Result<(), String> {
    write_bytes(payload, offset, &value.to_le_bytes())
}

fn write_bytes(payload: &mut [u8], offset: usize, bytes: &[u8]) -> Result<(), String> {
    crate::package_payload::write_bytes(payload, offset, bytes)
        .map_err(|error| format!("Sandbox-perk write: {error}"))
}

fn read_array<const N: usize>(payload: &[u8], offset: usize) -> Result<[u8; N], String> {
    crate::package_payload::bytes_at(payload, offset)
        .map_err(|error| format!("Sandbox-perk read: {error}"))
}

#[cfg(test)]
mod tests;
