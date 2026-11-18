//! Rise of Iron sword guards on the base's keyed guard owners.
//!
//! A guard owner is selected by the name the weapon holds and scales the damage its entity takes
//! while guarding. Source callback `007CC120` and native `00DA7A80` are the same routine: each
//! row that matches the damage multiplies it by the row's share and adds the row's cost, the cost
//! is taken from the weapon's ammunition, and a remainder under the definition's threshold breaks
//! the guard. A row is an incoming damage row with that cost: damage type masks, label masks,
//! their bypass flags and the share. Shadowkeep's row keeps the ammunition cost at `+A0`, zero on
//! every stock row, beside the sword energy cost it added at `+A4`.
//!
//! A source guard therefore translates whole onto a native guard owner: its key, its exclusions
//! and one native row per source row with a constant share, the source's ammunition cost and no
//! energy cost. The base's keyed guard owners carry the source guards in order, and the base's
//! open guard yields to each carried name.
use super::*;
use crate::d1_roi::native::{
    damage_profile::{NATIVE_DICTIONARY, SOURCE_DICTIONARY},
    perk::label,
};

pub(super) const BINDING: u32 = 0x5211_1D67;

/// One source guard: its name, the names that exclude it and its damage rows.
struct Guard {
    owner: u32,
    key: u32,
    exclusions: Vec<u32>,
    rows: Vec<Row>,
}

struct Row {
    types: [u8; 8],
    include: [u8; 32],
    exclude: [u8; 32],
    bypass: [u8; 4],
    share: [u8; 4],
    cost: [u8; 4],
}

fn read(source: &Source, tag: u32) -> Result<Option<Guard>> {
    let p = source.read(tag)?;
    let definition = p.pointer(24)?;
    if definition < 4 || p.u32(definition - 4)? != 0x8080_315A {
        return Ok(None);
    }
    let key = p.u32(definition + 0x270)?;
    ensure!(
        ![0, u32::MAX, EMPTY].contains(&key),
        "Source sword guard {tag:08X} has no name"
    );
    let exclusions = p
        .array(definition + 0x278, 4, Some(0x8080_0070))?
        .into_iter()
        .map(|at| p.u32(at))
        .collect::<Result<Vec<_>>>()?;
    let mut rows = Vec::new();
    for at in p.array(definition + 0x108, 0x54, Some(0x8080_2B5E))? {
        let row = Row {
            types: p.bytes(at)?,
            include: p.bytes(at + 8)?,
            exclude: p.bytes(at + 0x28)?,
            bypass: p.bytes(at + 0x48)?,
            share: p.bytes(at + 0x4C)?,
            cost: p.bytes(at + 0x50)?,
        };
        // Damage type indices 0 to 3 are the four types both builds share.
        ensure!(
            row.types[0] & !0x0F == 0
                && row.types[4] & !0x0F == 0
                && row.types[1..4] == [0; 3]
                && row.types[5..] == [0; 3]
                && row.bypass.iter().all(|&flag| flag <= 1)
                && row.bypass[2] == u8::from(row.include == [0; 32])
                && row.bypass[3] == u8::from(row.exclude == [0; 32])
                && f32::from_le_bytes(row.share).is_finite()
                && f32::from_le_bytes(row.cost).is_finite()
                && f32::from_le_bytes(row.cost) >= 0.0,
            "Source sword guard {tag:08X} row has other damage types, flags, share or cost"
        );
        rows.push(row);
    }
    ensure!(!rows.is_empty(), "Source sword guard {tag:08X} has no row");
    Ok(Some(Guard {
        owner: tag,
        key,
        exclusions,
        rows,
    }))
}

/// A native array of names at `descriptor`, replaced by `names`, as an appended array or a
/// cleared descriptor.
fn names(
    blob: &mut Blob,
    edits: &mut BTreeMap<usize, [u8; 4]>,
    arrays: &mut Vec<(usize, usize, u64)>,
    payload: &Payload,
    descriptor: usize,
    names: &[u32],
) -> Result<()> {
    if names.is_empty() {
        for word in 0..4 {
            if payload.bytes::<4>(descriptor + word * 4)? != [0; 4] {
                edits.insert(descriptor + word * 4, [0; 4]);
            }
        }
        return Ok(());
    }
    let (header, at) = blob.array(names.len(), 0x8080_0070, 4);
    for (index, name) in names.iter().enumerate() {
        blob.word(at + index * 4, *name);
    }
    arrays.push((descriptor, header, names.len() as u64));
    Ok(())
}

/// The base's guard owners under [`BINDING`] by resource index: owner tag, payload, resource
/// offset and definition.
type Carrier = (usize, u32, Payload, usize, usize);

fn report(
    index: usize,
    (tag, payload, resource): (u32, &Payload, usize),
    edits: &BTreeMap<usize, [u8; 4]>,
    blob: &Blob,
    arrays: &[(usize, usize, u64)],
    evidence: Value,
) -> Result<Value> {
    let relative = |at: usize| -> Result<u32> {
        Ok(u32::try_from(
            at.checked_sub(resource)
                .context("Guard edit precedes its resource")?,
        )?)
    };
    let edits = edits
        .iter()
        .map(|(&at, after)| {
            Ok(json!({"offset":relative(at)?,"before":payload.bytes::<4>(at)?,"after":after}))
        })
        .collect::<Result<Vec<_>>>()?;
    let appends = if blob.bytes.is_empty() {
        Vec::new()
    } else {
        vec![json!({"bytes":hex::encode(&blob.bytes),
            "arrays":arrays.iter().map(|&(descriptor, header, count)| Ok(json!([relative(descriptor)?, header, count]))).collect::<Result<Vec<_>>>()?,
            "references":blob.absolute,
            "retained_pointers":blob.retained.iter().map(|&(word, target)| Ok(json!([word, relative(target)?]))).collect::<Result<Vec<_>>>()?})]
    };
    let mut report = json!({"resource_index":index,"native_owner":tag,"native_resource":resource,
        "native_sha256":hex::encode(Sha256::digest(&payload.0)),"edits":edits,"appends":appends});
    for (key, value) in evidence.as_object().into_iter().flatten() {
        report[key] = value.clone();
    }
    Ok(report)
}

/// Translates the source entity's guards onto the base's guard owners and returns their reports
/// and the names the carried guards read.
pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    entity: &Payload,
    runtime: &[u8],
) -> Result<(Vec<Value>, BTreeSet<u32>, Vec<Value>)> {
    let mut guards = Vec::new();
    for r in entity.array(32, 12, Some(0x8080_0715))? {
        if let Some(guard) = read(source, entity.u32(r)?)? {
            guards.push(guard);
        }
    }
    let mut carriers: Vec<Carrier> = Vec::new();
    for (index, binding) in weapon_component_bindings(runtime, BINDING)
        .map_err(anyhow::Error::msg)?
        .iter()
        .enumerate()
    {
        let payload = (*native.tag(binding.owner_tag, Some(0x8080_9C36))?).clone();
        let resource = usize::try_from(binding.resource_offset)?;
        ensure!(
            resource >= 4
                && resource == payload.pointer(16)?
                && payload.u32(resource - 4)? == 0x8080_42AC
                && payload.u32(resource)? == binding.owner_tag,
            "The base's guard resource has another class"
        );
        let definition = usize::try_from(payload.u64(resource + 8)?)?;
        ensure!(
            definition == payload.pointer(24)? && payload.u32(definition - 4)? == 0x8080_3C69,
            "The base's guard definition has another class"
        );
        carriers.push((index, binding.owner_tag, payload, resource, definition));
    }
    let registry = native.tag(NATIVE_DICTIONARY, None)?;
    let (keyed, open): (Vec<_>, Vec<_>) =
        carriers.iter().partition(|(_, _, payload, _, definition)| {
            payload.u32(definition + 0x2B8).ok() != Some(EMPTY)
        });
    let mut reports = Vec::new();
    let mut keys = BTreeSet::new();
    let mut unsupported = Vec::new();
    let mut carried = keyed.iter();
    for guard in &guards {
        let Some(&&(index, tag, ref payload, resource, definition)) = carried.next() else {
            unsupported.push(json!({"guard":guard.owner,"key":guard.key,
                "reason":"The base has no further keyed guard owner to carry this guard"}));
            continue;
        };
        let stock = payload.array(definition + 0x110, 0xA8, Some(0x8080_29B7))?;
        let live = payload.array(resource + 0x80, 0x50, Some(0x8080_29B6))?;
        ensure!(
            !stock.is_empty() && stock.len() == live.len(),
            "The base's guard rows and their instances differ in number"
        );
        let template = payload.bytes::<0xA8>(stock[0])?;
        let mut blob = Blob::default();
        let count = guard.rows.len();
        let (live_header, live_rows) = blob.array(count, 0x8080_29B6, 0x50);
        let (rows_header, rows) = blob.array(count, 0x8080_29B7, 0xA8);
        let mut evidence = Vec::new();
        let mut records = Vec::new();
        for (ordinal, row) in guard.rows.iter().enumerate() {
            let (at, instance) = (rows + ordinal * 0xA8, live_rows + ordinal * 0x50);
            let (lists, mode, record) = label::masks(
                source,
                SOURCE_DICTIONARY,
                row.include,
                row.exclude,
                &registry,
            )?;
            // With no labels the shared compiler leaves the predicate unset. The record with
            // both bypass flags set skips both label tests, as stock rows without labels do.
            let (mode, (class, bytes)) = match record {
                Some(record) => (mode, record),
                None => {
                    let mut bytes = vec![0; 84];
                    bytes[80..82].copy_from_slice(&[1, 1]);
                    (0, (0x8080_93F6, bytes))
                }
            };
            ensure!(bytes.len() == 84, "Compiled label record has another size");
            blob.bytes[at..at + 0xA8].copy_from_slice(&template);
            blob.word(at, tag);
            blob.word(at + 4, 0x8080_29B6);
            blob.absolute.push((at + 8, instance));
            blob.bytes[at + 0x10..at + 0x18].copy_from_slice(&row.types);
            blob.bytes[at + 0x18..at + 0x28].fill(0);
            blob.bytes[at + 0x18] = mode;
            blob.bytes[at + 0x28..at + 0x2A].copy_from_slice(&row.bypass[..2]);
            blob.word(at + 0x50, tag);
            blob.word(at + 0x54, 0x8080_89F7);
            blob.absolute.push((at + 0x58, instance + 0x20));
            // The share is one constant, read by no input.
            blob.bytes[at + 0x60..at + 0xA8].fill(0);
            blob.bytes[at + 0x60] = 4;
            blob.bytes[at + 0x70] = 1;
            blob.bytes[at + 0x80] = 1;
            blob.bytes[at + 0x88] = 1;
            blob.bytes[at + 0xA0..at + 0xA4].copy_from_slice(&row.cost);
            blob.bytes[instance..instance + 0x50].fill(0);
            blob.word(instance, tag);
            blob.word(instance + 4, 0x8080_29B7);
            blob.absolute.push((instance + 8, at));
            blob.retained.push((instance + 0x10, resource));
            blob.word(instance + 0x20, tag);
            blob.word(instance + 0x24, 0x8080_89F8);
            blob.absolute.push((instance + 0x28, at + 0x50));
            evidence.push(
                json!({"types":hex::encode(row.types),"bypass":hex::encode(row.bypass),
                "include":lists[0],"exclude":lists[2],
                "share":f32::from_le_bytes(row.share),"cost":f32::from_le_bytes(row.cost)}),
            );
            records.push((class, bytes));
        }
        // Each row's label record, code and constant follow the two row arrays.
        for (ordinal, (row, (class, bytes))) in guard.rows.iter().zip(&records).enumerate() {
            let (at, instance) = (rows + ordinal * 0xA8, live_rows + ordinal * 0x50);
            let object = blob.typed(*class, bytes, 4);
            blob.point(at + 0x20, object);
            let (header, code) = blob.array(4, 0x8080_0009, 1);
            blob.bytes[code..code + 4].copy_from_slice(&[0x34, 0, 0x3E, 0]);
            blob.point(at + 0x68, header);
            let (header, constant) = blob.array(1, 0x8080_0090, 16);
            for lane in 0..4 {
                blob.bytes[constant + lane * 4..constant + lane * 4 + 4]
                    .copy_from_slice(&row.share);
            }
            blob.point(at + 0x78, header);
            blob.point(instance + 0x30, instance);
        }
        let mut edits = BTreeMap::new();
        let mut arrays = vec![
            (resource + 0x80, live_header, count as u64),
            (definition + 0x110, rows_header, count as u64),
        ];
        edits.insert(definition + 0x2B8, guard.key.to_le_bytes());
        names(
            &mut blob,
            &mut edits,
            &mut arrays,
            payload,
            definition + 0x2C0,
            &guard.exclusions,
        )?;
        keys.insert(guard.key);
        reports.push(report(
            index,
            (tag, payload, resource),
            &edits,
            &blob,
            &arrays,
            json!({"source_guard":guard.owner,"key":guard.key,"exclusions":guard.exclusions,
                "rows":evidence}),
        )?);
    }
    // The base's open guard yields to every carried name, as it does to its own keyed guards.
    for &&(index, tag, ref payload, resource, definition) in &open {
        let mut excluded = payload
            .array(definition + 0x2C0, 4, Some(0x8080_0070))?
            .into_iter()
            .map(|at| payload.u32(at))
            .collect::<Result<Vec<_>>>()?;
        let before = excluded.len();
        for &key in &keys {
            if !excluded.contains(&key) {
                excluded.push(key);
            }
        }
        if excluded.len() == before {
            continue;
        }
        let (mut blob, mut edits, mut arrays) = (Blob::default(), BTreeMap::new(), Vec::new());
        names(
            &mut blob,
            &mut edits,
            &mut arrays,
            payload,
            definition + 0x2C0,
            &excluded,
        )?;
        reports.push(report(
            index,
            (tag, payload, resource),
            &edits,
            &blob,
            &arrays,
            json!({"open_guard":true,"exclusions":excluded}),
        )?);
    }
    Ok((reports, keys, unsupported))
}
