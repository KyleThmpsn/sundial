//! Source material effects, the `foot` tags a projectile or weapon names for its contact
//! responses.
//!
//! The tag holds one block at `+0x0` with one 0x24-byte row per response type. Each row holds
//! three blocks, retired materials at `+0x0`, sounds at `+0xC` and effects at `+0x18`. Sound and
//! effect entries are 0x2C bytes: the reference at `+0x0`, a secondary reference at `+0x10`, the
//! material name at `+0x20`, the global material index at `+0x24`, the sweetener mode at `+0x26`
//! and the maximum visibility distance at `+0x28`. This follows the Halo 3 order. Every entry is
//! checked against its expected reference class, so a different layout refuses to read.
use crate::halo_reach::cache::Cache;
use anyhow::Result;
use serde_json::{Value, json};

const ROW: usize = 0x24;
const ENTRY: usize = 0x2c;

fn entries(cache: &Cache, block: usize, group: &str) -> Result<Vec<Value>> {
    cache
        .block(block, ENTRY)?
        .into_iter()
        .map(|entry| {
            Ok(json!({"tag":cache.reference(entry, Some(group))?,
                "secondary":cache.reference(entry + 0x10, Some(group))?,
                "material":cache.string_id(cache.u32(entry + 0x20)?)?,
                "global_material":cache.i16(entry + 0x24)?,
                "sweetener":cache.u8(entry + 0x26)?,
                "maximum_visibility_metres":cache.f32(entry + 0x28)?}))
        })
        .collect()
}

/// Every response type's sound and effect entries.
pub(in crate::halo_reach) fn read(cache: &Cache, at: usize) -> Result<Value> {
    let rows = cache
        .block(at, ROW)?
        .into_iter()
        .map(|row| {
            Ok(json!({"sounds":entries(cache, row + 0xc, "snd!")?,
                "effects":entries(cache, row + 0x18, "effe")?}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"types":rows}))
}
