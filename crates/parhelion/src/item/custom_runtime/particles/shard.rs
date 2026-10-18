//! Namespace target-owned collectors after the authored item receives its identity.
use super::*;
use parhelion_import::d2_mot::payload::Payload;

pub(super) fn bind(
    manager: &PackageManager,
    nodes: &[Node],
    item: u32,
    edited: &mut BTreeMap<String, Vec<u8>>,
) -> AuthoringResult<()> {
    let components = nodes
        .iter()
        .filter(|node| {
            manager
                .get_entry(TagHash(node.template))
                .is_some_and(|entry| entry.reference == 0x80809c36)
        })
        .collect::<Vec<_>>();
    let checked = |node: &Node| -> AuthoringResult<(Payload, usize)> {
        let p = Payload(node.payload.clone());
        let d = p.pointer(24).map_err(|e| invalid(e.to_string()))?;
        Ok((p, d))
    };
    let mut keys = BTreeMap::new();
    let mut writes = Vec::new();
    for node in &components {
        let (p, d) = checked(node)?;
        if p.u32(
            d.checked_sub(4)
                .ok_or_else(|| invalid("Shard definition offset"))?,
        )
        .map_err(|e| invalid(e.to_string()))?
            != 0x80802cda
        {
            continue;
        }
        let old = read_u32(&node.payload, d + 0x60)?;
        if matches!(old, 0 | u32::MAX) {
            return Err(invalid("Imported shard collector has no equality key"));
        }
        let key = format!("parhelion/{item:08x}/target-shards/{old:08x}")
            .bytes()
            .fold(0x811c9dc5u32, |hash, byte| {
                hash.wrapping_mul(0x01000193) ^ u32::from(byte)
            });
        keys.insert(old, key);
        writes.push((node.symbol.clone(), d + 0x60, key));
    }
    if keys.is_empty() {
        return Ok(());
    }
    let unique = keys.values().copied().collect::<BTreeSet<_>>();
    if unique.len() != keys.len()
        || unique
            .iter()
            .any(|key| matches!(*key, 0 | u32::MAX) || keys.contains_key(key))
    {
        return Err(invalid("Private shard collector key collision"));
    }
    let mut paired = BTreeSet::new();
    for node in &components {
        let (p, _) = checked(node)?;
        // Embedded lifecycle components are addressed through a reciprocal
        // 3B32 / 3B33 pair. Never replace matching bytes in arbitrary programs.
        for at in (0..node.payload.len().saturating_sub(15)).step_by(8) {
            if read_u32(&node.payload, at + 4)? != 0x80803b33 {
                continue;
            }
            let d = usize::try_from(read_u64(&node.payload, at + 8)?)
                .map_err(|_| invalid("Shard lifecycle offset overflow"))?;
            if p.u32(d + 4).ok() != Some(0x80803b32) || p.u64(d + 8).ok() != Some(at as u64) {
                continue;
            }
            let old = read_u32(&node.payload, d + 0x9c)?;
            if let Some(&key) = keys.get(&old) {
                paired.insert(old);
                writes.push((node.symbol.clone(), d + 0x9c, key));
            }
        }
    }
    if paired.len() != keys.len() {
        return Err(invalid(
            "Imported shard collector lacks its matching private lifecycle",
        ));
    }
    for (symbol, at, key) in writes {
        let node = nodes.iter().find(|node| node.symbol == symbol).unwrap();
        if node.patches.iter().any(|(offset, _)| *offset == at) {
            return Err(invalid("Shard equality key overlaps a package reference"));
        }
        let payload = edited.entry(symbol).or_insert_with(|| node.payload.clone());
        payload
            .get_mut(at..at + 4)
            .ok_or_else(|| invalid("Shard key write exceeds component"))?
            .copy_from_slice(&key.to_le_bytes());
    }
    Ok(())
}
