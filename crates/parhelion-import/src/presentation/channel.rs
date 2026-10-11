//! Append inherited native vector inputs without changing existing shader ordinals.
use super::{Graph, append, put};
use crate::tiger::{channel::object_channel_map, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn patches(g: &Graph, symbol: &str) -> Result<Vec<Value>> {
    Ok(g.node(symbol)?["patches"]
        .as_array()
        .context("Presentation patches")?
        .clone())
}
fn patch(entries: &mut Vec<Value>, at: usize, symbol: &str) {
    entries.retain(|p| p["offset"].as_u64() != Some(at as u64));
    entries.push(json!({"offset":at,"symbol":symbol}));
}
fn bytes(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.array(at, stride, Some(class))?
        .into_iter()
        .flat_map(|row| p.0[row..row + stride].iter().copied())
        .collect())
}

/// Update the model-input allocation, including an inherited model schema.
pub(crate) fn input_allocation(
    p: &mut Payload,
    descriptor: usize,
    before: usize,
    after: usize,
) -> Result<usize> {
    fn visit(
        p: &mut Payload,
        row: usize,
        before: usize,
        after: usize,
        seen: &mut BTreeSet<usize>,
    ) -> Result<usize> {
        ensure!(
            seen.len() < 1024 && seen.insert(row),
            "Native input allocation repeats a schema record"
        );
        p.bytes::<40>(row)?;
        let mut changed = 0;
        if p.u32(row + 16)? == 0x80809788 {
            ensure!(
                p.u32(row)? == 0xFC3956AD && p.u32(row + 20)? as usize == before,
                "Native owner input allocation differs"
            );
            put(&mut p.0, row + 20, &u32::try_from(after)?.to_le_bytes())?;
            changed += 1;
        }
        if p.u64(row + 8)? != 0 {
            let inherited = p.pointer(row + 8)?;
            ensure!(
                inherited >= 4 && p.u32(inherited - 4)? == 0x80808852,
                "Native inherited allocation schema differs"
            );
            changed += visit(p, inherited, before, after, seen)?;
        }
        for child in p.array(row + 24, 40, Some(0x80808852))? {
            changed += visit(p, child, before, after, seen)?;
        }
        Ok(changed)
    }
    visit(
        p,
        descriptor
            .checked_sub(24)
            .context("Input allocation root")?,
        before,
        after,
        &mut BTreeSet::new(),
    )
}

struct Bank {
    tag: u32,
    schema: usize,
    names: Vec<u32>,
}

fn select_bank(native: &mut Reader, entity: &Payload) -> Result<(u32, Payload)> {
    let mut banks = Vec::new();
    for row in entity.array(16, 12, Some(0x80809C04))? {
        let tag = entity.u32(row)?;
        // The caller has already replaced its private component references.
        if tag == u32::MAX {
            continue;
        }
        let owner = native.tag(tag, Some(0x80809C36))?;
        let instance = owner.pointer(16)?;
        if instance >= 4 && owner.u32(instance - 4)? == 0x8080979F {
            banks.push((tag, (*owner).clone()));
        }
    }
    ensure!(
        banks.len() == 1,
        "Native presentation needs one inherited channel bank"
    );
    Ok(banks.remove(0))
}

fn allocation(
    native: &mut Reader,
    g: &mut Graph,
    old: &Payload,
    before: usize,
    after: usize,
) -> Result<()> {
    let tag = old.u32(0x44)?;
    let mut p = (*native.tag(tag, None)?).clone();
    let rows = p.array(0x20, 40, Some(0x80808852))?;
    for (name, class) in [(0xFC2F3D6F, 0x80800090), (0x3A55A801, 0x8080979C)] {
        let matches = rows
            .iter()
            .copied()
            .filter(|row| p.u32(*row).ok() == Some(name))
            .collect::<Vec<_>>();
        ensure!(matches.len() == 1, "Native channel allocation is ambiguous");
        let row = matches[0];
        ensure!(
            p.u32(row + 16)? == class && p.u32(row + 20)? as usize == before,
            "Native channel allocation capacity differs"
        );
        put(&mut p.0, row + 20, &u32::try_from(after)?.to_le_bytes())?;
    }
    g.add("object-channel-allocation", tag, &p.0, None, vec![])
}

fn extend_bank(native: &mut Reader, g: &mut Graph, names: &[u32]) -> Result<Bank> {
    let (tag, old) = select_bank(native, &g.read("entity")?)?;
    let instance = old.pointer(16)?;
    let schema = old.pointer(24)?;
    ensure!(
        old.u32(schema - 4)? == 0x80809790,
        "Native channel definition differs"
    );
    // Local source connections and procedural resources require their own relocation.
    for (at, stride) in [
        (instance + 0x30, 80),
        (schema + 0xC8, 40),
        (instance + 0x60, 16),
        (instance + 0x80, 16),
        (schema + 0xF8, 12),
        (schema + 0x118, 12),
    ] {
        ensure!(
            old.array(at, stride, None)?.is_empty(),
            "Channel bank is not an inherited numeric bank"
        );
    }
    let rows = old.array(schema + 0xD8, 112, Some(0x808097A1))?;
    let mut order = rows
        .iter()
        .map(|row| old.u32(*row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !rows.is_empty() && order.iter().copied().collect::<BTreeSet<_>>().len() == rows.len(),
        "Native channel declaration names differ"
    );
    let lookup = old.array(schema + 0x108, 12, Some(0x808097A0))?;
    ensure!(
        lookup.len() == rows.len(),
        "Native channel names and declarations differ"
    );
    for row in lookup {
        let ordinal = usize::try_from(old.u32(row + 8)?)?;
        ensure!(
            old.u32(row)? == 0x811C9DC5 && order.get(ordinal) == Some(&old.u32(row + 4)?),
            "Native inherited channel namespace differs"
        );
    }
    for name in names {
        if !order.contains(name) {
            order.push(*name);
        }
    }
    ensure!(
        order.len() <= 64,
        "Native channel dependency mask exceeds one word"
    );
    let mut sorted = order
        .iter()
        .enumerate()
        .map(|(i, name)| (*name, i))
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    let slots = sorted
        .iter()
        .enumerate()
        .map(|(slot, (name, _))| (*name, slot))
        .collect::<BTreeMap<_, _>>();
    let mut declarations = Vec::new();
    let mut dependencies = Vec::new();
    for (i, name) in order.iter().enumerate() {
        let mut row = if let Some(&at) = rows.get(i) {
            ensure!(
                old.u64(at + 8)? == 0
                    && old.0[at + 16..at + 72].iter().all(|v| *v == 0)
                    && old.u64(at + 96)? == 0,
                "Native inherited channel has a procedure or default"
            );
            dependencies.push(bytes(&old, at + 80, 8, 0x8080000B)?);
            old.0[at..at + 112].to_vec()
        } else {
            dependencies.push((1u64 << i).to_le_bytes().to_vec());
            let mut row = vec![0; 112];
            // The native callback range is inclusive. Inherited values have no
            // local receivers, matching the stock empty range FF..FE.
            row[104..106].copy_from_slice(&[u8::MAX, u8::MAX - 1]);
            put(&mut row, 106, &u16::MAX.to_le_bytes())?;
            row
        };
        put(&mut row, 0, &name.to_le_bytes())?;
        put(&mut row, 72, &u16::try_from(slots[name])?.to_le_bytes())?;
        row[80..96].fill(0);
        declarations.extend(row);
    }
    let mut bank = old.clone();
    append(&mut bank.0, schema + 0xD8, 0x808097A1, &declarations, 112)?;
    for (row, deps) in bank
        .array(schema + 0xD8, 112, None)?
        .into_iter()
        .zip(dependencies)
    {
        append(&mut bank.0, row + 80, 0x8080000B, &deps, 8)?;
    }
    let mut cached = bytes(&old, instance + 0x50, 16, 0x80800090)?;
    let mut states = bytes(&old, instance + 0x90, 16, 0x8080979C)?;
    ensure!(
        cached.len() == rows.len() * 16
            && cached.iter().all(|v| *v == 0)
            && states.len() == rows.len() * 16
            && states.iter().all(|v| *v == 255),
        "Native channel initial storage differs"
    );
    cached.resize(order.len() * 16, 0);
    states.resize(order.len() * 16, 255);
    append(&mut bank.0, instance + 0x50, 0x80800090, &cached, 16)?;
    append(&mut bank.0, instance + 0x90, 0x8080979C, &states, 16)?;
    let mut lookup = Vec::new();
    for (name, ordinal) in sorted {
        lookup.extend(0x811C9DC5u32.to_le_bytes());
        lookup.extend(name.to_le_bytes());
        lookup.extend(u32::try_from(ordinal)?.to_le_bytes());
    }
    append(&mut bank.0, schema + 0x108, 0x808097A0, &lookup, 12)?;
    for at in [schema + 0x134, schema + 0x140] {
        put(&mut bank.0, at, &u32::try_from(order.len())?.to_le_bytes())?;
    }
    put(&mut bank.0, schema + 0x128, &0u16.to_le_bytes())?;
    put(
        &mut bank.0,
        schema + 0x12A,
        &u16::try_from(order.len() - 1)?.to_le_bytes(),
    )?;
    allocation(native, g, &old, rows.len(), order.len())?;
    let mut patches = vec![json!({"offset":0x44,"symbol":"object-channel-allocation"})];
    put(&mut bank.0, 0x44, &u32::MAX.to_le_bytes())?;
    for at in (0..bank.0.len().saturating_sub(15)).step_by(4) {
        if bank.u32(at)? == tag {
            ensure!(
                bank.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && bank.u64(at + 8)? < bank.0.len() as u64,
                "Untyped native bank self-reference"
            );
            put(&mut bank.0, at, &u32::MAX.to_le_bytes())?;
            patch(&mut patches, at, "object-channels");
        }
    }
    g.add("object-channels", tag, &bank.0, None, patches)?;
    Ok(Bank {
        tag,
        schema,
        names: order,
    })
}

fn extend_owner(native: &mut Reader, g: &mut Graph, names: &[u32]) -> Result<Vec<usize>> {
    let old = g.read("owner")?;
    let instance = old.pointer(16)?;
    let parent = instance + 0x100;
    let descriptor = usize::try_from(old.u64(parent + 8)?)? + 0x48;
    let inputs = old.array(instance + 0x120, 96, Some(0x80809788))?;
    let links = old.array(descriptor, 40, Some(0x80809789))?;
    ensure!(
        !inputs.is_empty() && inputs.len() == links.len(),
        "Native model input templates differ"
    );
    let mut data = Vec::new();
    let mut definitions = Vec::new();
    for i in 0..names.len() {
        let input = inputs.get(i).unwrap_or(&inputs[0]);
        let link = links.get(i).unwrap_or(&links[0]);
        data.extend_from_slice(&old.0[*input..input + 96]);
        definitions.extend_from_slice(&old.0[*link..link + 40]);
    }
    let mut owner = old.clone();
    append(&mut owner.0, instance + 0x120, 0x80809788, &data, 96)?;
    append(&mut owner.0, descriptor, 0x80809789, &definitions, 40)?;
    let new_inputs = owner.array(instance + 0x120, 96, None)?;
    let new_links = owner.array(descriptor, 40, None)?;
    let mut patches = patches(g, "owner")?;
    for ((input, link), name) in new_inputs.iter().zip(&new_links).zip(names) {
        put(&mut owner.0, *input, &u32::MAX.to_le_bytes())?;
        put(&mut owner.0, input + 8, &(*link as u64).to_le_bytes())?;
        put(
            &mut owner.0,
            input + 16,
            &(parent as i64 - (input + 16) as i64).to_le_bytes(),
        )?;
        put(&mut owner.0, *link, &u32::MAX.to_le_bytes())?;
        put(&mut owner.0, link + 8, &(*input as u64).to_le_bytes())?;
        put(&mut owner.0, link + 32, &name.to_le_bytes())?;
        patch(&mut patches, *input, "owner");
        patch(&mut patches, *link, "owner");
    }
    let tag = old.u32(0x44)?;
    let mut allocation = (*native.tag(tag, None)?).clone();
    ensure!(
        input_allocation(&mut allocation, 0x20, inputs.len(), names.len())? == 1,
        "Native model input allocation is ambiguous"
    );
    g.add(
        "object-channel-input-allocation",
        tag,
        &allocation.0,
        None,
        vec![],
    )?;
    put(&mut owner.0, 0x44, &u32::MAX.to_le_bytes())?;
    patch(&mut patches, 0x44, "object-channel-input-allocation");
    g.replace("owner", &owner.0, patches)?;
    Ok(new_links)
}

fn connect(g: &mut Graph, bank: &Bank, names: &[u32], links: &[usize]) -> Result<()> {
    let old = g.read("entity")?;
    let mut entity = old.clone();
    let mut patches = patches(g, "entity")?;
    let components = old.array(16, 12, Some(0x80809C04))?;
    for at in (0..old.0.len().saturating_sub(15)).step_by(4) {
        if old.u32(at)? == bank.tag {
            ensure!(
                components.contains(&at) || old.u32(at + 4)? & 0xFFFF0000 == 0x80800000,
                "Untyped native entity channel reference"
            );
            put(&mut entity.0, at, &u32::MAX.to_le_bytes())?;
            patch(&mut patches, at, "object-channels");
        }
    }
    let is_input = |row: usize| {
        old.u32(row + 12).ok() == Some(0x80809789)
            && patches
                .iter()
                .any(|p| p["offset"].as_u64() == Some((row + 8) as u64) && p["symbol"] == "owner")
    };
    let connections = old.array(0x20, 72, Some(0x80809BC9))?;
    let template = *connections
        .iter()
        .find(|row| is_input(**row))
        .context("Native model vector connection")?;
    ensure!(
        old.u32(template + 40)? == bank.tag
            && old.u32(template + 44)? == 0x808097C2
            && old.u64(template + 48)? == (bank.schema + 0x60) as u64,
        "Native model uses a different vector provider"
    );
    let mut data = Vec::new();
    let mut moved = Vec::new();
    for row in connections.iter().copied().filter(|row| !is_input(*row)) {
        let base = data.len();
        data.extend_from_slice(&entity.0[row..row + 72]);
        for p in &patches {
            let at = usize::try_from(p["offset"].as_u64().context("Connection patch offset")?)?;
            if (row..row + 72).contains(&at) {
                moved.push((
                    base + at - row,
                    p["symbol"]
                        .as_str()
                        .context("Connection patch symbol")?
                        .to_owned(),
                ));
            }
        }
    }
    for (name, link) in names.iter().zip(links) {
        let base = data.len();
        data.extend_from_slice(&entity.0[template..template + 72]);
        put(&mut data, base + 16, &(*link as u64).to_le_bytes())?;
        let ordinal = bank
            .names
            .iter()
            .position(|n| n == name)
            .context("Bank declaration for model input")?;
        put(&mut data, base + 64, &u32::try_from(ordinal)?.to_le_bytes())?;
        moved.push((base + 8, "owner".to_owned()));
        moved.push((base + 40, "object-channels".to_owned()));
    }
    append(&mut entity.0, 0x20, 0x80809BC9, &data, 72)?;
    let first = entity.pointer(0x28)? + 16;
    for (at, symbol) in moved {
        patch(&mut patches, first + at, &symbol);
    }
    g.replace("entity", &entity.0, patches)
}

/// Names resolve through the native parent hierarchy, using private bank and allocation copies.
pub(crate) fn inherit(
    native: &mut Reader,
    g: &mut Graph,
    requested: &[u32],
) -> Result<BTreeMap<String, u8>> {
    let old = object_channel_map(&g.read("owner")?.0)?;
    let mut ordered = old
        .iter()
        .map(|(name, index)| Ok((*index, u32::from_str_radix(name, 16)?)))
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_unstable();
    let mut names = ordered.iter().map(|(_, name)| *name).collect::<Vec<_>>();
    for name in requested {
        if !names.contains(name) {
            names.push(*name);
        }
    }
    if names.len() == old.len() {
        return Ok(old);
    }
    ensure!(names.len() <= 256, "Native model input capacity exceeded");
    let bank = extend_bank(native, g, &names)?;
    let links = extend_owner(native, g, &names)?;
    connect(g, &bank, &names, &links)?;
    object_channel_map(&g.read("owner")?.0)
}
