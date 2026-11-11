//! Leave carrier response groups out of a private movement copy and its allocation, in place.
//!
//! A group definition (`808037CD`, `58`) and its instance twin (`808037CC`, `30`) name each other
//! by absolute offset at `+08`. Each group keeps relative descriptors for its materials (`+20`)
//! and nodes (`+50`), and each twin for the instance root (`+10`) and its node twins (`+28`). Node
//! twins (`808037CF`, `70`) refer back to their group twin relatively at `+10`. Later rows move up
//! over the left-out ones, so nothing is appended and the component needs no new seal. The
//! left-out groups' nodes and node twins stay in the payload, unreferenced.
use super::*;

fn shift(p: &mut Payload, at: usize, by: i64) -> Result<()> {
    let value = p.u64(at)? as i64;
    put(&mut p.0, at, &(value + by).to_le_bytes())
}

/// Move the rows of the array at `descriptor` over the `dropped` rows. `relative` names each
/// row's self-relative pointers, which keep their targets. Returns old and new row offsets.
fn compact(
    p: &mut Payload,
    descriptor: usize,
    stride: usize,
    class: u32,
    dropped: &[usize],
    relative: &[usize],
) -> Result<Vec<(usize, usize)>> {
    let rows = p.array(descriptor, stride, Some(class))?;
    ensure!(
        dropped.iter().all(|&index| index < rows.len()),
        "Left-out response row is outside its array"
    );
    let kept = (0..rows.len())
        .filter(|index| !dropped.contains(index))
        .collect::<Vec<_>>();
    let mut moved = Vec::new();
    // Kept rows only move to lower slots, and every kept row below a slot has already moved.
    for (slot, &index) in kept.iter().enumerate() {
        let (from, to) = (rows[index], rows[slot]);
        if from != to {
            let row = p.0[from..from + stride].to_vec();
            p.0[to..to + stride].copy_from_slice(&row);
            // An empty descriptor holds no pointer to keep.
            for &field in relative {
                if p.u64(to + field)? != 0 {
                    shift(p, to + field, (from - to) as i64)?;
                }
            }
        }
        moved.push((from, to));
    }
    // The native loader walks physical records past the shortened array. Its abandoned
    // rows must be padding, or their owner references are interpreted as class markers.
    for &at in &rows[kept.len()..] {
        p.0[at..at + stride].fill(0);
    }
    let count = kept.len() as u64;
    let header = p.pointer(descriptor + 8)?;
    put(&mut p.0, descriptor, &count.to_le_bytes())?;
    put(&mut p.0, header, &count.to_le_bytes())?;
    Ok(moved)
}

pub(super) fn drop_groups(
    p: &mut Payload,
    owner: u32,
    definition: usize,
    instance: usize,
    dropped: &[usize],
) -> Result<()> {
    let groups = p.array(definition + 0x218, 0x58, Some(0x808037CD))?;
    let twins = p.array(instance + 0x30, 0x30, Some(0x808037CC))?;
    ensure!(
        groups.len() == twins.len(),
        "Projectile group twins differ from their groups"
    );
    for (&group, &twin) in groups.iter().zip(&twins) {
        ensure!(
            p.u32(group)? == owner
                && p.u32(twin)? == owner
                && p.u64(group + 8)? == twin as u64
                && p.u64(twin + 8)? == group as u64
                && p.pointer(twin + 0x10)? == instance,
            "Projectile group and twin are not reciprocal"
        );
        for node in p.array(twin + 0x20, 0x70, Some(0x808037CF))? {
            ensure!(
                p.pointer(node + 0x10)? == twin,
                "Projectile node twin does not name its group twin"
            );
        }
    }
    // Only the reciprocal fields address these rows by offset.
    for at in (0..p.0.len().saturating_sub(15)).step_by(4) {
        if p.u32(at)? == owner && matches!(p.u32(at + 4)?, 0x808037CC | 0x808037CD) {
            ensure!(
                groups.contains(&at) || twins.contains(&at),
                "Another record addresses a projectile response group"
            );
        }
    }
    let groups = compact(
        p,
        definition + 0x218,
        0x58,
        0x808037CD,
        dropped,
        &[0x20, 0x50],
    )?;
    let twins = compact(p, instance + 0x30, 0x30, 0x808037CC, dropped, &[0x10, 0x28])?;
    for (&(_, group), &(_, twin)) in groups.iter().zip(&twins) {
        put(&mut p.0, group + 8, &(twin as u64).to_le_bytes())?;
        put(&mut p.0, twin + 8, &(group as u64).to_le_bytes())?;
        for node in p.array(twin + 0x20, 0x70, Some(0x808037CF))? {
            put(
                &mut p.0,
                node + 0x10,
                &(twin as i64 - (node + 0x10) as i64).to_le_bytes(),
            )?;
        }
    }
    Ok(())
}

/// The movement allocation keeps one child list per group twin under member `5680393D`.
pub(super) fn drop_allocation(
    allocation: &mut Payload,
    groups: usize,
    dropped: &[usize],
) -> Result<()> {
    let rows = allocation
        .array(0x20, 40, Some(0x80808852))?
        .into_iter()
        .filter(|&row| allocation.u32(row).ok() == Some(0x5680393D))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        anyhow::bail!("Projectile movement allocation has no single group member");
    };
    ensure!(
        allocation.u32(row + 16)? == 0x808037CC
            && allocation.u32(row + 20)? as usize == groups
            && allocation.array(row + 24, 40, None)?.len() == groups,
        "Projectile movement allocation differs from its groups"
    );
    let class = {
        let header = allocation.pointer(row + 32)?;
        allocation.u32(header + 8)?
    };
    compact(allocation, row + 24, 40, class, dropped, &[0x20])?;
    put(
        &mut allocation.0,
        row + 20,
        &u32::try_from(groups - dropped.len())?.to_le_bytes(),
    )
}
