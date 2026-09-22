//! Read native Collections topology and the item's collectible binding.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn name(r: &mut Reader, banks: &Payload, p: &Payload, at: usize) -> Result<String> {
    let index = p.u32(at)? as usize;
    let hash = p.u32(at + 4)?;
    if index == 0xFFFF || hash == 0x811C9DC5 {
        return Ok(String::new());
    }
    let rows = banks.array(8, 8, None)?;
    let bank = r.tag(
        banks.u32(*rows.get(index).context("native bank index")? + 4)?,
        Some(0x80809A88),
    )?;
    let hashes = bank.array(8, 4, None)?;
    let index = hashes
        .iter()
        .position(|&o| bank.u32(o).ok() == Some(hash))
        .context("native name hash")?;
    let p = r.tag(bank.u32(24)?, Some(0x80809A8A))?;
    let combos = p.array(0x48, 16, None)?;
    ensure!(hashes.len() == combos.len(), "native hash/combo counts");
    let combo = combos[index];
    let parts = p.array(8, 32, None)?;
    let first = p.pointer(combo)?;
    let count = usize::try_from(p.u64(combo + 8)?)?;
    if count == 0 {
        return Ok(String::new());
    }
    let start = parts
        .binary_search(&first)
        .map_err(|_| anyhow::anyhow!("native combo part"))?;
    let mut value = String::new();
    for &part in parts
        .get(start..start.checked_add(count).context("part extent")?)
        .context("native parts")?
    {
        let start = p.pointer(part + 8)?;
        let end = start + usize::from(p.u16(part + 0x14)?);
        let text = std::str::from_utf8(p.0.get(start..end).context("native characters")?)?;
        let shift = u32::from(p.u16(part + 0x18)?);
        for c in text.chars() {
            value.push(char::from_u32(c as u32 + shift).context("native Unicode")?);
        }
    }
    Ok(value)
}

pub fn inspect(r: &mut Reader, item: u32) -> Result<Value> {
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let root = r.tag(globals.u32(16)?, None)?;
    let nodes = r.tag(root.u32(8 + 63 * 16)?, None)?;
    let labels = r.tag(globals.u32(16 + 41 * 16)?, None)?;
    let banks = r.tag(globals.u32(16 + 72 * 16)?, None)?;
    let label_rows = labels.array(8, 0x2C, Some(0x80802E15))?;
    let mut tree = vec![];
    for (index, row) in nodes
        .array(8, 0xA8, Some(0x80803056))?
        .into_iter()
        .enumerate()
    {
        let label = *label_rows.get(index).context("node label")?;
        let parents = nodes
            .array(row + 0x18, 2, None)?
            .into_iter()
            .map(|o| nodes.u16(o))
            .collect::<Result<Vec<_>>>()?;
        let children = nodes
            .array(row + 0x68, 24, None)?
            .into_iter()
            .map(|o| nodes.u16(o))
            .collect::<Result<Vec<_>>>()?;
        let collectibles = nodes
            .array(row + 0x78, 4, None)?
            .into_iter()
            .map(|o| nodes.u16(o))
            .collect::<Result<Vec<_>>>()?;
        let localized = match name(r, &banks, &labels, label + 8) {
            Ok(value) => json!({"name":value}),
            Err(error) => json!({"name":null,"error":format!("{error:#}")}),
        };
        tree.push(json!({"index":index,"hash":format!("{:08X}", nodes.u32(row+0x28)?),"name":localized["name"],"localization":localized,"parents":parents,"children":children,"collectibles":collectibles,"objective":nodes.u16(row+0x50)?}));
    }
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = items.array(8, 24, None)?;
    let index = rows
        .iter()
        .position(|&o| items.u32(o).ok() == Some(item))
        .context("native item")?;
    let definition = r.tag(items.u32(rows[index] + 16)?, None)?;
    let colls = r.tag(root.u32(8 + 19 * 16)?, None)?;
    let displays = r.tag(globals.u32(16 + 15 * 16)?, None)?;
    let display_rows = displays.array(8, 0x60, None)?;
    let mut matches = vec![];
    for (i, row) in colls.array(8, 0xB8, None)?.into_iter().enumerate() {
        if colls.u16(row + 0x2C)? as usize != index {
            continue;
        }
        let parents = colls
            .array(row + 0x18, 2, None)?
            .into_iter()
            .map(|o| colls.u16(o))
            .collect::<Result<Vec<_>>>()?;
        matches.push(json!({"index":i,"hash":format!("{:08X}",colls.u32(row+0x28)?),"parents":parents,"name":name(r,&banks,&displays,display_rows[i]+8)?,"row":hex::encode(&colls.0[row..row+0xB8])}));
    }
    let result = json!({"nodes":tree,"item":item,"item_index":index,"definition":hex::encode(&definition.0),"collectibles":matches});
    write_json(&r.output.join("collections.json"), &result)?;
    r.finish()?;
    Ok(result)
}
