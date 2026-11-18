//! Move a sealed health owner's effect pool onto its class's alignment.
use super::{Payload, put};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(super) const INSTANCE: u32 = 0x80804BEE;
const POOL: u32 = 0x80804C42;
const POOL_DEFINITION: u32 = 0x80804C43;
const POOL_SIZE: usize = 64;
/// The instance's relative pointer to its effect pool.
const INSTANCE_POOL: usize = 0x6E0;

/// The client's stream walker steps from a class word to the next boundary of that class's
/// alignment, 16 bytes for the effect pool, and walks every record between the instance and the
/// schema view. Sealing puts the pool inside that span. A pool at an 8-byte boundary is read 8
/// bytes late, the walk leaves the record grid and the client faults on the first word that is
/// no class, while it prepares for orbit. Imports made while pools were written on 8-byte
/// boundaries carry such a pool.
///
/// This moves it into the zero gap before the sealed copy, keeps what its two relative pointers
/// name, and repoints the instance, the pool definition and the copy of that definition, whose
/// typed references name offsets of the first stream. Returns whether a pool moved.
pub(super) fn realign_pool(p: &mut Payload, patches: &mut [Value]) -> Result<bool> {
    let instance = p.pointer(16)?;
    let schema = p.pointer(24)?;
    ensure!(
        instance >= 4 && instance < schema && p.u32(instance - 4)? == INSTANCE,
        "Health component layout differs"
    );
    if p.u64(instance + INSTANCE_POOL)? == 0 {
        return Ok(false);
    }
    let old = p.pointer(instance + INSTANCE_POOL)?;
    // A pool behind the schema view is not walked, and one on its boundary is read where it is.
    if old.is_multiple_of(16) || old >= schema {
        return Ok(false);
    }
    let owner = p.u32(instance)?;
    let definition = usize::try_from(p.u64(old + 8)?)?;
    ensure!(
        p.u32(old - 4)? == POOL
            && p.u32(old)? == owner
            && p.u32(old + 4)? == POOL_DEFINITION
            && p.u32(definition - 4)? == POOL_DEFINITION
            && p.u32(definition)? == owner
            && p.u32(definition + 4)? == POOL
            && p.u64(definition + 8)? == old as u64
            && p.pointer(old + 16)? == instance,
        "Health effect pool pair differs"
    );
    // A sealed owner is its stream before sealing, zeros up to the next 16-byte boundary and the
    // first record, then a copy of that stream from its first record on.
    let length = p.0.len();
    let delta = (length / 2).next_multiple_of(16);
    let first = p
        .pointer(8)?
        .checked_sub(4)
        .context("component record start")?;
    let original = length.checked_sub(delta).context("sealed owner extent")?;
    ensure!(
        first >= 0x60
            && original.next_multiple_of(16) == delta
            && schema >= delta + first
            && p.u32(definition + delta + 4)? == POOL
            && p.u64(definition + delta + 8)? == old as u64,
        "Health owner holds its effect pool in an instance span that is not sealed"
    );
    let new = (original + 4).next_multiple_of(16);
    let room =
        p.0.get(original..delta + first)
            .context("sealed owner gap")?;
    ensure!(
        new + POOL_SIZE <= delta + first && room.iter().all(|byte| *byte == 0),
        "Sealed health owner has no room for its effect pool"
    );
    let rows = if p.u64(old + 32)? == 0 {
        None
    } else {
        Some(p.pointer(old + 40)?)
    };
    let record = p.0[old..old + POOL_SIZE].to_vec();
    p.0[old - 4..old + POOL_SIZE].fill(0);
    put(&mut p.0, new - 4, &POOL.to_le_bytes())?;
    put(&mut p.0, new, &record)?;
    let relative = |p: &mut Payload, at: usize, target: usize| {
        put(&mut p.0, at, &(target as i64 - at as i64).to_le_bytes())
    };
    relative(p, new + 16, instance)?;
    if let Some(rows) = rows {
        relative(p, new + 40, rows)?;
    }
    relative(p, instance + INSTANCE_POOL, new)?;
    for at in [definition, definition + delta] {
        put(&mut p.0, at + 8, &(new as u64).to_le_bytes())?;
    }
    for patch in patches {
        let offset = usize::try_from(patch["offset"].as_u64().context("component patch offset")?)?;
        if (old..old + POOL_SIZE).contains(&offset) {
            patch["offset"] = json!(offset - old + new);
        }
    }
    Ok(true)
}
