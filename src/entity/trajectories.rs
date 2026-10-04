//! A projectile component's trajectory pool: one row for each trajectory a shot can have in flight
//! at once. A weapon fires one trajectory per pellet, and the client's row lookup (exe+CED7A0)
//! returns no row for an index past the pool, which it then reads through. A graph whose pool is
//! smaller than its host's pellets per shot therefore faults on the second pellet. Stock graphs
//! that fire pellets carry 15 rows (Legend of Acrius, Lord of Wolves), and every graph that fires
//! one round carries 1.
//!
//! Read from stock owners: the projectile instance holds a count at +0x1A8 and a relative pointer
//! to a native array of 0x210-byte rows (class 808037BA). Row +0x00 is a typed reference to its
//! handle and row +0x10 a relative pointer back to the instance. The handles (class 808037BB, 0x18
//! bytes each) sit in an array in the definition, each a typed reference to its row. The instance
//! keeps a free-row count at +0x1C0 and the definition a capacity at +0x80, both the pool's size in
//! every stock owner. The rows end
//! the instance region, which the owner header sizes at +0x48 and which the client copies for
//! each component instance, so a larger pool is inserted there and the definition moves along.

use super::*;

const POOL: usize = 0x1A8;
/// The instance's count of free rows, which starts at the pool's size.
const FREE_ROWS: usize = 0x1C0;
/// The definition's own record of the pool's size.
const DEFINITION_CAPACITY: usize = 0x80;
const ROW_CLASS: u32 = 0x8080_37BA;
const HANDLE_CLASS: u32 = 0x8080_37BB;
const ROW_SIZE: usize = 0x210;
const HANDLE_SIZE: usize = 0x18;
const ARRAY_MARKER: u32 = 0x8080_9FBD;
/// Owner header words: where the instance and definition regions start, the tail array, and the
/// instance region's size.
const INSTANCE_POINTER: usize = 0x10;
const DEFINITION_POINTER: usize = 0x18;
const TAIL_POINTER: usize = 0x38;
const INSTANCE_SIZE: usize = 0x48;
/// Within a row: its typed reference to its handle, and its pointer back to the instance.
const ROW_HANDLE: usize = 0x00;
const ROW_INSTANCE: usize = 0x10;

/// Where an owner keeps one instance's pool.
struct Pool {
    count: usize,
    header: usize,
    rows: usize,
    handles: usize,
    /// The definition's count and pointer naming the handle array.
    handle_descriptor: usize,
}

/// The number of trajectories the projectile instance at `instance` has room for.
pub fn projectile_trajectory_capacity(
    owner: &[u8],
    owner_tag: u32,
    instance: usize,
) -> Result<usize, String> {
    pool(owner, owner_tag, instance).map(|pool| pool.count)
}

/// Gives the projectile instance at `instance` room for `capacity` trajectories, in a private copy
/// of its owner whose `graph` entity names it as `owner_tag`. New rows copy the last one, each
/// with a handle of its own in a new handle array. Every typed reference past the inserted rows,
/// in the owner and in the graph, follows the bytes it names. A pool that already has room is
/// left alone, and an owner holding any other reference across the instance region is refused.
pub fn grow_projectile_trajectories(
    graph: &mut [u8],
    owner_tag: u32,
    owner: &mut Vec<u8>,
    instance: usize,
    capacity: usize,
) -> Result<(), String> {
    let pool = pool(owner, owner_tag, instance)?;
    if capacity <= pool.count {
        return Ok(());
    }
    if capacity > 0x7FFF {
        return Err(format!("A trajectory pool of {capacity} is too large"));
    }
    let instance_start = target(owner, INSTANCE_POINTER)?;
    let definition = target(owner, DEFINITION_POINTER)?;
    let cut = pool.rows + pool.count * ROW_SIZE;
    if instance_start > instance
        || cut > definition
        || usize::try_from(read_u64(owner, INSTANCE_SIZE)?).ok()
            != Some(definition - instance_start)
        || read_u32(owner, instance + FREE_ROWS)? as usize != pool.count
        || read_u64(owner, definition + DEFINITION_CAPACITY)? != pool.count as u64
    {
        return Err("The projectile owner's instance region is not laid out as expected".into());
    }
    let delta = (capacity - pool.count) * ROW_SIZE;
    let moved = |offset: usize| {
        if offset >= cut {
            offset + delta
        } else {
            offset
        }
    };

    let typed = typed_references(owner, owner_tag);
    let tail = target(owner, TAIL_POINTER)?;
    let crossing = crossing_pointers(owner, &typed, cut)?;
    if let Some((at, _)) = crossing
        .iter()
        .find(|(at, _)| ![DEFINITION_POINTER, TAIL_POINTER].contains(at) && *at < tail)
    {
        return Err(format!(
            "The projectile owner has another reference across its instance region at 0x{at:X}"
        ));
    }

    let template = owner[cut - ROW_SIZE..cut].to_vec();
    let mut grown = Vec::with_capacity(owner.len() + delta + (capacity + 2) * HANDLE_SIZE);
    grown.extend_from_slice(&owner[..cut]);
    for _ in pool.count..capacity {
        grown.extend_from_slice(&template);
    }
    grown.extend_from_slice(&owner[cut..]);
    for &at in &typed {
        let named = to_usize(read_u64(owner, at + 8)?)?;
        if named >= cut {
            write_u64(&mut grown, moved(at) + 8, to_u64(named + delta)?)?;
        }
    }
    for (at, named) in crossing {
        write_relative_pointer(&mut grown, moved(at), moved(named))?;
    }
    write_u64(
        &mut grown,
        INSTANCE_SIZE,
        to_u64(definition - instance_start + delta)?,
    )?;
    write_u64(&mut grown, instance + POOL, to_u64(capacity)?)?;
    write_u64(&mut grown, pool.header, to_u64(capacity)?)?;
    write_u32(
        &mut grown,
        instance + FREE_ROWS,
        u32::try_from(capacity).map_err(|_| "Pool size overflow")?,
    )?;
    write_u64(
        &mut grown,
        moved(definition) + DEFINITION_CAPACITY,
        to_u64(capacity)?,
    )?;

    // The handles move to an array of their own. The old one is cleared, so nothing names a row
    // through it any more.
    let old_handles = moved(pool.handles) + 16;
    let handle_template = grown[old_handles..old_handles + HANDLE_SIZE].to_vec();
    grown[old_handles..old_handles + pool.count * HANDLE_SIZE].fill(0);
    grown.resize((grown.len() + 8).next_multiple_of(16), 0);
    let handles = grown.len();
    write_u32(&mut grown, handles - 4, ARRAY_MARKER)?;
    grown.extend_from_slice(&to_u64(capacity)?.to_le_bytes());
    grown.extend_from_slice(&HANDLE_CLASS.to_le_bytes());
    grown.extend_from_slice(&0_u32.to_le_bytes());
    for index in 0..capacity {
        let handle = grown.len();
        let row = pool.rows + index * ROW_SIZE;
        grown.extend_from_slice(&handle_template);
        write_u64(&mut grown, handle + 8, to_u64(row)?)?;
        write_u64(&mut grown, row + ROW_HANDLE + 8, to_u64(handle)?)?;
        write_relative_pointer(&mut grown, row + ROW_INSTANCE, instance)?;
    }
    grown.resize(grown.len().next_multiple_of(16), 0);
    let descriptor = moved(pool.handle_descriptor);
    write_u64(&mut grown, descriptor, to_u64(capacity)?)?;
    write_relative_pointer(&mut grown, descriptor + 8, handles)?;
    let length = to_u64(grown.len())?;
    write_u64(&mut grown, 0, length)?;

    // The graph names objects in this owner by absolute offset too: its resource descriptors,
    // resource map rows and event endpoints.
    for at in typed_references(graph, owner_tag) {
        let named = to_usize(read_u64(graph, at + 8)?)?;
        if named >= cut && named < owner.len() {
            write_u64(graph, at + 8, to_u64(named + delta)?)?;
        }
    }
    if pool_count(&grown, owner_tag, instance)? != capacity {
        return Err("The grown trajectory pool does not read back".into());
    }
    *owner = grown;
    Ok(())
}

fn pool(owner: &[u8], owner_tag: u32, instance: usize) -> Result<Pool, String> {
    let count = to_usize(read_u64(owner, instance + POOL)?)?;
    let header = target(owner, instance + POOL + 8)?;
    if count == 0 || count > 0x7FFF || !is_array(owner, header, count, ROW_CLASS)? {
        return Err("The projectile has no trajectory pool where one is expected".into());
    }
    let rows = header + 16;
    let handle = typed_target(owner, rows + ROW_HANDLE, owner_tag, HANDLE_CLASS)?;
    let handles = handle
        .checked_sub(16)
        .filter(|handles| is_array(owner, *handles, count, HANDLE_CLASS).unwrap_or(false))
        .ok_or("The projectile's trajectory handles are not an array")?;
    for index in 0..count {
        let row = rows + index * ROW_SIZE;
        let handle = handles + 16 + index * HANDLE_SIZE;
        if typed_target(owner, row + ROW_HANDLE, owner_tag, HANDLE_CLASS)? != handle
            || typed_target(owner, handle, owner_tag, ROW_CLASS)? != row
            || target(owner, row + ROW_INSTANCE)? != instance
        {
            return Err(format!(
                "Trajectory row {index} is not paired with its handle"
            ));
        }
    }
    let descriptors = (8..owner.len().saturating_sub(7))
        .step_by(8)
        .filter(|&at| {
            target(owner, at).is_ok_and(|named| named == handles)
                && read_u64(owner, at - 8).is_ok_and(|value| value == count as u64)
        })
        .collect::<Vec<_>>();
    let [handle_pointer] = descriptors.as_slice() else {
        return Err("The trajectory handles are not named exactly once".into());
    };
    Ok(Pool {
        count,
        header,
        rows,
        handles,
        handle_descriptor: handle_pointer - 8,
    })
}

fn pool_count(owner: &[u8], owner_tag: u32, instance: usize) -> Result<usize, String> {
    pool(owner, owner_tag, instance).map(|pool| pool.count)
}

/// Whether a native array of `count` rows of `class` has its header at `header`.
fn is_array(data: &[u8], header: usize, count: usize, class: u32) -> Result<bool, String> {
    Ok(header >= 4
        && read_u32(data, header - 4)? == ARRAY_MARKER
        && read_u64(data, header)? == count as u64
        && read_u32(data, header + 8)? == class)
}

/// The aligned typed references to `owner_tag`: the tag, a native class, then an offset.
fn typed_references(data: &[u8], owner_tag: u32) -> Vec<usize> {
    (0..data.len().saturating_sub(15))
        .step_by(8)
        .filter(|&at| {
            read_u32(data, at) == Ok(owner_tag)
                && read_u32(data, at + 4).is_ok_and(|class| class & 0xFFFF_0000 == 0x8080_0000)
        })
        .collect()
}

/// The offset a typed reference of `class` names.
fn typed_target(data: &[u8], at: usize, owner_tag: u32, class: u32) -> Result<usize, String> {
    if read_u32(data, at)? != owner_tag || read_u32(data, at + 4)? != class {
        return Err(format!(
            "Expected a reference to class 0x{class:08X} at 0x{at:X}"
        ));
    }
    to_usize(read_u64(data, at + 8)?)
}

/// Every relative pointer whose slot and target lie on opposite sides of `cut`, as (slot, target).
/// A word is read as a pointer when it names an aligned offset inside the payload; the words of
/// typed references are skipped, since their offsets are absolute.
fn crossing_pointers(
    data: &[u8],
    typed: &[usize],
    cut: usize,
) -> Result<Vec<(usize, usize)>, String> {
    let skipped = typed
        .iter()
        .flat_map(|&at| [at, at + 8])
        .collect::<BTreeSet<_>>();
    let mut crossing = Vec::new();
    for at in (0..data.len().saturating_sub(7)).step_by(8) {
        if skipped.contains(&at) {
            continue;
        }
        let Ok(named) = target(data, at) else {
            continue;
        };
        if named % 8 == 0 && (at < cut) != (named < cut) {
            crossing.push((at, named));
        }
    }
    Ok(crossing)
}

/// The offset a self-relative pointer at `at` names, when it names one inside the payload.
fn target(data: &[u8], at: usize) -> Result<usize, String> {
    let value = read_u64(data, at)? as i64;
    if value == 0 {
        return Err(format!("No pointer at 0x{at:X}"));
    }
    at.checked_add_signed(isize::try_from(value).map_err(|_| "Pointer overflow")?)
        .filter(|named| *named < data.len())
        .ok_or_else(|| format!("The pointer at 0x{at:X} leaves the payload"))
}

fn to_usize(value: u64) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| "Offset does not fit".to_owned())
}

fn to_u64(value: usize) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| "Offset does not fit".to_owned())
}
