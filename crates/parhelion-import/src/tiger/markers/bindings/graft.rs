//! Compose native component registrations and rebuild their lookup chains.
use super::*;
use crate::tiger::{markers::optics::ModelInputs, reader::Reader};
use std::collections::BTreeSet;

/// A selector entry: its priority, whether the closure template supplies it,
/// and its row index in that payload's interface groups.
type Entry = (i32, bool, usize);

fn lookup(seed: u32, definitions: &[(u32, u32)]) -> Result<Vec<u8>> {
    let capacity = definitions
        .len()
        .checked_mul(2)
        .and_then(|n| n.max(2).checked_next_power_of_two())
        .context("Component lookup capacity overflow")?;
    ensure!(capacity <= 65536, "Component lookup exceeds native bounds");
    let mut slots = vec![(u32::MAX, 0u32); capacity];
    for &(key, packed) in definitions {
        // Archived native consumer 0x9EBF80. The sign bit belongs to the
        // collision chain, so copying it from an old key placement is invalid.
        let mixed = (seed ^ key).wrapping_add(key.wrapping_shl(4));
        let mixed = (mixed ^ (mixed >> 10)).wrapping_mul(0x81);
        let mut at = (mixed ^ (mixed >> 13)) as usize & (capacity - 1);
        while slots[at].0 != u32::MAX {
            slots[at].1 |= 0x80000000;
            at = (at + 1) & (capacity - 1);
        }
        slots[at] = (key, packed & 0x7FFFFFFF);
    }
    Ok(slots
        .into_iter()
        .flat_map(|(key, value)| [key.to_le_bytes(), value.to_le_bytes()].concat())
        .collect())
}

/// Append the closure's component rows after the entity's own. Returns the new
/// component index of each owner.
fn components(
    entity: &mut Payload,
    patches: &mut Vec<Value>,
    template: &Payload,
    owners: &BTreeMap<u32, String>,
) -> Result<BTreeMap<u32, u32>> {
    let components = entity.array(16, 12, Some(0x80809C04))?;
    let mut indices = BTreeMap::new();
    let (mut bytes, mut refs) = copy(entity, &components, 12, patches)?;
    for row in template.array(16, 12, Some(0x80809C04))? {
        let owner = template.u32(row)?;
        if let Some(symbol) = owners.get(&owner) {
            indices.insert(owner, u32::try_from(bytes.len() / 12)?);
            refs.push(json!({"offset":bytes.len(),"symbol":symbol}));
            bytes.extend(u32::MAX.to_le_bytes());
            bytes.extend_from_slice(&template.0[row + 4..row + 12]);
        }
    }
    ensure!(
        indices.len() == owners.len(),
        "component closure is not unique in its entity"
    );
    append(entity, patches, 16, 12, 0x80809C04, &bytes, &refs)?;
    Ok(indices)
}

/// Collect one interface key's entries from the entity and the closure, sorted by
/// descending priority.
fn entries(
    key: u32,
    entity: &Payload,
    current: &Groups,
    template: &Payload,
    source: &Groups,
    owners: &BTreeMap<u32, String>,
) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    if let Some((_, group)) = current.rows.get(&key) {
        for &i in group {
            entries.push((entity.u32(current.resources[i] + 8)? as i32, false, i));
        }
    }
    if let Some((_, group)) = source.rows.get(&key) {
        for &i in group {
            if owners.contains_key(&template.u32(source.descriptors[i])?) {
                entries.push((template.u32(source.resources[i] + 8)? as i32, true, i));
            }
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    Ok(entries)
}

/// Copy the closure's events whose endpoints are composed components or model
/// inputs, relocating each endpoint to its new component index.
fn events(
    entity: &mut Payload,
    patches: &mut Vec<Value>,
    template: &Payload,
    owners: &BTreeMap<u32, String>,
    indices: &BTreeMap<u32, u32>,
    inputs: &ModelInputs,
) -> Result<()> {
    let (mut events, mut refs) = copy(
        entity,
        &entity.array(0x20, 72, Some(0x80809BC9))?,
        72,
        patches,
    )?;
    for row in template.array(0x20, 72, Some(0x80809BC9))? {
        if ![8, 40]
            .iter()
            .any(|delta| owners.contains_key(&template.u32(row + delta).unwrap_or(0)))
        {
            continue;
        }
        let mut event = template.0[row..row + 72].to_vec();
        for at in [8, 40] {
            let owner = template.u32(row + at)?;
            if let Some(symbol) = owners.get(&owner) {
                event[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                event[at + 16..at + 20].copy_from_slice(&indices[&owner].to_le_bytes());
                refs.push(json!({"offset":events.len()+at,"symbol":symbol}));
            } else if let Some((symbol, target, index)) =
                inputs.get(&(owner, template.u64(row + at + 8)?))
            {
                ensure!(
                    template.u32(row + at + 4)? == 0x80809789,
                    "composed model endpoint is not a vector input"
                );
                event[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                event[at + 8..at + 16].copy_from_slice(&target.to_le_bytes());
                event[at + 16..at + 20].copy_from_slice(&index.to_le_bytes());
                refs.push(json!({"offset":events.len()+at,"symbol":symbol}));
            } else {
                ensure!(
                    template.u64(row + at)? == u64::MAX && template.u64(row + at + 8)? == 0,
                    "component event escapes its copied closure"
                );
            }
        }
        events.extend(event);
    }
    append(entity, patches, 0x20, 72, 0x80809BC9, &events, &refs)?;
    Ok(())
}

/// Add a validated component closure and its native event wiring. The map names
/// private payload symbols. It includes every internal endpoint of the closure.
pub(crate) fn insert(
    _reader: &Reader,
    entity: &mut Payload,
    patches: &mut Vec<Value>,
    template: &Payload,
    owners: &BTreeMap<u32, String>,
    inputs: &ModelInputs,
) -> Result<()> {
    ensure!(!owners.is_empty(), "empty component closure");
    let current = groups(entity)?;
    let source = groups(template)?;
    let mut required = current.rows.keys().copied().collect::<BTreeSet<_>>();
    let mut expected = 0;
    for (&key, (_, indices)) in &source.rows {
        for &i in indices {
            if owners.contains_key(&template.u32(source.descriptors[i])?) {
                required.insert(key);
                expected += 1;
            }
        }
    }
    ensure!(expected > 0, "component closure has no interfaces");
    let (mut masks, _) = copy(entity, &entity.array(0x78, 2, Some(0x80800006))?, 2, &[])?;
    let mask_offset = masks.len() / 2;
    let (extra_masks, _) = copy(
        template,
        &template.array(0x78, 2, Some(0x80800006))?,
        2,
        &[],
    )?;
    let source_mask_count = extra_masks.len() / 2;
    masks.extend(extra_masks);
    ensure!(
        masks.len() / 2 <= i16::MAX as usize,
        "Component condition masks exceed native bounds"
    );
    let indices = components(entity, patches, template, owners)?;
    let mut resources = Vec::new();
    let mut descriptors = Vec::new();
    let mut resource_refs = Vec::new();
    let mut descriptor_refs = Vec::new();
    let mut slots = Vec::new();
    let mut added = 0;
    for key in required {
        let entries = entries(key, entity, &current, template, &source, owners)?;
        let start = descriptors.len() / 24;
        let count = entries.len();
        ensure!(
            count > 0 && start + count <= i16::MAX as usize,
            "composed selector overflow"
        );
        for (_, extra, i) in entries {
            let (mut r, mut d, rr, dr) = if extra {
                let r = source.resources[i];
                let d = source.descriptors[i];
                let owner = template.u32(d)?;
                let symbol = &owners[&owner];
                let mut descriptor = template.0[d..d + 24].to_vec();
                descriptor[16..20].copy_from_slice(&indices[&owner].to_le_bytes());
                (
                    template.0[r..r + 40].to_vec(),
                    descriptor,
                    vec![json!({"offset":16,"symbol":symbol})],
                    vec![json!({"offset":0,"symbol":symbol})],
                )
            } else {
                let (r, rr) = copy(entity, &[current.resources[i]], 40, patches)?;
                let (d, dr) = copy(entity, &[current.descriptors[i]], 24, patches)?;
                (r, d, rr, dr)
            };
            if extra {
                let conditions = u32::from_le_bytes(r[..4].try_into()?);
                if conditions >> 16 != 0 {
                    let word = (conditions & 0xFFFF) as usize;
                    ensure!(
                        word < source_mask_count,
                        "Component condition mask exceeds source"
                    );
                    r[..4].copy_from_slice(
                        &((conditions & 0xFFFF0000) | u32::try_from(word + mask_offset)?)
                            .to_le_bytes(),
                    );
                }
                r[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                d[..4].copy_from_slice(&u32::MAX.to_le_bytes());
                added += 1;
            }
            for p in rr {
                resource_refs
                    .push(json!({"offset":resources.len()+offset(&p)?,"symbol":p["symbol"]}));
            }
            for p in dr {
                descriptor_refs
                    .push(json!({"offset":descriptors.len()+offset(&p)?,"symbol":p["symbol"]}));
            }
            resources.extend(r);
            descriptors.extend(d);
        }
        slots.push((key, ((count as u32) << 16) | start as u32));
    }
    ensure!(added == expected, "composed lookup dropped an interface");
    append(
        entity,
        patches,
        0x58,
        40,
        0x80809C22,
        &resources,
        &resource_refs,
    )?;
    append(
        entity,
        patches,
        0x68,
        24,
        0x80809C20,
        &descriptors,
        &descriptor_refs,
    )?;
    let slots = lookup(entity.u32(0x40)?, &slots)?;
    append(entity, patches, 0x48, 8, 0x80809C25, &slots, &[])?;
    append(entity, patches, 0x78, 2, 0x80800006, &masks, &[])?;
    events(entity, patches, template, owners, &indices, inputs)?;
    groups(entity)?;
    Ok(())
}
