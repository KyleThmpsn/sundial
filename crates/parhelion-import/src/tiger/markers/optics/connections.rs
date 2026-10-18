//! The default aim pose is local to its connected movement provider.
use super::*;

pub(super) fn source(entity: &Payload, tag: u32, owner: &Payload) -> Result<[u32; 2]> {
    let data = owner.pointer(24)?;
    let components = entity.array(8, 12, Some(0x80809ACD))?;
    let indices = components
        .iter()
        .enumerate()
        .filter_map(|(i, &row)| (entity.u32(row).ok() == Some(tag)).then_some(i))
        .collect::<Vec<_>>();
    ensure!(indices.len() == 1, "source optic component is ambiguous");
    let events = entity.array(24, 56, Some(0x80809A8F))?;
    let mut parents = [0; 2];
    for (index, offset) in [0x170, 0x210].into_iter().enumerate() {
        let matches = events
            .iter()
            .copied()
            .filter(|&row| {
                entity.u32(row).ok() == Some(tag)
                    && entity.u32(row + 4).ok() == Some(0x80809A9D)
                    && entity.u64(row + 8).ok() == Some((data + offset) as u64)
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "source aim input has no unique movement connection"
        );
        let row = matches[0];
        ensure!(
            entity.u32(row + 16)? == 0x811C9DC5
                && entity.u32(row + 20)? as usize == indices[0]
                && entity.u64(row + 24)? == u64::MAX
                && entity.u64(row + 32)? == 0
                && entity.u32(row + 40)? == 0xAE7D0AED
                && entity.u32(row + 44)? == 0x0003FFFF
                && entity.u64(row + 48)? == 0,
            "source aim input needs an unsupported movement provider"
        );
        parents[index] = entity.u32(row + 40)?;
    }
    Ok(parents)
}

pub(super) fn apply(
    entity: &mut Payload,
    patches: &mut Vec<Value>,
    tag: u32,
    symbol: &str,
    owner: &Payload,
    parents: [u32; 2],
) -> Result<()> {
    let data = owner.pointer(24)?;
    let is_owner = |at| {
        entity.u32(at).ok() == Some(tag)
            || patches
                .iter()
                .any(|p| p["offset"].as_u64() == Some(at as u64) && p["symbol"] == symbol)
    };
    let components = entity.array(16, 12, Some(0x80809C04))?;
    let indices = components
        .iter()
        .enumerate()
        .filter_map(|(i, &row)| is_owner(row).then_some(i))
        .collect::<Vec<_>>();
    ensure!(indices.len() == 1, "native optic component is ambiguous");
    let events = entity.array(32, 72, Some(0x80809BC9))?;
    let mut changes = Vec::new();
    for (index, offset) in [0x158, 0x1E8].into_iter().enumerate() {
        let matches = events
            .iter()
            .copied()
            .filter(|&row| {
                is_owner(row + 8)
                    && entity.u32(row + 12).ok() == Some(0x80809BD7)
                    && entity.u64(row + 16).ok() == Some((data + offset) as u64)
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "native aim input has no unique movement connection"
        );
        let row = matches[0];
        ensure!(
            entity.u64(row)? == 0x811C9DC5
                && entity.u64(row + 24)? == indices[0] as u64
                && entity.u64(row + 64)? == 0,
            "native aim input connection layout differs"
        );
        let parent = entity.u64(row + 40)? == u64::MAX
            && entity.u64(row + 48)? == 0
            && entity.u64(row + 56)? == 0x0003FFFF;
        // Mode two resolves the adapter against the supplied instance. Its
        // low component-index word is not read by A1DDF0 on this branch.
        let adapter =
            entity.u32(row + 44)? == 0x80809BE3 && entity.u64(row + 56)? & !0xFFFF == 0x00020000;
        ensure!(
            parent || adapter,
            "native aim input movement connection is unsupported"
        );
        changes.push((row, parents[index]));
    }
    // Resolve and validate both inputs before replacing either connection. Keep
    // the provider's own component index and every unrelated controller event.
    for (row, parent) in changes {
        patches.retain(|patch| {
            !patch["offset"]
                .as_u64()
                .is_some_and(|at| (row as u64 + 32..row as u64 + 72).contains(&at))
        });
        entity.0[row + 32..row + 72].fill(0);
        entity.0[row + 32..row + 36].copy_from_slice(&parent.to_le_bytes());
        entity.0[row + 40..row + 48].copy_from_slice(&u64::MAX.to_le_bytes());
        entity.0[row + 56..row + 60].copy_from_slice(&0x0003FFFFu32.to_le_bytes());
    }
    Ok(())
}
