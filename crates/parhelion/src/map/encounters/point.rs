//! Inline point components are constructed even when the spawner also names an external rule.
//! Their placement and typed volume dependencies must belong to the authored encounter too.
use parhelion_import::halo_reach::level::spawn::Spawn;
use sundial::package_authoring::{PackageManager, sandbox_perk::entity::residency};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{array_at, read_i64, read_u32, read_u64, write_u64},
};

use super::{Placed, chain, class, keyed, target};

const POINT_SET: u32 = 0x8080_7EBE;
const PLACEMENT: u32 = 0x8080_7EBF;
const VOLUME: u32 = 0x8080_6725;

struct Inline {
    row: usize,
    points: std::ops::Range<usize>,
    volume: usize,
}

fn read(record: &[u8]) -> AuthoringResult<Option<Inline>> {
    let primary = target(record, 0x10)?;
    let secondary = keyed(record)?;
    let set_field = primary + 0x5D0;
    let placement_field = secondary + 0xB8;
    if read_i64(record, set_field)? == 0 {
        if read_i64(record, placement_field)? != 0 {
            return Err(invalid("Inline spawn placement has no point set"));
        }
        return Ok(None);
    }
    let set = target(record, set_field)?;
    let placement = target(record, placement_field)?;
    let owner = read_u32(record, primary)?;
    if set < 4
        || placement < 4
        || read_u32(record, set - 4)? != POINT_SET
        || read_u32(record, placement - 4)? != PLACEMENT
        || read_u32(record, set)? != owner
        || read_u32(record, placement)? != owner
        || read_u32(record, set + 4)? != PLACEMENT
        || read_u32(record, placement + 4)? != POINT_SET
        || read_u64(record, set + 8)? != placement as u64
        || read_u64(record, placement + 8)? != set as u64
    {
        return Err(invalid(
            "Inline spawn point has inconsistent component endpoints",
        ));
    }
    let row = placement + 0x10;
    let identity = chain::row_identity(record, row)?;
    let (count, _, points, element) = array_at(record, set + 0x40)?;
    let end = points
        .checked_add(count.saturating_mul(0x48))
        .filter(|&end| end <= record.len())
        .ok_or_else(|| invalid("Inline spawn point array exceeds its record"))?;
    if count == 0 || element != 0x8080_9840 {
        return Err(invalid("Inline spawn point array has an unsupported shape"));
    }
    for at in (points..end).step_by(0x48) {
        if read_u64(record, at)? != identity {
            return Err(invalid("Inline spawn point array names another placement"));
        }
    }
    let volume = target(record, row + 0x78)?;
    if volume < 4 || read_u32(record, volume - 4)? != VOLUME {
        return Err(invalid(
            "Inline spawn point has an unsupported volume resource",
        ));
    }
    // Both tag lanes must be readable before mutation. Native construction consumes +0x28
    // directly, while the resource declaration also requires +0x20 to be resident.
    read_u64(record, volume + 0x28)?;
    Ok(Some(Inline {
        row,
        points: points..end,
        volume,
    }))
}

pub(super) fn dependencies(
    manager: &PackageManager,
    placed: &Placed,
) -> AuthoringResult<Vec<TagHash>> {
    let record = &placed.payloads[2];
    let Some(inline) = read(record)? else {
        return Ok(Vec::new());
    };
    let entity = read_u32(record, inline.row)?;
    let mut required = residency::additions(manager, [entity])
        .map_err(|error| invalid(format!("Inline spawn point entity: {error}")))?;
    let mut resources = Vec::new();
    for (offset, expected) in [(0x20, 0x8080_7F52), (0x28, 0x8080_6745)] {
        let tag = read_u32(record, inline.volume + offset)?;
        if tag == u32::MAX {
            continue;
        }
        if class(manager, tag) != Some(expected) {
            return Err(invalid(format!(
                "Inline spawn point resource at +0x{offset:X} has an unsupported class"
            )));
        }
        resources.push(tag);
    }
    required.extend(
        residency::resource_additions(manager, resources)
            .map_err(|error| invalid(format!("Inline spawn point resources: {error}")))?,
    );
    Ok(required.into_iter().map(TagHash).collect())
}

pub(super) fn place(record: &mut [u8], anchor: Spawn, identity: u64) -> AuthoringResult<()> {
    let Some(inline) = read(record)? else {
        return Ok(());
    };
    chain::place_row(record, inline.row, anchor.yaw, anchor.position)?;
    chain::set_row_identity(record, inline.row, identity)?;
    for at in inline.points.step_by(0x48) {
        write_u64(record, at, identity)?;
    }
    Ok(())
}
