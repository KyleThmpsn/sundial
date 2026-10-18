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
struct Symbols<'a> {
    entity: &'a str,
    owner: &'a str,
    bank: String,
    allocation: String,
    inputs: String,
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

fn extend_bank(
    native: &mut Reader,
    g: &mut Graph,
    names: &[u32],
    symbols: &Symbols<'_>,
) -> Result<Bank> {
    let (tag, old) = select_bank(native, &g.read(symbols.entity)?)?;
    let schema = old.pointer(24)?;
    ensure!(
        old.u32(schema - 4)? == 0x80809790,
        "Native channel definition differs"
    );
    let rows = old.array(schema + 0xD8, 112, Some(0x808097A1))?;
    let mut order = rows
        .iter()
        .map(|row| old.u32(*row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !rows.is_empty() && order.iter().copied().collect::<BTreeSet<_>>().len() == rows.len(),
        "Native channel declaration names differ"
    );
    // A mixed bank may already compute the requested value locally. Its vector
    // provider addresses declaration ordinals, independently of inherited slots.
    // Connect those declarations without relocating their procedures or arrays.
    if names.iter().all(|name| order.contains(name)) {
        return publish_bank(g, tag, old, order, Vec::new(), symbols);
    }
    for name in names {
        if !order.contains(name) {
            order.push(*name);
        }
    }
    let allocation_tag = old.u32(0x44)?;
    let allocation = native.tag(allocation_tag, None)?;
    let (mut bank, allocation) =
        crate::tiger::channel::inherited::extend(&old, &allocation, names)?;
    g.add(
        &symbols.allocation,
        allocation_tag,
        &allocation.0,
        None,
        vec![],
    )?;
    let patches = vec![json!({"offset":0x44,"symbol":symbols.allocation})];
    put(&mut bank.0, 0x44, &u32::MAX.to_le_bytes())?;
    publish_bank(g, tag, bank, order, patches, symbols)
}

fn publish_bank(
    g: &mut Graph,
    tag: u32,
    mut bank: Payload,
    names: Vec<u32>,
    mut patches: Vec<Value>,
    symbols: &Symbols<'_>,
) -> Result<Bank> {
    let schema = bank.pointer(24)?;
    for at in (0..bank.0.len().saturating_sub(15)).step_by(4) {
        if bank.u32(at)? == tag {
            ensure!(
                bank.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && bank.u64(at + 8)? < bank.0.len() as u64,
                "Untyped native bank self-reference"
            );
            put(&mut bank.0, at, &u32::MAX.to_le_bytes())?;
            patch(&mut patches, at, &symbols.bank);
        }
    }
    g.add(&symbols.bank, tag, &bank.0, None, patches)?;
    Ok(Bank { tag, schema, names })
}

fn extend_owner(
    native: &mut Reader,
    g: &mut Graph,
    names: &[u32],
    symbols: &Symbols<'_>,
) -> Result<Vec<usize>> {
    let old = g.read(symbols.owner)?;
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
    let mut patches = patches(g, symbols.owner)?;
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
        patch(&mut patches, *input, symbols.owner);
        patch(&mut patches, *link, symbols.owner);
    }
    let tag = old.u32(0x44)?;
    let mut allocation = (*native.tag(tag, None)?).clone();
    ensure!(
        input_allocation(&mut allocation, 0x20, inputs.len(), names.len())? == 1,
        "Native model input allocation is ambiguous"
    );
    g.add(&symbols.inputs, tag, &allocation.0, None, vec![])?;
    put(&mut owner.0, 0x44, &u32::MAX.to_le_bytes())?;
    patch(&mut patches, 0x44, &symbols.inputs);
    g.replace(symbols.owner, &owner.0, patches)?;
    Ok(new_links)
}

fn connect(
    g: &mut Graph,
    bank: &Bank,
    names: &[u32],
    links: &[usize],
    symbols: &Symbols<'_>,
) -> Result<()> {
    let old = g.read(symbols.entity)?;
    let mut entity = old.clone();
    let mut patches = patches(g, symbols.entity)?;
    let components = old.array(16, 12, Some(0x80809C04))?;
    for at in (0..old.0.len().saturating_sub(15)).step_by(4) {
        if old.u32(at)? == bank.tag {
            ensure!(
                components.contains(&at) || old.u32(at + 4)? & 0xFFFF0000 == 0x80800000,
                "Untyped native entity channel reference"
            );
            put(&mut entity.0, at, &u32::MAX.to_le_bytes())?;
            patch(&mut patches, at, &symbols.bank);
        }
    }
    let is_input = |row: usize| {
        old.u32(row + 12).ok() == Some(0x80809789)
            && patches.iter().any(|p| {
                p["offset"].as_u64() == Some((row + 8) as u64) && p["symbol"] == symbols.owner
            })
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
        moved.push((base + 8, symbols.owner.to_owned()));
        moved.push((base + 40, symbols.bank.clone()));
    }
    append(&mut entity.0, 0x20, 0x80809BC9, &data, 72)?;
    let first = entity.pointer(0x28)? + 16;
    for (at, symbol) in moved {
        patch(&mut patches, first + at, &symbol);
    }
    g.replace(symbols.entity, &entity.0, patches)
}

/// Names resolve through the native parent hierarchy, using private bank and allocation copies.
pub(crate) fn inherit(
    native: &mut Reader,
    g: &mut Graph,
    requested: &[u32],
) -> Result<BTreeMap<String, u8>> {
    inherit_for(
        native,
        g,
        requested,
        Symbols {
            entity: "entity",
            owner: "owner",
            bank: "object-channels".into(),
            allocation: "object-channel-allocation".into(),
            inputs: "object-channel-input-allocation".into(),
        },
    )
}

/// Connect another part's model while retaining the part's native channel sources.
pub(crate) fn inherit_named(
    native: &mut Reader,
    g: &mut Graph,
    requested: &[u32],
    entity: &str,
    owner: &str,
    prefix: &str,
) -> Result<BTreeMap<String, u8>> {
    inherit_for(
        native,
        g,
        requested,
        Symbols {
            entity,
            owner,
            bank: format!("{prefix}-channels"),
            allocation: format!("{prefix}-channel-allocation"),
            inputs: format!("{prefix}-input-allocation"),
        },
    )
}

fn inherit_for(
    native: &mut Reader,
    g: &mut Graph,
    requested: &[u32],
    symbols: Symbols<'_>,
) -> Result<BTreeMap<String, u8>> {
    let old = object_channel_map(&g.read(symbols.owner)?.0)?;
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
    let bank = extend_bank(native, g, &names, &symbols)?;
    let links = extend_owner(native, g, &names, &symbols)?;
    connect(g, &bank, &names, &links, &symbols)?;
    for (symbol, layout) in [
        (symbols.owner, "owner"),
        (symbols.bank.as_str(), "object-channels"),
    ] {
        let layout = crate::tiger::instance::LAYOUTS
            .iter()
            .find(|l| l.symbol == layout)
            .context("Channel instance layout")?;
        let mut payload = g.read(symbol)?;
        let mut patches = patches(g, symbol)?;
        crate::tiger::instance::seal(&mut payload, &mut patches, layout)?;
        g.replace(symbol, &payload.0, patches)?;
    }
    object_channel_map(&g.read(symbols.owner)?.0)
}
