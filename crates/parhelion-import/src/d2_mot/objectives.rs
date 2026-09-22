//! Read-only evidence for native collection count expression graphs.
use crate::d2_mot::reader::{Reader, write_json};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub fn inspect(r: &mut Reader, root: usize) -> Result<Value> {
    let globals = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|e| e.name == "investment_globals")
        .context("globals")?
        .hash
        .0;
    let globals = r.tag(globals, None)?;
    let root_table = r.tag(globals.u32(16)?, None)?;
    let tag = root_table.u32(8 + 109 * 16)?;
    let pools = r.tag(tag, None)?;
    let rows = pools.array(8, 24, Some(0x80807C4F))?;
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut programs = BTreeMap::new();
    while let Some(index) = pending.pop() {
        ensure!(index < rows.len(), "invalid pool index {index}");
        if !seen.insert(index) {
            continue;
        }
        let mut tokens = vec![];
        for row in pools.array(rows[index] + 8, 8, Some(0x80807D31))? {
            let op = pools.u8(row)?;
            let arg = pools.u16(row + 4)?;
            if op == 12 {
                pending.push(arg as usize);
            }
            tokens.push((op, arg));
        }
        programs.insert(index, tokens);
    }
    let report = json!({"table":format!("{tag:08X}"),"root":root,"programs":programs});
    write_json(&r.output.join("objectives.json"), &report)?;
    r.finish()?;
    Ok(report)
}
