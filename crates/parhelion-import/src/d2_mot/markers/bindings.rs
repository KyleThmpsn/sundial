//! Marker component registrations and cached resource references must move together.
use super::*;
use anyhow::Context;
use serde_json::{Value, json};
pub(super) mod graft;

fn offset(patch: &Value) -> Result<usize> {
    Ok(usize::try_from(
        patch["offset"].as_u64().context("marker patch offset")?,
    )?)
}

fn copy(
    payload: &Payload,
    rows: &[usize],
    stride: usize,
    patches: &[Value],
) -> Result<(Vec<u8>, Vec<Value>)> {
    let mut bytes = Vec::new();
    let mut refs = Vec::new();
    for &row in rows {
        let end = row.checked_add(stride).context("marker row overflow")?;
        for patch in patches {
            let at = offset(patch)?;
            if (row..end).contains(&at) {
                refs.push(json!({"offset":bytes.len()+at-row,"symbol":patch["symbol"]}));
            }
        }
        bytes.extend_from_slice(
            payload
                .0
                .get(row..end)
                .context("marker row outside entity")?,
        );
    }
    Ok((bytes, refs))
}

pub(super) fn append(
    payload: &mut Payload,
    patches: &mut Vec<Value>,
    descriptor: usize,
    stride: usize,
    class: u32,
    rows: &[u8],
    refs: &[Value],
) -> Result<()> {
    ensure!(
        rows.len().is_multiple_of(stride),
        "marker array has a partial row"
    );
    let old = payload.array(descriptor, stride, Some(class))?;
    if let (Some(&start), Some(&last)) = (old.first(), old.last()) {
        let end = last + stride;
        let mut kept = Vec::new();
        for patch in patches.drain(..) {
            if !(start..end).contains(&offset(&patch)?) {
                kept.push(patch);
            }
        }
        *patches = kept;
    }
    let header = (payload.0.len() + 19) & !15;
    payload.0.resize(header - 4, 0);
    payload.0.extend(0x80809FBDu32.to_le_bytes());
    payload
        .0
        .extend(((rows.len() / stride) as u64).to_le_bytes());
    payload.0.extend(u64::from(class).to_le_bytes());
    payload.0.extend(rows);
    payload.0[descriptor..descriptor + 8]
        .copy_from_slice(&((rows.len() / stride) as u64).to_le_bytes());
    payload.0[descriptor + 8..descriptor + 16]
        .copy_from_slice(&(i64::try_from(header)? - i64::try_from(descriptor + 8)?).to_le_bytes());
    for patch in refs {
        patches.push(json!({"offset":header+16+offset(patch)?,"symbol":patch["symbol"]}));
    }
    let size = payload.0.len() as u64;
    payload.0[..8].copy_from_slice(&size.to_le_bytes());
    Ok(())
}

struct Groups {
    rows: BTreeMap<u32, (u32, Vec<usize>)>,
    resources: Vec<usize>,
    descriptors: Vec<usize>,
}

fn groups(payload: &Payload) -> Result<Groups> {
    let resources = payload.array(0x58, 40, Some(0x80809C22))?;
    let descriptors = payload.array(0x68, 24, Some(0x80809C20))?;
    ensure!(
        resources.len() == descriptors.len(),
        "marker resource cache count differs"
    );
    let mut rows = BTreeMap::new();
    let mut covered = Vec::<usize>::new();
    for row in payload.array(0x48, 8, Some(0x80809C25))? {
        let key = payload.u32(row)?;
        if key == u32::MAX {
            continue;
        }
        let packed = payload.u32(row + 4)?;
        let start = (packed & 0xFFFF) as usize;
        let count = ((packed >> 16) & 0x7FFF) as usize;
        ensure!(
            count > 0 && start + count <= resources.len(),
            "marker resource selector outside entity"
        );
        let indices = (start..start + count).collect::<Vec<_>>();
        covered.extend(&indices);
        ensure!(
            rows.insert(key, (packed & 0x80000000, indices)).is_none(),
            "duplicate marker binding key"
        );
    }
    covered.sort_unstable();
    ensure!(
        covered == (0..resources.len()).collect::<Vec<_>>(),
        "marker selectors do not partition resources"
    );
    Ok(Groups {
        rows,
        resources,
        descriptors,
    })
}

pub(super) fn retarget(
    entity: &mut Payload,
    patches: &mut Vec<Value>,
    row: usize,
    owner: u32,
    payload: &Payload,
    symbol: &str,
) -> Result<()> {
    let mut original = entity.clone();
    original.0[row..row + 4].copy_from_slice(&owner.to_le_bytes());
    // A newly separated part may still point to the assembled entity's private
    // component. Follow that existing component binding before giving it a new
    // private symbol, including all live typed references and event endpoints.
    let previous = patches
        .iter()
        .find(|patch| patch["offset"].as_u64() == Some(row as u64))
        .and_then(|patch| patch["symbol"].as_str())
        .unwrap_or(symbol)
        .to_owned();
    for patch in patches.iter().filter(|patch| patch["symbol"] == previous) {
        let at = offset(patch)?;
        original
            .0
            .get_mut(at..at + 4)
            .context("marker patch outside entity")?
            .copy_from_slice(&owner.to_le_bytes());
    }
    // Appending a component table leaves the previous serialized table behind.
    // Only live descriptors participate in entity lookup. Scanning the whole
    // payload mistakes retired component rows for malformed resource pointers.
    let mut slots = vec![row];
    let mut references = Vec::new();
    for at in original.array(0x20, 72, Some(0x80809BC9))? {
        references.extend([at + 8, at + 40]);
    }
    references.extend(
        original
            .array(0x58, 40, Some(0x80809C22))?
            .into_iter()
            .map(|at| at + 16),
    );
    references.extend(original.array(0x68, 24, Some(0x80809C20))?);
    for at in references {
        if original.u32(at)? != owner {
            continue;
        }
        ensure!(
            original.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                && original
                    .u64(at + 8)?
                    .checked_add(16)
                    .is_some_and(|end| end <= payload.0.len() as u64),
            "live component reference at {at:X} is untyped or outside its owner"
        );
        slots.push(at);
    }
    ensure!(slots.len() > 1, "component has no live resource references");
    for at in slots {
        entity.0[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        patches.retain(|p| p["offset"].as_u64() != Some(at as u64));
        patches.push(json!({"offset":at,"symbol":symbol}));
    }
    Ok(())
}

/// Use a native definition lookup that already accommodates every required key.
/// Its buckets and seed stay paired with its slots. Empty unused slots are safe
/// because lookup verifies the stored key. No guessed hash function is involved.
pub(super) fn insert(
    entity: &mut Payload,
    patches: &mut Vec<Value>,
    template: &Payload,
    owner: u32,
    symbol: &str,
) -> Result<()> {
    let components = entity.array(16, 12, Some(0x80809C04))?;
    ensure!(
        !components.is_empty() && components.len() < 64,
        "invalid marker entity component count"
    );
    let component = u32::try_from(components.len())?;
    let (mut bytes, mut refs) = copy(entity, &components, 12, patches)?;
    refs.push(json!({"offset":bytes.len(),"symbol":symbol}));
    bytes.extend(u32::MAX.to_le_bytes());
    bytes.extend([0; 8]);
    append(entity, patches, 16, 12, 0x80809C04, &bytes, &refs)?;

    let current = groups(entity)?;
    let source = groups(template)?;
    ensure!(
        current.rows.keys().all(|key| source.rows.contains_key(key)),
        "native marker lookup does not cover this entity's interfaces"
    );
    let mut resources = Vec::new();
    let mut descriptors = Vec::new();
    let mut resource_refs = Vec::new();
    let mut descriptor_refs = Vec::new();
    let mut lookup = Vec::new();
    let mut added = 0;
    for row in template.array(0x48, 8, Some(0x80809C25))? {
        let key = template.u32(row)?;
        if key == u32::MAX {
            lookup.extend_from_slice(&template.0[row..row + 8]);
            continue;
        }
        let mut entries = Vec::new();
        if let Some((_, indices)) = current.rows.get(&key) {
            for &index in indices {
                entries.push((
                    entity.u32(current.resources[index] + 8)? as i32,
                    false,
                    index,
                ));
            }
        }
        let before = entries.len();
        for &index in &source.rows[&key].1 {
            if template.u32(source.descriptors[index])? == owner {
                entries.push((
                    template.u32(source.resources[index] + 8)? as i32,
                    true,
                    index,
                ));
            }
        }
        if entries.is_empty() {
            lookup.extend(u32::MAX.to_le_bytes());
            lookup.extend(0u32.to_le_bytes());
            continue;
        }
        let flags = current.rows.get(&key).map_or(source.rows[&key].0, |v| v.0);
        ensure!(
            entries.len() == before || flags == source.rows[&key].0,
            "marker aggregate flags differ"
        );
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        let start = descriptors.len() / 24;
        let count = entries.len();
        ensure!(
            start + count < 65536 && count < 32768,
            "marker resource selector overflow"
        );
        for (_, extra, index) in entries {
            let (mut r, mut d, rr, dr) = if extra {
                let r = source.resources[index];
                let d = source.descriptors[index];
                (
                    template.0[r..r + 40].to_vec(),
                    template.0[d..d + 24].to_vec(),
                    vec![json!({"offset":16,"symbol":symbol})],
                    vec![json!({"offset":0,"symbol":symbol})],
                )
            } else {
                let (r, rr) = copy(entity, &[current.resources[index]], 40, patches)?;
                let (d, dr) = copy(entity, &[current.descriptors[index]], 24, patches)?;
                (r, d, rr, dr)
            };
            if extra {
                r[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
                d[..4].copy_from_slice(&u32::MAX.to_le_bytes());
                d[16..20].copy_from_slice(&component.to_le_bytes());
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
        lookup.extend(key.to_le_bytes());
        lookup.extend((flags | ((count as u32) << 16) | start as u32).to_le_bytes());
    }
    ensure!(added == 3, "unsupported native marker interface set");
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
    append(entity, patches, 0x48, 8, 0x80809C25, &lookup, &[])?;
    ensure!(
        entity.bytes::<4>(0x88)? == template.bytes::<4>(0x88)?
            && entity.bytes::<24>(0x90)? == template.bytes::<24>(0x90)?,
        "native marker lookup metadata differs"
    );
    let (buckets, refs) = copy(
        template,
        &template.array(0x78, 2, Some(0x80800006))?,
        2,
        &[],
    )?;
    append(entity, patches, 0x78, 2, 0x80800006, &buckets, &refs)?;
    // The lookup header preceding its slots travels with the slot layout too.
    // Keeping the old entity's header makes even its existing interfaces miss.
    entity.0[0x40..0x48].copy_from_slice(&template.bytes::<8>(0x40)?);
    entity.0[0x8c..0x90].copy_from_slice(&template.bytes::<4>(0x8c)?);

    let (mut events, mut refs) = copy(
        entity,
        &entity.array(0x20, 72, Some(0x80809BC9))?,
        72,
        patches,
    )?;
    let mut count = 0;
    for row in template.array(0x20, 72, Some(0x80809BC9))? {
        if template.u32(row + 8)? != owner && template.u32(row + 40)? != owner {
            continue;
        }
        ensure!(
            template.u32(row + 8)? == owner
                && template.u64(row + 40)? == u64::MAX
                && template.u64(row + 48)? == 0,
            "unsupported marker event endpoint"
        );
        let mut event = template.0[row..row + 72].to_vec();
        event[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        event[24..28].copy_from_slice(&component.to_le_bytes());
        refs.push(json!({"offset":events.len()+8,"symbol":symbol}));
        events.extend(event);
        count += 1;
    }
    ensure!(count == 1, "unsupported native marker event set");
    append(entity, patches, 0x20, 72, 0x80809BC9, &events, &refs)
}
