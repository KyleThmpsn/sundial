//! Assemble converted components using native registration and lifecycle contracts.
//!
//! A component supplies its complete converted payload and a native owner of the same
//! implementation. Registration rows come from an entity which actually owns that template.
//! Every provider is resolved through its reciprocal definition in the converted payload.
//! Connection rows are supplied by the typed cross-version linker.
pub mod replication;

use super::links::NativeRows;
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Component<'a> {
    pub owner: u32,
    pub payload: &'a Payload,
    pub template: &'a Payload,
    pub entity: &'a Payload,
}

fn put(p: &mut Payload, at: usize, bytes: &[u8]) -> Result<()> {
    p.0.get_mut(
        at..at
            .checked_add(bytes.len())
            .context("entity write overflow")?,
    )
    .context("entity write outside payload")?
    .copy_from_slice(bytes);
    Ok(())
}

fn array(p: &mut Payload, field: usize, class: u32, stride: usize, rows: &[Vec<u8>]) -> Result<()> {
    ensure!(
        rows.iter().all(|row| row.len() == stride),
        "compiled entity row width differs"
    );
    put(p, field, &(rows.len() as u64).to_le_bytes())?;
    if rows.is_empty() {
        return put(p, field + 8, &0u64.to_le_bytes());
    }
    let header = p.0.len().checked_add(19).context("entity array overflow")? & !15;
    p.0.resize(header + 16, 0);
    put(p, header - 4, &0x80809FBDu32.to_le_bytes())?;
    put(p, header, &(rows.len() as u64).to_le_bytes())?;
    put(p, header + 8, &class.to_le_bytes())?;
    put(
        p,
        field + 8,
        &(header as i64 - (field + 8) as i64).to_le_bytes(),
    )?;
    for row in rows {
        p.0.extend(row);
    }
    Ok(())
}

fn lookup(seed: u32, spans: &[(u32, u32)]) -> Result<Vec<Vec<u8>>> {
    let count = spans
        .len()
        .checked_mul(2)
        .and_then(|n| n.max(2).checked_next_power_of_two())
        .context("compiled entity lookup overflow")?;
    ensure!(
        count <= 65536,
        "compiled entity lookup exceeds native capacity"
    );
    let mut slots = vec![(u32::MAX, 0u32); count];
    for &(key, span) in spans {
        ensure!(
            key != u32::MAX && span & 0x80000000 == 0,
            "invalid compiled interface span"
        );
        // Archived Shadowkeep consumer 9EBF80. Continuation belongs to slot placement.
        let mixed = (seed ^ key).wrapping_add(key.wrapping_shl(4));
        let mixed = (mixed ^ (mixed >> 10)).wrapping_mul(0x81);
        let mut at = (mixed ^ (mixed >> 13)) as usize & (count - 1);
        while slots[at].0 != u32::MAX {
            slots[at].1 |= 0x80000000;
            at = (at + 1) & (count - 1);
        }
        slots[at] = (key, span);
    }
    Ok(slots
        .into_iter()
        .map(|(key, span)| [key.to_le_bytes(), span.to_le_bytes()].concat())
        .collect())
}

struct Registration {
    priority: i32,
    map: Vec<u8>,
    descriptor: Vec<u8>,
}

fn registrations(
    component: &Component<'_>,
    ordinal: usize,
    masks: &mut Vec<Vec<u8>>,
    groups: &mut BTreeMap<u32, Vec<Registration>>,
) -> Result<()> {
    let p = component.payload;
    let t = component.template;
    let entity = component.entity;
    let (ti, td) = (t.pointer(16)?, t.pointer(24)?);
    let ni = p.pointer(16)?;
    // Sealed controllers keep their original reciprocal definitions while the
    // header names a duplicate schema beyond appended mutable runtime arrays.
    // Registrations address those original records, not the loader's boundary.
    let nd = usize::try_from(p.u64(ni + 8)?)?;
    let boundary = p.pointer(24)?;
    let template_owner = t.u32(ti)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && p.u32(ni)? == component.owner
            && p.u32(nd)? == component.owner
            && ni < nd
            && nd <= boundary
            && p.u32(boundary)? == component.owner
            && p.u32(boundary + 4)? == t.u32(td + 4)?
            && p.u32(ni + 4)? == t.u32(ti + 4)?
            && p.u32(nd + 4)? == t.u32(td + 4)?,
        "compiled component implementation differs"
    );
    let owners = entity.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners
            .iter()
            .filter(|&&at| entity.u32(at).ok() == Some(template_owner))
            .count()
            == 1,
        "native registration template does not uniquely own its component"
    );
    let maps = entity.array(88, 40, Some(0x80809C22))?;
    let descriptors = entity.array(104, 24, Some(0x80809C20))?;
    ensure!(
        maps.len() == descriptors.len(),
        "native registration tables differ in length"
    );
    let old_masks = entity.array(120, 2, Some(0x80800006))?;
    let mask_base = masks.len();
    for &at in &old_masks {
        ensure!(
            entity.u16(at)? == 0,
            "native template condition state is initialized"
        );
        masks.push(vec![0; 2]);
    }
    ensure!(
        masks.len() <= i16::MAX as usize,
        "compiled condition mask capacity exceeded"
    );
    for binding in entity.array(72, 8, Some(0x80809C25))? {
        let key = entity.u32(binding)?;
        if key == u32::MAX {
            continue;
        }
        let span = entity.u32(binding + 4)?;
        let start = (span & 0xFFFF) as usize;
        let count = ((span >> 16) & 0x7FFF) as usize;
        ensure!(
            count != 0 && start + count <= maps.len(),
            "native interface span exceeds registration rows"
        );
        for index in start..start + count {
            let dr = descriptors[index];
            if entity.u32(dr)? != template_owner {
                continue;
            }
            let mr = maps[index];
            ensure!(
                entity.u32(mr + 16)? == template_owner && entity.u32(mr + 20)? == 0x80809C50,
                "native component registration owner differs"
            );
            let old_provider = usize::try_from(entity.u64(mr + 24)?)?;
            let delta = old_provider
                .checked_sub(td)
                .context("native provider precedes its definition")?;
            let provider = nd
                .checked_add(delta)
                .context("compiled provider overflow")?;
            ensure!(
                p.bytes::<16>(provider + 8)? == t.bytes::<16>(old_provider + 8)?,
                "converted component changed native provider metadata or priority"
            );
            let definition = p.pointer(provider)?;
            let instance = usize::try_from(p.u64(definition + 8)?)?;
            ensure!(
                p.u32(definition)? == component.owner
                    && p.u32(instance)? == component.owner
                    && p.u64(instance + 8)? == definition as u64,
                "compiled registration for template {template_owner:08X}, owner {:08X}, provider {provider:X}, definition {definition:X}, instance {instance:X} does not resolve a reciprocal object: {:X?}",
                component.owner,
                (
                    p.u32(definition).ok(),
                    p.u32(instance).ok(),
                    p.u64(instance + 8).ok()
                )
            );
            let old_definition = t.pointer(old_provider)?;
            ensure!(
                t.u64(old_definition + 8)? == entity.u64(dr + 8)?
                    && t.u32(old_definition + 4)? == entity.u32(dr + 4)?
                    && p.u32(definition + 4)? == entity.u32(dr + 4)?,
                "compiled registration instance type differs"
            );
            let mut map = entity.0[mr..mr + 40].to_vec();
            let mut descriptor = entity.0[dr..dr + 24].to_vec();
            if entity.u16(mr + 2)? != 0 {
                let old =
                    usize::try_from(entity.i16(mr)?).context("negative condition mask index")?;
                ensure!(
                    old < old_masks.len(),
                    "native condition mask index exceeds table"
                );
                map[..2].copy_from_slice(&i16::try_from(mask_base + old)?.to_le_bytes());
            }
            let relative = i32::try_from(instance as i64 - ni as i64)?;
            let priority = i32::from(p.i16(provider + 16)?);
            ensure!(
                priority == entity.u32(mr + 8)? as i32,
                "native callback priority differs"
            );
            map[4..8].copy_from_slice(&relative.to_le_bytes());
            map[16..20].copy_from_slice(&component.owner.to_le_bytes());
            map[24..32].copy_from_slice(&(provider as u64).to_le_bytes());
            descriptor[..4].copy_from_slice(&component.owner.to_le_bytes());
            descriptor[8..16].copy_from_slice(&(instance as u64).to_le_bytes());
            descriptor[16..20].copy_from_slice(&u32::try_from(ordinal)?.to_le_bytes());
            groups.entry(key).or_default().push(Registration {
                priority,
                map,
                descriptor,
            });
        }
    }
    Ok(())
}

/// Build the complete native component tables. Native-only registration keys come from
/// the selected component implementations, while ordinary events follow the source graph.
pub fn emit(
    template: &Payload,
    components: &[Component<'_>],
    connections: &NativeRows,
) -> Result<Payload> {
    ensure!(
        !components.is_empty() && components.len() < 0xFFFF,
        "invalid compiled component count"
    );
    let mut output = Payload(
        template
            .0
            .get(..0xA0)
            .context("native entity prefix")?
            .to_vec(),
    );
    let mut masks = Vec::new();
    let mut groups = BTreeMap::new();
    let mut owners = Vec::new();
    for (ordinal, component) in components.iter().enumerate() {
        registrations(component, ordinal, &mut masks, &mut groups)?;
        let mut row = vec![0; 12];
        row[..4].copy_from_slice(&component.owner.to_le_bytes());
        owners.push(row);
    }
    let mut maps = Vec::new();
    let mut descriptors = Vec::new();
    let mut spans = Vec::new();
    for (key, mut entries) in groups {
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.priority));
        let start = maps.len();
        let count = entries.len();
        ensure!(
            start + count <= i16::MAX as usize,
            "compiled interface iterator capacity exceeded"
        );
        spans.push((key, ((count as u32) << 16) | start as u32));
        for entry in entries {
            maps.push(entry.map);
            descriptors.push(entry.descriptor);
        }
    }
    let definitions = lookup(template.u32(64)?, &spans)?;
    for (field, class, stride, rows) in [
        (16, 0x80809C04, 12, &owners),
        (32, 0x80809BC9, 72, &connections.connections),
        (48, 0x80809BC9, 72, &connections.named_connections),
        (72, 0x80809C25, 8, &definitions),
        (88, 0x80809C22, 40, &maps),
        (104, 0x80809C20, 24, &descriptors),
        (120, 0x80800006, 2, &masks),
    ] {
        array(&mut output, field, class, stride, rows)?;
    }
    let size = output.0.len() as u64;
    put(&mut output, 0, &size.to_le_bytes())?;
    Ok(output)
}
