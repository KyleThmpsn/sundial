//! Compose native component registrations without depending on the weapon donor
//! to already contain the component. Lookup carriers provide format metadata only.
use super::*;
use crate::d2_mot::{markers::optics::ModelInputs, reader::Reader};
use std::{
    collections::BTreeSet,
    sync::{Mutex, OnceLock},
};

/// A selector entry: its priority, whether the closure template supplies it,
/// and its row index in that payload's interface groups.
type Entry = (i32, bool, usize);

fn compatible(entity: &Payload, candidate: &Payload, required: &BTreeSet<u32>) -> bool {
    let Ok(g) = groups(candidate) else {
        return false;
    };
    required.iter().all(|key| g.rows.contains_key(key))
        && entity.bytes::<4>(0x88).ok() == candidate.bytes::<4>(0x88).ok()
        && entity.bytes::<24>(0x90).ok() == candidate.bytes::<24>(0x90).ok()
}

fn lookup(reader: &Reader, entity: &Payload, required: &BTreeSet<u32>) -> Result<Payload> {
    // Cached payloads supply only keys, lookup seeds and bucket metadata. No
    // asset reference or component payload is carried from this format cache.
    static CACHE: OnceLock<Mutex<Vec<Payload>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("component lookup cache poisoned"))?;
    if let Some(found) = cache.iter().find(|p| compatible(entity, p, required)) {
        return Ok(found.clone());
    }
    let mut candidates = reader
        .manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .flat_map(|(&pkg, entries)| {
            entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.reference == 0x80809C0F)
                .map(move |(i, _)| tiger_pkg::TagHash::new(pkg, i as u16))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|tag| tag.0);
    for tag in candidates {
        let Ok(bytes) = reader.manager.read_tag(tag) else {
            continue;
        };
        let payload = Payload(bytes);
        if compatible(entity, &payload, required) {
            cache.push(payload.clone());
            return Ok(payload);
        }
    }
    anyhow::bail!("no native lookup layout covers the composed component interfaces")
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
/// descending priority, with the interface's aggregation flags.
fn entries(
    key: u32,
    entity: &Payload,
    current: &Groups,
    template: &Payload,
    source: &Groups,
    owners: &BTreeMap<u32, String>,
) -> Result<(Vec<Entry>, u32)> {
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
    let flags = current
        .rows
        .get(&key)
        .or_else(|| source.rows.get(&key))
        .context("composed interface flags")?
        .0;
    if entries.iter().any(|e| e.1) && current.rows.contains_key(&key) {
        ensure!(
            flags == source.rows[&key].0,
            "composed interface aggregation flags differ"
        );
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    Ok((entries, flags))
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
pub(in crate::d2_mot::markers) fn insert(
    reader: &Reader,
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
    let layout = lookup(reader, entity, &required)?;
    let indices = components(entity, patches, template, owners)?;
    let mut resources = Vec::new();
    let mut descriptors = Vec::new();
    let mut resource_refs = Vec::new();
    let mut descriptor_refs = Vec::new();
    let mut slots = Vec::new();
    let mut added = 0;
    for row in layout.array(0x48, 8, Some(0x80809C25))? {
        let key = layout.u32(row)?;
        if key == u32::MAX || !required.contains(&key) {
            slots.extend(u32::MAX.to_le_bytes());
            slots.extend(0u32.to_le_bytes());
            continue;
        }
        let (entries, flags) = entries(key, entity, &current, template, &source, owners)?;
        let start = descriptors.len() / 24;
        let count = entries.len();
        ensure!(
            count > 0 && count < 32768 && start + count < 65536,
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
        slots.extend(key.to_le_bytes());
        slots.extend((flags | ((count as u32) << 16) | start as u32).to_le_bytes());
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
    append(entity, patches, 0x48, 8, 0x80809C25, &slots, &[])?;
    let (buckets, _) = copy(&layout, &layout.array(0x78, 2, Some(0x80800006))?, 2, &[])?;
    append(entity, patches, 0x78, 2, 0x80800006, &buckets, &[])?;
    entity.0[0x40..0x48].copy_from_slice(&layout.bytes::<8>(0x40)?);
    entity.0[0x8c..0x90].copy_from_slice(&layout.bytes::<4>(0x8c)?);
    events(entity, patches, template, owners, &indices, inputs)?;
    groups(entity)?;
    Ok(())
}
