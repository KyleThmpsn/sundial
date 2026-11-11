//! The sandbox pattern table, which names each item's runtime identity.
//!
//! An item's translation block holds its row index (`u16` at `+58`). The table's 48-byte rows
//! hold the pattern's global identity at `+04`, its content key at `+10` and its translation
//! group at `+14`. The global identity selects the runtime entity, the content key selects the
//! entity's content variants and the translation group selects the stat translator's table. The
//! modern table is the single `808052AA` resource with `808052AE` rows. Shadowkeep's is named by
//! `investment_globals` at `+10 + 70 * 16`, with `80805B7C` rows.
use crate::tiger::reader::Reader;
use anyhow::{Context, Result, ensure};

/// One pattern row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pattern {
    pub key: u32,
    pub content: u32,
    pub translation_group: u32,
}

/// Every pattern row, in index order.
pub(crate) fn rows(r: &mut Reader, modern: bool) -> Result<Vec<Pattern>> {
    let tables = if modern {
        r.classes(0x808052AA)
    } else {
        let tag = r
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|e| e.name == "investment_globals")
            .context("globals")?
            .hash
            .0;
        let globals = r.tag(tag, None)?;
        vec![globals.u32(16 + 70 * 16)?]
    };
    ensure!(tables.len() == 1, "ambiguous pattern tables");
    let table = r.tag(tables[0], None)?;
    table
        .array(8, 48, Some(if modern { 0x808052AE } else { 0x80805B7C }))?
        .into_iter()
        .map(|row| {
            Ok(Pattern {
                key: table.u32(row + 4)?,
                content: table.u32(row + 16)?,
                translation_group: table.u32(row + 20)?,
            })
        })
        .collect()
}

/// The pattern row at `index`.
pub(crate) fn at(r: &mut Reader, index: usize, modern: bool) -> Result<Pattern> {
    rows(r, modern)?
        .get(index)
        .copied()
        .context("pattern index")
}
